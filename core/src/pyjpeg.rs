//! JPEG/EXIF/MPF edits with the Python CLI's exact semantics and messages.
//!
//! Originally ported from `motionlive/jpeg.py`. The segment walk, coding hash
//! and APP builder are shared with `jpeg` (re-exported below); this module adds
//! TIFF directories, Apple MakerNote handling,
//! header edits with MPF address repair, and MPF image ranges. No image
//! decoder or encoder is used here.

use crate::tiff::{self, entry, pack_u16, pack_u32, serialize, u16_at, u32_at, Tiff};
use crate::{fail, Result};
use std::collections::HashMap;
use std::ops::Range;

pub use crate::jpeg::{
    app, coding_hash as image_hash, segments, Segment, EXIF as EXIF_HDR, MPF as MPF_HDR,
};

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
        let kept: Vec<[u8; 12]> = entries
            .into_iter()
            .filter(|e| u16_at(e, 0, little) as usize != 0x11)
            .collect();
        // Growing the front table moves old data: adjust external value offsets.
        let kept_len = kept.len();
        let delta = (kept_len as isize + 1 - n as isize) * 12;
        // TIFF 6.0 field sizes, plus BigTIFF LONG8/IFD8: real iPhone
        // MakerNotes use type 16 (LONG8) for LivePhotoVideoIndex.
        let sizes: HashMap<usize, usize> = [
            (1, 1),
            (2, 1),
            (3, 2),
            (4, 4),
            (5, 8),
            (6, 1),
            (7, 1),
            (8, 2),
            (9, 4),
            (10, 8),
            (11, 4),
            (12, 8),
            (13, 4),
            (16, 8),
            (17, 8),
        ]
        .into_iter()
        .collect();
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
        patched.push(entry(little, 0x11, 2, value.len(), note_end));
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
        out.extend_from_slice(&entry(false, 0x11, 2, value.len(), 32));
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
    let pointer = tiff::find(&root, 0x8769);
    let (exif, exif_next) = match pointer {
        Some(p) => {
            let off = p.value_u32(t.little);
            t.directory(off)?
        }
        None => (Vec::new(), 0),
    };
    let old_entry = tiff::find(&exif, 0x927C);
    let mut old: Vec<u8> = Vec::new();
    if let Some(e) = old_entry {
        let ty = e.ty;
        let count = e.count;
        let offset = e.value_u32(t.little);
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
    let mut kept: Vec<[u8; 12]> = exif
        .iter()
        .filter(|e| e.tag != 0x927C)
        .map(|e| e.raw)
        .collect();
    kept.push(entry(t.little, 0x927C, 7, note.len(), note_at));
    out.extend_from_slice(&serialize(t.little, &kept, exif_next));
    let root_at = out.len();
    let mut kept_root: Vec<[u8; 12]> = root
        .iter()
        .filter(|e| e.tag != 0x8769)
        .map(|e| e.raw)
        .collect();
    kept_root.push(entry(t.little, 0x8769, 4, 1, exif_at));
    out.extend_from_slice(&serialize(t.little, &kept_root, root_next));
    out[4..8].copy_from_slice(&pack_u32(t.little, root_at));
    let mut result = EXIF_HDR.to_vec();
    result.extend_from_slice(&out);
    Ok(result)
}

/// Apply segment edits and fix MPF addresses after all header size changes.
pub fn edit_header(
    data: &[u8],
    replacements: &HashMap<usize, Vec<u8>>,
    insert: &[u8],
) -> Result<Vec<u8>> {
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
        if s.marker != 0xE2
            || !s.payload(data).starts_with(MPF_HDR)
            || replacements.contains_key(&s.start)
        {
            continue;
        }
        let old_base = s.start + 8;
        let new_base = mapped(s.start)? + 8;
        let t = Tiff::new(&s.payload(data)[4..])?;
        let (entries, _) = t.root()?;
        let entry = match tiff::find(&entries, 0xB002) {
            Some(e) => e,
            None => return fail("MPF has no MPEntry index"),
        };
        let count = entry.count;
        let offset = entry.value_u32(t.little);
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
                out[pos..pos + 4]
                    .copy_from_slice(&pack_u32(t.little, (size as isize + delta) as usize));
            } else if relative != 0 {
                let target = old_base + relative;
                if target + size > data.len()
                    || data.get(target..target + 2) != Some(&[0xFF, 0xD8][..])
                {
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
    let segs = segments(data)?;
    mpf_images_from_segments(data, &segs)
}

/// `mpf_images` over a precomputed segment list (perf item 3).
pub(crate) fn mpf_images_from_segments(data: &[u8], segs: &[Segment]) -> Result<Vec<Range<usize>>> {
    crate::jpeg::mpf_ranges(data, segs)
}
