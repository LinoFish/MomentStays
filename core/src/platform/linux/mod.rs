//! Linux system codecs (`--features linux-native`): libjpeg-turbo, lcms2,
//! libheif and GStreamer, all linked dynamically and all present in the
//! GNOME 50 Flatpak runtime (HEVC decoding comes from the auto-installed
//! org.freedesktop.Platform.codecs-extra extension). Semantics follow the
//! Apple backend (`apple.rs`, `apple_video.rs`); known differences are in
//! core/LINUX_NATIVE.md. A missing decoder is reported, never a panic:
//! `image_decodes` answers None (unknown), the codec steps fail with
//! `ErrorKind::Unsupported`.
mod cms;
mod gst;
mod heif;
mod tj;

use super::portable;
use crate::engine::{Gray, HeicGain, Native, Rgba};
use crate::{fail, Error, ErrorKind, Result};
use std::path::Path;

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| Error::with_kind(ErrorKind::Io, e.to_string()))
}

fn is_jpeg(d: &[u8]) -> bool {
    d.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// Image formats ImageIO decodes that this backend does not check.
fn other_image(d: &[u8]) -> bool {
    d.starts_with(b"\x89PNG\r\n\x1a\n")
        || d.starts_with(b"GIF8")
        || d.starts_with(b"II*\0")
        || d.starts_with(b"MM\0*")
        || d.starts_with(b"BM")
        || (d.len() >= 12 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP")
}

fn incomplete<T>() -> Result<T> {
    fail("图像解码不完整")
}

fn heic_failure(f: heif::Failure) -> Error {
    f.into_error()
}

/// libheif applies `clap`, `irot` and `imir`; ImageIO returns the cropped
/// image in coded orientation (the orientation is a property). Undo the
/// rotation/mirror of `item` so both backends hand out the same axes.
fn coded(
    plane: heif::Plane,
    data: &[u8],
    item: Option<usize>,
    channels: usize,
) -> Result<heif::Plane> {
    let o = match item {
        Some(id) => portable::heif_item_orientation(data, id)?,
        None => portable::heif_primary_orientation(data)?,
    };
    let Some(o) = o.filter(|o| *o != portable::Orientation::IDENTITY) else {
        return Ok(plane);
    };
    let (pixels, width, height) = o.inverse().apply(
        &plane.pixels,
        plane.width,
        plane.height,
        plane.stride,
        channels,
    )?;
    Ok(heif::Plane {
        width,
        height,
        stride: width * channels,
        pixels,
    })
}

/// Apple gain map of a HEIC through libheif (see `heic_gain::read`).
pub(crate) fn heic_gain(data: &[u8]) -> Result<Option<crate::heic_gain::GainMap>> {
    let ctx = heif::Context::read(data).map_err(heic_failure)?;
    let primary = ctx.primary().map_err(heic_failure)?;
    let maps = primary
        .auxiliary(crate::apple_hdr::GAIN_MAP_TYPE)
        .map_err(heic_failure)?;
    let Some(map) = maps.first() else {
        // Like ImageIO's null result: a declared map must not become SDR.
        if memchr::memmem::find(data, b"hdrgainmap").is_some() {
            return fail("HEIC declares a gain map but libheif could not read it");
        }
        return Ok(None);
    };
    let mut result = crate::heic_gain::GainMap {
        versions: vec![],
        headrooms: vec![],
        image: None,
    };
    for (_, xml) in map.metadata("mime") {
        let end = xml.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
        let Ok(root) = crate::pyxml::parse(&xml[..end]) else {
            continue;
        };
        let ns = crate::xmp::APPLE;
        result
            .versions
            .extend(crate::pyxml::values(&root, ns, "HDRGainMapVersion"));
        for v in crate::pyxml::values(&root, ns, "HDRGainMapHeadroom") {
            let n: f64 = v
                .parse()
                .map_err(|_| Error::new("Invalid gain-map headroom"))?;
            if !n.is_finite() {
                return fail("Non-finite gain-map headroom");
            }
            result.headrooms.push(n);
        }
    }
    // ImageIO exposes only 8-bit one-component maps as L008 pixels.
    if map.luma_bits() != 8 {
        return Ok(Some(result));
    }
    let item = crate::apple_hdr::read(data)?
        .heic
        .and_then(|h| h.gain_map_item);
    let Some(item) = item else {
        return fail("HEIC gain-map item is ambiguous; not decoded");
    };
    let plane = coded(map.gray().map_err(heic_failure)?, data, Some(item), 1)?;
    result.image = Some(crate::heic_gain::Gray {
        width: plane.width,
        height: plane.height,
        stride: plane.stride,
        pixels: plane.pixels,
    });
    Ok(Some(result))
}

fn reencode_heic(data: &[u8], quality: f64) -> Result<Vec<u8>> {
    let ctx = heif::Context::read(data).map_err(heic_failure)?;
    let primary = ctx.primary().map_err(heic_failure)?;
    let rgb = coded(primary.rgb().map_err(heic_failure)?, data, None, 3)?;
    let mut segments = Vec::new();
    let mut exif = primary
        .metadata("Exif")
        .into_iter()
        .find_map(|(_, item)| portable::heif_exif_segment(&item));
    // Pixels stay in coded orientation; the orientation property says how
    // to show them (ImageIO derives it from irot/imir for HEIC).
    if let Some(o) = portable::heif_primary_orientation(data)? {
        match exif.as_mut() {
            Some(seg) => {
                portable::set_exif_orientation(seg, o.exif());
            }
            None if o.exif() != 1 => exif = Some(portable::exif_orientation_segment(o.exif())),
            None => {}
        }
    }
    segments.extend(exif);
    if let Some(icc) = primary.icc() {
        segments.extend(portable::icc_segments(&icc)?);
    }
    let encoded = tj::encode(
        &rgb.pixels,
        rgb.width,
        rgb.stride,
        rgb.height,
        tj::PF_RGB,
        tj::SAMP_420,
        quality,
    )?;
    portable::insert_segments(&encoded, &segments)
}

fn reencode_jpeg(data: &[u8], quality: f64) -> Result<Vec<u8>> {
    let h = tj::header(data)?;
    if h.colorspace == tj::CS_CMYK || h.colorspace == tj::CS_YCCK {
        return Err(Error::with_kind(
            ErrorKind::Unsupported,
            "CMYK JPEG 暂不支持重新编码",
        ));
    }
    let gray = h.colorspace == tj::CS_GRAY;
    let (format, channels, subsamp) = if gray {
        (tj::PF_GRAY, 1, tj::SAMP_GRAY)
    } else {
        (tj::PF_RGB, 3, tj::SAMP_420)
    };
    let (pixels, stride, outcome) = tj::decode(data, format, channels, h.width, h.height)?;
    if matches!(outcome, tj::Outcome::Truncated) {
        return incomplete();
    }
    let encoded = tj::encode(&pixels, h.width, stride, h.height, format, subsamp, quality)?;
    portable::insert_segments(&encoded, &portable::carried_segments(data)?)
}

pub struct SystemNative;
impl Native for SystemNative {
    fn audio_decodes(&self, path: &Path, expected_tracks: usize) -> Result<Option<bool>> {
        gst::audio_decodes(path, expected_tracks)
    }

    fn image_decodes(&self, data: &[u8]) -> Option<bool> {
        if is_jpeg(data) {
            return Some(tj::decodes(data));
        }
        if crate::heif::matches(data) {
            let ctx = match heif::Context::read(data) {
                Ok(c) => c,
                Err(heif::Failure::MissingDecoder(_)) => return None,
                Err(heif::Failure::Invalid(_)) => return Some(false),
            };
            let Ok(primary) = ctx.primary() else {
                return Some(false);
            };
            let (w, h) = primary.size();
            if w == 0 || h == 0 || w > tj::MAX_SIDE || h > tj::MAX_SIDE {
                return Some(false);
            }
            return match primary.decodes() {
                Ok(()) => Some(true),
                // No HEVC/AV1 decoder installed: unknown, not damaged.
                Err(heif::Failure::MissingDecoder(_)) => None,
                Err(heif::Failure::Invalid(_)) => Some(false),
            };
        }
        if other_image(data) {
            return None;
        }
        Some(false)
    }

    fn srgb_icc(&self) -> Result<Vec<u8>> {
        cms::srgb_icc()
    }

    fn decode_gray_jpeg(&self, jpeg: &[u8]) -> Result<Option<Gray>> {
        if !is_jpeg(jpeg) {
            return fail("无法读取图像");
        }
        let h = tj::header(jpeg)?;
        if h.colorspace != tj::CS_GRAY {
            return Ok(None);
        }
        let (pixels, stride, outcome) = tj::decode(jpeg, tj::PF_GRAY, 1, h.width, h.height)?;
        if matches!(outcome, tj::Outcome::Truncated) {
            return incomplete();
        }
        Ok(Some(Gray {
            width: h.width,
            height: h.height,
            stride,
            pixels,
        }))
    }

    fn decode_srgb_jpeg(&self, jpeg: &[u8]) -> Result<Option<Rgba>> {
        if !is_jpeg(jpeg) {
            return fail("无法读取图像");
        }
        let h = tj::header(jpeg)?;
        if h.colorspace == tj::CS_CMYK || h.colorspace == tj::CS_YCCK {
            return Err(Error::with_kind(
                ErrorKind::Unsupported,
                "CMYK JPEG 暂不支持转换为 sRGB",
            ));
        }
        let icc = portable::jpeg_icc(jpeg)?;
        let space = icc.as_deref().and_then(cms::profile_space);
        let gray = h.colorspace == tj::CS_GRAY;
        let count = h.width * h.height;
        let rgb = if gray && space == Some(cms::Input::Gray) {
            let (px, _, outcome) = tj::decode(jpeg, tj::PF_GRAY, 1, h.width, h.height)?;
            if matches!(outcome, tj::Outcome::Truncated) {
                return incomplete();
            }
            cms::to_srgb(icc.as_deref().unwrap(), cms::Input::Gray, &px, count)?
        } else {
            let (px, _, outcome) = tj::decode(jpeg, tj::PF_RGB, 3, h.width, h.height)?;
            if matches!(outcome, tj::Outcome::Truncated) {
                return incomplete();
            }
            match (gray, space) {
                (false, Some(cms::Input::Rgb)) => {
                    cms::to_srgb(icc.as_deref().unwrap(), cms::Input::Rgb, &px, count)?
                }
                // Untagged (or unusable profile): already treated as sRGB.
                _ => px,
            }
        };
        let mut pixels = Vec::with_capacity(count * 4);
        for p in rgb.as_chunks::<3>().0 {
            pixels.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        Ok(Some(Rgba {
            width: h.width,
            height: h.height,
            stride: h.width * 4,
            pixels,
        }))
    }

    fn encode_gray_jpeg(&self, image: &Gray, quality: f64) -> Result<Vec<u8>> {
        let n = image
            .stride
            .checked_mul(image.height)
            .filter(|&n| n <= 256 * 1024 * 1024);
        if image.width == 0
            || image.height == 0
            || image.width > tj::MAX_SIDE
            || image.height > tj::MAX_SIDE
            || image.stride < image.width
            || n.is_none()
        {
            return fail("图像尺寸或行跨度无效");
        }
        if image.pixels.len() < n.unwrap() {
            return fail("增益图像素被截断");
        }
        tj::encode(
            &image.pixels,
            image.width,
            image.stride,
            image.height,
            tj::PF_GRAY,
            tj::SAMP_GRAY,
            quality,
        )
    }

    fn reencode_jpeg(&self, path: &Path, quality: f64) -> Result<Vec<u8>> {
        tj::quality(quality)?;
        let data = read(path)?;
        if is_jpeg(&data) {
            reencode_jpeg(&data, quality)
        } else if crate::heif::matches(&data) {
            reencode_heic(&data, quality)
        } else {
            Err(Error::with_kind(
                ErrorKind::Unsupported,
                "此图像格式暂不支持重新编码",
            ))
        }
    }

    fn heic_gain_map(&self, path: &Path) -> Result<Option<HeicGain>> {
        let input = read(path)?;
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

    fn frame_jpeg(&self, video: &Path, track: usize, time: f64) -> Result<(Vec<u8>, u32, u32)> {
        gst::frame(video, track, time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_profile_is_icc_v2() {
        let icc = SystemNative.srgb_icc().unwrap();
        assert_eq!(&icc[36..40], b"acsp");
        assert_eq!(icc[8], 2);
        assert_eq!(&icc[16..20], b"RGB ");
        assert_eq!(cms::profile_space(&icc), Some(cms::Input::Rgb));
    }

    #[test]
    fn quality_mapping_is_linear_and_bounded() {
        assert_eq!(tj::quality(1.0).unwrap(), 100);
        assert_eq!(tj::quality(0.95).unwrap(), 95);
        assert_eq!(tj::quality(0.0).unwrap(), 1);
        assert!(tj::quality(f64::NAN).is_err());
        assert!(tj::quality(1.5).is_err());
    }

    #[test]
    fn unknown_bytes_do_not_decode_but_other_formats_are_unknown() {
        assert_eq!(SystemNative.image_decodes(b"not an image"), Some(false));
        assert_eq!(SystemNative.image_decodes(b"\x89PNG\r\n\x1a\nxxxx"), None);
    }
}
