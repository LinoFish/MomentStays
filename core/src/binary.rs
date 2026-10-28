//! Big/little-endian helpers mirroring `Binary` in native/PhotoFormat.swift.
use crate::{fail, Result};

pub fn read(d: &[u8], p: usize, count: usize, little: bool) -> Result<u64> {
    if count == 0 || count > 8 || p > d.len() || count > d.len() - p {
        return fail("文件元数据被截断");
    }
    let mut r: u64 = 0;
    for i in 0..count {
        r = (r << 8) | d[p + if little { count - i - 1 } else { i }] as u64;
    }
    Ok(r)
}

/// Same contract as Swift `Binary.int`: values above Int.max are rejected.
pub fn int(d: &[u8], p: usize, count: usize, little: bool) -> Result<usize> {
    let v = read(d, p, count, little)?;
    if v > i64::MAX as u64 || v > usize::MAX as u64 {
        return fail("文件偏移超出范围");
    }
    Ok(v as usize)
}

pub fn be(d: &[u8], p: usize, count: usize) -> Result<usize> { int(d, p, count, false) }
pub fn le(d: &[u8], p: usize, count: usize) -> Result<usize> { int(d, p, count, true) }

pub fn bytes(value: u64, count: usize, little: bool) -> Vec<u8> {
    (0..count)
        .map(|i| {
            let shift = 8 * if little { i } else { count - i - 1 };
            if shift >= 64 { 0 } else { (value >> shift) as u8 }
        })
        .collect()
}

pub fn put(value: usize, data: &mut [u8], at: usize, count: usize, little: bool) -> Result<()> {
    if at.checked_add(count).map_or(true, |e| e > data.len()) || (count == 4 && value > u32::MAX as usize) {
        return fail("无法安全修正文件偏移");
    }
    data[at..at + count].copy_from_slice(&bytes(value as u64, count, little));
    Ok(())
}

pub fn ascii(d: &[u8]) -> String { String::from_utf8_lossy(d).into_owned() }
