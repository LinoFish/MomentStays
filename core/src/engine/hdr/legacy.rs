//! Legacy Apple gain interpretation; independent of live-photo pairing.
use crate::engine::{failure, Gray, Native};
use crate::Result;

/// Apple Rec.709-encoded linear boost to Ultra HDR log gain (Swift
/// `legacyGain` / `AndroidHDR.convert` lookup table).
pub fn gain_lookup(stops: f64) -> [u8; 256] {
    let headroom = 2f64.powf(stops);
    let mut t = [0u8; 256];
    for (i, slot) in t.iter_mut().enumerate() {
        let v = i as f64 / 255.0;
        let linear = if v < 0.081 {
            v / 4.5
        } else {
            ((v + 0.099) / 1.099).powf(1.0 / 0.45)
        };
        let g = ((1.0 + (headroom - 1.0) * linear).log2() / stops * 255.0).round();
        *slot = g.clamp(0.0, 255.0) as u8;
    }
    t
}

/// Map a gray gain image through [`gain_lookup`] (tight stride).
pub fn convert_gain(image: &Gray, stops: f64) -> Result<Gray> {
    if image.stride < image.width || image.pixels.len() / image.stride.max(1) < image.height {
        return failure("增益图数据被截断");
    }
    let lut = gain_lookup(stops);
    let mut pixels = vec![0u8; image.width * image.height];
    for y in 0..image.height {
        let row = &image.pixels[y * image.stride..y * image.stride + image.width];
        for (x, p) in row.iter().enumerate() {
            pixels[y * image.width + x] = lut[*p as usize];
        }
    }
    Ok(Gray {
        width: image.width,
        height: image.height,
        stride: image.width,
        pixels,
    })
}

/// Swift `legacyGain`: the converted gain-map JPEG and the map's size, or
/// Ok(None) = "unchanged" (not an 8-bit monochrome JPEG).
pub fn legacy_gain(
    native: &dyn Native,
    old_map: &[u8],
    stops: f64,
) -> Result<Option<(Vec<u8>, usize, usize)>> {
    if !(stops.is_finite() && stops > 0.0 && stops <= 16.0) {
        return Ok(None);
    }
    let Some(image) = native.decode_gray_jpeg(old_map)? else {
        return Ok(None);
    };
    let converted = convert_gain(&image, stops)?;
    Ok(Some((
        native.encode_gray_jpeg(&converted, 1.0)?,
        image.width,
        image.height,
    )))
}

use serde_json::{json, Map, Value};

/// The legacy-HDR branch of Swift `convert` (ISO upgrade or old Apple gain
/// map to Ultra HDR). Errors become warnings in the caller.
pub(super) fn apply(
    native: &dyn Native,
    image: &mut Vec<u8>,
    hdr: &mut Map<String, Value>,
) -> Result<()> {
    if crate::scan::has_iso(image)? {
        if let Some(upgraded) = super::upgrade_iso(image)? {
            *image = upgraded;
            hdr.insert("status".into(), json!("converted"));
            hdr.insert("output".into(), json!("ultra-hdr+iso"));
            hdr.insert(
                "reason".into(),
                json!("已有 ISO 增益图已补充 Ultra HDR 元数据；主图及增益图均未重新编码"),
            );
        } else {
            hdr.insert(
                "reason".into(),
                json!("ISO HDR 数据已保留；缺少可验证的单增益图布局，未推测转换"),
            );
        }
        return Ok(());
    }
    let (legacy, reason) = crate::apple_hdr::legacy(image)?;
    hdr.insert("reason".into(), json!(reason));
    let Some(legacy) = legacy else {
        return Ok(());
    };
    match super::legacy_gain(native, &legacy.gain, legacy.stops)? {
        Some((gain, width, height)) => {
            let icc = native.srgb_icc()?;
            let tagged = super::color_profile(image, &icc)?;
            *image = super::ultra(&tagged, &gain, legacy.stops)?;
            hdr.insert("status".into(), json!("converted"));
            hdr.insert("stops".into(), json!(legacy.stops));
            hdr.insert("width".into(), json!(width));
            hdr.insert("height".into(), json!(height));
            hdr.insert(
                "reason".into(),
                json!("已有旧 Apple 增益图已转换，主图未重新编码"),
            );
            hdr.insert("output".into(), json!("ultra-hdr"));
        }
        None => {
            hdr.insert(
                "reason".into(),
                json!("Legacy map cannot be decoded as an 8-bit monochrome JPEG"),
            );
        }
    }
    Ok(())
}
