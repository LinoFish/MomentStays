//! Time-only copies of confirmed ordinary stills; source bytes are never edited.
use super::options::ConversionOptions;
use super::{failure, hash_file, output, paths, read, write, write_json, Native, StopToken};
use crate::Result;
use serde_json::{json, Value};
use std::path::Path;

pub fn convert(
    native: &dyn Native,
    source: &Path,
    target: &Path,
    options: &ConversionOptions,
    token: &StopToken,
    overwrite: bool,
) -> Result<Value> {
    let source_hash = hash_file(source)?;
    if !overwrite && target.exists() {
        let m = output::verify(target)?;
        if m["source_format"] != "static-time"
            || m["source_sha256"] != source_hash
            || m["options"] != options.json()
        {
            return failure("已有时间处理结果与本轮设置不同，请显式重新转换");
        }
        return Ok(
            json!({"status":"resumed","source":paths::json(source),"output":paths::json(target),"verification":m,"detail":"已校验时间处理副本"}),
        );
    }
    let data = read(source)?;
    if !crate::scan::confirmed_static(&data) {
        return failure("时间补全只接收已确认的静态照片，不拆动态容器");
    }
    let stage = output::Stage::new(target)?;
    let name = paths::file_name(source);
    write(&stage.path().join(&name), &data)?;
    let _ = super::stamp_output_time(source, &stage.path().join(&name), options.exif_file_times);
    let m = json!({"source_format":"static-time","implementation":"phototime-1","processing_kind":"time-metadata-repair",
        "source":paths::json(source),"source_sha256":source_hash,"options":options.json(),
        "output_files":{"static":name},"output_sha256":{name.clone():hash_file(&stage.path().join(&name))?},
        "timing":{"applicable":false},"hdr":{"applicable":false}});
    write_json(&m, &stage.path().join(output::MANIFEST))?;
    output::publish(
        native,
        &stage,
        target,
        source,
        &source_hash,
        token,
        overwrite,
        options.export_data_files,
    )?;
    Ok(
        json!({"status":"converted","source":paths::json(source),"output":paths::json(target),"output_files":{"static":name},
        "processing_kind":"time-metadata-repair","detail":"PhotoTime 已处理输出副本的缺失时间；未转换动态或重建 HDR"}),
    )
}
