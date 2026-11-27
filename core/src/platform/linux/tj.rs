//! libjpeg-turbo through its TurboJPEG 2.x C API (also exported by 3.x).
use crate::{Error, ErrorKind, Result};
use std::ffi::{c_char, c_int, c_ulong, c_void, CStr};

type Handle = *mut c_void;
extern "C" {
    fn tjInitDecompress() -> Handle;
    fn tjInitCompress() -> Handle;
    fn tjDestroy(h: Handle) -> c_int;
    fn tjDecompressHeader3(
        h: Handle,
        buf: *const u8,
        size: c_ulong,
        width: *mut c_int,
        height: *mut c_int,
        subsamp: *mut c_int,
        colorspace: *mut c_int,
    ) -> c_int;
    fn tjDecompress2(
        h: Handle,
        buf: *const u8,
        size: c_ulong,
        dst: *mut u8,
        width: c_int,
        pitch: c_int,
        height: c_int,
        format: c_int,
        flags: c_int,
    ) -> c_int;
    fn tjCompress2(
        h: Handle,
        src: *const u8,
        width: c_int,
        pitch: c_int,
        height: c_int,
        format: c_int,
        out: *mut *mut u8,
        size: *mut c_ulong,
        subsamp: c_int,
        quality: c_int,
        flags: c_int,
    ) -> c_int;
    fn tjFree(buf: *mut u8);
    fn tjGetErrorStr2(h: Handle) -> *const c_char;
    fn tjGetErrorCode(h: Handle) -> c_int;
}

pub const PF_RGB: c_int = 0;
pub const PF_GRAY: c_int = 6;
pub const PF_CMYK: c_int = 11;
pub const SAMP_444: c_int = 0;
pub const SAMP_420: c_int = 2;
pub const SAMP_GRAY: c_int = 3;
pub const CS_GRAY: c_int = 2;
pub const CS_CMYK: c_int = 3;
pub const CS_YCCK: c_int = 4;
const FLAG_ACCURATEDCT: c_int = 4096;
const ERR_WARNING: c_int = 0;
/// Same limit as the Apple backend's `dimensions`.
pub const MAX_SIDE: usize = 16384;

struct Tj(Handle);
impl Drop for Tj {
    fn drop(&mut self) {
        unsafe {
            tjDestroy(self.0);
        }
    }
}
impl Tj {
    fn decompress() -> Result<Tj> {
        let h = unsafe { tjInitDecompress() };
        if h.is_null() {
            return Err(Error::new("无法初始化 JPEG 解码器"));
        }
        Ok(Tj(h))
    }
    fn message(&self) -> String {
        unsafe {
            let p = tjGetErrorStr2(self.0);
            if p.is_null() {
                "JPEG 编解码失败".into()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub width: usize,
    pub height: usize,
    pub colorspace: c_int,
}

pub fn header(jpeg: &[u8]) -> Result<Header> {
    let tj = Tj::decompress()?;
    // The stored subsampling is not needed: re-encodes choose their own.
    let (mut w, mut h, mut s, mut c) = (0, 0, 0, 0);
    let rc = unsafe {
        tjDecompressHeader3(
            tj.0,
            jpeg.as_ptr(),
            jpeg.len() as c_ulong,
            &mut w,
            &mut h,
            &mut s,
            &mut c,
        )
    };
    if rc != 0 {
        return Err(Error::new(format!("无法读取图像：{}", tj.message())));
    }
    let (width, height) = (w.max(0) as usize, h.max(0) as usize);
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(Error::new("图像尺寸或行跨度无效"));
    }
    Ok(Header {
        width,
        height,
        colorspace: c,
    })
}

/// How a decode ended.
pub enum Outcome {
    Complete,
    /// libjpeg reported data missing at the end: ImageIO's
    /// "incomplete" status, which the Apple backend treats as failure.
    Truncated,
}

fn truncated(message: &str) -> bool {
    // libjpeg's JWRN_JPEG_EOF ("Premature end of JPEG file") and the
    // "Corrupt JPEG data: premature end of data segment" warning.
    message.to_ascii_lowercase().contains("premature end")
}

/// Decode into `format` at a scale that fits `width`×`height` (pass the
/// header size for full resolution). Returns pixels and the row stride.
pub fn decode(
    jpeg: &[u8],
    format: c_int,
    channels: usize,
    width: usize,
    height: usize,
) -> Result<(Vec<u8>, usize, Outcome)> {
    let tj = Tj::decompress()?;
    let pitch = width
        .checked_mul(channels)
        .ok_or_else(|| Error::new("图像尺寸或行跨度无效"))?;
    let mut out = vec![0u8; pitch * height];
    let rc = unsafe {
        tjDecompress2(
            tj.0,
            jpeg.as_ptr(),
            jpeg.len() as c_ulong,
            out.as_mut_ptr(),
            width as c_int,
            pitch as c_int,
            height as c_int,
            format,
            FLAG_ACCURATEDCT,
        )
    };
    if rc == 0 {
        return Ok((out, pitch, Outcome::Complete));
    }
    let message = tj.message();
    if unsafe { tjGetErrorCode(tj.0) } == ERR_WARNING {
        let outcome = if truncated(&message) {
            Outcome::Truncated
        } else {
            // Benign warnings (extraneous bytes before a marker, …): the
            // image decoded completely, as ImageIO also accepts them.
            Outcome::Complete
        };
        return Ok((out, pitch, outcome));
    }
    Err(Error::new(format!("图像解码失败：{message}")))
}

/// Full decode check at 1/8 scale: the whole entropy-coded stream is still
/// parsed (truncation and corruption are found), only the IDCT is cheaper.
pub fn decodes(jpeg: &[u8]) -> bool {
    let Ok(h) = header(jpeg) else {
        return false;
    };
    let (w, ht) = (h.width.div_ceil(8), h.height.div_ceil(8));
    let format = if h.colorspace == CS_CMYK || h.colorspace == CS_YCCK {
        PF_CMYK
    } else {
        PF_GRAY
    };
    let channels = if format == PF_CMYK { 4 } else { 1 };
    matches!(
        decode(jpeg, format, channels, w, ht),
        Ok((_, _, Outcome::Complete))
    )
}

/// Apple quality (0..=1) → libjpeg quality (1..=100). ImageIO's internal
/// mapping is not documented; linear is the documented approximation
/// (see core/LINUX_NATIVE.md).
pub fn quality(q: f64) -> Result<c_int> {
    if !q.is_finite() || !(0.0..=1.0).contains(&q) {
        return Err(Error::new("JPEG 质量无效"));
    }
    Ok(((q * 100.0).round() as c_int).clamp(1, 100))
}

pub fn encode(
    pixels: &[u8],
    width: usize,
    stride: usize,
    height: usize,
    format: c_int,
    subsamp: c_int,
    q: f64,
) -> Result<Vec<u8>> {
    let quality = quality(q)?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(Error::new("图像尺寸或行跨度无效"));
    }
    let h = unsafe { tjInitCompress() };
    if h.is_null() {
        return Err(Error::new("无法初始化 JPEG 编码器"));
    }
    let tj = Tj(h);
    let mut out: *mut u8 = std::ptr::null_mut();
    let mut size: c_ulong = 0;
    let rc = unsafe {
        tjCompress2(
            tj.0,
            pixels.as_ptr(),
            width as c_int,
            stride as c_int,
            height as c_int,
            format,
            &mut out,
            &mut size,
            subsamp,
            quality,
            FLAG_ACCURATEDCT,
        )
    };
    let result = if rc != 0 || out.is_null() {
        Err(Error::with_kind(
            ErrorKind::Malformed,
            format!("JPEG 编码失败：{}", tj.message()),
        ))
    } else {
        Ok(unsafe { std::slice::from_raw_parts(out, size as usize) }.to_vec())
    };
    if !out.is_null() {
        unsafe { tjFree(out) };
    }
    result
}
