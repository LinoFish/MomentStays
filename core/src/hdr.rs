//! Honor private EHDR tail recogniser (PhotoHDR.honorPrivate).
use crate::binary::le;
use crate::{fail, Result};

pub fn honor_private(data: &[u8], image_end: usize) -> Result<Option<&[u8]>> {
    let mut footer = b"HiHonor_ExtInfo".to_vec();
    footer.extend_from_slice(&[0; 5]);
    if data.len() < footer.len() || data[data.len() - footer.len()..] != footer[..] { return Ok(None); }
    if image_end > data.len() { return fail("荣耀私有 HDR 静态扩展范围无效，未丢弃原数据"); }
    let count = data.len() - image_end;
    if !(193..=1_048_576).contains(&count) { return fail("荣耀私有 HDR 静态扩展范围无效，未丢弃原数据"); }
    let tail = &data[image_end..];
    let name = count - 45;
    let size_at = name - 4;
    let mut en = b"HiHonor_EhdrEn".to_vec();
    en.extend_from_slice(&[0; 6]);
    let ok = tail[name..name + 20] == en[..]
        && le(tail, count - 24, 4)? == 1
        && tail[count - 25] == 0
        && le(tail, size_at, 4)? == 132
        && le(tail, size_at - 4, 4)? == 128;
    if !ok { return fail("荣耀私有 HDR 扩展版本尚不支持，未丢弃原数据"); }
    let payload = size_at as isize - 132;
    let prefix = payload - 12;
    if prefix < 4 { return fail("荣耀私有 HDR 扩展长度无效，未丢弃原数据"); }
    let (payload, prefix) = (payload as usize, prefix as usize);
    if &tail[payload - 8..payload] != b"CDRDATA\0" || le(tail, prefix, 4)? != prefix || le(tail, 0, 4)? > prefix {
        return fail("荣耀私有 HDR 扩展长度无效，未丢弃原数据");
    }
    Ok(Some(tail))
}
