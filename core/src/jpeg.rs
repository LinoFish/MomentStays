//! JPEG marker walk, MPF image ranges and static-only extraction (MLJPEG).
use crate::binary::{be, put};
use crate::tiff::Tiff;
use crate::{fail, hdr, hex, Result};
use sha2::{Digest, Sha256};
use std::ops::Range;

pub const EXIF: &[u8] = b"Exif\0\0";
pub const MPF: &[u8] = b"MPF\0";

#[derive(Clone, Debug)]
pub struct Segment { pub marker: u8, pub start: usize, pub end: usize, pub payload: Range<usize> }

impl Segment {
    pub fn payload<'a>(&self, d: &'a [u8]) -> &'a [u8] { &d[self.payload.clone()] }
}

fn rst(m: u8) -> bool { (208..=215).contains(&m) }

pub fn segments(d: &[u8]) -> Result<Vec<Segment>> {
    if !d.starts_with(&[255, 216]) { return fail("不是 JPEG 文件"); }
    let mut pos = 2;
    let mut out = Vec::new();
    while pos < d.len() {
        let start = pos;
        if d[pos] != 255 { return fail("JPEG 标记无效"); }
        while pos < d.len() && d[pos] == 255 { pos += 1; }
        if pos >= d.len() { break; }
        let marker = d[pos];
        pos += 1;
        if marker == 217 {
            out.push(Segment { marker, start, end: pos, payload: pos..pos });
            return Ok(out);
        }
        if marker == 216 || marker == 1 || rst(marker) { return fail("JPEG 中出现意外独立标记"); }
        let length = be(d, pos, 2)?;
        let end = pos + length;
        if length < 2 || end > d.len() { return fail("JPEG 数据段被截断"); }
        let payload = pos + 2..end;
        pos = end;
        if marker == 218 {
            while pos < d.len() {
                let at = match d[pos..].iter().position(|&b| b == 255) {
                    Some(i) => pos + i,
                    None => return fail("JPEG 扫描数据未结束"),
                };
                let mut p = at + 1;
                while p < d.len() && d[p] == 255 { p += 1; }
                if p >= d.len() { return fail("JPEG 扫描标记被截断"); }
                if d[p] == 0 || rst(d[p]) { pos = p + 1; } else { pos = at; break; }
            }
        }
        out.push(Segment { marker, start, end: pos, payload });
    }
    fail("缺少 JPEG 结束标记")
}

/// SHA-256 over every segment except APPn and COM: the image coding itself.
pub fn coding_hash(d: &[u8]) -> Result<String> {
    let mut h = Sha256::new();
    for s in segments(d)? {
        if !(224..=239).contains(&s.marker) && s.marker != 254 { h.update(&d[s.start..s.end]); }
    }
    Ok(hex(&h.finalize()))
}

pub fn app(marker: u8, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() + 2 > 65535 { return fail("JPEG 元数据段超过 64 KiB，无法安全写入"); }
    let mut v = vec![255, marker];
    v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    v.extend_from_slice(payload);
    Ok(v)
}

/// MPF images as byte ranges of `data` (index 0 is the primary image).
pub fn auxiliary(data: &[u8]) -> Result<Vec<Range<usize>>> {
    for s in segments(data)? {
        let payload = s.payload(data);
        if s.marker != 226 || !payload.starts_with(MPF) { continue; }
        let t = Tiff::new(&payload[4..])?;
        let (fields, _) = t.root()?;
        let f = match fields.iter().find(|f| f.tag == 0xb002) { Some(f) => f, None => return fail("MPF 索引缺失") };
        let offset = f.offset(t.little)?;
        if f.count % 16 != 0 || offset.saturating_add(f.count) > t.data.len() { return fail("MPF 索引无效"); }
        let mut out = Vec::new();
        for i in 0..f.count / 16 {
            let at = offset + i * 16;
            let size = crate::binary::int(t.data, at + 4, 4, t.little)?;
            let relative = crate::binary::int(t.data, at + 8, 4, t.little)?;
            let start = if i == 0 { 0 } else { s.start + 8 + relative };
            if size == 0 || start.saturating_add(size) > data.len() { return fail("MPF 图片被截断"); }
            out.push(start..start + size);
        }
        return Ok(out);
    }
    Ok(vec![])
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool { a.start < b.end && b.start < a.end }

pub fn image_ranges(data: &[u8]) -> Result<Vec<Range<usize>>> {
    let segs = segments(data)?;
    let primary_end = segs.last().unwrap().end;
    let mut ranges = vec![0..primary_end];
    let indexes: Vec<&Segment> = segs.iter().filter(|s| s.marker == 226 && s.payload(data).starts_with(MPF)).collect();
    if indexes.len() > 1 { return fail("JPEG 包含多个 MPF 索引"); }
    if let Some(s) = indexes.first() {
        let t = Tiff::new(&s.payload(data)[4..])?;
        let fields = t.root()?.0;
        let f = match fields.iter().find(|f| f.tag == 0xb002) {
            Some(f) if f.count >= 16 && f.count % 16 == 0 && f.count / 16 <= 64 => f,
            _ => return fail("MPF 图片索引无效"),
        };
        let at = f.offset(t.little)?;
        if at > t.data.len() || f.count > t.data.len() - at { return fail("MPF 图片索引被截断"); }
        if crate::binary::int(t.data, at + 8, 4, t.little)? != 0 { return fail("MPF 主图偏移无效"); }
        for i in 1..f.count / 16 {
            let size = crate::binary::int(t.data, at + i * 16 + 4, 4, t.little)?;
            let relative = crate::binary::int(t.data, at + i * 16 + 8, 4, t.little)?;
            let start = s.start + 8 + relative;
            if size == 0 || start > data.len() || size > data.len() - start { return fail("MPF 附加图超出范围"); }
            let range = start..start + size;
            if ranges.iter().any(|r| overlaps(r, &range)) || segments(&data[range.clone()])?.last().unwrap().end != size {
                return fail("MPF 附加图包含非图片数据或范围重叠");
            }
            ranges.push(range);
        }
    }
    Ok(ranges)
}

pub fn image_end(data: &[u8]) -> Result<usize> { Ok(image_ranges(data)?.iter().map(|r| r.end).max().unwrap()) }

/// Primary JPEG plus MPF images, re-indexed, with any recognised Honor EHDR tail.
pub fn static_only(data: &[u8]) -> Result<Vec<u8>> {
    let ranges = image_ranges(data)?;
    let honor = hdr::honor_private(data, ranges.iter().map(|r| r.end).max().unwrap())?;
    let mut out = Vec::with_capacity(data.len());
    let mut starts = Vec::new();
    for r in &ranges { starts.push(out.len()); out.extend_from_slice(&data[r.clone()]); }
    let mpf_seg = segments(&out)?.into_iter().find(|s| s.marker == 226 && s.payload(&out).starts_with(MPF));
    if let Some(s) = mpf_seg {
        let (at, little) = {
            let t = Tiff::new(&out[s.payload.start + 4..s.payload.end])?;
            let fields = t.root()?.0;
            (fields.iter().find(|f| f.tag == 0xb002).unwrap().offset(t.little)?, t.little)
        };
        for i in 0..ranges.len() {
            put(ranges[i].len(), &mut out, s.start + 8 + at + i * 16 + 4, 4, little)?;
            let rel = if i == 0 { 0 } else { starts[i] - s.start - 8 };
            put(rel, &mut out, s.start + 8 + at + i * 16 + 8, 4, little)?;
        }
    }
    if let Some(tail) = honor { out.extend_from_slice(tail); }
    Ok(out)
}
