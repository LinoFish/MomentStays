//! Huawei/Honor LIVE_ tail and Samsung SEF directory (VendorMotion), plus EXIF Make identification.
use crate::binary::{be, le};
use crate::{fail, jpeg, tiff::Tiff, Result};
use std::ops::Range;

pub const OPPO: &str = "http://ns.oplus.com/photos/1.0/camera/";
pub const VIVO: &str = "http://ns.vivo.com/photos/1.0/camera/";

#[derive(Clone, Debug, PartialEq)]
pub struct Location { pub start: usize, pub end: usize, pub frame: Option<i64>, pub format: &'static str }

fn trim(b: &[u8]) -> String { String::from_utf8_lossy(b).trim().to_string() }
fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool { a.start < b.end && b.start < a.end }

fn huawei_frame(first: &str) -> Option<i64> {
    // ^v[1-6]_f[0-9]+$
    let b = first.as_bytes();
    if b.len() < 5 || b[0] != b'v' || !(b'1'..=b'6').contains(&b[1]) || &b[2..4] != b"_f" { return None; }
    if !b[4..].iter().all(u8::is_ascii_digit) { return None; }
    first[4..].parse().ok()
}

pub fn locate(d: &[u8], image_end: usize) -> Result<Option<Location>> {
    if d.len() >= 60 {
        let tail = &d[d.len() - 60..];
        let last = trim(&tail[40..60]);
        if last.starts_with("LIVE_") {
            let first = trim(&tail[..20]);
            let frame = huawei_frame(&first);
            let length = crate::xmp::swift_int(&last[5..]);
            match (frame, length) {
                (Some(frame), Some(length)) if length >= 36 && (length as i128) <= d.len() as i128 - 40 => {
                    let length = length as usize;
                    let end = d.len() - 60;
                    let start = end as isize - (length as isize - 20);
                    if start < image_end as isize || start as usize >= end { return fail("华为/荣耀视频范围与静态图片重叠"); }
                    return Ok(Some(Location { start: start as usize, end, frame: Some(frame), format: "huawei-honor" }));
                }
                _ => return fail("华为/荣耀 LIVE_ 尾标无效"),
            }
        }
    }
    if d.len() < 20 || &d[d.len() - 4..] != b"SEFT" { return Ok(None); }
    let footer = d.len() - 8;
    let size = le(d, footer, 4)?;
    if size < 12 || size > footer { return fail("三星 SEF 目录长度无效"); }
    let head = footer - size;
    if &d[head..head + 4] != b"SEFH" { return fail("三星 SEFH 目录头无效"); }
    let count = le(d, head + 8, 4)?;
    if count > (size - 12) / 12 || 12 + count * 12 != size { return fail("三星 SEF 目录越界"); }
    let mut found: Option<Location> = None;
    let mut owned: Vec<Range<usize>> = Vec::new();
    for i in 0..count {
        let entry = head + 12 + i * 12;
        let marker = le(d, entry + 2, 2)?;
        let back = le(d, entry + 4, 4)?;
        let length = le(d, entry + 8, 4)?;
        if back > head || length < 8 || length > back { return fail("三星 SEF 数据范围无效"); }
        let at = head - back;
        let range = at..at + length;
        if at < image_end || owned.iter().any(|r| overlaps(r, &range)) || d[entry..entry + 4] != d[at..at + 4] {
            return fail("三星 SEF 记录重叠或头部不一致");
        }
        owned.push(range);
        let name_len = le(d, at + 4, 4)?;
        if name_len > length - 8 { return fail("三星 SEF 名称越界"); }
        if marker == 0x0a30 {
            if found.is_some() || name_len != 16 || &d[at + 8..at + 24] != b"MotionPhoto_Data" {
                return fail("三星动态视频条目不唯一或名称错误");
            }
            let (mut start, mut end) = (at + 24, at + length);
            if end - start == 12 && &d[start..start + 4] == b"mpv2" {
                let offset = be(d, start + 4, 4)?;
                let bytes = be(d, start + 8, 4)?;
                if offset < image_end || offset > at || bytes > at - offset { return fail("三星 mpv2 视频指针越界"); }
                start = offset;
                end = offset + bytes;
            }
            found = Some(Location { start, end, frame: None, format: "samsung-sef" });
        }
    }
    Ok(found)
}

/// EXIF Make/Model of a JPEG, best effort (never fails conversion).
pub fn make_model(d: &[u8]) -> (String, String) {
    let inner = || -> Result<(String, String)> {
        for s in jpeg::segments(d)? {
            let pl = s.payload(d);
            if s.marker != 225 || !pl.starts_with(jpeg::EXIF) { continue; }
            let t = Tiff::new(&pl[6..])?;
            let fields = t.root()?.0;
            let text = |tag: usize| -> Result<String> {
                let f = match fields.iter().find(|f| f.tag == tag) {
                    Some(f) if f.ty == 2 && f.count > 0 && f.count <= 1024 => f,
                    _ => return Ok(String::new()),
                };
                let bytes: &[u8] = if f.count <= 4 { &f.value[..f.count] } else {
                    let off = f.offset(t.little)?;
                    if off > t.data.len() || f.count > t.data.len() - off { return Ok(String::new()); }
                    &t.data[off..off + f.count]
                };
                let z = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
                Ok(trim(&bytes[..z]))
            };
            return Ok((text(0x010f)?, text(0x0110)?));
        }
        Ok((String::new(), String::new()))
    };
    inner().unwrap_or_default()
}
