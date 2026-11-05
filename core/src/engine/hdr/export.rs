//! Still/HDR preparation for export; no pairing, movie or vendor assembly.
use super::convert_gain;
use crate::engine::{failure, read, Native};
use crate::{apple_hdr, heif, jpeg, xmp, Error, Result};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub struct ExportOptions {
    pub preserve_heic: bool,
    pub preserve_jpeg: bool,
    pub image_quality: f64,
    pub gain_quality: f64,
}

pub(crate) fn is_ultra(original: &[u8]) -> Result<bool> {
    let has = |d: &[u8]| -> Result<bool> {
        Ok(xmp::packets(d)?
            .iter()
            .any(|p| !p.values(xmp::HDR, "Version").is_empty()))
    };
    if has(original)? {
        return Ok(true);
    }
    for r in jpeg::auxiliary(original)?.iter().skip(1) {
        if has(&original[r.clone()]).unwrap_or(false) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Swift `AndroidHDR.prepare`: the still to embed and its HDR report.
pub fn prepare_export(
    native: &dyn Native,
    path: &Path,
    options: &ExportOptions,
) -> Result<(Vec<u8>, Value)> {
    let q_ok = |q: f64| q > 0.0 && q <= 1.0;
    if !(q_ok(options.image_quality) && q_ok(options.gain_quality)) {
        return failure("JPEG 质量需在 1–100% 之间");
    }
    let original = read(path)?;
    let heic = heif::matches(&original);
    if options.preserve_heic {
        if !heic {
            return failure("保留原编码的 HEIC 输出需要 HEIC 原图；未将 JPEG 再编码成 HEIC");
        }
        heif::layout(&original)?;
        return Ok((
            original,
            json!({"status": "preserved", "input": "heic", "reason": "HEIC 全部原始图像/增益图编码及属性保留；Apple 私有 HDR 的安卓渲染兼容性未保证"}),
        ));
    }
    let mut base = if !heic && options.preserve_jpeg {
        jpeg::static_only(&original)?
    } else {
        native.reencode_jpeg(path, options.image_quality)?
    };
    if !heic {
        let maps = jpeg::auxiliary(&original)?;
        let ultra = is_ultra(&original)?;
        if ultra && options.preserve_jpeg {
            return Ok((
                base,
                json!({"status": "preserved", "input": "ultra-hdr", "reason": "已有 Ultra HDR 原始 JPEG 和增益图保留；质量设置仅在重新编码时生效"}),
            ));
        }
        if ultra {
            if maps.len() != 2 {
                return failure("Ultra HDR 增益图布局不受支持");
            }
            base = super::pack_gain_map(&base, &original[maps[1].clone()])?;
            return Ok((
                base,
                json!({"status": "preserved-gainmap", "reason": "主图按指定质量重编码，已有 Ultra HDR 增益图与参数保留"}),
            ));
        }
        if let (Some(legacy), _) = apple_hdr::legacy(&original)? {
            let Some(image) = native.decode_gray_jpeg(&legacy.gain)? else {
                return failure("Apple HDR 需要 8 位单通道增益图");
            };
            let gain = native
                .encode_gray_jpeg(&convert_gain(&image, legacy.stops)?, options.gain_quality)?;
            return Ok((
                super::ultra(&base, &gain, legacy.stops)?,
                json!({"status": "converted", "input": "apple-hdr", "output": "ultra-hdr", "reason": "已有 Apple 增益图按 Rec.709 模型转为 Ultra HDR"}),
            ));
        }
        if maps.len() > 1 {
            return failure("原始 JPEG 含未知辅助图/HDR 参数，未丢弃或推测");
        }
        return Ok((base, json!({"status": "sdr", "reason": "SDR 不生成增益图"})));
    }
    let Some(gain) = native.heic_gain_map(path)? else {
        heif::layout_lenient(&original)?;
        if apple_hdr::read(&original)?.status != apple_hdr::Status::None {
            return failure("此 HEIC HDR 不能在 macOS 12 安全提取；请保留 HEIC 原编码，或提供支持的 Apple HDR 增益图");
        }
        return Ok((
            base,
            json!({"status": "sdr", "reason": "未检测到可恢复 HDR；不生成增益图"}),
        ));
    };
    if gain.versions.is_empty() {
        return failure("HEIC 增益图缺少可验证的 Apple HDR 版本；请保留原编码");
    }
    if gain.versions != ["65536"] && gain.versions != ["131072"] {
        return failure("此 HEIC 增益图不是支持的 Apple HDR；请保留 HEIC 原编码");
    }
    let maker_stops = apple_hdr::stops(&original)?;
    if gain.versions == ["131072"] {
        let report = apple_hdr::read(&original)?;
        let tmap = report.heic.as_ref().is_some_and(|h| h.iso_tmap);
        let ok = gain.headrooms.len() == 1
            && gain.headrooms[0].is_finite()
            && gain.headrooms[0] > 1.0
            && maker_stops.is_some_and(|s| (gain.headrooms[0].log2() - s).abs() < 0.002)
            && !tmap;
        if !ok {
            return failure("新版 Apple HDR 参数不能验证为已支持模型；请保留 HEIC 原编码");
        }
    }
    let incomplete = || Error::new("Apple HEIC HDR 增益图/亮度参数不完整，未降为 SDR 或猜测 HDR");
    let stops = maker_stops.ok_or_else(incomplete)?;
    let image = gain.image.ok_or_else(incomplete)?;
    if !(1..=16384).contains(&image.width) || !(1..=16384).contains(&image.height) {
        return Err(incomplete());
    }
    let gain_jpeg = native.encode_gray_jpeg(&convert_gain(&image, stops)?, options.gain_quality)?;
    Ok((
        super::ultra(&base, &gain_jpeg, stops)?,
        json!({"status": "converted", "input": "apple-hdr", "output": "ultra-hdr", "reason": "Apple HEIC 原始增益图转 Ultra HDR；主 JPEG 和增益 JPEG 使用独立质量"}),
    ))
}
