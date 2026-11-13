//! Standalone HDR delivery. No movie, cover time or pairing is synthesized.
use super::{
    failure, hash_file, hdr,
    options::ConversionOptions,
    output::{self, Stage},
    paths, read, write, write_json, Native, StopToken, Timings,
};
use crate::{apple_hdr, format, heif, jpeg, scan, Result};
use serde_json::{json, Value};
use std::path::Path;

/// Admit only complete stills without motion claims and with known HDR resources.
pub fn inspect(native: &dyn Native, data: &[u8]) -> Result<Option<&'static str>> {
    if crate::live::still_content_identifier(data)?.is_some() {
        return failure("图片带 Apple 实况配对标识，必须检查完整配对；不能作为独立静态 HDR 处理");
    }
    if !scan::confirmed_static(data) {
        return Ok(None);
    }
    let kind = if heif::matches(data) {
        match apple_hdr::read(data)?.status {
            apple_hdr::Status::GainMap => {}
            apple_hdr::Status::None => return Ok(None),
            _ => return failure("未知或不完整的静态 HEIC HDR，未推测处理"),
        }
        "HEIC HDR"
    } else {
        let ultra = hdr::is_ultra(data)?;
        let maps = jpeg::auxiliary(data)?;
        if ultra && maps.len() != 2 {
            return failure("静态 Ultra HDR 缺少唯一有效增益图");
        }
        let kind = scan::classify_hdr(data, ultra)?;
        if kind == "ISO HDR" && maps.len() != 2 {
            return failure("静态 ISO HDR 缺少唯一有效增益图");
        }
        if kind == "SDR / 其他" {
            if maps.len() > 1 {
                return failure("静态图片包含未知 HDR/辅助资源，未推测或丢弃");
            }
            return Ok(None);
        }
        kind
    };
    if native.image_decodes(data) == Some(false) {
        return failure("静态 HDR 图像损坏");
    }
    Ok(Some(kind))
}

fn settings(options: &ConversionOptions) -> Value {
    json!({"direction":options.direction,"legacy_hdr":options.legacy_hdr,
        "android":options.android.json(),"hdr_policy":hdr::POLICY,"export_data_files":options.export_data_files})
}

pub fn convert(
    native: &dyn Native,
    source: &Path,
    target: &Path,
    options: &ConversionOptions,
    token: &StopToken,
    overwrite: bool,
    timings: &Timings,
) -> Result<Value> {
    if options.output_mode != "livephoto"
        || options.video_track != "primary"
        || options.cover_index.is_some()
        || options.manual_cover.is_some()
        || options
            .cover_selections
            .contains_key(&*paths::normalized(source).to_string_lossy())
        || options
            .manual_covers
            .contains_key(&*paths::normalized(source).to_string_lossy())
    {
        return failure("静态 HDR 不支持视频轨、辅助轨或封面选择，请使用原静态图片模式");
    }
    let source_hash = timings.measure("source_hash", || hash_file(source))?;
    let original = read(source)?;
    let kind =
        inspect(native, &original)?.ok_or_else(|| crate::Error::new("来源不是可处理的静态 HDR"))?;
    if options.direction == "to-android"
        && options.android.container == "heic"
        && !heif::matches(&original)
    {
        return failure("静态 HEIC 输出需要 HEIC 原图，未将 JPEG 再编码为 HEIC");
    }
    let opts = settings(options);
    if !overwrite && target.exists() {
        let m = output::verify(target)?;
        if m["source_format"] != "static-hdr"
            || m["implementation"] != "static-hdr-3"
            || m["source_sha256"] != source_hash
            || m["options"] != opts
            || m["hdr"]
                .get("profile")
                .is_some_and(|v| v != hdr::honor::PROFILE)
        {
            return failure("已有静态输出与来源或设置不同，请显式选择覆盖");
        }
        if token.is_cancelled() {
            return failure("已停止");
        }
        return Ok(
            json!({"status":"resumed","source":paths::json(source),"output":paths::json(target),
            "output_files":m["output_files"],"hdr":m["hdr"],"processing_kind":m["processing_kind"],"detail":hdr::summary(&m["hdr"])}),
        );
    }
    if token.is_cancelled() {
        return failure("已停止");
    }
    let processed = hdr::process(
        native,
        original.clone(),
        hdr::Options {
            try_private_hdr: options.legacy_hdr,
            already_ultra_hdr: kind == "Ultra HDR",
            allow_legacy_conversion: true,
        },
    )?;
    let mut image = processed.image;
    let mut report = Value::Object(processed.report);
    // Reuse the existing HEIC/JPEG exporter when its actual encoding options apply.
    // Private extensions stay on the independent HDR path, never a lossy generic re-encode.
    if options.direction == "to-android" {
        let (exported, exported_report) = hdr::prepare_with_policy(
            native,
            source,
            &hdr::ExportOptions {
                preserve_heic: options.android.container == "heic",
                preserve_jpeg: options.android.preserve_jpeg,
                image_quality: options.android.image_quality,
                gain_quality: options.android.gain_quality,
            },
            options.legacy_hdr,
        )?;
        image = exported;
        report = exported_report;
    }
    // Existing standard gain maps need no empirical reconstruction. Deliver
    // their complete original bytes through the same verified transaction,
    // classified as a copy rather than a new HDR conversion.
    let original_copy = image == original
        && kind == "Ultra HDR"
        && report["outcome"] == "preserved"
        && crate::hdr::known_ultra_version(&image)?;
    if image == original && !original_copy {
        return Ok(
            json!({"status":"skipped","source":paths::json(source),"hdr":report,
            "detail":format!("{}；没有可执行的静态转换，可按已选策略原样导出",hdr::summary(&report))}),
        );
    }
    if hdr::honor_private(&image)? != processed.honor_extension {
        return failure("静态私有 HDR 扩展未完整保留");
    }
    if native.image_decodes(&image) == Some(false) {
        return failure("静态 HDR 输出无法解码");
    }
    if options.direction != "to-android"
        && format::coding_hash(&image)? != format::coding_hash(&original)?
    {
        return failure("静态主图编码发生意外变化");
    }
    let stage = Stage::new(target)?;
    let name = format!(
        "{}.{}",
        paths::stem(source),
        if heif::matches(&image) { "heic" } else { "jpg" }
    );
    let names = json!({"static":name});
    write(&stage.path().join(&name), &image)?;
    let _ = super::stamp_output_time(source, &stage.path().join(&name), options.exif_file_times);
    let processing_kind = if original_copy {
        "original-copy"
    } else {
        "hdr-conversion"
    };
    let m = json!({"source_format":"static-hdr","implementation":"static-hdr-3","processing_kind":processing_kind,"source":paths::json(source),
        "source_sha256":source_hash,"options":opts,"output_files":names,
        "output_sha256":{name.clone():hash_file(&stage.path().join(&name))?},
        "timing":{"applicable":false},"hdr":report,"warnings":processed.warnings});
    write_json(&m, &stage.path().join(output::MANIFEST))?;
    output::publish(
        &stage,
        target,
        source,
        &source_hash,
        token,
        overwrite,
        options.export_data_files,
    )?;
    Ok(
        json!({"status":"converted","source":paths::json(source),"output":paths::json(target),
        "output_files":names,"processing_kind":processing_kind,"hdr":report,"warnings":processed.warnings,"detail":format!("{}；{}",if original_copy {"静态 HDR 原样输出，直接保留已有 gainmap，未重建 HDR"} else {"静态图片已处理"},hdr::summary(&report))}),
    )
}
