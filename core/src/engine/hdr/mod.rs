//! Still-image HDR processing, independent of live-photo/video assembly.
//!
//! Vendor readers and empirical mappings belong here. Callers supply only
//! still bytes, HDR policy and codecs; pairing IDs, cover times, video tracks
//! and output folders are deliberately outside this interface.
mod export;
pub mod honor;
mod legacy;
mod metadata;

pub(crate) use export::is_ultra;
pub use export::{prepare_export, ExportOptions};
pub use legacy::{convert_gain, gain_lookup, legacy_gain};
pub use metadata::{
    color_profile, pack_gain_map, swift_double, ultra, ultra_with_offset, upgrade_iso,
};

use super::Native;
use crate::{heif, jpeg, sha256_hex, Result};
use serde_json::{json, Map, Value};

/// Policies supplied by an exporter, without a dependency on its options type.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub try_private_hdr: bool,
    pub already_ultra_hdr: bool,
    pub allow_legacy_conversion: bool,
}

pub struct Processed {
    pub image: Vec<u8>,
    pub report: Map<String, Value>,
    pub warnings: Vec<String>,
    /// Retained for the caller's subsequent metadata/pairing validation.
    pub honor_extension: Option<Vec<u8>>,
}

/// The complete Honor extension on a standalone still (None for HEIC).
pub fn honor_private(data: &[u8]) -> Result<Option<Vec<u8>>> {
    if heif::matches(data) {
        return Ok(None);
    }
    let end = jpeg::image_end(data)?;
    Ok(crate::hdr::honor_private(data, end)?.map(<[u8]>::to_vec))
}

/// Process HDR without changing live-photo metadata or any movie data.
/// Invalid framing is an error. Unsupported conversion leaves `image` intact.
pub fn process(native: &dyn Native, image: Vec<u8>, options: Options) -> Result<Processed> {
    let extension = honor_private(&image)?;
    let mut out = Processed {
        image,
        report: Map::new(),
        warnings: Vec::new(),
        honor_extension: extension,
    };
    if heif::matches(&out.image) {
        out.report = json!({"input": "heic", "status": "preserved", "reason": "HEIC 图像编码、色彩与方向属性原样保留；未新增或转换 HDR"})
            .as_object().cloned().unwrap_or_default();
        return Ok(out);
    }
    if options.already_ultra_hdr {
        out.report = json!({"input": "ultra-hdr", "status": "preserved", "reason": "Ultra HDR 主图及增益图原样保留"})
            .as_object().cloned().unwrap_or_default();
        if let Some(ext) = &out.honor_extension {
            out.report
                .insert("private_extension_sha256".into(), json!(sha256_hex(ext)));
        }
        return Ok(out);
    }
    if let Some(ext) = &out.honor_extension {
        out.report
            .insert("input".into(), json!("honor-private-ehdr"));
        out.report
            .insert("status".into(), json!("unsupported-preserved"));
        out.report
            .insert("private_extension_sha256".into(), json!(sha256_hex(ext)));
        let reason = if options.try_private_hdr {
            match honor::convert(native, &out.image) {
                Ok(converted) => {
                    out.image = converted;
                    out.report
                        .insert("status".into(), json!("converted-experimental"));
                    out.report.insert("output".into(), json!("ultra-hdr"));
                    out.report.insert("profile".into(), json!(honor::PROFILE));
                    out.report.insert("stops".into(), json!(honor::STOPS));
                    out.report.insert("offset_sdr".into(), json!(honor::OFFSET));
                    out.report.insert("offset_hdr".into(), json!(honor::OFFSET));
                    "荣耀 EHDR 已按实验配置转为 Ultra HDR，无需参考图；主图编码及原私有扩展均已保留。已比较四组官方编辑成片，但未证明完整原 HDR 还原，局部差异仍可能存在".to_string()
                }
                Err(e) => format!(
                    "{}；原私有 HDR 静态扩展已保留，Apple 设备可能仅显示 SDR",
                    e.0
                ),
            }
        } else {
            "私有 HDR 尝试转换已关闭，荣耀静态扩展原样保留；Apple 设备可能仅显示 SDR".to_string()
        };
        out.report.insert("reason".into(), json!(reason));
        out.warnings.push(reason);
        return Ok(out);
    }
    out.report.insert(
        "input".into(),
        json!(if crate::scan::has_iso(&out.image)? {
            "iso-hdr"
        } else {
            "legacy-or-sdr"
        }),
    );
    out.report.insert("status".into(), json!("unchanged"));
    out.report
        .insert("reason".into(), json!("旧版 HDR 转换已关闭"));
    if options.try_private_hdr && options.allow_legacy_conversion {
        if let Err(e) = legacy::apply(native, &mut out.image, &mut out.report) {
            out.warnings.push(e.0);
            out.report.insert(
                "reason".into(),
                json!("旧 HDR 尝试转换未完成，原格式已保留"),
            );
        }
    }
    Ok(out)
}
