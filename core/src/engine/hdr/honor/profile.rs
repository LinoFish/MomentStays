//! Frozen four-pair empirical mapping, evaluated on final HDR images.
//! Not the vendor's decoded rendering formula. No sample pixels or hashes.
use crate::engine::{failure, Gray, Rgba};
use crate::Result;

pub const PROFILE: &str = "honor-elp-an00-11.0.0.110-empirical-v3";
pub const STOPS: f64 = 1.943416;
pub const OFFSET: f64 = 0.0001;
pub const COEFFICIENTS: [f64; 4] = [
    0.31366626005407605,
    2.2095798608255417,
    -0.6920780058081931,
    0.0007934951143701933,
];
pub const MAX_CODE: u8 = 141;
pub(super) const FIRMWARE: &[u8] = b"ELP-AN00 11.0.0.110(SP5C00E110R103P1)\0";

// Only configurations evaluated with this frozen formula are accepted.
// u32@12 matches ISP ISO and u32@16 matches exposure records /10. Other
// fields remain incompletely understood; do not generalize by their ranges.
const HEADER_PREFIXES: [[u8; 24]; 4] = [
    [
        2, 1, 0, 0, 1, 1, 0, 0, 1, 2, 57, 1, 214, 0, 0, 0, 208, 7, 0, 0, 214, 0, 0, 0,
    ],
    [
        2, 1, 0, 0, 1, 1, 0, 0, 1, 4, 100, 0, 246, 1, 0, 0, 112, 23, 0, 0, 246, 1, 0, 0,
    ],
    [
        2, 1, 0, 0, 1, 1, 0, 0, 1, 4, 100, 0, 205, 10, 0, 0, 160, 15, 0, 0, 204, 10, 0, 0,
    ],
    [
        2, 1, 0, 0, 1, 1, 0, 0, 1, 4, 100, 0, 133, 4, 0, 0, 64, 31, 0, 0, 133, 4, 0, 0,
    ],
];

pub fn supports_header(header: &[u8]) -> bool {
    header.len() == 128 && HEADER_PREFIXES.iter().any(|p| header.starts_with(p))
}

pub fn supports_dimensions(width: usize, height: usize) -> bool {
    matches!((width, height), (3072, 4096) | (3000, 4000))
}

/// L is mean private code/255; Y is sRGB-code luma/255, not linear light.
pub fn log_gain(luma_code: f64, srgb_code_luma: f64) -> f64 {
    let [c0, c1, c2, c3] = COEFFICIENTS;
    c0 + c1 * luma_code + c2 * luma_code * luma_code + c3 * srgb_code_luma
}

/// Full-frame 4x4 private-code means and bilinear sRGB luma at block centers.
/// Source pixels are decoded only for this small term; they are not re-encoded.
pub fn map_luma(image: &Gray, base: &Rgba) -> Result<Gray> {
    let (w, h) = (image.width, image.height);
    let bad = || crate::Error::new("荣耀 EHDR 实验转换未执行：亮度图或 sRGB 主图像素范围无效");
    let rgba_row = w.checked_mul(4).ok_or_else(bad)?;
    if w == 0
        || h == 0
        || w > 4096
        || h > 4096
        || w % 4 != 0
        || h % 4 != 0
        || image.stride < w
        || image
            .stride
            .checked_mul(h)
            .is_none_or(|n| n > image.pixels.len())
        || (base.width, base.height) != (w, h)
        || base.stride < rgba_row
        || base
            .stride
            .checked_mul(h)
            .is_none_or(|n| n > base.pixels.len())
    {
        return Err(bad());
    }
    for y in 0..h {
        if image.pixels[y * image.stride..y * image.stride + w]
            .iter()
            .any(|&v| v > MAX_CODE)
        {
            return failure("荣耀 EHDR 实验转换未执行：亮度码值超出已验证的 0–141 范围");
        }
        if base.pixels[y * base.stride..y * base.stride + rgba_row]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] != 255)
        {
            return failure("荣耀 EHDR 实验转换未执行：主图必须是不透明 sRGB JPEG");
        }
    }
    let (width, height) = (w / 4, h / 4);
    let mut pixels = Vec::with_capacity(width * height);
    let code_luma = |x: usize, y: usize| {
        let p = &base.pixels[y * base.stride + x * 4..][..3];
        0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64
    };
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0u32;
            for dy in 0..4 {
                for dx in 0..4 {
                    sum += image.pixels[(4 * y + dy) * image.stride + 4 * x + dx] as u32;
                }
            }
            let row0 =
                0.5 * code_luma(4 * x + 1, 4 * y + 1) + 0.5 * code_luma(4 * x + 2, 4 * y + 1);
            let row1 =
                0.5 * code_luma(4 * x + 1, 4 * y + 2) + 0.5 * code_luma(4 * x + 2, 4 * y + 2);
            let gain = log_gain(
                sum as f64 / (16.0 * 255.0),
                (0.5 * row0 + 0.5 * row1) / 255.0,
            );
            pixels.push((gain / STOPS * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    Ok(Gray {
        width,
        height,
        stride: width,
        pixels,
    })
}
