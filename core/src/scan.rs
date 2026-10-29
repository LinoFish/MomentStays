//! Scan rows aligned with Swift `BatchEngine.scanFile` and the
//! `NativeDriver` `scan` output (ROADMAP item 4).
//!
//! A row carries the same fields the Swift side shows per file:
//! motion flag, HDR class, cover text, detail text, vendor label with
//! its detail, and the confirmed-static flag for non-motion files.
//! The `momentstays scan` subcommand is intentionally untouched: it
//! mirrors the Python CLI's Google-only scan output, while this module
//! (used by `momentstays-convert scan`) follows the Swift app's
//! all-vendor scan semantics.
//!
//! Deliberate differences from Swift, all documented here rather than
//! papered over:
//!
//! * `confirmed_static` replaces the `CGImageSource` decode check with
//!   header dimension parsing (JPEG SOF / HEIC `ispe`). A file whose
//!   headers are intact but whose pixels do not decode would differ;
//!   that comparison is a Needs Mac check.
//! * Unreadable files become error rows with the OS error text; Swift
//!   reports the platform `NSError` text instead, which cannot match
//!   on Linux.
//! * `identify` and the HDR order below are otherwise literal ports,
//!   including Swift's quirks: a classifier error turns a parsed
//!   motion photo into an error row, and HEIC format names fall back
//!   to the raw string because Swift's format-name table has no HEIC
//!   entries.

use crate::binary::be;
use crate::format::{self, NOT_MOTION};
use crate::heif;
use crate::hdr;
use crate::jpeg;
use crate::mp4;
use crate::pyjpeg;
use crate::pymotion::{self, XMP_HDR};
use crate::vendor;
use crate::xmp;
use crate::Result;

/// Vendor label plus the detail text Swift shows under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub detail: String,
}

fn format_name(format: &str) -> &str {
    match format {
        "huawei-honor" => "华为/荣耀 LIVE_",
        "samsung-sef" => "三星 SEF",
        "oppo" => "OPPO/OnePlus",
        "vivo-v2" => "vivo V2",
        "google-compatible" => "Google 兼容",
        other => other,
    }
}

/// Port of Swift `PhotoVendor.identify`: EXIF Make/Model brand table
/// (prefix match on the uppercased Make), format fallback, unknown
/// fallback. HEIF inputs are re-read through their Exif item wrapped
/// as a one-segment JPEG, exactly like Swift; identification never
/// fails, it degrades to the fallbacks.
pub fn identify(data: &[u8], format: &str) -> Identity {
    if heif::matches(data) {
        if let Ok(exif) = heif::exif(data) {
            if let Ok(len) = u16::try_from(exif.len() + 2) {
                let mut fake = vec![0xFF, 0xD8, 0xFF, 0xE1];
                fake.extend(len.to_be_bytes());
                fake.extend(&exif);
                fake.extend([0xFF, 0xD9]);
                return identify(&fake, format);
            }
        }
    }
    let (make, model) = vendor::make_model(data);
    let normalized = make.to_uppercase();
    let name = format_name(format);
    let camera = [make.as_str(), model.as_str()]
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" / ");
    const BRANDS: [(&str, &[&str]); 10] = [
        ("荣耀", &["HONOR", "荣耀"]),
        ("华为", &["HUAWEI", "华为"]),
        ("小米", &["XIAOMI", "REDMI", "POCO", "小米", "红米"]),
        ("三星", &["SAMSUNG", "三星"]),
        ("OPPO", &["OPPO"]),
        ("OnePlus", &["ONEPLUS", "一加"]),
        ("realme", &["REALME", "真我"]),
        ("vivo", &["VIVO", "IQOO"]),
        ("Google", &["GOOGLE"]),
        ("Apple", &["APPLE"]),
    ];
    if let Some((brand, _)) = BRANDS
        .iter()
        .find(|(_, aliases)| aliases.iter().any(|a| normalized.starts_with(a)))
    {
        return Identity {
            name: brand.to_string(),
            detail: format!("厂商依据：原片 EXIF Make；{camera}\n动态格式：{name}"),
        };
    }
    let fallback = match format {
        "huawei-honor" => Some("华为/荣耀"),
        "samsung-sef" => Some("三星"),
        "oppo" => Some("OPPO/OnePlus"),
        "vivo-v2" => Some("vivo"),
        _ => None,
    };
    if let Some(brand) = fallback {
        return Identity {
            name: brand.to_string(),
            detail: format!(
                "厂商依据：动态照片格式（非机身识别）\nEXIF：{}\n动态格式：{name}",
                if camera.is_empty() { "未记录" } else { &camera }
            ),
        };
    }
    Identity {
        name: "未知".to_string(),
        detail: format!(
            "未找到可确认的厂商字段\nEXIF：{}\n动态格式：{}。Google 兼容格式不代表 Google 厂商。",
            if camera.is_empty() { "未记录" } else { &camera },
            if name.is_empty() { "未知" } else { name }
        ),
    }
}

/// Port of Swift `PhotoHDR.hasISO`: an APP2 segment carrying the
/// ISO 21496-1 urn, in the still or any MPF auxiliary image.
pub(crate) fn has_iso(still: &[u8]) -> Result<bool> {
    const ISO_URN: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
    let mut images: Vec<&[u8]> = vec![still];
    for r in pyjpeg::mpf_images(still)?.iter().skip(1) {
        images.push(&still[r.clone()]);
    }
    for image in images {
        for s in pyjpeg::segments(image)? {
            if s.marker == 0xE2 && s.payload(image).starts_with(ISO_URN) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Port of Swift `scanFile`'s HDR chain, in order: HEIC, Ultra HDR,
/// Honor private EHDR, ISO 21496 gain map, legacy Apple gain map,
/// otherwise SDR. A classifier error propagates, like Swift's `try`,
/// and turns the row into an error row (see `scan_row`).
pub fn classify_hdr(still: &[u8], ultra: bool) -> Result<&'static str> {
    if heif::matches(still) {
        return Ok("HEIC（原样保留）");
    }
    if ultra {
        return Ok("Ultra HDR");
    }
    let ranges = jpeg::image_ranges(still)?;
    let image_end = ranges.iter().map(|r| r.end).max().unwrap_or(still.len());
    if hdr::honor_private(still, image_end)?.is_some() {
        return Ok("荣耀私有 HDR");
    }
    if has_iso(still)? {
        return Ok("ISO HDR");
    }
    if pymotion::legacy_apple(still)?.0.is_some() {
        return Ok("旧 Apple HDR");
    }
    Ok("SDR / 其他")
}

/// Port of Swift `scanFile`'s cover text: frame index first, then
/// microsecond time at six decimals, then unrecorded.
pub fn cover_text(cover_frame: Option<i64>, timestamp_us: Option<i64>) -> String {
    if let Some(n) = cover_frame {
        format!("原始第 {n} 帧（从0计）")
    } else if let Some(us) = timestamp_us {
        format!("{:.6} 秒", us as f64 / 1e6)
    } else {
        "未记录".to_string()
    }
}

fn detail_text(hdr: &str, cover: &str, format: &str) -> String {
    if hdr == "荣耀私有 HDR" {
        "荣耀私有 HDR：保留静态扩展，暂不能转为 Ultra HDR；Apple 设备显示 SDR".to_string()
    } else if cover == "未记录" {
        "缺少封面时间；默认不转换".to_string()
    } else {
        format!("{format}：等待转换")
    }
}

const MOTION_CLAIMS: [&str; 5] = [
    "MotionPhoto",
    "MicroVideo",
    "VideoLength",
    "MediaDataOffset",
    "MotionPhoto_Data",
];

fn has_motion_claim(packets: &[xmp::Packet]) -> bool {
    packets
        .iter()
        .any(|p| MOTION_CLAIMS.iter().any(|c| p.xml.contains(c)))
}

/// JPEG dimensions from the first SOF marker (the `CGImageSource`
/// dimension half of Swift's check; see the module docs).
fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let segs = pyjpeg::segments(data).ok()?;
    for s in &segs {
        let m = s.marker;
        let is_sof = (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC;
        if !is_sof {
            continue;
        }
        let pl = s.payload(data);
        if pl.len() < 5 {
            return None;
        }
        let h = u16::from_be_bytes([pl[1], pl[2]]) as u32;
        let w = u16::from_be_bytes([pl[3], pl[4]]) as u32;
        return Some((w, h));
    }
    None
}

/// HEIC dimensions from the `ispe` box (meta/iprp/ipco/ispe).
fn heif_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let top = mp4::boxes(data, 0, data.len()).ok()?;
    let meta = top.iter().find(|b| b.is(b"meta"))?;
    let inner = mp4::boxes(data, meta.payload + 4, meta.end).ok()?;
    let iprp = inner.iter().find(|b| b.is(b"iprp"))?;
    let inner = mp4::boxes(data, iprp.payload, iprp.end).ok()?;
    let ipco = inner.iter().find(|b| b.is(b"ipco"))?;
    let inner = mp4::boxes(data, ipco.payload, ipco.end).ok()?;
    let ispe = inner.iter().find(|b| b.is(b"ispe"))?;
    if ispe.end - ispe.payload < 12 {
        return None;
    }
    let w = be(data, ispe.payload + 4, 4).ok()? as u32;
    let h = be(data, ispe.payload + 8, 4).ok()? as u32;
    Some((w, h))
}

/// Port of Swift `BatchEngine.confirmedStatic`, minus the pixel-decode
/// half (see the module docs): a complete still container with no
/// motion claims and positive dimensions.
pub fn confirmed_static(data: &[u8]) -> bool {
    if heif::matches(data) {
        let Ok(layout) = heif::layout(data) else {
            return false;
        };
        if !layout.boxes.iter().all(|b| {
            b.is(b"ftyp") || b.is(b"meta") || b.is(b"mdat") || b.is(b"free") || b.is(b"skip")
        }) {
            return false;
        }
        for item in layout.items.iter().filter(|i| i.ty == "mime") {
            let bytes = heif::item_bytes(data, item);
            let Ok(len) = u16::try_from(XMP_HDR.len() + bytes.len() + 2) else {
                return false;
            };
            let mut fake = vec![0xFF, 0xD8, 0xFF, 0xE1];
            fake.extend(len.to_be_bytes());
            fake.extend(XMP_HDR);
            fake.extend(&bytes);
            fake.extend([0xFF, 0xD9]);
            let Ok(packets) = xmp::packets(&fake) else {
                return false;
            };
            if has_motion_claim(&packets) {
                return false;
            }
        }
        return heif_dimensions(data).map_or(false, |(w, h)| w > 0 && h > 0);
    }
    let Ok(ranges) = jpeg::image_ranges(data) else {
        return false;
    };
    if ranges.last().map_or(true, |r| r.end != data.len()) {
        return false;
    }
    let Ok(packets) = xmp::packets(data) else {
        return false;
    };
    if has_motion_claim(&packets) {
        return false;
    }
    jpeg_dimensions(data).map_or(false, |(w, h)| w > 0 && h > 0)
}

/// One Swift-aligned scan row. Never fails: every failure mode becomes
/// an error row, like Swift's `scanFile` catch block.
#[derive(Clone, Debug)]
pub struct ScanRow {
    pub is_motion: bool,
    pub hdr: String,
    pub cover: String,
    pub detail: String,
    pub manufacturer: String,
    pub manufacturer_detail: String,
    pub confirmed_static: bool,
}

fn error_row(data: &[u8], message: &str) -> ScanRow {
    let maker = identify(data, "");
    let is_static = message == NOT_MOTION && confirmed_static(data);
    ScanRow {
        is_motion: false,
        hdr: "—".to_string(),
        cover: "—".to_string(),
        detail: if is_static {
            "静态照片，不含动态内容，不参与转换".to_string()
        } else {
            message.to_string()
        },
        manufacturer: maker.name,
        manufacturer_detail: maker.detail,
        confirmed_static: is_static,
    }
}

pub fn scan_row(data: &[u8]) -> ScanRow {
    match format::parse(data) {
        Ok(photo) => {
            let maker = identify(&photo.still, &photo.format);
            match classify_hdr(&photo.still, photo.ultra) {
                Ok(hdr) => {
                    let cover = cover_text(photo.cover_frame, photo.timestamp);
                    let detail = detail_text(hdr, &cover, &photo.format);
                    ScanRow {
                        is_motion: true,
                        hdr: hdr.to_string(),
                        cover,
                        detail,
                        manufacturer: maker.name,
                        manufacturer_detail: maker.detail,
                        confirmed_static: false,
                    }
                }
                Err(e) => error_row(data, &e.to_string()),
            }
        }
        Err(e) => error_row(data, &e.0),
    }
}
