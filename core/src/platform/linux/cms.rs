//! Little CMS 2: the sRGB profile and conversions into sRGB.
use crate::{Error, Result};
use std::ffi::c_void;

type Profile = *mut c_void;
type Transform = *mut c_void;
extern "C" {
    fn cmsCreate_sRGBProfile() -> Profile;
    fn cmsOpenProfileFromMem(mem: *const c_void, size: u32) -> Profile;
    fn cmsCloseProfile(p: Profile) -> i32;
    fn cmsSetProfileVersion(p: Profile, version: f64);
    fn cmsSaveProfileToMem(p: Profile, mem: *mut c_void, needed: *mut u32) -> i32;
    fn cmsGetColorSpace(p: Profile) -> u32;
    fn cmsCreateTransform(
        input: Profile,
        in_format: u32,
        output: Profile,
        out_format: u32,
        intent: u32,
        flags: u32,
    ) -> Transform;
    fn cmsDoTransform(t: Transform, input: *const c_void, output: *mut c_void, pixels: u32);
    fn cmsDeleteTransform(t: Transform);
}

// lcms2.h: COLORSPACE_SH(PT_RGB=4 | PT_GRAY=3) | CHANNELS_SH(n) | BYTES_SH(1).
pub const TYPE_RGB_8: u32 = (4 << 16) | (3 << 3) | 1;
pub const TYPE_GRAY_8: u32 = (3 << 16) | (1 << 3) | 1;
const SIG_RGB: u32 = u32::from_be_bytes(*b"RGB ");
const SIG_GRAY: u32 = u32::from_be_bytes(*b"GRAY");
const INTENT_PERCEPTUAL: u32 = 0;

struct Owned(Profile);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            cmsCloseProfile(self.0);
        }
    }
}

fn srgb() -> Result<Owned> {
    let p = unsafe { cmsCreate_sRGBProfile() };
    if p.is_null() {
        return Err(Error::new("无法创建 sRGB 色彩描述"));
    }
    Ok(Owned(p))
}

/// Serialized sRGB profile. Written as ICC v2.1 (like the Apple
/// `kCGColorSpaceSRGB` data) for the widest decoder support; the bytes
/// differ from Apple's profile but describe the same colour space.
pub fn srgb_icc() -> Result<Vec<u8>> {
    let p = srgb()?;
    unsafe {
        cmsSetProfileVersion(p.0, 2.1);
        let mut size = 0u32;
        if cmsSaveProfileToMem(p.0, std::ptr::null_mut(), &mut size) == 0 || size == 0 {
            return Err(Error::new("无法生成 sRGB ICC"));
        }
        let mut out = vec![0u8; size as usize];
        if cmsSaveProfileToMem(p.0, out.as_mut_ptr().cast(), &mut size) == 0 {
            return Err(Error::new("无法生成 sRGB ICC"));
        }
        out.truncate(size as usize);
        Ok(out)
    }
}

/// Input layout of [`to_srgb`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Rgb,
    Gray,
}

/// Which input an embedded ICC profile describes, if lcms can open it.
pub fn profile_space(icc: &[u8]) -> Option<Input> {
    let p = unsafe { cmsOpenProfileFromMem(icc.as_ptr().cast(), icc.len() as u32) };
    if p.is_null() {
        return None;
    }
    let p = Owned(p);
    match unsafe { cmsGetColorSpace(p.0) } {
        SIG_RGB => Some(Input::Rgb),
        SIG_GRAY => Some(Input::Gray),
        _ => None,
    }
}

/// Convert packed 8-bit pixels described by `icc` into packed sRGB RGB.
pub fn to_srgb(icc: &[u8], input: Input, pixels: &[u8], count: usize) -> Result<Vec<u8>> {
    let src = unsafe { cmsOpenProfileFromMem(icc.as_ptr().cast(), icc.len() as u32) };
    if src.is_null() {
        return Err(Error::new("嵌入的 ICC 色彩描述无效"));
    }
    let src = Owned(src);
    let dst = srgb()?;
    let format = match input {
        Input::Rgb => TYPE_RGB_8,
        Input::Gray => TYPE_GRAY_8,
    };
    let channels = if input == Input::Rgb { 3 } else { 1 };
    if pixels.len() < count * channels || count > u32::MAX as usize {
        return Err(Error::new("像素缓冲区被截断"));
    }
    let t = unsafe { cmsCreateTransform(src.0, format, dst.0, TYPE_RGB_8, INTENT_PERCEPTUAL, 0) };
    if t.is_null() {
        return Err(Error::new("无法建立到 sRGB 的色彩转换"));
    }
    let mut out = vec![0u8; count * 3];
    unsafe {
        cmsDoTransform(
            t,
            pixels.as_ptr().cast(),
            out.as_mut_ptr().cast(),
            count as u32,
        );
        cmsDeleteTransform(t);
    }
    Ok(out)
}
