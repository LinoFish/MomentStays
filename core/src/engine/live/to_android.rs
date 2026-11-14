//! Apple Live Photo -> Android workflow and still-HDR option adaptation.
use super::motion::{assemble, remux};
use super::pairing::pair;
use crate::engine::hdr;
use crate::engine::options::{AndroidOptions, ConversionOptions};
use crate::engine::output::{self, Stage};
use crate::engine::paths::stem;
use crate::engine::{failure, hash_file, read, Native, StopToken, Timings};
use crate::{format, sha256_hex, Result};
use serde_json::{json, Value};
use std::path::Path;

pub use crate::engine::contracts::TO_ANDROID_REVISION as REVISION;

/// Delegate still/HDR preparation; pairing and movie assembly stay here.
pub fn prepare_still(
    native: &dyn Native,
    path: &Path,
    options: &AndroidOptions,
    enabled: bool,
) -> Result<(Vec<u8>, Value)> {
    hdr::prepare_with_policy(
        native,
        path,
        &hdr::ExportOptions {
            preserve_heic: options.container == "heic",
            preserve_jpeg: options.preserve_jpeg,
            image_quality: options.image_quality,
            gain_quality: options.gain_quality,
        },
        enabled,
    )
}

/// Swift `AndroidExport.convert`.
pub fn convert(
    native: &dyn Native,
    source: &Path,
    target: &Path,
    options: &ConversionOptions,
    token: &StopToken,
    overwrite: bool,
    timings: &Timings,
) -> Result<Value> {
    let config = &options.android;
    if ![
        "google", "xiaomi", "samsung", "huawei", "honor", "oppo", "vivo",
    ]
    .contains(&config.vendor.as_str())
        || !["jpeg", "heic"].contains(&config.container.as_str())
    {
        return failure("安卓目标厂商或容器无效");
    }
    if config.container == "heic"
        && !["google", "xiaomi", "samsung"].contains(&config.vendor.as_str())
    {
        return failure("此厂商的 HEIC 动态封装尚无可验证方案；请选择 JPEG");
    }
    let source_hash = timings.measure("source_hash", || hash_file(source))?;
    if !overwrite && target.exists() {
        let previous = output::verify(target)?;
        if previous["implementation"].as_str() != Some(REVISION) {
            return failure("已有输出使用旧版 HDR 封装，请选择覆盖以修复");
        }
        if config.vendor == "honor"
            && previous["motion_layout"].as_str() != Some("google-compatible")
        {
            return failure("已有荣耀输出使用旧版私有尾标，可能导致视频方向错误；请选择覆盖以修复");
        }
        if previous["mp4_layout"].as_str() != Some("iso-sample-copy-v1") {
            return failure("已有输出使用旧版视频封装，请选择覆盖后重新转换");
        }
        if config.container == "jpeg"
            && ["google", "honor"].contains(&config.vendor.as_str())
            && previous["xmp_profile"].as_str() != Some("scoped-native-v1")
        {
            return failure("已有输出使用旧版XMP写法，请选择覆盖后重新转换");
        }
        let same_movie = match (
            previous["paired_video_source"].as_str(),
            previous["paired_video_sha256"].as_str(),
        ) {
            (Some(m), Some(h)) => hash_file(Path::new(m)).ok().as_deref() == Some(h),
            _ => false,
        };
        if previous["source_sha256"].as_str() != Some(source_hash.as_str())
            || !crate::engine::same_json(&previous["options"], &options.json())
            || !same_movie
        {
            return failure("已有输出的输入或设置不同，请选择覆盖或跳过");
        }
        return Ok(
            json!({"status": "resumed", "source": source.to_string_lossy(), "output": target.to_string_lossy(), "hdr":previous["hdr"], "detail":format!("已验证安卓动态照片；{}",hdr::summary(&previous["hdr"]))}),
        );
    }
    let paired = pair(source, &options.time_domain)?;
    let video_hash = hash_file(&paired.movie)?;
    let stage = Stage::new(target)?;
    if token.is_cancelled() {
        return failure("已停止");
    }
    let (video, mut timing) = remux(&read(&paired.movie)?)?;
    let time = paired.cover_us;
    let (still_bytes, hdr) = timings.measure("image_and_hdr", || {
        prepare_still(native, source, config, options.legacy_hdr)
    })?;
    let result = assemble(&still_bytes, &video, config, time, paired.frame as i64)?;
    let decoded = format::parse(&result)?;
    if decoded.video(&result) != video.as_slice()
        || format::coding_hash(&decoded.still)? != format::coding_hash(&still_bytes)?
        || format::auxiliary_hashes(&decoded.still)? != format::auxiliary_hashes(&still_bytes)?
    {
        return failure("安卓封装读回校验失败：图像或视频数据不同");
    }
    if config.vendor == "huawei" {
        if decoded.cover_frame != Some(paired.frame as i64) {
            return failure("厂商封面帧号校验失败");
        }
    } else if decoded.timestamp != Some(time) {
        return failure("安卓封面时间校验失败");
    }
    let name = format!(
        "{}.MP.{}",
        stem(source),
        if config.container == "heic" {
            "heic"
        } else {
            "jpg"
        }
    );
    let file = stage.path().join(&name);
    crate::engine::write(&file, &result)?;
    let _ = crate::engine::stamp_output_time(source, &file, options.exif_file_times);
    timing["source_still_image_time_seconds"] = json!(paired.cover_seconds);
    timing["cover_timestamp_us"] = json!(time);
    timing["cover_frame_index"] = json!(paired.frame);
    let mut warnings = vec![if config.container == "heic" {
        "HEIC 原始 HDR 编码已保留；目标安卓厂商对 Apple 私有 HDR 的显示兼容性需真机验证"
    } else {
        "厂商封装已通过本地解析校验；不同手机固件识别与 HDR 显示仍需真机验证"
    }
    .to_string()];
    if timing["audio_codecs"].as_array().is_some_and(|v| {
        v.iter()
            .any(|c| matches!(c.as_str(), Some("ipcm" | "fpcm")))
    }) {
        warnings.push(
            "源视频音频为 PCM，部分安卓相册可能无法识别或播放；已保留音频，未自动转码或静音".into(),
        );
    }
    if config.vendor == "honor" {
        warnings.push("荣耀格式仅作备用，新款荣耀推荐选择 Google / 通用。此兼容性针对留刻导出的文件，不代表能识别 Google 相机原片".into());
    }
    let manifest = json!({
        "version": "1.2.1",
        "implementation": REVISION,
        "source": source.to_string_lossy(),
        "source_sha256": source_hash,
        "paired_video_source": paired.movie.to_string_lossy(),
        "paired_video_sha256": video_hash,
        "options": options.json(),
        "identifier": paired.identifier,
        "mp4_layout": "iso-sample-copy-v1",
        "xmp_profile": if config.container == "jpeg" && ["google", "honor"].contains(&config.vendor.as_str()) { "scoped-native-v1" } else { "existing" },
        "motion_layout": if config.vendor == "honor" { "google-compatible" } else { config.vendor.as_str() },
        "output_files": {"motionphoto": name},
        "output_sha256": {name.clone(): sha256_hex(&result)},
        "timing": timing,
        "hdr": hdr,
        "warnings": warnings,
    });
    crate::engine::write_json(&manifest, &stage.path().join("manifest.json"))?;
    if hash_file(&paired.movie)? != video_hash {
        return failure("配对视频在转换期间发生变化，未发布");
    }
    timings.measure("validation_and_publish", || {
        output::publish(
            native,
            &stage,
            target,
            source,
            &source_hash,
            token,
            overwrite,
            options.export_data_files,
        )
    })?;
    Ok(json!({
        "status": "converted",
        "source": source.to_string_lossy(),
        "output": target.to_string_lossy(),
        "detail": format!("已导出 {} {} 动态照片；封面 {} 秒；{}", config.vendor, config.container.to_uppercase(), hdr::swift_double(time as f64 / 1_000_000.0),hdr::summary(&hdr)),
        "hdr": hdr,
        "timing": timing,
        "warnings": warnings,
        "verification": manifest,
    }))
}
