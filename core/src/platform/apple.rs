//! Direct ImageIO/CoreGraphics codecs; no Swift callbacks or application policy.
use crate::engine::{Gray, HeicGain, Native, Rgba};
use crate::{Error, Result};
use std::{ffi::c_void, path::Path, ptr};
type Ref = *const c_void;
const MAX_BYTES: usize = 256 * 1024 * 1024;
#[repr(C)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}
#[repr(C)]
struct Rect {
    origin: Point,
    size: Size,
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(v: Ref);
    fn CFDataCreate(a: Ref, p: *const u8, n: isize) -> Ref;
    fn CFDataCreateMutable(a: Ref, n: isize) -> Ref;
    fn CFDataGetLength(v: Ref) -> isize;
    fn CFDataGetBytePtr(v: Ref) -> *const u8;
    fn CFStringCreateWithBytes(a: Ref, p: *const u8, n: isize, enc: u32, ext: u8) -> Ref;
    fn CFNumberCreate(a: Ref, kind: isize, p: *const c_void) -> Ref;
    fn CFDictionaryCreateMutableCopy(a: Ref, n: isize, dict: Ref) -> Ref;
    fn CFDictionarySetValue(dict: Ref, key: Ref, val: Ref);
}
#[link(name = "ImageIO", kind = "framework")]
extern "C" {
    static kCGImageDestinationLossyCompressionQuality: Ref;
    fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
    fn CGImageSourceCreateImageAtIndex(src: Ref, i: usize, opts: Ref) -> Ref;
    fn CGImageSourceGetStatusAtIndex(src: Ref, i: usize) -> i32;
    fn CGImageSourceCopyPropertiesAtIndex(src: Ref, i: usize, opts: Ref) -> Ref;
    fn CGImageDestinationCreateWithData(data: Ref, kind: Ref, n: usize, opts: Ref) -> Ref;
    fn CGImageDestinationAddImage(dest: Ref, image: Ref, properties: Ref);
    fn CGImageDestinationFinalize(dest: Ref) -> u8;
}
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    static kCGColorSpaceSRGB: Ref;
    fn CGColorSpaceCreateWithName(name: Ref) -> Ref;
    fn CGColorSpaceCreateDeviceGray() -> Ref;
    fn CGColorSpaceCopyICCData(space: Ref) -> Ref;
    fn CGImageGetWidth(image: Ref) -> usize;
    fn CGImageGetHeight(image: Ref) -> usize;
    fn CGImageGetBitsPerComponent(image: Ref) -> usize;
    fn CGImageGetBitsPerPixel(image: Ref) -> usize;
    fn CGImageGetBytesPerRow(image: Ref) -> usize;
    fn CGImageGetDataProvider(image: Ref) -> Ref;
    fn CGDataProviderCopyData(provider: Ref) -> Ref;
    fn CGDataProviderCreateWithCFData(data: Ref) -> Ref;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        stride: usize,
        space: Ref,
        bitmap: u32,
    ) -> Ref;
    fn CGContextDrawImage(context: Ref, rect: Rect, image: Ref);
    fn CGContextRelease(context: Ref);
    fn CGImageCreate(
        w: usize,
        h: usize,
        bpc: usize,
        bpp: usize,
        stride: usize,
        space: Ref,
        bitmap: u32,
        provider: Ref,
        decode: *const f64,
        interpolate: u8,
        intent: i32,
    ) -> Ref;
}
struct Owned(Ref);
struct BitmapContext(Ref);
impl Drop for BitmapContext {
    fn drop(&mut self) {
        unsafe { CGContextRelease(self.0) }
    }
}
impl Owned {
    fn new(v: Ref, what: &str) -> Result<Self> {
        if v.is_null() {
            Err(Error::new(format!("Apple codec: {what}")))
        } else {
            Ok(Self(v))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}
fn data(bytes: &[u8]) -> Result<Owned> {
    let len = isize::try_from(bytes.len()).map_err(|_| Error::new("图像数据过大"))?;
    unsafe { Owned::new(CFDataCreate(ptr::null(), bytes.as_ptr(), len), "CFData") }
}
unsafe fn bytes(v: Ref) -> Result<Vec<u8>> {
    let len = usize::try_from(CFDataGetLength(v)).map_err(|_| Error::new("图像长度无效"))?;
    if len > MAX_BYTES {
        return Err(Error::new("图像超过 256MiB 限制"));
    }
    if len == 0 {
        return Ok(Vec::new());
    }
    let p = CFDataGetBytePtr(v);
    if p.is_null() {
        return Err(Error::new("图像数据缺失"));
    }
    Ok(std::slice::from_raw_parts(p, len).to_vec())
}
fn source(input: &[u8]) -> Result<Owned> {
    let d = data(input)?;
    unsafe {
        Owned::new(
            CGImageSourceCreateWithData(d.0, ptr::null()),
            "无法读取图像",
        )
    }
}
fn dimensions(w: usize, h: usize, stride: usize) -> Result<usize> {
    let n = stride.checked_mul(h).filter(|&n| n <= MAX_BYTES);
    if w == 0 || h == 0 || w > 16384 || h > 16384 || stride < w || n.is_none() {
        return Err(Error::new("图像尺寸或行跨度无效"));
    }
    Ok(n.unwrap())
}
unsafe fn image(src: Ref) -> Result<Owned> {
    let img = Owned::new(
        CGImageSourceCreateImageAtIndex(src, 0, ptr::null()),
        "图像解码失败",
    )?;
    dimensions(
        CGImageGetWidth(img.0),
        CGImageGetHeight(img.0),
        CGImageGetWidth(img.0),
    )?;
    if CGImageSourceGetStatusAtIndex(src, 0) != 0 {
        return Err(Error::new("图像解码不完整"));
    }
    Ok(img)
}
unsafe fn jpeg(img: Ref, quality: f64, props: Ref) -> Result<Vec<u8>> {
    if !quality.is_finite() || !(0.0..=1.0).contains(&quality) {
        return Err(Error::new("JPEG 质量无效"));
    }
    let out = Owned::new(CFDataCreateMutable(ptr::null(), 0), "JPEG data")?;
    let kind = b"public.jpeg";
    let kind = Owned::new(
        CFStringCreateWithBytes(
            ptr::null(),
            kind.as_ptr(),
            kind.len() as isize,
            0x08000100,
            0,
        ),
        "JPEG type",
    )?;
    let dest = Owned::new(
        CGImageDestinationCreateWithData(out.0, kind.0, 1, ptr::null()),
        "无法创建 JPEG",
    )?;
    // Start with source properties so EXIF/orientation/ICC are retained.
    // A tiny dictionary with standard CF callbacks is created by copying an
    // empty ImageIO properties dictionary when encoding a newly created image.
    let base;
    let props = if props.is_null() {
        // XML property-list dictionary avoids relying on CF callback struct ABI.
        base = empty_dictionary()?;
        base.0
    } else {
        props
    };
    let options = Owned::new(
        CFDictionaryCreateMutableCopy(ptr::null(), 0, props),
        "JPEG options",
    )?;
    let q = Owned::new(
        CFNumberCreate(ptr::null(), 6, (&quality as *const f64).cast()),
        "JPEG quality",
    )?;
    CFDictionarySetValue(options.0, kCGImageDestinationLossyCompressionQuality, q.0);
    CGImageDestinationAddImage(dest.0, img, options.0);
    if CGImageDestinationFinalize(dest.0) == 0 {
        return Err(Error::new("JPEG 编码失败"));
    }
    bytes(out.0)
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFPropertyListCreateWithData(
        a: Ref,
        data: Ref,
        options: usize,
        format: *mut isize,
        err: *mut Ref,
    ) -> Ref;
}
unsafe fn empty_dictionary() -> Result<Owned> {
    let d = data(b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict/></plist>")?;
    Owned::new(
        CFPropertyListCreateWithData(ptr::null(), d.0, 0, ptr::null_mut(), ptr::null_mut()),
        "properties",
    )
}
pub struct SystemNative;
impl Native for SystemNative {
    fn audio_decodes(&self, path: &Path, expected_tracks: usize) -> Result<Option<bool>> {
        video::audio_decodes(path, expected_tracks).map(Some)
    }
    fn image_decodes(&self, input: &[u8]) -> Option<bool> {
        Some(source(input).and_then(|s| unsafe { image(s.0) }).is_ok())
    }
    fn srgb_icc(&self) -> Result<Vec<u8>> {
        unsafe {
            let space = Owned::new(CGColorSpaceCreateWithName(kCGColorSpaceSRGB), "sRGB")?;
            let icc = Owned::new(CGColorSpaceCopyICCData(space.0), "sRGB ICC")?;
            bytes(icc.0)
        }
    }
    fn decode_gray_jpeg(&self, input: &[u8]) -> Result<Option<Gray>> {
        let src = source(input)?;
        unsafe {
            let img = image(src.0)?;
            if CGImageGetBitsPerComponent(img.0) != 8 || CGImageGetBitsPerPixel(img.0) != 8 {
                return Ok(None);
            }
            let (w, h, s) = (
                CGImageGetWidth(img.0),
                CGImageGetHeight(img.0),
                CGImageGetBytesPerRow(img.0),
            );
            let n = dimensions(w, h, s)?;
            let provider = CGImageGetDataProvider(img.0);
            if provider.is_null() {
                return Err(Error::new("增益图缺少像素"));
            }
            let pixels = Owned::new(CGDataProviderCopyData(provider), "增益图像素")?;
            let pixels = bytes(pixels.0)?;
            if pixels.len() < n {
                return Err(Error::new("增益图像素被截断"));
            }
            Ok(Some(Gray {
                width: w,
                height: h,
                stride: s,
                pixels,
            }))
        }
    }
    fn decode_srgb_jpeg(&self, input: &[u8]) -> Result<Option<Rgba>> {
        let src = source(input)?;
        unsafe {
            let img = image(src.0)?;
            if CGImageGetBitsPerComponent(img.0) != 8 {
                return Ok(None);
            }
            let (width, height) = (CGImageGetWidth(img.0), CGImageGetHeight(img.0));
            let stride = width
                .checked_mul(4)
                .ok_or_else(|| Error::new("主图行跨度溢出"))?;
            let count = dimensions(width, height, stride)?;
            let mut pixels = vec![0u8; count];
            let space = Owned::new(CGColorSpaceCreateWithName(kCGColorSpaceSRGB), "sRGB")?;
            // Match the validated ImageIO probe: sRGB, RGBA bytes, opaque JPEG,
            // no EXIF transform, no row reversal and no source re-encoding.
            let context = CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                width,
                height,
                8,
                stride,
                space.0,
                0x4001, // kCGBitmapByteOrder32Big | kCGImageAlphaPremultipliedLast
            );
            if context.is_null() {
                return Err(Error::new("无法创建 sRGB 主图解码上下文"));
            }
            let context = BitmapContext(context);
            CGContextDrawImage(
                context.0,
                Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: width as f64,
                        height: height as f64,
                    },
                },
                img.0,
            );
            drop(context);
            Ok(Some(Rgba {
                width,
                height,
                stride,
                pixels,
            }))
        }
    }
    fn encode_gray_jpeg(&self, gray: &Gray, quality: f64) -> Result<Vec<u8>> {
        let n = dimensions(gray.width, gray.height, gray.stride)?;
        if gray.pixels.len() < n {
            return Err(Error::new("增益图像素被截断"));
        }
        let d = data(&gray.pixels[..n])?;
        unsafe {
            let provider = Owned::new(CGDataProviderCreateWithCFData(d.0), "增益图 provider")?;
            let space = Owned::new(CGColorSpaceCreateDeviceGray(), "灰度色彩空间")?;
            let img = Owned::new(
                CGImageCreate(
                    gray.width,
                    gray.height,
                    8,
                    8,
                    gray.stride,
                    space.0,
                    0,
                    provider.0,
                    ptr::null(),
                    0,
                    0,
                ),
                "灰度图",
            )?;
            jpeg(img.0, quality, ptr::null())
        }
    }
    fn reencode_jpeg(&self, path: &Path, quality: f64) -> Result<Vec<u8>> {
        let input = std::fs::read(path).map_err(|e| Error::new(e.to_string()))?;
        let src = source(&input)?;
        unsafe {
            let img = image(src.0)?;
            let props = Owned::new(
                CGImageSourceCopyPropertiesAtIndex(src.0, 0, ptr::null()),
                "图像属性",
            )?;
            jpeg(img.0, quality, props.0)
        }
    }
    fn frame_jpeg(&self, path: &Path, track: usize, time: f64) -> Result<(Vec<u8>, u32, u32)> {
        video::frame(path, track, time)
    }
    fn heic_gain_map(&self, path: &Path) -> Result<Option<HeicGain>> {
        let input = std::fs::read(path).map_err(|e| Error::new(e.to_string()))?;
        Ok(crate::heic_gain::read(&input)?.map(|m| HeicGain {
            versions: m.versions,
            headrooms: m.headrooms,
            image: m.image.map(|g| Gray {
                width: g.width,
                height: g.height,
                stride: g.stride,
                pixels: g.pixels,
            }),
        }))
    }
}

#[path = "apple_video.rs"]
mod video;
