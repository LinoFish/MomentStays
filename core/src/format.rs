//! Motion Photo parsing entry point (PhotoFormat.parse / PhotoHEIF.parse).
use crate::mp4::{boxes, image_times, top_level_types};
use crate::vendor::{locate, OPPO, VIVO};
use crate::xmp::{integer, packets, vendor_integer, HDR, HEADER};
use crate::{fail, heif, jpeg, sha256_hex, Error, Result};
use std::collections::HashSet;
use std::ops::Range;

pub const NOT_MOTION: &str = "不是受支持的动态照片";

#[derive(Clone, Debug)]
pub struct MotionPhoto {
    /// Static image bytes (JPEG rebuilt from marker-delimited images, or the HEIF before `mpvd`).
    pub still: Vec<u8>,
    /// Byte range of the embedded MP4 inside the source file.
    pub video: Range<usize>,
    pub timestamp: Option<i64>,
    pub timestamp_tag: Option<String>,
    pub ultra: bool,
    pub cover_frame: Option<i64>,
    pub format: String,
}

impl MotionPhoto {
    pub fn video<'a>(&self, source: &'a [u8]) -> &'a [u8] { &source[self.video.clone()] }
}

pub fn coding_hash(d: &[u8]) -> Result<String> {
    if heif::matches(d) { heif::coding_hash(d) } else { jpeg::coding_hash(d) }
}

pub fn auxiliary_hashes(d: &[u8]) -> Result<Vec<String>> {
    if heif::matches(d) { return Ok(vec![]); }
    Ok(jpeg::auxiliary(d)?.into_iter().skip(1).map(|r| sha256_hex(&d[r])).collect())
}

fn wrap_xmp(xml: &[u8]) -> Result<Vec<u8>> {
    let mut payload = HEADER.to_vec();
    payload.extend_from_slice(xml);
    let mut v = vec![255, 216];
    v.extend(jpeg::app(225, &payload)?);
    v.extend_from_slice(&[255, 217]);
    Ok(v)
}

pub fn parse(d: &[u8]) -> Result<MotionPhoto> {
    if heif::matches(d) { return parse_heif(d); }
    let pk = packets(d)?;
    let motion = integer(&pk, "MotionPhoto")?;
    let micro = integer(&pk, "MicroVideo")?;
    let identified = motion == Some(1) || micro == Some(1);
    if !(motion != Some(0) && (identified || (motion.is_none() && micro.is_none()))) { return fail(NOT_MOTION); }
    let mut lengths: Vec<i64> = Vec::new();
    for p in &pk {
        for (semantic, length) in p.items() {
            if semantic.as_deref() == Some("MotionPhoto") {
                match length.as_deref().and_then(crate::xmp::swift_int) {
                    Some(n) => lengths.push(n),
                    None => return fail("动态视频长度缺失"),
                }
            }
        }
    }
    if lengths.iter().collect::<HashSet<_>>().len() > 1 { return fail("动态视频长度冲突"); }
    let mut length = match lengths.first() { Some(l) => Some(*l), None => integer(&pk, "MicroVideoOffset")? };
    if length.is_none() { length = integer(&pk, "MediaDataOffset")?; }
    if length.is_none() { length = vendor_integer(&pk, OPPO, "MediaDataOffset")?; }
    let image_end = jpeg::image_end(d)?;
    let vendor = locate(d, image_end)?;
    let recovered = !identified && length.is_none() && vendor.is_none();
    if recovered {
        if image_end + 8 > d.len() || &d[image_end + 4..image_end + 8] != b"ftyp" { return fail(NOT_MOTION); }
        length = Some((d.len() - image_end) as i64);
    }
    let (start, end) = if let Some(v) = &vendor { (v.start, v.end) } else {
        let count = match length { Some(c) if c >= 16 && (c as i128) < d.len() as i128 => c as usize, _ => return fail("无法从 XMP 定位动态视频") };
        let start = d.len() - count;
        let end = match vendor_integer(&pk, OPPO, "VideoLength")? {
            Some(b) => {
                if b < 16 || (b as i128) > (d.len() - start) as i128 { return fail("OPPO 视频长度无效"); }
                start + b as usize
            }
            None => d.len(),
        };
        (start, end)
    };
    if start < image_end || end > d.len() || end < start || end - start < 16 { return fail("视频与 JPEG 图像范围重叠"); }
    let still = jpeg::static_only(&d[..start])?;
    let video = &d[start..end];
    if &video[4..8] != b"ftyp" { return fail("XMP 偏移没有指向 MP4 视频"); }
    let types: HashSet<String> = top_level_types(video, "MP4")?.into_iter().collect();
    if !types.contains("moov") || !types.contains("mdat") { return fail("视频缺少 moov/mdat"); }
    let mut time = integer(&pk, "MotionPhotoPresentationTimestampUs")?;
    let mut tag = "MotionPhotoPresentationTimestampUs";
    if time.map_or(true, |t| t < 0) {
        let old = integer(&pk, "MicroVideoPresentationTimestampUs")?;
        time = old.filter(|t| *t >= 0);
        tag = "MicroVideoPresentationTimestampUs";
    }
    if time.is_none() {
        if let Some(v) = vendor_integer(&pk, OPPO, "MotionPhotoPrimaryPresentationTimestampUs")? {
            if v >= 0 { time = Some(v); tag = "OpCamera:MotionPhotoPrimaryPresentationTimestampUs"; }
        }
    }
    if recovered {
        match image_times(video)?.and_then(|t| t.primary) {
            Some(t) => { time = Some(t); tag = "MP4:motionphoto-image-meta"; }
            None => return fail("原片缺少主 XMP，内嵌视频也未记录有效封面时间，无法无损恢复封面位置"),
        }
    }
    let mut ultra = pk.iter().any(|p| !p.values(HDR, "Version").is_empty());
    let images = jpeg::auxiliary(&still)?;
    if !ultra {
        for r in images.iter().skip(1) {
            if packets(&still[r.clone()])?.iter().any(|p| !p.values(HDR, "Version").is_empty()) { ultra = true; }
        }
    }
    if ultra {
        if images.len() < 2 { return fail("声明为 Ultra HDR，但缺少 MPF 增益图"); }
        for r in images.iter().skip(1) { jpeg::segments(&still[r.clone()])?; }
    }
    let oppo_len = vendor_integer(&pk, OPPO, "VideoLength")?;
    let is_vivo = pk.iter().any(|p| !p.values(VIVO, "VMotionPhotoVersion").is_empty());
    let frame = vendor.as_ref().and_then(|v| v.frame);
    let format = match &vendor {
        Some(v) => v.format.to_string(),
        None if oppo_len.is_some() => "oppo".into(),
        None if is_vivo => "vivo-v2".into(),
        None => "google-compatible".into(),
    };
    Ok(MotionPhoto {
        still, video: start..end, timestamp: time,
        timestamp_tag: if frame.is_none() { time.map(|_| tag.to_string()) } else { Some("Huawei:frame-index".into()) },
        ultra, cover_frame: frame, format,
    })
}

fn parse_heif(d: &[u8]) -> Result<MotionPhoto> {
    let vendor = locate(d, 0)?;
    let mut p = 0;
    let mut mpvd: Option<(usize, usize)> = None;
    while p < d.len() {
        if d.len() - p < 8 { return fail("HEIC 顶层 box 被截断"); }
        let mut size = crate::binary::be(d, p, 4)?;
        let mut header = 8;
        if size == 1 { size = crate::binary::be(d, p + 8, 8)?; header = 16; }
        if size < header || size > d.len() - p { return fail("HEIC 顶层 box 长度无效"); }
        if &d[p + 4..p + 8] == b"mpvd" { mpvd = Some((p, p + size)); break; }
        p += size;
    }
    let (boundary, mpvd_end) = match mpvd { Some(v) => v, None => return fail(NOT_MOTION) };
    let consistent = mpvd_end == d.len()
        && vendor.as_ref().map_or(true, |v| v.format == "samsung-sef" && v.start == boundary + 8 && v.end <= mpvd_end);
    if !consistent { return fail("三星 HEIC 的 mpvd/SEF 视频边界不一致"); }
    let still = &d[..boundary];
    let l = heif::layout(still)?;
    let mut xmps = Vec::new();
    for item in l.items.iter().filter(|i| i.ty == "mime") {
        let w = wrap_xmp(&heif::item_bytes(still, item))?;
        if integer(&packets(&w)?, "MotionPhoto")? == Some(1) { xmps.push(item); }
    }
    if xmps.len() != 1 { return fail("HEIC 缺少唯一动态照片 XMP 项目"); }
    let pk = packets(&wrap_xmp(&heif::item_bytes(still, xmps[0]))?)?;
    if integer(&pk, "MotionPhoto")? != Some(1) { return fail(NOT_MOTION); }
    let mut time = integer(&pk, "MotionPhotoPresentationTimestampUs")?;
    if time.map_or(true, |t| t < 0) { time = integer(&pk, "MicroVideoPresentationTimestampUs")?; }
    if time.map_or(false, |t| t < 0) { time = None; }
    let range = match &vendor { Some(v) => v.start..v.end, None => boundary + 8..mpvd_end };
    let video = &d[range.clone()];
    let top = boxes(video, 0, video.len())?;
    if top.first().map_or(true, |b| !b.is(b"ftyp")) || !top.iter().any(|b| b.is(b"moov")) || !top.iter().any(|b| b.is(b"mdat")) {
        return fail("HEIC 动态视频无效");
    }
    heif::exif(still)?;
    Ok(MotionPhoto {
        still: still.to_vec(), video: range, timestamp: time,
        timestamp_tag: time.map(|_| "MotionPhotoPresentationTimestampUs".to_string()),
        ultra: false, cover_frame: None,
        format: if vendor.is_none() { "google-heic".into() } else { "samsung-heic-sef".into() },
    })
}

#[allow(dead_code)]
fn _unused(_: Error) {}
