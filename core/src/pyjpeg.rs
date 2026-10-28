//! JPEG/EXIF/MPF edits with the Python CLI's exact semantics and messages.
//!
//! This mirrors `motionlive/jpeg.py` (not the Swift-port `jpeg` module):
//! segment walks, coding hashes, TIFF directories, Apple MakerNote handling,
//! header edits with MPF address repair, and MPF image ranges. No image
//! decoder or encoder is used here.

use crate::{fail, hex, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ops::Range;

pub const EXIF_HDR: &[u8] = b"Exif\0\0";
pub const MPF_HDR: &[u8] = b"MPF\0";

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
    (0xD0..=0xD7).contains(&m)
}

/// Parse one JPEG, including progressive scans and stuffed entropy bytes.
pub fn segments(data: &[u8]) -> Result<Vec<Segment>> {
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return fail("Not a JPEG");
    }
    let mut result = Vec::new();
    let mut pos = 2usize;
    while pos < data.len() {
        let start = pos;
        if data[pos] != 255 {
            return fail(format!("Invalid JPEG marker at {pos}"));
        }
        while pos < data.len() && data[pos] == 255 {
            pos += 1;
        }
        if pos >= data.len() {
            break;
        }
        let marker = data[pos];
        pos += 1;
        if marker == 0xD9 {
            result.push(Segment { marker, start, end: pos, payload: pos..pos });
            return Ok(result);
        }
        if marker == 0xD8 || marker == 0x01 || rst(marker) {
            return fail("Unexpected standalone JPEG marker");
        }
        if pos + 2 > data.len() {
            break;
        }
        let length = ((data[pos] as usize) << 8) | data[pos + 1] as usize;
        let end = pos + length;
        if length < 2 || end > data.len() {
            return fail("Truncated JPEG segment");
        }
        let payload = pos + 2..end;
        pos = end;
        if marker == 0xDA {
            // Include the entropy stream with SOS; restart markers are preserved.
            while pos < data.len() {
                let at = match data[pos..].iter().position(|b| *b == 0xFF) {
                    Some(i) => pos + i,
                    None => return fail("Unterminated JPEG entropy stream"),
                };
                let mut p = at + 1;
                while p < data.len() && data[p] == 255 {
                    p += 1;
                }
                if p >= data.len() {
                    return fail("Truncated JPEG entropy marker");
                }
                if data[p] == 0 || rst(data[p]) {
                    pos = p + 1;
                } else {
                    pos = at;
                    break;
                }
            }
        }
        result.push(Segment { marker, start, end: pos, payload });
    }
    fail("Missing JPEG EOI")
}

/// Hash coding tables, frame/scan headers and entropy, excluding metadata.
pub fn image_hash(data: &[u8]) -> Result<String> {
    let mut h = Sha256::new();
    for s in segments(data)? {
        if !(0xE0..=0xEF).contains(&s.marker) && s.marker != 0xFE {
            h.update(&data[s.start..s.end]);
        }
    }
    Ok(hex(&h.finalize()))
}

pub fn app(marker: u8, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() + 2 > 65535 {
        return fail("JPEG metadata exceeds 64 KiB segment limit");
    }
    let mut v = vec![255, marker];
    v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    v.extend_from_slice(payload);
    Ok(v)
}

// ---------------------------------------------------------------------------
// TIFF (Python motionlive.jpeg.TIFF semantics)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct TiffEntry {
    pub raw: [u8; 12],
}

impl TiffEntry {
    pub fn tag(&self, little: bool) -> usize {
        u16_at(&self.raw, 0, little) as usize
    }
    pub fn ty(&self, little: bool) -> usize {
        u16_at(&self.raw, 2, little) as usize
    }
    pub fn count(&self, little: bool) -> usize {
        u32_at(&self.raw, 4, little) as usize
    }
    pub fn value(&self, little: bool) -> usize {
        u32_at(&self.raw, 8, little) as usize
    }
}

fn u16_at(d: &[u8], p: usize, little: bool) -> u16 {
    if little {
        (d[p] as u16) | ((d[p + 1] as u16) << 8)
    } else {
        ((d[p] as u16) << 8) | d[p + 1] as u16
    }
}

fn u32_at(d: &[u8], p: usize, little: bool) -> u32 {
    if little {
        (d[p] as u32) | ((d[p + 1] as u32) << 8) | ((d[p + 2] as u32) << 16) | ((d[p + 3] as u32) << 24)
    } else {
        ((d[p] as u32) << 24) | ((d[p + 1] as u32) << 16) | ((d[p + 2] as u32) << 8) | d[p + 3] as u32
    }
}

pub struct Tiff<'a> {
    pub data: &'a [u8],
    pub little: bool,
}

impl<'a> Tiff<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() < 2 || (&data[..2] != b"II" && &data[..2] != b"MM") {
            return fail("Invalid TIFF byte order");
        }
        let little = data[0] == b'I';
        let t = Tiff { data, little };
        if t.u16(2)? != 42 {
            return fail("Invalid TIFF version");
        }
        Ok(t)
    }
    pub fn u16(&self, p: usize) -> Result<usize> {
        self.unpack(2, p).map(|v| v as usize)
    }
    pub fn u32(&self, p: usize) -> Result<usize> {
        self.unpack(4, p).map(|v| v as usize)
    }
    fn unpack(&self, size: usize, p: usize) -> Result<u32> {
        if p > self.data.len() || size > self.data.len() - p {
            return fail("Truncated TIFF");
        }
        Ok(if size == 2 { u16_at(self.data, p, self.little) as u32 } else { u32_at(self.data, p, self.little) })
    }
    pub fn directory(&self, offset: usize) -> Result<(Vec<TiffEntry>, usize)> {
        let n = self.u16(offset)?;
        if offset < 8 || offset + 2 + n * 12 + 4 > self.data.len() {
            return fail("Invalid TIFF directory");
        }
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let p = offset + 2 + i * 12;
            let mut raw = [0u8; 12];
            raw.copy_from_slice(&self.data[p..p + 12]);
            entries.push(TiffEntry { raw });
        }
        Ok((entries, self.u32(offset + 2 + n * 12)?))
    }
    pub fn root(&self) -> Result<(Vec<TiffEntry>, usize)> {
        let off = self.u32(4)?;
        self.directory(off)
    }
    pub fn tag<'b>(entries: &'b [TiffEntry], little: bool, tag: usize) -> Option<&'b TiffEntry> {
        entries.iter().find(|e| e.tag(little) == tag)
    }
    pub fn entry(little: bool, tag: usize, ty: usize, count: usize, value: usize) -> [u8; 12] {
        let mut out = [0u8; 12];
        let put16 = |out: &mut [u8; 12], at: usize, v: usize| {
            if little {
                out[at] = v as u8;
                out[at + 1] = (v >> 8) as u8;
            } else {
                out[at] = (v >> 8) as u8;
                out[at + 1] = v as u8;
            }
        };
        let put32 = |out: &mut [u8; 12], at: usize, v: usize| {
            if little {
                out[at] = v as u8;
                out[at + 1] = (v >> 8) as u8;
                out[at + 2] = (v >> 16) as u8;
                out[at + 3] = (v >> 24) as u8;
            } else {
                out[at] = (v >> 24) as u8;
                out[at + 1] = (v >> 16) as u8;
                out[at + 2] = (v >> 8) as u8;
                out[at + 3] = v as u8;
            }
        };
        put16(&mut out, 0, tag);
        put16(&mut out, 2, ty);
        put32(&mut out, 4, count);
        put32(&mut out, 8, value);
        out
    }
    pub fn serialize(little: bool, entries: &[[u8; 12]], next_ifd: usize) -> Vec<u8> {
        let mut sorted = entries.to_vec();
        sorted.sort_by_key(|e| u16_at(e, 0, little));
        let mut out = Vec::with_capacity(2 + sorted.len() * 12 + 4);
        if little {
            out.extend_from_slice(&(sorted.len() as u16).to_le_bytes());
        } else {
            out.extend_from_slice(&(sorted.len() as u16).to_be_bytes());
        }
        for e in &sorted {
            out.extend_from_slice(e);
        }
        if little {
            out.extend_from_slice(&(next_ifd as u32).to_le_bytes());
        } else {
            out.extend_from_slice(&(next_ifd as u32).to_be_bytes());
        }
        out
    }
}

fn pack_u16(little: bool, v: usize) -> [u8; 2] {
    if little { (v as u16).to_le_bytes() } else { (v as u16).to_be_bytes() }
}
fn pack_u32(little: bool, v: usize) -> [u8; 4] {
    if little { (v as u32).to_le_bytes() } else { (v as u32).to_be_bytes() }
}

/// Retain existing Apple gain-map tags and their original relative offsets.
pub fn maker_note(identifier: &str, old: &[u8]) -> Result<Vec<u8>> {
    let value = {
        let mut v = identifier.as_bytes().to_vec();
        v.push(0);
        v
    };
    if old.starts_with(b"Apple iOS\0\x00\x01") {
        let little = match old.get(12..14) {
            Some(b"MM") => false,
            Some(b"II") => true,
            _ => return fail("Malformed Apple MakerNote"),
        };
        if old.len() < 16 {
            return fail("Malformed Apple MakerNote");
        }
        let n = u16_at(old, 14, little) as usize;
        let end = 16 + n * 12 + 4;
        if end > old.len() {
            return fail("Truncated Apple MakerNote");
        }
        let mut entries: Vec<[u8; 12]> = Vec::new();
        for i in 0..n {
            let mut raw = [0u8; 12];
            raw.copy_from_slice(&old[16 + i * 12..28 + i * 12]);
            entries.push(raw);
        }
        let kept: Vec<[u8; 12]> = entries.into_iter().filter(|e| u16_at(e, 0, little) as usize != 0x11).collect();
        // Growing the front table moves old data: adjust external value offsets.
        let kept_len = kept.len();
        let delta = (kept_len as isize + 1 - n as isize) * 12;
        let sizes: HashMap<usize, usize> = [
            (1, 1), (2, 1), (3, 2), (4, 4), (5, 8), (6, 1), (7, 1),
            (8, 2), (9, 4), (10, 8), (11, 4), (12, 8),
        ].into_iter().collect();
        let mut patched: Vec<[u8; 12]> = Vec::new();
        for e in kept {
            let ty = u16_at(&e, 2, little) as usize;
            let count = u32_at(&e, 4, little) as usize;
            let offset = u32_at(&e, 8, little) as usize;
            let size = match sizes.get(&ty) {
                Some(s) => *s,
                None => return fail("Unknown Apple MakerNote field type"),
            };
            let mut e = e;
            if size * count > 4 {
                if offset < end || offset + size * count > old.len() {
                    return fail("Unsupported Apple MakerNote offset");
                }
                let new_off = (offset as isize + delta) as usize;
                e[8..12].copy_from_slice(&pack_u32(little, new_off));
            }
            patched.push(e);
        }
        let note_end = (old.len() as isize + delta) as usize;
        patched.push(Tiff::entry(little, 0x11, 2, value.len(), note_end));
        patched.sort_by_key(|e| u16_at(e, 0, little));
        let mut out = Vec::new();
        out.extend_from_slice(&old[..14]);
        out.extend_from_slice(&pack_u16(little, patched.len()));
        for e in &patched {
            out.extend_from_slice(e);
        }
        out.extend_from_slice(&[0u8; 4]);
        out.extend_from_slice(&old[end..]);
        out.extend_from_slice(&value);
        Ok(out)
    } else {
        let mut out = b"Apple iOS\0\x00\x01MM".to_vec();
        out.extend_from_slice(&pack_u16(false, 1));
        out.extend_from_slice(&Tiff::entry(false, 0x11, 2, value.len(), 32));
        out.extend_from_slice(&[0u8; 4]);
        out.extend_from_slice(&value);
        Ok(out)
    }
}

pub fn exif_identifier(payload: Option<&[u8]>, identifier: &str) -> Result<Vec<u8>> {
    let raw: Vec<u8> = match payload {
        Some(p) if !p.is_empty() => p.get(6..).unwrap_or(&[]).to_vec(),
        _ => b"MM\0*\0\0\0\x08\0\0\0\0\0\0".to_vec(),
    };
    let t = Tiff::new(&raw)?;
    let (root, root_next) = t.root()?;
    let pointer = Tiff::tag(&root, t.little, 0x8769);
    let (exif, exif_next) = match pointer {
        Some(p) => {
            let off = p.value(t.little);
            t.directory(off)?
        }
        None => (Vec::new(), 0),
    };
    let old_entry = Tiff::tag(&exif, t.little, 0x927C);
    let mut old: Vec<u8> = Vec::new();
    if let Some(e) = old_entry {
        let ty = e.ty(t.little);
        let count = e.count(t.little);
        let offset = e.value(t.little);
        if ty != 7 || count <= 4 || offset + count > raw.len() {
            return fail("Unsupported MakerNote layout");
        }
        old = raw[offset..offset + count].to_vec();
    }
    let note = maker_note(identifier, &old)?;
    // Append directories, retaining all original offsets, GPS, thumbnail and data.
    let mut out: Vec<u8> = raw.clone();
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let note_at = out.len();
    out.extend_from_slice(&note);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let exif_at = out.len();
    let mut kept: Vec<[u8; 12]> = exif.iter().filter(|e| e.tag(t.little) != 0x927C).map(|e| e.raw).collect();
    kept.push(Tiff::entry(t.little, 0x927C, 7, note.len(), note_at));
    out.extend_from_slice(&Tiff::serialize(t.little, &kept, exif_next));
    let root_at = out.len();
    let mut kept_root: Vec<[u8; 12]> = root.iter().filter(|e| e.tag(t.little) != 0x8769).map(|e| e.raw).collect();
    kept_root.push(Tiff::entry(t.little, 0x8769, 4, 1, exif_at));
    out.extend_from_slice(&Tiff::serialize(t.little, &kept_root, root_next));
    out[4..8].copy_from_slice(&pack_u32(t.little, root_at));
    let mut result = EXIF_HDR.to_vec();
    result.extend_from_slice(&out);
    Ok(result)
}

/// Apply segment edits and fix MPF addresses after all header size changes.
pub fn edit_header(data: &[u8], replacements: &HashMap<usize, Vec<u8>>, insert: &[u8]) -> Result<Vec<u8>> {
    let segs = segments(data)?;
    let mut edits: Vec<(usize, usize, Vec<u8>)> = segs
        .iter()
        .filter(|s| replacements.contains_key(&s.start))
        .map(|s| (s.start, s.end, replacements[&s.start].clone()))
        .collect();
    if !insert.is_empty() {
        edits.push((2, 2, insert.to_vec()));
    }
    edits.sort_by_key(|e| (e.0, e.1));

    let mapped = |p: usize| -> Result<usize> {
        let mut delta: isize = 0;
        for (a, b, replacement) in &edits {
            if *b <= p {
                delta += replacement.len() as isize - (*b as isize - *a as isize);
            } else if *a < p && p < *b {
                return fail("MPF pointer targets replaced metadata");
            }
        }
        Ok((p as isize + delta) as usize)
    };

    let mut out: Vec<u8> = Vec::new();
    let mut cursor = 0usize;
    for (a, b, replacement) in &edits {
        if *a < cursor {
            return fail("Overlapping JPEG edits");
        }
        out.extend_from_slice(&data[cursor..*a]);
        out.extend_from_slice(replacement);
        cursor = *b;
    }
    out.extend_from_slice(&data[cursor..]);
    let delta = out.len() as isize - data.len() as isize;
    for s in &segs {
        if s.marker != 0xE2 || !s.payload(data).starts_with(MPF_HDR) || replacements.contains_key(&s.start) {
            continue;
        }
        let old_base = s.start + 8;
        let new_base = mapped(s.start)? + 8;
        let t = Tiff::new(&s.payload(data)[4..])?;
        let (entries, _) = t.root()?;
        let entry = match Tiff::tag(&entries, t.little, 0xB002) {
            Some(e) => e,
            None => return fail("MPF has no MPEntry index"),
        };
        let count = entry.count(t.little);
        let offset = entry.value(t.little);
        if count % 16 != 0 || offset + count > t.data.len() {
            return fail("Invalid MPEntry array");
        }
        for index in 0..count / 16 {
            let at = offset + index * 16;
            let size = u32_at(t.data, at + 4, t.little) as usize;
            let relative = u32_at(t.data, at + 8, t.little) as usize;
            if index == 0 {
                if relative != 0 {
                    return fail("Invalid primary MPEntry offset");
                }
                let pos = new_base + at + 4;
                out[pos..pos + 4].copy_from_slice(&pack_u32(t.little, (size as isize + delta) as usize));
            } else if relative != 0 {
                let target = old_base + relative;
                if target + size > data.len() || data.get(target..target + 2) != Some(&[0xFF, 0xD8][..]) {
                    return fail("MPF auxiliary image lies outside still data");
                }
                let pos = new_base + at + 8;
                out[pos..pos + 4].copy_from_slice(&pack_u32(t.little, mapped(target)? - new_base));
            }
        }
    }
    if image_hash(data)? != image_hash(&out)? {
        return fail("JPEG coding data changed");
    }
    Ok(out)
}

pub fn mpf_images(data: &[u8]) -> Result<Vec<Range<usize>>> {
    for s in segments(data)? {
        if s.marker != 0xE2 || !s.payload(data).starts_with(MPF_HDR) {
            continue;
        }
        let t = Tiff::new(&s.payload(data)[4..])?;
        let (entries, _) = t.root()?;
        let e = match Tiff::tag(&entries, t.little, 0xB002) {
            Some(e) => e,
            None => return fail("MPF index missing"),
        };
        let count = e.count(t.little);
        let offset = e.value(t.little);
        if count % 16 != 0 || offset + count > t.data.len() {
            return fail("Invalid MPF array");
        }
        let mut result = Vec::new();
        for i in 0..count / 16 {
            let size = u32_at(t.data, offset + i * 16 + 4, t.little) as usize;
            let relative = u32_at(t.data, offset + i * 16 + 8, t.little) as usize;
            let start = if i == 0 { 0 } else { s.start + 8 + relative };
            if start > data.len() || size > data.len() - start {
                return fail("Truncated MPF image");
            }
            result.push(start..start + size);
        }
        return Ok(result);
    }
    Ok(vec![])
}
