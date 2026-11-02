//! Apple HEIC auxiliary gain-map pixels, read by Rust directly through ImageIO.
//! This module does not infer HDR parameters or convert unsupported maps.
//! Apple platforms only; no Swift callback or pixel re-encoding is involved.
use crate::{fail, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Gray {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub pixels: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct GainMap {
    pub versions: Vec<String>,
    pub headrooms: Vec<f64>,
    /// None for unsupported pixel formats. Never reinterpret them as gray8.
    pub image: Option<Gray>,
}

/// Read one HEIC's primary-image auxiliary map. Absence is distinct from a
/// malformed map. Returned pixels own their storage, including row padding.
/// `None` means no Apple map was exposed, not that the image is SDR. ISO/tmap
/// and HDR eligibility still require the caller's container/parameter checks.
pub fn read(data: &[u8]) -> Result<Option<GainMap>> {
    if !crate::heif::matches(data) {
        return fail("HEIC gain-map reader requires HEIC input");
    }
    // Reject invalid container boundaries before asking a system decoder.
    crate::heif::layout(data)?;
    #[cfg(target_vendor = "apple")]
    {
        apple::read(data)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        fail("HEIC gain-map pixel backend is unavailable on this platform")
    }
}

#[cfg(target_vendor = "apple")]
mod apple {
    use super::*;
    use std::{
        ffi::{c_char, c_void},
        ptr,
    };
    type Ref = *const c_void;
    const UTF8: u32 = 0x08000100;
    const MAX_PIXELS: usize = 16384 * 16384;
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: Ref);
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFDataGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFDataCreate(a: Ref, bytes: *const u8, length: isize) -> Ref;
        fn CFDataGetLength(data: Ref) -> isize;
        fn CFDataGetBytePtr(data: Ref) -> *const u8;
        fn CFDictionaryGetValue(dict: Ref, key: Ref) -> Ref;
        fn CFNumberGetValue(number: Ref, kind: isize, value: *mut c_void) -> u8;
        fn CFStringCreateWithBytes(
            a: Ref,
            bytes: *const u8,
            length: isize,
            encoding: u32,
            external: u8,
        ) -> Ref;
        fn CFStringGetLength(value: Ref) -> isize;
        fn CFStringGetCString(value: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
        fn CFArrayGetCount(array: Ref) -> isize;
        fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
    }
    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        static kCGImageAuxiliaryDataTypeHDRGainMap: Ref;
        static kCGImageAuxiliaryDataInfoData: Ref;
        static kCGImageAuxiliaryDataInfoDataDescription: Ref;
        static kCGImageAuxiliaryDataInfoMetadata: Ref;
        fn CGImageSourceCreateWithData(data: Ref, options: Ref) -> Ref;
        fn CGImageSourceCopyAuxiliaryDataInfoAtIndex(source: Ref, index: usize, kind: Ref) -> Ref;
        fn CGImageMetadataGetTypeID() -> usize;
        fn CGImageMetadataCopyTags(metadata: Ref) -> Ref;
        fn CGImageMetadataTagCopyNamespace(tag: Ref) -> Ref;
        fn CGImageMetadataTagCopyName(tag: Ref) -> Ref;
        fn CGImageMetadataTagCopyValue(tag: Ref) -> Ref;
    }
    // Create/Copy rules: every non-null owned CF reference is released once.
    struct Owned(Ref);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) }
            }
        }
    }
    unsafe fn typed(value: Ref, id: usize) -> bool {
        !value.is_null() && CFGetTypeID(value) == id
    }
    unsafe fn string(value: Ref) -> Option<String> {
        if !typed(value, CFStringGetTypeID()) {
            return None;
        }
        let length = usize::try_from(CFStringGetLength(value)).ok()?;
        // Metadata text must not trigger unbounded secondary allocations.
        if length > 65536 {
            return None;
        }
        let mut bytes = vec![0u8; length.checked_mul(4)?.checked_add(1)?];
        if CFStringGetCString(value, bytes.as_mut_ptr().cast(), bytes.len() as isize, UTF8) == 0 {
            return None;
        }
        bytes.truncate(bytes.iter().position(|b| *b == 0)?);
        String::from_utf8(bytes).ok()
    }
    unsafe fn integer(dict: Ref, key: &str) -> Option<usize> {
        let key = Owned(CFStringCreateWithBytes(
            ptr::null(),
            key.as_ptr(),
            key.len() as isize,
            UTF8,
            0,
        ));
        if key.0.is_null() {
            return None;
        }
        let number = CFDictionaryGetValue(dict, key.0);
        if !typed(number, CFNumberGetTypeID()) {
            return None;
        }
        let mut n = 0i64;
        // kCFNumberSInt64Type = 4. Reject non-integral and negative values.
        if CFNumberGetValue(number, 4, (&mut n as *mut i64).cast()) == 0 {
            return None;
        }
        usize::try_from(n).ok()
    }
    pub(super) fn read(data: &[u8]) -> Result<Option<GainMap>> {
        let length =
            isize::try_from(data.len()).map_err(|_| crate::Error::new("HEIC too large"))?;
        // CFData copies the input; no borrowed pointer can escape this call.
        unsafe {
            let bytes = Owned(CFDataCreate(ptr::null(), data.as_ptr(), length));
            if bytes.0.is_null() {
                return fail("Cannot allocate HEIC input");
            }
            let source = Owned(CGImageSourceCreateWithData(bytes.0, ptr::null()));
            if source.0.is_null() {
                return fail("ImageIO cannot open HEIC");
            }
            let aux = Owned(CGImageSourceCopyAuxiliaryDataInfoAtIndex(
                source.0,
                0,
                kCGImageAuxiliaryDataTypeHDRGainMap,
            ));
            if aux.0.is_null() {
                // ImageIO can also return null when its decoder service fails.
                // A declared gain map must never silently become SDR.
                if memchr::memmem::find(data, b"hdrgainmap").is_some() {
                    return fail("HEIC declares a gain map but ImageIO could not read it");
                }
                return Ok(None);
            }
            if !typed(aux.0, CFDictionaryGetTypeID()) {
                return fail("Invalid HEIC auxiliary dictionary");
            }
            let mut result = GainMap {
                versions: vec![],
                headrooms: vec![],
                image: None,
            };
            let metadata = CFDictionaryGetValue(aux.0, kCGImageAuxiliaryDataInfoMetadata);
            if typed(metadata, CGImageMetadataGetTypeID()) {
                let tags = Owned(CGImageMetadataCopyTags(metadata));
                if !tags.0.is_null() {
                    let count = CFArrayGetCount(tags.0);
                    if !(0..=4096).contains(&count) {
                        return fail("Too many gain-map metadata tags");
                    }
                    for i in 0..count {
                        let tag = CFArrayGetValueAtIndex(tags.0, i);
                        let ns = Owned(CGImageMetadataTagCopyNamespace(tag));
                        if string(ns.0).as_deref() != Some("http://ns.apple.com/HDRGainMap/1.0/") {
                            continue;
                        }
                        let name = Owned(CGImageMetadataTagCopyName(tag));
                        let value = Owned(CGImageMetadataTagCopyValue(tag));
                        let text = string(value.0).or_else(|| {
                            if !typed(value.0, CFNumberGetTypeID()) {
                                return None;
                            }
                            let mut n = 0f64;
                            (CFNumberGetValue(value.0, 6, (&mut n as *mut f64).cast()) != 0)
                                .then(|| n.to_string())
                        });
                        match (string(name.0).as_deref(), text) {
                            (Some("HDRGainMapVersion"), Some(v)) => result.versions.push(v),
                            (Some("HDRGainMapHeadroom"), Some(v)) => {
                                let n: f64 = v
                                    .parse()
                                    .map_err(|_| crate::Error::new("Invalid gain-map headroom"))?;
                                if !n.is_finite() {
                                    return fail("Non-finite gain-map headroom");
                                }
                                result.headrooms.push(n);
                            }
                            _ => {}
                        }
                    }
                }
            }
            let desc = CFDictionaryGetValue(aux.0, kCGImageAuxiliaryDataInfoDataDescription);
            if !typed(desc, CFDictionaryGetTypeID()) {
                return fail("Missing gain-map description");
            }
            if integer(desc, "PixelFormat") != Some(u32::from_be_bytes(*b"L008") as usize) {
                return Ok(Some(result));
            }
            let width = integer(desc, "Width").unwrap_or(0);
            let height = integer(desc, "Height").unwrap_or(0);
            let stride = integer(desc, "BytesPerRow").unwrap_or(0);
            let raw = CFDictionaryGetValue(aux.0, kCGImageAuxiliaryDataInfoData);
            if !typed(raw, CFDataGetTypeID()) {
                return fail("Missing gain-map pixels");
            }
            let count = usize::try_from(CFDataGetLength(raw)).unwrap_or(0);
            validate(width, height, stride, count)?;
            let size = stride * height;
            let pointer = CFDataGetBytePtr(raw);
            if pointer.is_null() {
                return fail("Missing gain-map pixel storage");
            }
            result.image = Some(Gray {
                width,
                height,
                stride,
                pixels: std::slice::from_raw_parts(pointer, size).to_vec(),
            });
            Ok(Some(result))
        }
    }
    fn validate(width: usize, height: usize, stride: usize, count: usize) -> Result<()> {
        if width == 0 || height == 0 || width > 16384 || height > 16384 || stride < width {
            return fail("Invalid gain-map dimensions");
        }
        match stride.checked_mul(height) {
            Some(size) if size <= count && size <= MAX_PIXELS => Ok(()),
            _ => fail("Truncated or oversized gain-map pixels"),
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn rejects_bad_layouts() {
            for (w, h, s, n) in [
                (0, 1, 1, 1),
                (2, 1, 1, 2),
                (1, 2, 1, 1),
                (1, 2, usize::MAX, usize::MAX),
                (16385, 1, 16385, 16385),
            ] {
                assert!(validate(w, h, s, n).is_err());
            }
            assert!(validate(2, 2, 4, 8).is_ok());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_non_heic() {
        assert!(super::read(b"not an image").is_err());
    }
    #[test]
    fn rejects_truncated_heic_before_decode() {
        let mut data = 24u32.to_be_bytes().to_vec();
        data.extend_from_slice(b"ftypheic");
        data.extend_from_slice(&[0; 4]);
        assert!(super::read(&data).is_err());
    }
}
