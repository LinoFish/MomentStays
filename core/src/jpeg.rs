//! JPEG marker walk, MPF image ranges and static-only extraction (MLJPEG).
use crate::binary::{be, put};
use crate::tiff::Tiff;
use crate::{fail, fail_as, hdr, hex, ErrorKind, Result};
use sha2::{Digest, Sha256};
use std::ops::Range;

pub const EXIF: &[u8] = b"Exif\0\0";
pub const MPF: &[u8] = b"MPF\0";

#[derive(Clone, Debug)]
pub struct Segment {
    pub marker: u8,
    pub start: usize,
    pub end: usize,
    pub payload: Range<usize>,
}

impl Segment {
    pub fn payload<'a>(&self, d: &'a [u8]) -> &'a [u8] {
        &d[self.payload.clone()]
    }
}

fn rst(m: u8) -> bool {
    (208..=215).contains(&m)
}

pub fn segments(d: &[u8]) -> Result<Vec<Segment>> {
    if !d.starts_with(&[255, 216]) {
        return fail("不是 JPEG 文件");
    }
    let mut pos = 2;
    let mut out = Vec::new();
    while pos < d.len() {
        let start = pos;
        if d[pos] != 255 {
            return fail("JPEG 标记无效");
        }
        while pos < d.len() && d[pos] == 255 {
            pos += 1;
        }
        if pos >= d.len() {
            break;
        }
        let marker = d[pos];
        pos += 1;
        if marker == 217 {
            out.push(Segment {
                marker,
                start,
                end: pos,
                payload: pos..pos,
            });
            return Ok(out);
        }
        if marker == 216 || marker == 1 || rst(marker) {
            return fail("JPEG 中出现意外独立标记");
        }
        let length = be(d, pos, 2)?;
        let end = pos + length;
        if length < 2 || end > d.len() {
            return fail("JPEG 数据段被截断");
        }
        let payload = pos + 2..end;
        pos = end;
        if marker == 218 {
            while pos < d.len() {
                let at = match memchr::memchr(255, &d[pos..]) {
                    Some(i) => pos + i,
                    None => return fail("JPEG 扫描数据未结束"),
                };
                let mut p = at + 1;
                while p < d.len() && d[p] == 255 {
                    p += 1;
                }
                if p >= d.len() {
                    return fail("JPEG 扫描标记被截断");
                }
                if d[p] == 0 || rst(d[p]) {
                    pos = p + 1;
                } else {
                    pos = at;
                    break;
                }
            }
        }
        out.push(Segment {
            marker,
            start,
            end: pos,
            payload,
        });
    }
    fail("缺少 JPEG 结束标记")
}

/// SHA-256 over every segment except APPn and COM: the image coding itself.
pub fn coding_hash(d: &[u8]) -> Result<String> {
    let mut h = Sha256::new();
    for s in segments(d)? {
        if !(224..=239).contains(&s.marker) && s.marker != 254 {
            h.update(&d[s.start..s.end]);
        }
    }
    Ok(hex(&h.finalize()))
}

/// Coding hash and full SHA-256 in a single pass over the still (perf
/// item 4): the coding hash covers non-APP/non-COM segment bytes, the
/// full hash covers every byte. Segments tile the still from the first
/// marker on, but the SOI bytes, any gaps, and bytes after the last
/// segment (MPF auxiliary images, Honor tail) belong only to the full
/// hash, so gaps and the tail are fed to it explicitly. The result is
/// byte-identical to `(coding_hash(d), sha256_hex(d))` for the same
/// segment list. Note the saving is one memory read, not one hash:
/// both compressors still run over the data.
pub(crate) fn coding_and_full_hash(d: &[u8], segs: &[Segment]) -> (String, String) {
    let mut coding = Sha256::new();
    let mut full = Sha256::new();
    let mut covered = 0usize;
    for s in segs {
        if covered < s.start {
            full.update(&d[covered..s.start]);
        }
        full.update(&d[s.start..s.end]);
        if !(224..=239).contains(&s.marker) && s.marker != 254 {
            coding.update(&d[s.start..s.end]);
        }
        covered = s.end;
    }
    if covered < d.len() {
        full.update(&d[covered..]);
    }
    (hex(&coding.finalize()), hex(&full.finalize()))
}

pub fn app(marker: u8, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() + 2 > 65535 {
        return fail_as(
            ErrorKind::Unsupported,
            "JPEG 元数据段超过 64 KiB，无法安全写入",
        );
    }
    let mut v = vec![255, marker];
    v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    v.extend_from_slice(payload);
    Ok(v)
}

/// MPF images as byte ranges of `data` (index 0 is the primary image).
pub fn auxiliary(data: &[u8]) -> Result<Vec<Range<usize>>> {
    let segs = segments(data)?;
    auxiliary_from_segments(data, &segs)
}

/// `auxiliary` over a precomputed segment list (perf item 3).
pub(crate) fn auxiliary_from_segments(data: &[u8], segs: &[Segment]) -> Result<Vec<Range<usize>>> {
    let ranges = mpf_ranges(data, segs)?;
    if ranges.iter().any(|r| r.is_empty()) {
        return fail("MPF 图片被截断");
    }
    Ok(ranges)
}

/// The MPF image index of one APP2 segment, as stored: the entry array's
/// position and each image's (size, offset relative to the MPF header).
pub(crate) struct MpfIndex {
    /// Start of the APP2 segment (its marker).
    pub seg_start: usize,
    pub little: bool,
    /// Offset of the 16-byte entry array inside the segment's TIFF.
    pub at: usize,
    pub entries: Vec<(usize, usize)>,
}

impl MpfIndex {
    /// Where an entry's relative offsets count from: the TIFF header,
    /// 8 bytes into the segment (marker, length, "MPF\0").
    pub fn base(&self) -> usize {
        self.seg_start + 8
    }
    /// Byte ranges in the file; entry 0 is the primary image at 0.
    fn ranges(&self, data: &[u8]) -> Result<Vec<Range<usize>>> {
        let mut out = Vec::with_capacity(self.entries.len());
        for (i, &(size, rel)) in self.entries.iter().enumerate() {
            let start = if i == 0 { 0 } else { self.base() + rel };
            if start > data.len() || size > data.len() - start {
                return fail("MPF 图片被截断");
            }
            out.push(start..start + size);
        }
        Ok(out)
    }
}

/// The single MPF entry reader behind every MPF view in the crate.
pub(crate) fn mpf_index(data: &[u8], s: &Segment) -> Result<MpfIndex> {
    let t = Tiff::new(&s.payload(data)[MPF.len()..])?;
    let (fields, _) = t.root()?;
    // 1..=64 images: the primary plus at most 63 auxiliary images.
    let f = match crate::tiff::find(&fields, 0xb002) {
        Some(f) if f.count >= 16 && f.count % 16 == 0 && f.count / 16 <= 64 => f,
        _ => return fail("MPF 图片索引无效"),
    };
    let at = f.offset(t.little)?;
    if at > t.data.len() || f.count > t.data.len() - at {
        return fail("MPF 图片索引被截断");
    }
    let mut entries = Vec::with_capacity(f.count / 16);
    for i in 0..f.count / 16 {
        let size = crate::binary::int(t.data, at + i * 16 + 4, 4, t.little)?;
        let rel = crate::binary::int(t.data, at + i * 16 + 8, 4, t.little)?;
        entries.push((size, rel));
    }
    Ok(MpfIndex {
        seg_start: s.start,
        little: t.little,
        at,
        entries,
    })
}

fn is_mpf(data: &[u8], s: &Segment) -> bool {
    s.marker == 226 && s.payload(data).starts_with(MPF)
}

/// Image ranges from the first MPF index as stored (entry 0 = primary at
/// 0), or none without MPF. Zero-size entries are kept; see
/// [`auxiliary`] for the strict form and [`image_ranges`] for the
/// fully validated one.
pub(crate) fn mpf_ranges(data: &[u8], segs: &[Segment]) -> Result<Vec<Range<usize>>> {
    match segs.iter().find(|s| is_mpf(data, s)) {
        Some(s) => mpf_index(data, s)?.ranges(data),
        None => Ok(vec![]),
    }
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

pub fn image_ranges(data: &[u8]) -> Result<Vec<Range<usize>>> {
    let segs = segments(data)?;
    image_ranges_from_segments(data, &segs)
}

/// `image_ranges` over a precomputed segment list (perf item 3): the
/// caller guarantees `segs` is exactly `segments(data)`, so repeated
/// stages in one operation share a single SOS scan instead of each
/// re-walking the still.
pub(crate) fn image_ranges_from_segments(
    data: &[u8],
    segs: &[Segment],
) -> Result<Vec<Range<usize>>> {
    let primary_end = segs.last().unwrap().end;
    let mut ranges = vec![0..primary_end];
    let indexes: Vec<&Segment> = segs.iter().filter(|s| is_mpf(data, s)).collect();
    if indexes.len() > 1 {
        return fail("JPEG 包含多个 MPF 索引");
    }
    if let Some(s) = indexes.first() {
        let index = mpf_index(data, s)?;
        if index.entries[0].1 != 0 {
            return fail("MPF 主图偏移无效");
        }
        for &(size, rel) in &index.entries[1..] {
            let start = index.base() + rel;
            if size == 0 || start > data.len() || size > data.len() - start {
                return fail("MPF 附加图超出范围");
            }
            let range = start..start + size;
            if ranges.iter().any(|r| overlaps(r, &range))
                || segments(&data[range.clone()])?.last().map(|l| l.end) != Some(size)
            {
                return fail("MPF 附加图包含非图片数据或范围重叠");
            }
            ranges.push(range);
        }
    }
    Ok(ranges)
}

pub fn image_end(data: &[u8]) -> Result<usize> {
    Ok(image_ranges(data)?.iter().map(|r| r.end).max().unwrap())
}

pub(crate) fn image_end_from_segments(data: &[u8], segs: &[Segment]) -> Result<usize> {
    Ok(image_ranges_from_segments(data, segs)?
        .iter()
        .map(|r| r.end)
        .max()
        .unwrap())
}

/// Primary JPEG plus MPF images, re-indexed, with any recognised Honor EHDR tail.
pub fn static_only(data: &[u8]) -> Result<Vec<u8>> {
    let ranges = image_ranges(data)?;
    let honor = hdr::honor_private(data, ranges.iter().map(|r| r.end).max().unwrap())?;
    let mut out = Vec::with_capacity(data.len());
    let mut starts = Vec::new();
    for r in &ranges {
        starts.push(out.len());
        out.extend_from_slice(&data[r.clone()]);
    }
    let out_segs = segments(&out)?;
    if let Some(s) = out_segs.iter().find(|s| is_mpf(&out, s)) {
        let index = mpf_index(&out, s)?;
        let entries_at = index.base() + index.at;
        for i in 0..ranges.len() {
            put(
                ranges[i].len(),
                &mut out,
                entries_at + i * 16 + 4,
                4,
                index.little,
            )?;
            let rel = if i == 0 { 0 } else { starts[i] - index.base() };
            put(rel, &mut out, entries_at + i * 16 + 8, 4, index.little)?;
        }
    }
    if let Some(tail) = honor {
        out.extend_from_slice(tail);
    }
    Ok(out)
}

/// Borrowed `static_only`: when the input is already one JPEG image
/// with no MPF segment and no Honor private tail, the rebuild
/// `static_only` performs is byte-identical to the input, so return
/// it borrowed instead of copying. Anything else (MPF re-indexing
/// or an appended tail changes bytes) falls back to the owned
/// rebuild. This is the P2 zero-copy path for `format::parse`.
pub fn static_only_cow(data: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>> {
    let segs = segments(data)?;
    static_only_cow_from_segments(data, &segs)
}

/// `static_only_cow` over a precomputed segment list (perf item 3).
/// Caller guarantees `segs` is exactly `segments(data)`; see
/// `image_ranges_from_segments` for the sharing contract.
pub(crate) fn static_only_cow_from_segments<'a>(
    data: &'a [u8],
    segs: &[Segment],
) -> Result<std::borrow::Cow<'a, [u8]>> {
    let ranges = image_ranges_from_segments(data, segs)?;
    let honor = hdr::honor_private(data, ranges.iter().map(|r| r.end).max().unwrap())?;
    let has_mpf = segs
        .iter()
        .any(|s| s.marker == 226 && s.payload(data).starts_with(MPF));
    if ranges.len() == 1
        && ranges[0].start == 0
        && ranges[0].end == data.len()
        && honor.is_none()
        && !has_mpf
    {
        return Ok(std::borrow::Cow::Borrowed(data));
    }
    Ok(std::borrow::Cow::Owned(static_only(data)?))
}
