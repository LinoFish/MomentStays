//! Explicit Honor JPEG -> Google Motion Photo route; no generic Android admission.
use crate::engine::{
    failure, hash_file, hdr,
    output::{self, Stage},
    paths, read, write, write_json, Native, StopToken, Timings,
};
use crate::{
    format, jpeg,
    live::{self, TimeDomain},
    model::Cover,
    mp4::Movie,
    sha256_hex, MotionPhoto, Result,
};
use serde_json::{json, Value};
use std::path::Path;
const REVISION: &str = "honor-to-google-hdr-1";

pub fn inspect<'a>(native: &dyn Native, data: &'a [u8]) -> Result<MotionPhoto<'a>> {
    if !data.starts_with(&[255, 216]) {
        return failure("荣耀 → Google 仅支持已识别的荣耀实况 JPEG，不支持 HEIC");
    }
    let (make, _) = crate::vendor::make_model(data);
    if !make.trim().eq_ignore_ascii_case("HONOR") {
        return failure("荣耀 → Google 需要明确的 HONOR 来源，不接受其他厂商或未知来源");
    }
    if crate::live::still_content_identifier(data)?.is_some() {
        return failure("来源含 Apple 实况配对标识；请使用苹果实况 → Google，并提供完整配对资源");
    }
    let photo = format::parse(data)?;
    if native.image_decodes(&photo.still) == Some(false) {
        return failure("荣耀主图无法解码");
    }
    let _ = jpeg::coding_hash(&photo.still)?;
    let _ = jpeg::auxiliary(&photo.still)?;
    cover(&photo, data)?;
    Ok(photo)
}
fn cover(photo: &MotionPhoto<'_>, data: &[u8]) -> Result<i64> {
    let movie = Movie::parse(photo.video(data))?;
    let selected = if let Some(frame) = photo.cover_frame {
        Cover::Frame(frame)
    } else if let Some(time) = photo.timestamp {
        Cover::Time(time)
    } else {
        return failure("荣耀实况缺少可信封面时间或帧号");
    };
    let resolved =
        live::resolve_cover_in(photo.video(data), &movie, selected, TimeDomain::Auto, 0)?;
    let time = resolved.mapped_us.unwrap_or(resolved.frame_time_us);
    // Google timestamp must resolve unambiguously against the unchanged video.
    super::transfer::cover(photo.video(data), time)?;
    Ok(time)
}
pub fn scan_file(
    native: &dyn Native,
    source: &Path,
    relative: &str,
) -> crate::engine::scanrow::ScanRow {
    let before = crate::engine::scanrow::SourceVersion::read(source);
    let mut row = crate::engine::scanrow::scan_file(native, source, relative, None);
    let result = (|| {
        let data = read(source)?;
        // Standalone HDR uses the same domain and stays a single image.
        let (make, _) = crate::vendor::make_model(&data);
        if row.is_static_hdr()
            && make.trim().eq_ignore_ascii_case("HONOR")
            && data.starts_with(&[255, 216])
        {
            return Ok(());
        }
        inspect(native, &data).map(|_| ())
    })();
    if let Err(e) = result {
        row.is_motion = false;
        row.confirmed_static = false;
        row.burst = None;
        row.detail = e.0;
    }
    if before != crate::engine::scanrow::SourceVersion::read(source) {
        return crate::engine::scanrow::ScanRow::changed(source, relative);
    }
    row
}
pub fn convert(
    native: &dyn Native,
    source: &Path,
    target: &Path,
    options: &crate::engine::options::ConversionOptions,
    token: &StopToken,
    overwrite: bool,
    _timings: &Timings,
) -> Result<Value> {
    let hash = hash_file(source)?;
    let data = read(source)?;
    let photo = inspect(native, &data)?;
    let time = cover(&photo, &data)?;
    if !overwrite && target.exists() {
        let m = output::verify(target)?;
        if m["implementation"] != REVISION
            || m["source_sha256"] != hash
            || m["options"] != options.json()
        {
            return failure("已有荣耀 → Google 结果与来源或HDR策略不同，请显式覆盖");
        }
        if token.is_cancelled() {
            return failure("已停止");
        }
        return Ok(
            json!({"status":"resumed","source":paths::json(source),"output":paths::json(target),"output_files":m["output_files"],"hdr":m["hdr"],"verification":m,"detail":format!("实况已复用；{}",hdr::summary(&m["hdr"]))}),
        );
    }
    if token.is_cancelled() {
        return failure("已停止");
    }
    let processed = hdr::process(
        native,
        photo.still.to_vec(),
        hdr::Options {
            try_private_hdr: options.legacy_hdr,
            already_ultra_hdr: photo.ultra,
            allow_legacy_conversion: true,
        },
    )?;
    let report = Value::Object(processed.report);
    let video = photo.video(&data);
    let result = super::motion::assemble(
        &processed.image,
        video,
        &crate::engine::options::AndroidOptions::vendor("google"),
        time,
        0,
    )?;
    let after = format::parse(&result)?;
    if after.format != "google-compatible"
        || after.timestamp != Some(time)
        || after.video(&result) != video
        || format::coding_hash(&after.still)? != format::coding_hash(&processed.image)?
        || format::auxiliary_hashes(&after.still)? != format::auxiliary_hashes(&processed.image)?
        || super::transfer::metadata(&after.still)? != super::transfer::metadata(&processed.image)?
        || hdr::honor_private(&after.still)? != processed.honor_extension
    {
        return failure("荣耀 → Google 读回失败：主图/HDR/视频或封面不一致");
    }
    let timing = super::transfer::cover(video, time)?;
    let stage = Stage::new(target)?;
    let name = format!("{}.MP.jpg", paths::stem(source));
    write(&stage.path().join(&name), &result)?;
    let _ = crate::engine::stamp_output_time(
        source,
        &stage.path().join(&name),
        options.exif_file_times,
    );
    let m = json!({"implementation":REVISION,"source":paths::json(source),"source_sha256":hash,"options":options.json(),
        "output_files":{"motionphoto":name},"output_sha256":{name.clone():sha256_hex(&result)},"timing":timing,"hdr":report,"warnings":processed.warnings});
    write_json(&m, &stage.path().join(output::MANIFEST))?;
    output::publish(
        &stage,
        target,
        source,
        &hash,
        token,
        overwrite,
        options.export_data_files,
    )?;
    Ok(
        json!({"status":"converted","source":paths::json(source),"output":paths::json(target),"output_files":m["output_files"],"verification":m,"hdr":report,"detail":format!("荣耀 → Google 实况已转换；{}",hdr::summary(&report))}),
    )
}
