//! App conversion orchestration over the verified core (wiring layer).
//!
//! This module composes the already-tested pieces (`format`, `cover`,
//! `remux`, `live2motion`, `model`, `pymotion`, `heifwrite`) into the exact
//! conversions the Swift app performs today, so the app can call them
//! through the C ABI instead of reimplementing them in Swift:
//!
//! - `to_live`: Android Motion Photo -> Apple Live Photo (still + MOV),
//!   mirroring `BatchEngine.convert` (to-apple) + `MotionLive.movie()`.
//! - `to_motion`: Apple Live Photo -> Android Motion Photo, mirroring
//!   `AndroidExport.convert`.
//! - `vendor_convert`: Google -> Honor, mirroring `AndroidTransfer.convert`
//!   (strict structure acceptance + trial assembly + parse-back check).
//! - `live_pair`: Live Photo still+movie pairing, mirroring
//!   `AndroidExport.pair()` (identifiers + edit-list-aware cover).
//! - `export_auxiliary`: second video track -> silent MOV, mirroring
//!   `PhotoFormat.exportAuxiliary` (with the zero-duration repair).
//! - `candidate_times`: per-frame candidate timestamps, mirroring
//!   `CoverCandidateSession` (timing only; JPEG rendering stays in Swift).
//!
//! Deliberately NOT here (they need pixels or system frameworks):
//! JPEG/video decode and encode, the legacy-gain pixel lookup table,
//! HEIC gain extraction through Core Graphics, and candidate JPEG
//! rendering. When a conversion needs one of those, the function returns
//! `NeedsPixelWork` instead of guessing, so the app can do just that step
//! and retry with prepared bytes.

use crate::cover::{self, CoverMapping, CoverRequest};
use crate::model::{self, Cover, MotionModel, Vendor};
use crate::{
    format, hdr, heif, heifwrite, jpeg, live2motion, mp4, pymotion, pyxml, remux,
};
use crate::{fail, Result};

fn ticks_to_us(ticks: i64, ts: i64) -> i64 {
    (ticks * 1_000_000 + ts / 2) / ts
}

/// C ABI adapter for [`pymotion::attach_preconverted_gain`]: `icc` None
/// means attach without the color-profile step (Swift's
/// `AndroidHDR.convert` path).
pub fn attach_gain_for_cabi(
    base: &[u8],
    gain: &[u8],
    stops: f64,
    icc: Option<&[u8]>,
) -> Result<Vec<u8>> {
    pymotion::attach_preconverted_gain(base, gain, stops, icc)
}

/// Vendor name from the app's option strings. Xiaomi and Honor share the
/// Google-compatible layout, exactly like the convert CLI.
pub fn parse_vendor(s: &str) -> Result<Vendor> {
    match s {
        "google" | "xiaomi" | "honor" => Ok(Vendor::Google),
        "samsung" => Ok(Vendor::Samsung),
        "huawei" => Ok(Vendor::Huawei),
        "oppo" => Ok(Vendor::Oppo),
        "vivo" => Ok(Vendor::Vivo),
        _ => fail(format!("未知厂商: {s}")),
    }
}

/// Whether a standalone still carries an Ultra HDR gain map: an hdrgm
/// Version value on the primary image or on any auxiliary image, mirroring
/// Swift `AndroidExport.prepare`'s ultra detection.
pub fn still_is_ultra(still: &[u8]) -> Result<bool> {
    for packet in pymotion::xmp_packets(still)? {
        if !pyxml::values(&packet.root, pymotion::HDR, "Version").is_empty() {
            return Ok(true);
        }
    }
    for r in jpeg::auxiliary(still)?.iter().skip(1) {
        let image = &still[r.clone()];
        for packet in pymotion::xmp_packets(image)? {
            if !pyxml::values(&packet.root, pymotion::HDR, "Version").is_empty() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// What the app must do outside Rust before retrying a conversion.
#[derive(Clone, Debug)]
pub struct NeedsPixelWork {
    /// "legacy-hdr" | "heic-to-jpeg" | "reencode-requested".
    pub reason: &'static str,
    /// Headroom stops for "legacy-hdr", else None.
    pub stops: Option<f64>,
    pub detail: String,
}

/// A conversion result: either finished bytes, or a pixel-work request.
#[derive(Debug)]
pub enum ConvertOutcome<T> {
    Done(T),
    NeedsPixelWork(NeedsPixelWork),
}

/// Byte-level HDR handling for a JPEG still, mirroring Swift's HDR branch
/// order (ultra, then ISO, then legacy, then SDR). Honor-private tails are
/// handled by the caller, not here, because they ride along with the still
/// rather than replacing its HDR handling.
enum JpegHdr {
    UltraPreserved,
    IsoConverted(Vec<u8>),
    IsoRetained,
    Legacy { stops: f64 },
    Sdr,
}

fn jpeg_hdr(still: &[u8], ultra: bool) -> Result<JpegHdr> {
    if ultra {
        return Ok(JpegHdr::UltraPreserved);
    }
    if crate::scan::has_iso(still)? {
        return match pymotion::iso_upgrade(still)? {
            Some(upgraded) => Ok(JpegHdr::IsoConverted(upgraded)),
            None => Ok(JpegHdr::IsoRetained),
        };
    }
    if let (Some(legacy), _) = pymotion::legacy_apple(still)? {
        return Ok(JpegHdr::Legacy {
            stops: legacy.stops,
        });
    }
    Ok(JpegHdr::Sdr)
}

/// Cover in the form a vendor writer needs, converted on the primary video
/// track (unlike the CLI's single-track helper, this also accepts
/// two-track movies, where the primary track carries the cover).
fn convert_cover_primary(video: &[u8], cover: Cover, target: Vendor) -> Result<Cover> {
    let videos = cover::video_tracks(video)?;
    if videos.is_empty() {
        return fail("视频没有视频轨，无法换算封面");
    }
    let track = &videos[0].0;
    let need_frame = target == Vendor::Huawei;
    match (cover, need_frame) {
        (Cover::Frame(_), true) | (Cover::Time(_), false) => Ok(cover),
        (Cover::Time(t), true) => Ok(Cover::Frame(cover::time_us_to_frame(track, t)? as i64)),
        (Cover::Frame(f), false) => Ok(Cover::Time(cover::frame_to_time_us(track, f as usize)?)),
    }
}

// ---------------------------------------------------------------------------
// to-live (Android -> Apple)
// ---------------------------------------------------------------------------

/// Options for [`to_live`], mirroring Swift `movie()`'s request dictionary
/// plus the app's conversion options that select the still.
#[derive(Clone, Debug, Default)]
pub struct ToLiveOpts {
    pub uuid: String,
    pub timestamp_us: Option<i64>,
    pub cover_frame_index: Option<usize>,
    pub timestamp_source: Option<String>,
    pub time_domain: String,
    pub video_track: String,
    pub missing_time: String,
    /// "preconverted-ultra" (legacy gain attached by the app's pixel step)
    /// | "candidate" (decoded candidate JPEG) | None (infer from bytes).
    pub still_override_kind: Option<String>,
}

/// Output of [`to_live`]: the Live Photo pair plus its cover and HDR report.
#[derive(Debug)]
pub struct ToLiveOutput {
    pub still: Vec<u8>,
    pub mov: Vec<u8>,
    pub cover: CoverMapping,
    pub hdr_input: String,
    pub hdr_status: String,
    pub hdr_reason: String,
    pub hdr_output: Option<String>,
    pub video_tracks: usize,
}

pub fn to_live(
    motion: &[u8],
    opts: &ToLiveOpts,
    still_override: Option<&[u8]>,
) -> Result<ConvertOutcome<ToLiveOutput>> {
    if opts.uuid.is_empty() {
        return fail("内容标识不能为空");
    }
    let photo = format::parse(motion)?;
    let video = photo.video(motion);
    let videos = cover::video_tracks(video)?;
    if videos.is_empty() {
        return fail("视频没有视频轨，无法转换");
    }
    if videos.len() > 2 {
        return fail("视频轨道布局不受支持");
    }
    let video_index = if opts.video_track == "secondary" {
        if videos.len() < 2 {
            return fail("需要两个视频轨才能选择辅助轨");
        }
        1
    } else {
        0
    };
    let mapping = cover::map_cover(
        video,
        &CoverRequest {
            cover_frame_index: opts.cover_frame_index,
            timestamp_us: opts.timestamp_us,
            timestamp_source: opts.timestamp_source.clone(),
            time_domain: opts.time_domain.clone(),
            video_track: opts.video_track.clone(),
            missing_time: opts.missing_time.clone(),
        },
    )?;
    // The still actually converted: an override (candidate / preconverted
    // legacy) or the motion file's own still.
    let still_base: &[u8] = match still_override {
        Some(b) => b,
        None => &photo.still[..],
    };
    // Honor-private tail rides with the still Swift-side: it is stripped by
    // the JPEG cleaning below and re-appended verbatim, then verified.
    // When an override is used, a tail on the ORIGINAL still but not on the
    // override is Swift's guard failure (BatchEngine's honor check), so it
    // is refused here rather than silently dropped.
    let original_tail: Option<Vec<u8>> = if still_override.is_none() {
        None
    } else {
        let orig_end = jpeg::image_ranges(&photo.still)?
            .iter()
            .map(|r| r.end)
            .max()
            .unwrap_or(photo.still.len());
        hdr::honor_private(&photo.still, orig_end)?.map(|t| t.to_vec())
    };
    let (still_out, hdr_input, hdr_status, hdr_reason, hdr_output);
    if heif::matches(still_base) {
        if original_tail.is_some() {
            return fail("荣耀私有 HDR 静态扩展与候选封面不能共存");
        }
        still_out = heifwrite::live_still_heic(still_base, &opts.uuid)?;
        hdr_input = "heic".to_string();
        hdr_status = "preserved".to_string();
        hdr_reason =
            "HEIC 图像编码、色彩与方向属性原样保留；未新增或转换 HDR".to_string();
        hdr_output = None;
    } else {
        let base_end = jpeg::image_ranges(still_base)?
            .iter()
            .map(|r| r.end)
            .max()
            .unwrap_or(still_base.len());
        let tail = hdr::honor_private(still_base, base_end)?.map(|t| t.to_vec());
        if original_tail.is_some() && tail.as_ref() != original_tail.as_ref() {
            return fail("荣耀私有 HDR 静态扩展与候选封面不能共存");
        }
        if let Some(t) = &tail {
            let cleaned = pymotion::live_still_with_identifier(still_base, &opts.uuid)?;
            let mut out = cleaned;
            out.extend_from_slice(t);
            let back_end = jpeg::image_ranges(&out)?
                .iter()
                .map(|r| r.end)
                .max()
                .unwrap_or(out.len());
            if hdr::honor_private(&out, back_end)?.map(|x| x.to_vec()) != Some(t.clone()) {
                return fail("荣耀私有 HDR 静态扩展字节校验失败");
            }
            still_out = out;
            hdr_input = "honor-private-ehdr".to_string();
            hdr_status = "unsupported-preserved".to_string();
            hdr_reason = "已保留荣耀私有 HDR 静态扩展；尚不能可靠转换为 Ultra HDR，Apple 设备显示 SDR。可先用荣耀相册转为通用 HDR 后再导入".to_string();
            hdr_output = None;
        } else if let Some(kind) = opts.still_override_kind.as_deref() {
            still_out = pymotion::live_still_with_identifier(still_base, &opts.uuid)?;
            match kind {
                "preconverted-ultra" => {
                    hdr_input = "legacy-or-sdr".to_string();
                    hdr_status = "converted".to_string();
                    hdr_reason = "已有旧 Apple 增益图已转换，主图未重新编码".to_string();
                    hdr_output = Some("ultra-hdr".to_string());
                }
                _ => {
                    hdr_input = "auxiliary-video".to_string();
                    hdr_status = "decoded-sdr".to_string();
                    hdr_reason = "辅助高清帧已解码为 JPEG 封面；主运动轨保持原编码，未复制原封面的 HDR 增益图".to_string();
                    hdr_output = None;
                }
            }
        } else {
            match jpeg_hdr(still_base, photo.ultra)? {
                JpegHdr::UltraPreserved => {
                    still_out = pymotion::live_still_with_identifier(still_base, &opts.uuid)?;
                    hdr_input = "ultra-hdr".to_string();
                    hdr_status = "preserved".to_string();
                    hdr_reason = "Ultra HDR 主图及增益图原样保留".to_string();
                    hdr_output = None;
                }
                JpegHdr::IsoConverted(upgraded) => {
                    still_out = pymotion::live_still_with_identifier(&upgraded, &opts.uuid)?;
                    hdr_input = "iso-hdr".to_string();
                    hdr_status = "converted".to_string();
                    hdr_reason = "已有 ISO 增益图已补充 Ultra HDR 元数据；主图及增益图均未重新编码".to_string();
                    hdr_output = Some("ultra-hdr+iso".to_string());
                }
                JpegHdr::IsoRetained => {
                    still_out = pymotion::live_still_with_identifier(still_base, &opts.uuid)?;
                    hdr_input = "iso-hdr".to_string();
                    hdr_status = "preserved".to_string();
                    hdr_reason = "ISO HDR 数据已保留；缺少可验证的单增益图布局，未推测转换".to_string();
                    hdr_output = None;
                }
                JpegHdr::Legacy { stops } => {
                    return Ok(ConvertOutcome::NeedsPixelWork(NeedsPixelWork {
                        reason: "legacy-hdr",
                        stops: Some(stops),
                        detail: "旧 Apple 增益图需经像素查表转换后重试".to_string(),
                    }));
                }
                JpegHdr::Sdr => {
                    still_out = pymotion::live_still_with_identifier(still_base, &opts.uuid)?;
                    hdr_input = "legacy-or-sdr".to_string();
                    hdr_status = "unchanged".to_string();
                    hdr_reason = "旧版 HDR 转换已关闭".to_string();
                    hdr_output = None;
                }
            }
        }
        // Coding data must survive the still cleaning, like Swift's guard.
        let out_end = jpeg::image_ranges(&still_out)?
            .iter()
            .map(|r| r.end)
            .max()
            .unwrap_or(still_out.len());
        let orig_stripped = match &tail {
            Some(_) => &still_base[..base_end],
            None => still_base,
        };
        let out_stripped = &still_out[..out_end];
        if format::coding_hash(out_stripped)? != format::coding_hash(orig_stripped)? {
            return fail("静态主图压缩数据发生变化");
        }
    }
    // Swift `movie()` writes the matched frame's media presentation time as
    // the still-image-time value (its own output convention, not the native
    // movie-timeline form); the fresh mett track maps it back exactly.
    let mov = remux::remux_to_mov_with_identifier_and_mett(
        video,
        video_index,
        &opts.uuid,
        mapping.frame_time_us,
    )?;
    remux::check_remux(video, &mov, video_index)?;
    let mett = live2motion::mett_cover_time(&mov)?;
    match mett {
        Some(t) if (t - mapping.frame_time_us).abs() <= 2 => {}
        _ => return fail("still-image-time 读回校验失败"),
    }
    let ident = remux::read_content_identifier(&mov)?;
    if ident.as_deref() != Some(opts.uuid.as_str()) {
        return fail("内容标识读回校验失败");
    }
    Ok(ConvertOutcome::Done(ToLiveOutput {
        still: still_out,
        mov,
        cover: mapping,
        hdr_input,
        hdr_status,
        hdr_reason,
        hdr_output,
        video_tracks: videos.len(),
    }))
}

// ---------------------------------------------------------------------------
// to-motion (Apple -> Android)
// ---------------------------------------------------------------------------

/// Options for [`to_motion`], mirroring `AndroidExport.convert`'s inputs.
#[derive(Clone, Debug, Default)]
pub struct ToMotionOpts {
    pub vendor: String,
    /// "jpeg" (default) | "heic".
    pub container: String,
    pub cover_time_us: Option<i64>,
    pub cover_frame: Option<i64>,
    /// False means the app wants a re-encoded base JPEG (system work).
    pub preserve_jpeg: bool,
}

/// Output of [`to_motion`].
#[derive(Debug)]
pub struct ToMotionOutput {
    pub bytes: Vec<u8>,
    pub hdr_input: String,
    pub hdr_status: String,
    pub hdr_reason: String,
    pub hdr_output: Option<String>,
    /// Cover actually written: time (us) or Huawei frame index.
    pub cover_time_us: Option<i64>,
    pub cover_frame: Option<i64>,
}

pub fn to_motion(
    still: &[u8],
    movie: &[u8],
    opts: &ToMotionOpts,
) -> Result<ConvertOutcome<ToMotionOutput>> {
    let vendor = parse_vendor(&opts.vendor)?;
    let container = if opts.container.is_empty() {
        "jpeg"
    } else {
        opts.container.as_str()
    };
    if container != "jpeg" && container != "heic" {
        return fail(format!("未知容器: {container}"));
    }
    let is_heic = heif::matches(still);
    if container == "heic" && !is_heic {
        return fail("HEIC 容器要求 HEIC 输入");
    }
    if container == "heic" && vendor != Vendor::Google && vendor != Vendor::Samsung {
        return fail("HEIC 容器仅支持 Google 与三星目标");
    }
    // Cover resolution, mirroring Swift `pair()` + `convert_cover`:
    // explicit cover wins, else the mett marker mapped edit-list-aware.
    let cover: Cover = if let Some(f) = opts.cover_frame {
        Cover::Frame(f)
    } else if let Some(t) = opts.cover_time_us {
        Cover::Time(t)
    } else {
        let mett = live2motion::mett_cover_time(movie)?;
        let t = match mett {
            Some(t) => t,
            None => return fail("缺少唯一有效 still-image-time 封面标记"),
        };
        let mapping = cover::map_cover(
            movie,
            &CoverRequest {
                timestamp_us: Some(t),
                time_domain: "auto".to_string(),
                video_track: "primary".to_string(),
                missing_time: "error".to_string(),
                ..Default::default()
            },
        )?;
        Cover::Time(mapping.frame_time_us)
    };
    let cover = convert_cover_primary(movie, cover, vendor)?;
    let (cover_time_us, cover_frame) = match cover {
        Cover::Time(t) => (Some(t), None),
        Cover::Frame(f) => (None, Some(f)),
    };
    if is_heic {
        if container == "jpeg" {
            return Ok(ConvertOutcome::NeedsPixelWork(NeedsPixelWork {
                reason: "heic-to-jpeg",
                stops: None,
                detail: "HEIC 输入转 JPEG 容器需经系统重编码后重试".to_string(),
            }));
        }
        let time = match cover {
            Cover::Time(t) => t,
            Cover::Frame(_) => return fail("HEIC 组装需要封面时间"),
        };
        let video_mp4 = remux::remux_to_mp4(movie)?;
        let bytes = heifwrite::assemble_heic(still, &video_mp4, vendor, time)?;
        return Ok(ConvertOutcome::Done(ToMotionOutput {
            bytes,
            hdr_input: "heic".to_string(),
            hdr_status: "preserved".to_string(),
            hdr_reason: "HEIC 图像编码、色彩与方向属性原样保留".to_string(),
            hdr_output: None,
            cover_time_us,
            cover_frame,
        }));
    }
    if !opts.preserve_jpeg {
        return Ok(ConvertOutcome::NeedsPixelWork(NeedsPixelWork {
            reason: "reencode-requested",
            stops: None,
            detail: "关闭保留 JPEG 时需经系统重编码后重试".to_string(),
        }));
    }
    let (still_final, hdr_input, hdr_status, hdr_reason, hdr_output) =
        match jpeg_hdr(still, still_is_ultra(still)?)? {
            JpegHdr::UltraPreserved => (
                still.to_vec(),
                "ultra-hdr".to_string(),
                "preserved".to_string(),
                "Ultra HDR 主图及增益图原样保留".to_string(),
                None,
            ),
            JpegHdr::IsoConverted(upgraded) => (
                upgraded,
                "iso-hdr".to_string(),
                "converted".to_string(),
                "已有 ISO 增益图已补充 Ultra HDR 元数据；主图及增益图均未重新编码".to_string(),
                Some("ultra-hdr+iso".to_string()),
            ),
            JpegHdr::IsoRetained => (
                still.to_vec(),
                "iso-hdr".to_string(),
                "preserved".to_string(),
                "ISO HDR 数据已保留；缺少可验证的单增益图布局，未推测转换".to_string(),
                None,
            ),
            JpegHdr::Legacy { stops } => {
                return Ok(ConvertOutcome::NeedsPixelWork(NeedsPixelWork {
                    reason: "legacy-hdr",
                    stops: Some(stops),
                    detail: "旧 Apple 增益图需经像素查表转换后重试".to_string(),
                }));
            }
            JpegHdr::Sdr => (
                still.to_vec(),
                "sdr".to_string(),
                "preserved".to_string(),
                "原始图像".to_string(),
                None,
            ),
        };
    let bytes = live2motion::assemble(
        &live2motion::LivePhotoInput {
            still_jpeg: still_final,
            movie_mov: movie.to_vec(),
        },
        vendor,
        cover,
    )?;
    Ok(ConvertOutcome::Done(ToMotionOutput {
        bytes,
        hdr_input,
        hdr_status,
        hdr_reason,
        hdr_output,
        cover_time_us,
        cover_frame,
    }))
}

// ---------------------------------------------------------------------------
// vendor_convert (Google -> Honor)
// ---------------------------------------------------------------------------

fn xmp_int_unique(packets: &[pymotion::Packet], key: &str) -> Result<Option<i64>> {
    let mut found: Option<i64> = None;
    for p in packets {
        for v in pyxml::values(&p.root, pymotion::CAMERA, key) {
            let n: i64 = match v.parse() {
                Ok(n) => n,
                Err(_) => return fail(format!("XMP 整数无效: {key}")),
            };
            match found {
                None => found = Some(n),
                Some(m) if m == n => {}
                Some(_) => return fail(format!("XMP 整数冲突: {key}")),
            }
        }
    }
    Ok(found)
}

fn walk_elements<'a>(el: &'a pyxml::Element, f: &mut dyn FnMut(&'a pyxml::Element)) {
    f(el);
    for c in &el.children {
        walk_elements(c, f);
    }
}

fn is_vendor_uri(uri: &str) -> bool {
    uri == "http://ns.oplus.com/photos/1.0/camera/"
        || uri == "http://ns.vivo.com/photos/1.0/camera/"
        || uri.to_lowercase().contains("samsung")
        || uri.to_lowercase().contains("xiaomi")
        || uri.to_lowercase().contains("huawei")
        || uri.to_lowercase().contains("honor")
}

/// Strict Google acceptance, port of Swift `AndroidTransfer.inspectStructure`
/// minus the CGImageSource decode check (system work, stays in the app).
/// Returns the parsed photo plus its cover time on success.
fn strict_google(motion: &[u8]) -> Result<(format::MotionPhoto<'_>, i64)> {
    if !motion.starts_with(&[255, 216]) {
        return fail("来源不支持：暂仅支持 Google → 荣耀的 Motion Photo JPEG，不支持 HEIC");
    }
    let photo = format::parse(motion)?;
    let still: &[u8] = &photo.still;
    let packets = pymotion::xmp_packets(still)?;
    let modern = xmp_int_unique(&packets, "MotionPhoto")?;
    let legacy = xmp_int_unique(&packets, "MicroVideo")?;
    if modern != Some(1) && legacy != Some(1) {
        return fail("来源不支持：缺少 Google Motion Photo 声明");
    }
    if modern.map_or(false, |v| v != 1) || legacy.map_or(false, |v| v != 1) {
        return fail("文件损坏：Google 动态声明冲突");
    }
    for key in ["MotionPhotoVersion", "MicroVideoVersion"] {
        if let Some(v) = xmp_int_unique(&packets, key)? {
            if v != 1 {
                return fail("来源不支持：未知 Google 动态版本");
            }
        }
    }
    for p in &packets {
        let mut check = |ns: &str, local: &str, value: &str| -> Result<()> {
            if is_vendor_uri(ns) {
                return fail("来源不支持：检测到其他厂商动态结构");
            }
            if ns == pymotion::HDR && local == "Version" && value != "1.0" {
                return fail("结构无法无损保留：未知增益图版本");
            }
            Ok(())
        };
        let mut scan_el = |el: &pyxml::Element| -> Result<()> {
            check(&el.ns, &el.local, el.text.as_deref().unwrap_or(""))?;
            for (ns, local, value) in &el.attrib {
                check(ns, local, value)?;
            }
            Ok(())
        };
        let mut err: Option<String> = None;
        walk_elements(&p.root, &mut |el| {
            if err.is_none() {
                if let Err(e) = scan_el(el) {
                    err = Some(e.to_string());
                }
            }
        });
        if let Some(e) = err {
            return fail(e);
        }
    }
    if photo.format != "google-compatible" || photo.cover_frame.is_some() {
        return fail("来源不支持：不是 Google 标准动态封装");
    }
    let mut ranges = jpeg::image_ranges(motion)?;
    ranges.sort_by_key(|r| r.start);
    if !(ranges.len() == 1 || (ranges.len() == 2 && photo.ultra)) {
        return fail("结构无法无损保留：附加图片无法确认是增益图");
    }
    let mut end = 0usize;
    for r in &ranges {
        if r.start != end {
            return fail("结构无法无损保留：图片之间存在未解释载荷");
        }
        end = r.end;
    }
    let video = photo.video(motion);
    if end + video.len() != motion.len() || &motion[end..] != video {
        return fail("结构无法无损保留：存在额外尾标、间隙或未解释载荷");
    }
    if let Some(offset) = xmp_int_unique(&packets, "MicroVideoOffset")? {
        if offset as usize != video.len() {
            return fail("文件损坏：旧版偏移与视频长度冲突");
        }
    }
    let mut directories: Vec<&pyxml::Element> = Vec::new();
    for p in &packets {
        walk_elements(&p.root, &mut |el| {
            if el.ns == pymotion::CONTAINER && el.local == "Directory" {
                directories.push(el);
            }
        });
    }
    if directories.len() > 1 || (modern == Some(1) && directories.len() != 1) {
        return fail("结构无法无损保留：Google 目录缺失或重复");
    }
    if let Some(dir) = directories.first() {
        let mut err: Option<String> = None;
        walk_elements(dir, &mut |el| {
            if err.is_some() {
                return;
            }
            let structural = (el.ns == pymotion::CONTAINER
                && (el.local == "Directory" || el.local == "Item"))
                || (el.ns == pymotion::RDF && (el.local == "Seq" || el.local == "li"));
            let property = el.ns == pymotion::ITEM
                && ["Semantic", "Mime", "Length", "Padding"].contains(&el.local.as_str());
            if !structural && !property {
                err = Some("结构无法无损保留：目录包含未知字段".to_string());
                return;
            }
            for (ns, local, value) in &el.attrib {
                let known = ns == pymotion::ITEM
                    && ["Semantic", "Mime", "Length", "Padding"].contains(&local.as_str());
                let resource = ns == pymotion::RDF
                    && local == "parseType"
                    && value == "Resource";
                if !known && !resource {
                    err = Some("结构无法无损保留：目录包含未知属性".to_string());
                    return;
                }
            }
        });
        if let Some(e) = err {
            return fail(e);
        }
        let mut items: Vec<&pyxml::Element> = Vec::new();
        walk_elements(dir, &mut |el| {
            if el.attrib.iter().any(|(ns, local, _)| {
                ns == pymotion::ITEM && local == "Semantic"
            }) {
                items.push(el);
            }
        });
        let semantics: Vec<String> = items
            .iter()
            .map(|el| {
                el.attrib
                    .iter()
                    .find(|(ns, local, _)| ns == pymotion::ITEM && local == "Semantic")
                    .map(|(_, _, v)| v.clone())
                    .unwrap_or_default()
            })
            .collect();
        let expected_semantics: Vec<&str> = if ranges.len() == 2 {
            vec!["Primary", "GainMap", "MotionPhoto"]
        } else {
            vec!["Primary", "MotionPhoto"]
        };
        if semantics.iter().map(|s| s.as_str()).collect::<Vec<_>>() != expected_semantics
            || ranges.len() > 2
        {
            return fail("结构无法无损保留：目录含未知或重复载荷");
        }
        for (i, item) in items.iter().enumerate() {
            for name in ["Semantic", "Mime", "Length", "Padding"] {
                if pyxml::values(item, pymotion::ITEM, name).len() > 1 {
                    return fail("文件损坏：目录属性重复或冲突");
                }
            }
            let semantic = semantics[i].as_str();
            let expected_len = if semantic == "Primary" {
                0
            } else if semantic == "GainMap" {
                ranges[1].len()
            } else {
                video.len()
            };
            let length = pyxml::values(item, pymotion::ITEM, "Length")
                .first()
                .cloned()
                .unwrap_or_else(|| "0".to_string());
            let padding = pyxml::values(item, pymotion::ITEM, "Padding")
                .first()
                .cloned()
                .unwrap_or_else(|| "0".to_string());
            let mime = pyxml::values(item, pymotion::ITEM, "Mime").first().cloned();
            let length_ok = length.parse::<usize>().map_or(false, |n| n == expected_len);
            let padding_ok = padding.parse::<usize>().map_or(false, |n| n == 0);
            let mime_ok = mime.as_deref()
                == Some(if semantic == "MotionPhoto" {
                    "video/mp4"
                } else {
                    "image/jpeg"
                });
            if !length_ok || !padding_ok || !mime_ok {
                return fail("文件损坏：目录长度、类型或填充与实际载荷不符");
            }
        }
    }
    let mut times: Vec<i64> = Vec::new();
    for key in [
        "MotionPhotoPresentationTimestampUs",
        "MicroVideoPresentationTimestampUs",
    ] {
        if let Some(v) = xmp_int_unique(&packets, key)? {
            times.push(v);
        }
    }
    if times.is_empty()
        || times.iter().any(|t| *t < 0)
        || times.iter().collect::<std::collections::HashSet<_>>().len() != 1
        || photo.timestamp != times.first().copied()
    {
        return fail("封面时间不明确：时间缺失、无效或声明冲突");
    }
    Ok((photo, times[0]))
}

/// Output of [`vendor_convert`].
#[derive(Debug)]
pub struct VendorConvertOutput {
    pub bytes: Vec<u8>,
    pub cover_time_us: i64,
}

pub fn vendor_convert(motion: &[u8], target: &str) -> Result<VendorConvertOutput> {
    let vendor = parse_vendor(target)?;
    if vendor != Vendor::Google {
        return fail("厂商转换目前仅支持 Google 目标（Google → 荣耀）");
    }
    let (photo, time) = strict_google(motion)?;
    let video = photo.video(motion).to_vec();
    let model = MotionModel {
        still: photo.still.to_vec(),
        video: video.clone(),
        cover: Cover::Time(time),
        source_format: photo.format.clone(),
    };
    let out = model::write(&model, Vendor::Google)?;
    // Parse-back validation, mirroring Swift `validate` minus its XMP
    // metadata-tree comparison (that comparison stays in the app as a
    // post-check; it is verification, not conversion).
    let after = format::parse(&out)?;
    if after.video(&out) != video.as_slice()
        || after.timestamp != photo.timestamp
        || format::coding_hash(&after.still)? != format::coding_hash(&photo.still)?
        || format::auxiliary_hashes(&after.still)? != format::auxiliary_hashes(&photo.still)?
    {
        return fail("结构无法无损保留：图像、EXIF、增益图或必要元数据校验失败");
    }
    Ok(VendorConvertOutput {
        bytes: out,
        cover_time_us: time,
    })
}

// ---------------------------------------------------------------------------
// live_pair (Apple still + movie pairing for to-android scan)
// ---------------------------------------------------------------------------

/// Output of [`live_pair`].
#[derive(Debug)]
pub struct LivePairInfo {
    pub still_identifier: String,
    pub movie_identifier: String,
    pub cover_time_us: i64,
    pub cover_frame: usize,
    pub domain: String,
    pub policy: String,
    pub maker33: bool,
}

pub fn live_pair(still: &[u8], movie: &[u8], time_domain: &str) -> Result<LivePairInfo> {
    if heif::matches(still) {
        return fail("配对暂仅支持 JPEG 静态图");
    }
    let still_id = match pymotion::maker_note_identifier(still)? {
        Some(s) => s,
        None => return fail("缺少内容标识，无法配对"),
    };
    let movie_id = match remux::read_content_identifier(movie)? {
        Some(s) => s,
        None => return fail("缺少内容标识，无法配对"),
    };
    if still_id != movie_id {
        return fail("内容标识不匹配");
    }
    let mett = match live2motion::mett_cover_time(movie)? {
        Some(t) => t,
        None => return fail("缺少唯一有效 still-image-time 封面标记"),
    };
    let mapping = cover::map_cover(
        movie,
        &CoverRequest {
            timestamp_us: Some(mett),
            time_domain: time_domain.to_string(),
            video_track: "primary".to_string(),
            missing_time: "error".to_string(),
            ..Default::default()
        },
    )?;
    Ok(LivePairInfo {
        still_identifier: still_id,
        movie_identifier: movie_id,
        cover_time_us: mapping.frame_time_us,
        cover_frame: mapping.frame,
        domain: mapping.domain,
        policy: mapping.policy,
        maker33: pymotion::maker_note_has_tag(still, 33)?,
    })
}

// ---------------------------------------------------------------------------
// export_auxiliary
// ---------------------------------------------------------------------------

/// Output of [`export_auxiliary`].
#[derive(Debug)]
pub struct AuxExport {
    pub mov: Vec<u8>,
    pub repaired: bool,
    pub repair_method: Option<String>,
    pub repaired_duration_seconds: Option<f64>,
    pub original_sample_sha256: Option<String>,
}

fn verify_single_track(src: &mp4::TrackSamples, out: &mp4::TrackSamples) -> Result<()> {
    if src.samples.len() != out.samples.len() || src.timescale != out.timescale {
        return fail("辅助轨重封装校验失败：样本数或时间尺度不同");
    }
    for (s, o) in src.samples.iter().zip(out.samples.iter()) {
        if s.size != o.size || s.presentation_time != o.presentation_time {
            return fail("辅助轨重封装校验失败：样本尺寸或显示时间不同");
        }
    }
    Ok(())
}

pub fn export_auxiliary(motion: &[u8]) -> Result<AuxExport> {
    let photo = format::parse(motion)?;
    let video = photo.video(motion);
    let repair = remux::repair_auxiliary_duration(video)?;
    let (video_eff, repaired_info) = match &repair {
        Some(r) => (r.repaired.as_slice(), Some(r)),
        None => (video, None),
    };
    let videos = cover::video_tracks(video_eff)?;
    if videos.len() != 2 {
        return fail("辅助导出需要两个视频轨");
    }
    let mov = remux::remux_to_mov_video_only(video_eff, 1)?;
    // Single-track verification (Swift `exportAuxiliary`'s hash/size/pts
    // check, without its 1-tick-plus-1us drift allowance: Rust copies the
    // tables verbatim, so the shift is zero).
    let src_tables = mp4::sample_tables(video_eff)?;
    let out_tables = mp4::sample_tables(&mov)?;
    let src_videos: Vec<&mp4::TrackSamples> = src_tables
        .iter()
        .filter(|t| t.handler == "vide")
        .collect();
    let out_videos: Vec<&mp4::TrackSamples> = out_tables
        .iter()
        .filter(|t| t.handler == "vide")
        .collect();
    if src_videos.len() != 2 || out_videos.len() != 1 {
        return fail("辅助轨重封装校验失败：轨道数不同");
    }
    verify_single_track(src_videos[1], out_videos[0])?;
    // Content check: the remuxed samples must equal the source samples.
    let src_samples: Vec<&[u8]> = src_videos[1]
        .samples
        .iter()
        .map(|s| &video_eff[s.offset..s.offset + s.size])
        .collect();
    let out_samples: Vec<&[u8]> = out_videos[0]
        .samples
        .iter()
        .map(|s| &mov[s.offset..s.offset + s.size])
        .collect();
    if src_samples != out_samples {
        return fail("辅助轨重封装校验失败：样本内容不同");
    }
    Ok(AuxExport {
        mov,
        repaired: repaired_info.is_some(),
        repair_method: repaired_info.map(|r| r.method.to_string()),
        repaired_duration_seconds: repaired_info.map(|r| r.repaired_duration_seconds),
        original_sample_sha256: repaired_info.map(|r| r.original_sample_sha256.clone()),
    })
}

// ---------------------------------------------------------------------------
// candidate_times
// ---------------------------------------------------------------------------

/// One candidate frame, mirroring Swift `CoverCandidate`.
#[derive(Debug)]
pub struct CandidateTime {
    pub index: usize,
    pub timestamp_us: i64,
    pub time_source: String,
    pub sample_time_us: i64,
}

pub fn candidate_times(video: &[u8]) -> Result<(Vec<CandidateTime>, bool)> {
    let repair = remux::repair_auxiliary_duration(video)?;
    let (video_eff, repaired) = match &repair {
        Some(r) => (r.repaired.as_slice(), true),
        None => (video, false),
    };
    let videos = cover::video_tracks(video_eff)?;
    if videos.len() != 2 {
        return fail("候选帧枚举需要两个视频轨");
    }
    let (aux_track, aux_trak) = (&videos[1].0, &videos[1].1);
    let ts = aux_track.timescale as i64;
    if ts <= 0 {
        return fail("视频时间尺度无效");
    }
    let mut points: Vec<i64> = aux_track.samples.iter().map(|s| s.presentation_time).collect();
    points.sort();
    if points.is_empty() {
        return fail("辅助轨没有可唯一定位的高清候选帧");
    }
    if points.windows(2).any(|w| w[0] >= w[1]) {
        return fail("辅助轨没有可唯一定位的高清候选帧");
    }
    let recorded: Vec<Option<i64>> = match mp4::image_times(video_eff)? {
        Some(t) => t.high_resolution,
        None => Vec::new(),
    };
    if !recorded.is_empty() && recorded.len() != points.len() {
        return fail("高清候选时间描述符与辅助轨帧数不一致");
    }
    let mut out = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let point_us = ticks_to_us(*point, ts);
        let presented = match cover::media_to_movie_us(video_eff, aux_trak, ts, point_us)? {
            Some(p) => p,
            None => return fail("辅助轨编辑时间线无法唯一定位候选画面"),
        };
        if presented < 0 {
            return fail("高清候选时间无效");
        }
        let declared = recorded.get(index).copied().flatten();
        let (us, source) = match declared {
            Some(d) => {
                if d < 0 {
                    return fail("高清候选时间无效");
                }
                (d, "google-high-res-metadata")
            }
            None => (presented, "auxiliary-track-presentation"),
        };
        if us < 0 {
            return fail("高清候选时间无效");
        }
        out.push(CandidateTime {
            index,
            timestamp_us: us,
            time_source: source.to_string(),
            sample_time_us: point_us,
        });
    }
    Ok((out, repaired))
}
