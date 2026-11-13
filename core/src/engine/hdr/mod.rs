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
fn process_inner(native: &dyn Native, image: Vec<u8>, options: Options) -> Result<Processed> {
    let extension = honor_private(&image)?;
    let mut out = Processed {
        image,
        report: Map::new(),
        warnings: Vec::new(),
        honor_extension: extension,
    };
    if heif::matches(&out.image) {
        let status = match crate::apple_hdr::read(&out.image)?.status {
            crate::apple_hdr::Status::None => "sdr",
            crate::apple_hdr::Status::Refuse => "unsupported-preserved",
            _ => "preserved",
        };
        out.report = json!({"input": "heic", "status": status, "reason": "HEIC 图像编码、色彩与方向属性原样保留；未新增或转换 HDR，未知 HDR 未重建"})
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
    let private_ehdr = crate::hdr::honor_ehdr(&out.image, jpeg::image_end(&out.image)?)?
        .is_some_and(|h| !h.header.is_empty());
    if let Some(ext) = out.honor_extension.as_ref().filter(|_| private_ehdr) {
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
        let original = out.image.clone();
        if let Err(e) = legacy::apply(native, &mut out.image, &mut out.report) {
            out.image = original;
            out.warnings.push(e.0);
            out.report.insert(
                "reason".into(),
                json!("旧 HDR 尝试转换未完成，原格式已保留"),
            );
        }
    }
    Ok(out)
}

/// Persisted policy boundary shared by live, burst and standalone HDR routes.
pub const POLICY: &str = "supported-private-hdr-2";

pub fn outcome(report: &Value) -> &'static str {
    match report["status"].as_str().unwrap_or("") {
        "converted" | "converted-experimental" => "converted",
        "preserved" | "preserved-gainmap" => "preserved",
        "sdr" | "preserved-sdr" | "not-applicable" => "not-applicable",
        _ => "ignored",
    }
}
pub fn summary(report: &Value) -> String {
    if report["reason"]
        .as_str()
        .is_some_and(|s| s.contains("独立照片原样输出"))
    {
        return "HDR 未处理：独立照片按原字节输出".into();
    }
    if report["status"] == "per-member" {
        return report["reason"]
            .as_str()
            .unwrap_or("HDR 逐成员处理")
            .to_string();
    }
    let label = match report["outcome"]
        .as_str()
        .unwrap_or_else(|| outcome(report))
    {
        "converted" => "HDR 已转换",
        "preserved" => "HDR 已保留",
        "not-applicable" => "SDR，不生成 HDR",
        _ => "HDR 已忽略",
    };
    format!(
        "{}：{}",
        label,
        report["reason"].as_str().unwrap_or("按实际支持处理")
    )
}
pub fn annotate(report: &mut Value) {
    report["policy"] = json!(POLICY);
    report["outcome"] = json!(outcome(report));
}
pub fn process(native: &dyn Native, image: Vec<u8>, options: Options) -> Result<Processed> {
    // Container and primary coding errors are never downgraded to HDR warnings.
    let _ = crate::format::coding_hash(&image)?;
    if native.image_decodes(&image) == Some(false) {
        return super::failure("主图无法解码；HDR 忽略策略不能绕过主资源校验");
    }
    if !heif::matches(&image) {
        let maps = jpeg::auxiliary(&image)?;
        if maps
            .iter()
            .skip(1)
            .any(|r| native.image_decodes(&image[r.clone()]) == Some(false))
        {
            let extension = honor_private(&image)?;
            let mut report = json!({"status":"unsupported-preserved","reason":"HDR 增益图解码不可用；原资源保留，继续其他已支持步骤"});
            annotate(&mut report);
            return Ok(Processed {
                image,
                report: report.as_object().cloned().unwrap_or_default(),
                warnings: vec!["HDR 增益图解码不可用".into()],
                honor_extension: extension,
            });
        }
    }
    if !heif::matches(&image)
        && options.already_ultra_hdr
        && !crate::hdr::known_ultra_version(&image)?
    {
        let extension = honor_private(&image)?;
        let mut report = json!({"input":"unknown-hdr","status":"unsupported-preserved","reason":"HDR 版本未知或冲突，已忽略 HDR 转换并保留原图和辅助资源；不重写为 Ultra HDR 1.0"});
        if let Some(ref ext) = extension {
            report["private_extension_sha256"] = json!(sha256_hex(ext));
        }
        annotate(&mut report);
        return Ok(Processed {
            image,
            report: report.as_object().cloned().unwrap_or_default(),
            warnings: vec![],
            honor_extension: extension,
        });
    }
    let mut processed = process_inner(native, image, options)?;
    let mut report = Value::Object(processed.report);
    // A standard ISO gain map is not a private-HDR conversion. Its encoded
    // resources stay intact, including when private-HDR reconstruction is off.
    if report["input"] == "iso-hdr" && jpeg::auxiliary(&processed.image)?.len() == 2 {
        if report["status"] == "converted" {
            report["status"] = json!("preserved");
        } else if !options.try_private_hdr {
            report["status"] = json!("preserved");
            report["reason"] =
                json!("已有 ISO HDR 图像、增益图及参数原样保留；未执行私有 HDR 重建");
        }
    }
    if report["input"] == "legacy-or-sdr" && report["status"] == "unchanged" {
        let kind = crate::scan::classify_hdr(&processed.image, false)?;
        if kind == "SDR / 其他" && jpeg::auxiliary(&processed.image)?.len() <= 1 {
            report["status"] = json!("sdr");
            report["reason"] = json!("未检测到 HDR，不生成增益图");
        }
    }
    annotate(&mut report);
    processed.report = report.as_object().cloned().unwrap_or_default();
    Ok(processed)
}

/// Existing Apple exporter plus the same private-HDR policy; fallback retains
/// resources whenever the target container can carry them. Primary codec errors
/// still propagate. No empirical model or input boundary is broadened here.
pub fn prepare_with_policy(
    native: &dyn Native,
    path: &std::path::Path,
    options: &ExportOptions,
    enabled: bool,
) -> Result<(Vec<u8>, Value)> {
    if ![options.image_quality, options.gain_quality]
        .iter()
        .all(|q| q.is_finite() && *q > 0.0 && *q <= 1.0)
    {
        return super::failure("JPEG 质量需在 1–100% 之间");
    }
    let original = super::read(path)?;
    if options.preserve_heic && !heif::matches(&original) {
        return super::failure("保留 HEIC 输出需要 HEIC 原图，不将 JPEG 伪装成 HEIC");
    }
    if heif::matches(&original) {
        let _ = heif::layout_lenient(&original)?;
    }
    if native.image_decodes(&original) == Some(false) {
        return super::failure("主图无法解码");
    }
    let _ = crate::format::coding_hash(&original)?;
    if !heif::matches(&original) {
        let processed = process(
            native,
            jpeg::static_only(&original)?,
            Options {
                try_private_hdr: enabled,
                already_ultra_hdr: is_ultra(&original)?,
                allow_legacy_conversion: true,
            },
        )?;
        let report = Value::Object(processed.report);
        if report["outcome"] != "not-applicable" || options.preserve_jpeg {
            let mut report = report;
            if !options.preserve_jpeg {
                let reason = report["reason"].as_str().unwrap_or("").to_string();
                report["reason"] = json!(format!(
                    "{}；HDR 资源保真路径保留主图原编码，未应用 JPEG 重编码质量",
                    reason
                ));
            }
            return Ok((processed.image, report));
        }
        // SDR re-encoding is independent of the private-HDR switch.
        let (image, mut report) = prepare_export(native, path, options)?;
        annotate(&mut report);
        return Ok((image, report));
    }
    if enabled || options.preserve_heic {
        match prepare_export(native, path, options) {
            Ok((image, mut report)) => {
                annotate(&mut report);
                return Ok((image, report));
            }
            Err(e) => {
                let image = if options.preserve_heic {
                    original
                } else {
                    native.reencode_jpeg(path, options.image_quality)?
                };
                let mut report = json!({"status":"unsupported-preserved","reason":e.0});
                annotate(&mut report);
                return Ok((image, report));
            }
        }
    }
    let image = native.reencode_jpeg(path, options.image_quality)?;
    let mut report = if crate::apple_hdr::read(&original)?.status == crate::apple_hdr::Status::None
    {
        json!({"status":"sdr","reason":"SDR 主图按所选容器输出，不生成增益图"})
    } else {
        json!({"status":"unsupported-preserved","reason":"私有 HDR 尝试转换已关闭；目标 JPEG 仅输出主图，来源原资源未改动"})
    };
    annotate(&mut report);
    Ok((image, report))
}
