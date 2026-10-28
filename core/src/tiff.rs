//! Minimal TIFF/IFD reader (MLTIFF).
use crate::binary::int;
use crate::{fail, Result};

#[derive(Clone, Debug)]
pub struct Field { pub tag: usize, pub ty: usize, pub count: usize, pub value: [u8; 4] }

pub struct Tiff<'a> { pub data: &'a [u8], pub little: bool }

impl Field {
    pub fn offset(&self, little: bool) -> Result<usize> { int(&self.value, 0, 4, little) }
}

impl<'a> Tiff<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() < 8 { return fail("TIFF 头被截断"); }
        if !(data[0..2] == [73, 73] || data[0..2] == [77, 77]) { return fail("TIFF 字节序无效"); }
        let little = data[0] == 73;
        if int(data, 2, 2, little)? != 42 { return fail("TIFF 版本无效"); }
        Ok(Tiff { data, little })
    }
    pub fn directory(&self, offset: usize) -> Result<(Vec<Field>, usize)> {
        if offset < 8 { return fail("TIFF 目录偏移无效"); }
        let (d, little) = (self.data, self.little);
        let n = int(d, offset, 2, little)?;
        if offset.saturating_add(2 + n * 12 + 4) > d.len() { return fail("TIFF 目录被截断"); }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let p = offset + 2 + i * 12;
            out.push(Field {
                tag: int(d, p, 2, little)?, ty: int(d, p + 2, 2, little)?, count: int(d, p + 4, 4, little)?,
                value: [d[p + 8], d[p + 9], d[p + 10], d[p + 11]],
            });
        }
        Ok((out, int(d, offset + 2 + n * 12, 4, little)?))
    }
    pub fn root(&self) -> Result<(Vec<Field>, usize)> { self.directory(int(self.data, 4, 4, self.little)?) }
}
