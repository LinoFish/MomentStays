//! TIFF/IFD reader and the small writer helpers used for EXIF/MPF edits.
//! The single TIFF implementation of the crate (MPF indexes, EXIF Make/Model,
//! Apple MakerNote and ContentIdentifier rewrites).
use crate::binary::int;
use crate::{fail, Result};

#[derive(Clone, Debug)]
pub struct Field {
    pub tag: usize,
    pub ty: usize,
    pub count: usize,
    pub value: [u8; 4],
    /// The entry exactly as stored, for rewriting directories verbatim.
    pub raw: [u8; 12],
}

pub struct Tiff<'a> {
    pub data: &'a [u8],
    pub little: bool,
}

impl Field {
    pub fn offset(&self, little: bool) -> Result<usize> {
        int(&self.value, 0, 4, little)
    }
    /// The 4-byte value/offset field as an unsigned integer.
    pub fn value_u32(&self, little: bool) -> usize {
        u32_at(&self.value, 0, little) as usize
    }
}

/// First field with `tag`.
pub fn find(fields: &[Field], tag: usize) -> Option<&Field> {
    fields.iter().find(|f| f.tag == tag)
}

pub(crate) fn u16_at(d: &[u8], p: usize, little: bool) -> u16 {
    let b = [d[p], d[p + 1]];
    if little {
        u16::from_le_bytes(b)
    } else {
        u16::from_be_bytes(b)
    }
}

pub(crate) fn u32_at(d: &[u8], p: usize, little: bool) -> u32 {
    let b = [d[p], d[p + 1], d[p + 2], d[p + 3]];
    if little {
        u32::from_le_bytes(b)
    } else {
        u32::from_be_bytes(b)
    }
}

pub(crate) fn pack_u16(little: bool, v: usize) -> [u8; 2] {
    if little {
        (v as u16).to_le_bytes()
    } else {
        (v as u16).to_be_bytes()
    }
}

pub(crate) fn pack_u32(little: bool, v: usize) -> [u8; 4] {
    if little {
        (v as u32).to_le_bytes()
    } else {
        (v as u32).to_be_bytes()
    }
}

/// One 12-byte IFD entry.
pub fn entry(little: bool, tag: usize, ty: usize, count: usize, value: usize) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[0..2].copy_from_slice(&pack_u16(little, tag));
    out[2..4].copy_from_slice(&pack_u16(little, ty));
    out[4..8].copy_from_slice(&pack_u32(little, count));
    out[8..12].copy_from_slice(&pack_u32(little, value));
    out
}

/// An IFD: entry count, entries sorted by tag, next-IFD offset.
pub fn serialize(little: bool, entries: &[[u8; 12]], next_ifd: usize) -> Vec<u8> {
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|e| u16_at(e, 0, little));
    let mut out = Vec::with_capacity(2 + sorted.len() * 12 + 4);
    out.extend_from_slice(&pack_u16(little, sorted.len()));
    for e in &sorted {
        out.extend_from_slice(e);
    }
    out.extend_from_slice(&pack_u32(little, next_ifd));
    out
}

impl<'a> Tiff<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() < 8 {
            return fail("TIFF 头被截断");
        }
        if !(data[0..2] == [73, 73] || data[0..2] == [77, 77]) {
            return fail("TIFF 字节序无效");
        }
        let little = data[0] == 73;
        if int(data, 2, 2, little)? != 42 {
            return fail("TIFF 版本无效");
        }
        Ok(Tiff { data, little })
    }
    pub fn directory(&self, offset: usize) -> Result<(Vec<Field>, usize)> {
        if offset < 8 {
            return fail("TIFF 目录偏移无效");
        }
        let (d, little) = (self.data, self.little);
        let n = int(d, offset, 2, little)?;
        if offset.saturating_add(2 + n * 12 + 4) > d.len() {
            return fail("TIFF 目录被截断");
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let p = offset + 2 + i * 12;
            let mut raw = [0u8; 12];
            raw.copy_from_slice(&d[p..p + 12]);
            out.push(Field {
                tag: int(d, p, 2, little)?,
                ty: int(d, p + 2, 2, little)?,
                count: int(d, p + 4, 4, little)?,
                value: [d[p + 8], d[p + 9], d[p + 10], d[p + 11]],
                raw,
            });
        }
        Ok((out, int(d, offset + 2 + n * 12, 4, little)?))
    }
    pub fn root(&self) -> Result<(Vec<Field>, usize)> {
        self.directory(int(self.data, 4, 4, self.little)?)
    }
}
