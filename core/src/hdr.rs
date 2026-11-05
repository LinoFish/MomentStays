//! Honor private EHDR framing. Recognising a resource does not interpret its
//! rendering parameters; conversion profiles live in `engine::hdr::honor`.
use crate::binary::le;
use crate::{fail, fail_as, ErrorKind, Result};

pub struct HonorEhdr<'a> {
    pub extension: &'a [u8],
    pub header: &'a [u8],
    /// Empty in the metadata-only variant; otherwise a complete JPEG.
    pub luma: &'a [u8],
}

pub fn honor_ehdr(data: &[u8], image_end: usize) -> Result<Option<HonorEhdr<'_>>> {
    let footer = b"HiHonor_ExtInfo\0\0\0\0\0";
    if !data.ends_with(footer) {
        return Ok(None);
    }
    if image_end > data.len() {
        return fail("荣耀私有 HDR 静态扩展范围无效，未丢弃原数据");
    }
    let count = data.len() - image_end;
    // A full-resolution luma JPEG can exceed 1 MiB even at 12 MP. Bound
    // parsing separately from the conversion profile's pixel/format checks.
    if !(193..=16 * 1024 * 1024).contains(&count) {
        return fail("荣耀私有 HDR 静态扩展范围无效，未丢弃原数据");
    }
    let tail = &data[image_end..];
    let name = count - 45;
    let size_at = name - 4;
    if tail[name..name + 20] != *b"HiHonor_EhdrEn\0\0\0\0\0\0"
        || le(tail, count - 24, 4)? != 1
        || tail[count - 25] != 0
        || le(tail, size_at - 4, 4)? != 128
    {
        return fail_as(
            ErrorKind::Unsupported,
            "荣耀私有 HDR 扩展版本尚不支持，未丢弃原数据",
        );
    }
    let size = le(tail, size_at, 4)?;
    let Some(payload) = size_at.checked_sub(size).filter(|_| size >= 132) else {
        return fail("荣耀私有 HDR 扩展长度无效，未丢弃原数据");
    };
    let Some(prefix) = payload.checked_sub(12).filter(|p| *p >= 4) else {
        return fail("荣耀私有 HDR 扩展长度无效，未丢弃原数据");
    };
    if &tail[payload - 8..payload] != b"CDRDATA\0"
        || le(tail, prefix, 4)? != prefix
        || le(tail, 0, 4)? > prefix
    {
        return fail("荣耀私有 HDR 扩展长度无效，未丢弃原数据");
    }
    let header_at = size_at - 132;
    let luma = &tail[payload..header_at];
    if !luma.is_empty() && crate::jpeg::segments(luma)?.last().map(|s| s.end) != Some(luma.len()) {
        return fail("荣耀私有 HDR 亮度 JPEG 范围无效，未丢弃原数据");
    }
    Ok(Some(HonorEhdr {
        extension: tail,
        header: &tail[header_at..header_at + 128],
        luma,
    }))
}

pub fn honor_private(data: &[u8], image_end: usize) -> Result<Option<&[u8]>> {
    Ok(honor_ehdr(data, image_end)?.map(|h| h.extension))
}
