//! Apple Live Photo -> Android Motion Photo (Swift `AndroidExport` and
//! `AndroidHDR`). Pairing, the MOV -> MP4 remux, vendor assembly and the
//! read-back checks are byte-level here; decoding or re-encoding a still
//! goes through [`Native`].

use crate::engine::batch::{self, Stage};
use crate::engine::options::{AndroidOptions, ConversionOptions};
use crate::engine::paths::{self, extension, stem};
use crate::engine::scanrow::ScanRow;
use crate::engine::still::{self, convert_gain};
use crate::engine::{failure, hash_file, read, Native, StopToken, Timings};
use crate::live::{self, TimeDomain};
use crate::model::{self, Cover, Vendor};
use crate::mp4::Movie;
use crate::{apple_hdr, format, heif, heifwrite, jpeg, live2motion, remux, sha256_hex, xmp};
use crate::{Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const REVISION: &str = "reverse-native-1.2.1";
const VIDEO_EXTENSIONS: [&str; 2] = ["mov", "mp4"];

/// A Live Photo still and its movie, with the cover frame.
#[derive(Clone, Debug)]
pub struct Pair {
    pub still: PathBuf,
    pub movie: PathBuf,
    pub identifier: String,
    /// Cover frame's media time, microseconds (rounded).
    pub cover_us: i64,
    /// Cover frame's media time, seconds (exact ratio).
    pub cover_seconds: f64,
    /// Presentation-sorted frame index of the cover.
    pub frame: usize,
}

pub fn image_identifier(still: &[u8]) -> Result<String> {
    match live::still_content_identifier(still) {
        Ok(Some(id)) if !id.is_empty() => Ok(id),
        _ => failure("照片缺少 Apple 实况内容标识；请从「照片」导出完整原片与配对视频"),
    }
}

pub fn video_identifier(movie: &[u8]) -> Result<String> {
    match remux::read_content_identifier(movie) {
        Ok(Some(id)) => Ok(id),
        _ => failure("视频缺少唯一实况内容标识"),
    }
}

fn siblings(of: &Path, exts: &[&str]) -> Result<Vec<PathBuf>> {
    let dir = of.parent().unwrap_or(Path::new("."));
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| crate::engine::io_error(dir, e))? {
        let Ok(e) = e else { continue };
        let p = e.path();
        if exts.contains(&extension(&p).as_str()) && p.is_file() {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Swift `AndroidExport.pair`: the sibling movie with the same content
/// identifier, and the still-image-time marker matched to a video frame.
pub fn pair(still_path: &Path, domain: &str) -> Result<Pair> {
    let id = image_identifier(&read(still_path)?)?;
    let mut matching = Vec::new();
    for candidate in siblings(still_path, &VIDEO_EXTENSIONS)? {
        if let Ok(bytes) = std::fs::read(&candidate) {
            if video_identifier(&bytes).ok().as_deref() == Some(id.as_str()) {
                matching.push((candidate, bytes));
            }
        }
    }
    if matching.len() != 1 {
        return failure(if matching.is_empty() {
            "未找到内容标识匹配的视频；仅分享静态图不能恢复实况"
        } else {
            "多个视频使用同一内容标识，请将正确配对单独放入文件夹"
        });
    }
    let (movie_path, mov) = matching.remove(0);
    let parsed = Movie::parse(&mov)?;
    let marker = match live2motion::mett_cover_time_in(&mov, &parsed) {
        Ok(Some(t)) => t,
        _ => return failure("实况视频缺少唯一有效 still-image-time 封面标记"),
    };
    let video = parsed.video(0).map_err(|_| Error::new("实况缺少视频轨"))?;
    if video.samples.samples.is_empty() {
        return failure("视频没有有效画面");
    }
    let domain = match domain {
        "media" => TimeDomain::Media,
        "presentation" => TimeDomain::Presentation,
        _ => TimeDomain::Auto,
    };
    let cover =
        live::resolve_cover_in(&mov, &parsed, Cover::Time(marker), domain, 0).map_err(|_| {
            Error::new("封面时间越界或编辑时间线存在歧义；未使用中点猜测，请检查高级时间域选项")
        })?;
    let sorted = crate::cover::sorted_presentation(&video.samples);
    let pts = sorted[cover.frame].0;
    let ts = video.samples.timescale.max(1) as f64;
    Ok(Pair {
        still: still_path.to_path_buf(),
        movie: movie_path,
        identifier: id,
        cover_us: cover.frame_time_us,
        cover_seconds: pts as f64 / ts,
        frame: cover.frame,
    })
}

/// Swift `AndroidExport.selection`: a chosen movie stands for its still.
pub fn selection(sources: &[PathBuf]) -> Result<(PathBuf, Vec<PathBuf>)> {
    let mut images = Vec::new();
    for source in sources {
        if VIDEO_EXTENSIONS.contains(&extension(source).as_str()) {
            let id = video_identifier(&read(source)?)?;
            let matching: Vec<PathBuf> = siblings(source, &paths::PHOTO_EXTENSIONS)?
                .into_iter()
                .filter(|p| {
                    std::fs::read(p)
                        .ok()
                        .and_then(|b| image_identifier(&b).ok())
                        .as_deref()
                        == Some(id.as_str())
                })
                .collect();
            if matching.len() != 1 {
                return failure("所选视频缺少唯一匹配的静态照片");
            }
            images.extend(matching);
        } else {
            images.extend(paths::files(source)?);
        }
    }
    if images.is_empty() {
        return failure("未找到完整实况照片");
    }
    paths::selection(&images)
}

/// Swift `AndroidExport.scanFile`.
pub fn scan_file(path: &Path, relative: &str) -> ScanRow {
    let result = (|| -> Result<(Pair, bool)> {
        let p = pair(path, "auto")?;
        let has_map = apple_hdr::read(&read(path)?)
            .map(|r| r.maker.has_33)
            .unwrap_or(false);
        Ok((p, has_map))
    })();
    match result {
        Ok((p, has_map)) => ScanRow {
            source: path.to_path_buf(),
            relative: relative.to_string(),
            is_motion: true,
            hdr: if has_map { "Apple HDR" } else { "原始图像" }.into(),
            cover: format!("{:.6} 秒", p.cover_seconds),
            detail: "实况内容标识匹配；封面来自视频 timed metadata".into(),
            manufacturer: "苹果".into(),
            manufacturer_detail: "配对依据：Apple ContentIdentifier".into(),
            ..ScanRow::default()
        },
        Err(e) => ScanRow {
            source: path.to_path_buf(),
            relative: relative.to_string(),
            is_motion: false,
            hdr: "—".into(),
            cover: "—".into(),
            detail: e.0,
            manufacturer: "苹果 / 待检查".into(),
            ..ScanRow::default()
        },
    }
}

/// Swift `AndroidExport.remux`: MOV -> MP4, samples copied verbatim.
pub fn remux(mov: &[u8]) -> Result<(Vec<u8>, Value)> {
    let parsed = Movie::parse(mov)?;
    let mp4 = remux::remux_mp4(mov, &parsed)?;
    let out = Movie::parse(&mp4)?;
    let tracks: Vec<Value> = out
        .tracks
        .iter()
        .filter(|t| t.samples.handler == "vide")
        .chain(out.tracks.iter().filter(|t| t.samples.handler == "soun"))
        .map(|t| {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            for s in &t.samples.samples {
                h.update(&mp4[s.offset..s.offset + s.size]);
            }
            json!({
                "type": t.samples.handler,
                "sha256": crate::hex(&h.finalize()),
                "samples": t.samples.samples.len(),
                "max_pts_shift_seconds": 0.0,
            })
        })
        .collect();
    Ok((
        mp4,
        json!({"tracks": tracks, "video_reencoded": false, "audio_reencoded": false}),
    ))
}

fn vendor_of(name: &str) -> Vendor {
    match name {
        "samsung" => Vendor::Samsung,
        "huawei" => Vendor::Huawei,
        "oppo" => Vendor::Oppo,
        "vivo" => Vendor::Vivo,
        // google, xiaomi and honor share the Google layout.
        _ => Vendor::Google,
    }
}

/// Swift `AndroidExport.assemble`.
pub fn assemble(
    still: &[u8],
    video: &[u8],
    options: &AndroidOptions,
    time: i64,
    frame: i64,
) -> Result<Vec<u8>> {
    let vendor = vendor_of(&options.vendor);
    let plan = if heif::matches(still) {
        heifwrite::plan_heic(still, video, vendor, time)?
    } else {
        let cleaned = still::live_still(still, "", false)?;
        model::plan_swift(&cleaned, video, time, frame, vendor)?
    };
    let out = plan.concat(video);
    plan.verify(&out, video)?;
    Ok(out)
}

fn is_ultra(original: &[u8]) -> Result<bool> {
    let has = |d: &[u8]| -> Result<bool> {
        Ok(xmp::packets(d)?
            .iter()
            .any(|p| !p.values(xmp::HDR, "Version").is_empty()))
    };
    if has(original)? {
        return Ok(true);
    }
    for r in jpeg::auxiliary(original)?.iter().skip(1) {
        if has(&original[r.clone()]).unwrap_or(false) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Swift `AndroidHDR.prepare`: the still to embed and its HDR report.
pub fn prepare_still(
    native: &dyn Native,
    path: &Path,
    options: &AndroidOptions,
) -> Result<(Vec<u8>, Value)> {
    let q_ok = |q: f64| q > 0.0 && q <= 1.0;
    if !(q_ok(options.image_quality) && q_ok(options.gain_quality)) {
        return failure("JPEG 质量需在 1–100% 之间");
    }
    let original = read(path)?;
    let heic = heif::matches(&original);
    if options.container == "heic" {
        if !heic {
            return failure("保留原编码的 HEIC 输出需要 HEIC 原图；未将 JPEG 再编码成 HEIC");
        }
        heif::layout(&original)?;
        return Ok((
            original,
            json!({"status": "preserved", "input": "heic", "reason": "HEIC 全部原始图像/增益图编码及属性保留；Apple 私有 HDR 的安卓渲染兼容性未保证"}),
        ));
    }
    let mut base = if !heic && options.preserve_jpeg {
        jpeg::static_only(&original)?
    } else {
        native.reencode_jpeg(path, options.image_quality)?
    };
    if !heic {
        let maps = jpeg::auxiliary(&original)?;
        let ultra = is_ultra(&original)?;
        if ultra && options.preserve_jpeg {
            return Ok((
                base,
                json!({"status": "preserved", "input": "ultra-hdr", "reason": "已有 Ultra HDR 原始 JPEG 和增益图保留；质量设置仅在重新编码时生效"}),
            ));
        }
        if ultra {
            if maps.len() != 2 {
                return failure("Ultra HDR 增益图布局不受支持");
            }
            base = still::pack_gain_map(&base, &original[maps[1].clone()])?;
            return Ok((
                base,
                json!({"status": "preserved-gainmap", "reason": "主图按指定质量重编码，已有 Ultra HDR 增益图与参数保留"}),
            ));
        }
        if let (Some(legacy), _) = apple_hdr::legacy(&original)? {
            let Some(image) = native.decode_gray_jpeg(&legacy.gain)? else {
                return failure("Apple HDR 需要 8 位单通道增益图");
            };
            let gain = native
                .encode_gray_jpeg(&convert_gain(&image, legacy.stops)?, options.gain_quality)?;
            return Ok((
                still::ultra(&base, &gain, legacy.stops)?,
                json!({"status": "converted", "input": "apple-hdr", "output": "ultra-hdr", "reason": "已有 Apple 增益图按 Rec.709 模型转为 Ultra HDR"}),
            ));
        }
        if maps.len() > 1 {
            return failure("原始 JPEG 含未知辅助图/HDR 参数，未丢弃或推测");
        }
        return Ok((base, json!({"status": "sdr", "reason": "SDR 不生成增益图"})));
    }
    let Some(gain) = native.heic_gain_map(path)? else {
        heif::layout_lenient(&original)?;
        if apple_hdr::read(&original)?.status != apple_hdr::Status::None {
            return failure("此 HEIC HDR 不能在 macOS 12 安全提取；请保留 HEIC 原编码，或提供支持的 Apple HDR 增益图");
        }
        return Ok((
            base,
            json!({"status": "sdr", "reason": "未检测到可恢复 HDR；不生成增益图"}),
        ));
    };
    if gain.versions.is_empty() {
        return failure("HEIC 增益图缺少可验证的 Apple HDR 版本；请保留原编码");
    }
    if gain.versions != ["65536"] && gain.versions != ["131072"] {
        return failure("此 HEIC 增益图不是支持的 Apple HDR；请保留 HEIC 原编码");
    }
    let maker_stops = apple_hdr::stops(&original)?;
    if gain.versions == ["131072"] {
        let report = apple_hdr::read(&original)?;
        let tmap = report.heic.as_ref().is_some_and(|h| h.iso_tmap);
        let ok = gain.headrooms.len() == 1
            && gain.headrooms[0].is_finite()
            && gain.headrooms[0] > 1.0
            && maker_stops.is_some_and(|s| (gain.headrooms[0].log2() - s).abs() < 0.002)
            && !tmap;
        if !ok {
            return failure("新版 Apple HDR 参数不能验证为已支持模型；请保留 HEIC 原编码");
        }
    }
    let incomplete = || Error::new("Apple HEIC HDR 增益图/亮度参数不完整，未降为 SDR 或猜测 HDR");
    let stops = maker_stops.ok_or_else(incomplete)?;
    let image = gain.image.ok_or_else(incomplete)?;
    if !(1..=16384).contains(&image.width) || !(1..=16384).contains(&image.height) {
        return Err(incomplete());
    }
    let gain_jpeg = native.encode_gray_jpeg(&convert_gain(&image, stops)?, options.gain_quality)?;
    Ok((
        still::ultra(&base, &gain_jpeg, stops)?,
        json!({"status": "converted", "input": "apple-hdr", "output": "ultra-hdr", "reason": "Apple HEIC 原始增益图转 Ultra HDR；主 JPEG 和增益 JPEG 使用独立质量"}),
    ))
}

/// Swift `AndroidExport.convert`.
pub fn convert(
    native: &dyn Native,
    source: &Path,
    target: &Path,
    options: &ConversionOptions,
    token: &StopToken,
    overwrite: bool,
    timings: &Timings,
) -> Result<Value> {
    let config = &options.android;
    if ![
        "google", "xiaomi", "samsung", "huawei", "honor", "oppo", "vivo",
    ]
    .contains(&config.vendor.as_str())
        || !["jpeg", "heic"].contains(&config.container.as_str())
    {
        return failure("安卓目标厂商或容器无效");
    }
    if config.container == "heic"
        && !["google", "xiaomi", "samsung"].contains(&config.vendor.as_str())
    {
        return failure("此厂商的 HEIC 动态封装尚无可验证方案；请选择 JPEG");
    }
    let source_hash = timings.measure("source_hash", || hash_file(source))?;
    if !overwrite && target.exists() {
        let previous = batch::verify(target)?;
        if previous["implementation"].as_str() != Some(REVISION) {
            return failure("已有输出使用旧版 HDR 封装，请选择覆盖以修复");
        }
        if config.vendor == "honor"
            && previous["motion_layout"].as_str() != Some("google-compatible")
        {
            return failure("已有荣耀输出使用旧版私有尾标，可能导致视频方向错误；请选择覆盖以修复");
        }
        let same_movie = match (
            previous["paired_video_source"].as_str(),
            previous["paired_video_sha256"].as_str(),
        ) {
            (Some(m), Some(h)) => hash_file(Path::new(m)).ok().as_deref() == Some(h),
            _ => false,
        };
        if previous["source_sha256"].as_str() != Some(source_hash.as_str())
            || !crate::engine::same_json(&previous["options"], &options.json())
            || !same_movie
        {
            return failure("已有输出的输入或设置不同，请选择覆盖或跳过");
        }
        return Ok(
            json!({"status": "resumed", "source": source.to_string_lossy(), "output": target.to_string_lossy(), "detail": "已验证安卓动态照片，跳过重复转换"}),
        );
    }
    let paired = pair(source, &options.time_domain)?;
    let video_hash = hash_file(&paired.movie)?;
    let stage = Stage::new(target)?;
    if token.is_cancelled() {
        return failure("已停止");
    }
    let (video, mut timing) = remux(&read(&paired.movie)?)?;
    let time = paired.cover_us;
    let (still_bytes, hdr) =
        timings.measure("image_and_hdr", || prepare_still(native, source, config))?;
    let result = assemble(&still_bytes, &video, config, time, paired.frame as i64)?;
    let decoded = format::parse(&result)?;
    if decoded.video(&result) != video.as_slice()
        || format::coding_hash(&decoded.still)? != format::coding_hash(&still_bytes)?
    {
        return failure("安卓封装读回校验失败：图像或视频数据不同");
    }
    if config.vendor == "huawei" {
        if decoded.cover_frame != Some(paired.frame as i64) {
            return failure("厂商封面帧号校验失败");
        }
    } else if decoded.timestamp != Some(time) {
        return failure("安卓封面时间校验失败");
    }
    let name = format!(
        "{}.MP.{}",
        stem(source),
        if config.container == "heic" {
            "heic"
        } else {
            "jpg"
        }
    );
    let file = stage.path().join(&name);
    crate::engine::write(&file, &result)?;
    let _ = crate::engine::copy_mtime(source, &file);
    timing["source_still_image_time_seconds"] = json!(paired.cover_seconds);
    timing["cover_timestamp_us"] = json!(time);
    timing["cover_frame_index"] = json!(paired.frame);
    let mut warnings = vec![if config.container == "heic" {
        "HEIC 原始 HDR 编码已保留；目标安卓厂商对 Apple 私有 HDR 的显示兼容性需真机验证"
    } else {
        "厂商封装已通过本地解析校验；不同手机固件识别与 HDR 显示仍需真机验证"
    }
    .to_string()];
    if config.vendor == "honor" {
        warnings.push("荣耀格式仅作备用，新款荣耀推荐选择 Google / 通用。此兼容性针对留刻导出的文件，不代表能识别 Google 相机原片".into());
    }
    let manifest = json!({
        "version": "1.2.1",
        "implementation": REVISION,
        "source": source.to_string_lossy(),
        "source_sha256": source_hash,
        "paired_video_source": paired.movie.to_string_lossy(),
        "paired_video_sha256": video_hash,
        "options": options.json(),
        "identifier": paired.identifier,
        "motion_layout": if config.vendor == "honor" { "google-compatible" } else { config.vendor.as_str() },
        "output_files": {"motionphoto": name},
        "output_sha256": {name.clone(): sha256_hex(&result)},
        "timing": timing,
        "hdr": hdr,
        "warnings": warnings,
    });
    crate::engine::write_json(&manifest, &stage.path().join("manifest.json"))?;
    if hash_file(&paired.movie)? != video_hash {
        return failure("配对视频在转换期间发生变化，未发布");
    }
    timings.measure("validation_and_publish", || {
        batch::publish(
            &stage,
            target,
            source,
            &source_hash,
            token,
            overwrite,
            options.export_data_files,
        )
    })?;
    Ok(json!({
        "status": "converted",
        "source": source.to_string_lossy(),
        "output": target.to_string_lossy(),
        "detail": format!("已导出 {} {} 动态照片；封面 {} 秒", config.vendor, config.container.to_uppercase(), crate::engine::still::swift_double(time as f64 / 1_000_000.0)),
        "hdr": hdr,
        "timing": timing,
        "warnings": warnings,
    }))
}
