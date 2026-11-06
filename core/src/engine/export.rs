//! Verified copies of frozen originals and complete conversion results.
//! Uses the existing transaction and host event boundary; no alternate queue.
use super::{
    batch::{ExistingOutput, Progress},
    failure,
    identity::SourceVersion,
    io_error, output, paths,
    request::{self, Events},
    Native, StopToken,
};
use crate::{Error, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const REVISION: &str = "verified-export-1";
const RECEIPT: &str = ".momentstays-export.json";
const MAX_FILES: usize = 4096;

fn check(token: &StopToken) -> Result<()> {
    if token.is_cancelled() {
        failure("已停止；未发布当前导出")
    } else {
        Ok(())
    }
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| Error::new(format!("导出缺少 {key}")))
}
fn safe_name(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', '\0'])
}
fn version(v: SourceVersion) -> Value {
    json!({"device":v.device,"inode":v.inode,"size":v.size,
        "modified_ns":v.modified_ns.to_string(),"changed_ns":v.changed_ns.to_string()})
}
// Copy and hash with bounded memory, cooperative cancellation and no source writes.
fn stream(source: &Path, target: Option<&Path>, token: &StopToken) -> Result<String> {
    check(token)?;
    let mut input = std::fs::File::open(source).map_err(|e| io_error(source, e))?;
    let mut output = match target {
        Some(p) => Some(
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(p)
                .map_err(|e| io_error(p, e))?,
        ),
        None => None,
    };
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        check(token)?;
        let n = input.read(&mut buffer).map_err(|e| io_error(source, e))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        if let Some(out) = output.as_mut() {
            out.write_all(&buffer[..n])
                .map_err(|e| Error::new(e.to_string()))?;
        }
    }
    if let Some(out) = output {
        out.sync_all().map_err(|e| Error::new(e.to_string()))?;
    }
    Ok(crate::hex(&hash.finalize()))
}
fn snapshot(source: &Path, token: &StopToken) -> Result<Value> {
    let before =
        SourceVersion::read(source).ok_or_else(|| Error::new("导出资源不是普通文件或不可读取"))?;
    let hash = stream(source, None, token)?;
    if SourceVersion::read(source) != Some(before) {
        return failure("原片或结果在检查期间变化，请重新检查");
    }
    let name = paths::file_name(source);
    if !safe_name(&name) || name == RECEIPT {
        return failure("导出资源名称无效或保留名称冲突");
    }
    Ok(
        json!({"path":paths::json(&paths::normalized(source)),"name":name,"version":version(before),"sha256":hash}),
    )
}
fn resources(v: &Value) -> Result<&Vec<Value>> {
    v["resources"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= MAX_FILES)
        .ok_or_else(|| Error::new("导出资源清单无效"))
}
fn fingerprint(plan: &Value) -> String {
    let mut p = plan.clone();
    p.as_object_mut().map(|o| o.remove("signature"));
    crate::sha256_hex(super::sorted(&p).to_string().as_bytes())
}
fn inventory(folder: &Path) -> Result<Vec<PathBuf>> {
    if SourceVersion::read_directory(folder).is_none() {
        return failure("转换结果不是可读取的普通目录");
    }
    let mut files = Vec::new();
    for e in std::fs::read_dir(folder).map_err(|e| io_error(folder, e))? {
        let e = e.map_err(|e| io_error(folder, e))?;
        if files.len() >= MAX_FILES {
            return failure("结果资源数量超过安全上限");
        }
        if !e.file_type().map_err(|e| io_error(folder, e))?.is_file() {
            return failure("结果包含目录或符号链接，未导出不可信资源");
        }
        if e.file_name() == RECEIPT {
            continue;
        }
        files.push(e.path());
    }
    files.sort();
    Ok(files)
}
fn validate_resources(plan: &Value, token: &StopToken) -> Result<()> {
    if plan["kind"] == "originals" {
        for c in plan["original_checks"]
            .as_array()
            .ok_or_else(|| Error::new("缺少原片资源范围记录"))?
        {
            if SourceVersion::read_directory(Path::new(text(c, "directory")?))
                .map(version)
                .as_ref()
                != Some(&c["directory_version"])
            {
                return failure("来源成员范围发生变化，请重新检查");
            }
        }
    }
    let mut names = HashSet::new();
    for r in resources(plan)? {
        let name = text(r, "name")?;
        if !safe_name(name) || name == RECEIPT || !names.insert(name.to_lowercase()) {
            return failure("导出文件名重复或不安全");
        }
        let p = Path::new(text(r, "path")?);
        if SourceVersion::read(p).map(version).as_ref() != Some(&r["version"])
            || stream(p, None, token)? != text(r, "sha256")?
        {
            return failure("来源已变化或资源缺失，导出确认已失效");
        }
    }
    if plan["kind"] == "results" {
        let folder = Path::new(text(plan, "source")?);
        let current: HashSet<_> = inventory(folder)?
            .into_iter()
            .map(|p| paths::normalized(&p))
            .collect();
        let expected: HashSet<_> = resources(plan)?
            .iter()
            .map(|r| PathBuf::from(r["path"].as_str().unwrap_or("")))
            .collect();
        if current != expected {
            return failure("结果资源范围已变化，请重新检查");
        }
        output::verify_manifest(folder, &plan["verification"])?;
    }
    Ok(())
}
fn image_bytes(source: &Path) -> Result<Vec<u8>> {
    let v = SourceVersion::read(source).ok_or_else(|| Error::new("原片不是可读普通文件"))?;
    if v.size > super::burst::MAX_IMAGE {
        return failure("原片检查超过 256 MiB 安全上限");
    }
    super::read(source)
}
fn original_resources(
    native: &dyn Native,
    item: &Value,
    direction: &str,
    token: &StopToken,
) -> Result<(Value, Vec<PathBuf>, Value)> {
    let source = Path::new(text(item, "source")?);
    let row = request::row_from_json(&item["scan_row"])
        .ok_or_else(|| Error::new("原样导出缺少冻结扫描记录"))?;
    if paths::normalized(&row.source) != paths::normalized(source)
        || row.version.is_none()
        || row.version != SourceVersion::read(source)
    {
        return failure("原片扫描版本已失效，请重新扫描");
    }
    if !paths::PHOTO_EXTENSIONS.contains(&paths::extension(source).as_str()) {
        return failure("原样导出仅接受已选照片资源");
    }
    if !matches!(item["source_kind"].as_str(), Some("local" | "photos")) {
        return failure("原样导出缺少来源类型");
    }
    if item["source_kind"] == "photos" && !row.is_motion {
        return failure("系统图库静态照片不在本轮原样导出范围");
    }
    check(token)?;
    let parent = source.parent().ok_or_else(|| Error::new("原片路径无效"))?;
    let dir_before =
        SourceVersion::read_directory(parent).ok_or_else(|| Error::new("来源目录不可读取"))?;
    let bytes = image_bytes(source)?;
    let primary = snapshot(source, token)?;
    if primary["sha256"] != crate::sha256_hex(&bytes) {
        return failure("图像检查期间原片已变化");
    }
    let mut frozen = vec![primary];
    if native.image_decodes(&bytes) == Some(false) {
        return failure("原片图像无法完整解码，未原样导出");
    }
    let apple_id = crate::live::still_content_identifier(&bytes).ok().flatten();
    let apple = apple_id.is_some() || (direction == "to-android" && row.is_motion);
    let mut files = vec![source.to_path_buf()];
    if apple {
        // Bound the existing pairing parser's total sibling movie input.
        let mut total = 0u64;
        let mut count = 0;
        let mut movies = BTreeMap::new();
        for e in std::fs::read_dir(parent).map_err(|e| io_error(parent, e))? {
            check(token)?;
            let e = e.map_err(|e| io_error(parent, e))?;
            count += 1;
            if count > 100_000 {
                return failure("配对目录过大，请缩小来源范围");
            }
            if ["mov", "mp4"].contains(&paths::extension(&e.path()).as_str()) {
                let v = SourceVersion::read(&e.path())
                    .ok_or_else(|| Error::new("配对候选不可读取或为符号链接"))?;
                total = total.saturating_add(v.size);
                if total > 512 << 20 {
                    return failure("配对视频检查超过安全上限，请隔离完整配对后重试");
                }
                movies.insert(paths::normalized(&e.path()), snapshot(&e.path(), token)?);
            }
        }
        let movie = super::live::pairing::pair(source, "auto")?.movie;
        frozen.push(
            movies
                .remove(&paths::normalized(&movie))
                .ok_or_else(|| Error::new("配对范围发生变化"))?,
        );
        files.push(movie);
    } else {
        let motion = crate::format::parse(&bytes).is_ok() || super::manual::parse(&bytes).is_ok();
        let complete = crate::scan::confirmed_static(&bytes) || motion;
        if !complete || (row.is_motion && !motion) {
            return failure("无法证明原片资源完整，未将未知或损坏照片算作原样导出");
        }
    }
    if row.version != SourceVersion::read(source) {
        return failure("原片在检查期间变化");
    }
    let burst = super::burst::inspect(source, &bytes);
    if SourceVersion::read_directory(parent) != Some(dir_before) {
        return failure("来源成员范围在检查期间变化");
    }
    Ok((
        burst.unwrap_or(Value::Null),
        files,
        json!({"resources":frozen,"directory":paths::json(&paths::normalized(parent)),"directory_version":version(dir_before)}),
    ))
}

/// Freeze complete resources and hashes; no output directory is written.
pub fn plan(native: &dyn Native, token: &StopToken, req: &Value) -> Result<Value> {
    let kind = text(req, "kind")?;
    if !["originals", "results"].contains(&kind) {
        return failure("未知导出类型");
    }
    let items = req["items"]
        .as_array()
        .filter(|a| a.len() <= 100_000)
        .ok_or_else(|| Error::new("导出范围无效"))?;
    let direction = req["direction"].as_str().unwrap_or("to-apple");
    let mut units: BTreeMap<String, Vec<(Value, Vec<PathBuf>)>> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        if token.is_cancelled() {
            break;
        }
        let source = item["source"].as_str().unwrap_or("");
        if !seen.insert(paths::normalized(Path::new(source))) {
            continue;
        }
        let result = (|| -> Result<(String, Value, Vec<PathBuf>)> {
            if kind == "originals" {
                let (burst, files, checks) = original_resources(native, item, direction, token)?;
                let key = burst["group_id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        paths::normalized(Path::new(source))
                            .to_string_lossy()
                            .into_owned()
                    });
                Ok((key, checks, files))
            } else {
                let folder = Path::new(source);
                let files = inventory(folder)?;
                let m = if item["verification"].is_object() {
                    item["verification"].clone()
                } else {
                    output::verify(folder)?
                };
                output::verify_manifest(folder, &m)?;
                Ok((
                    paths::normalized(folder).to_string_lossy().into_owned(),
                    m,
                    files,
                ))
            }
        })();
        match result {
            Ok((key, m, files)) => {
                let mut i = item.clone();
                i["verification"] = m;
                units.entry(key).or_default().push((i, files));
            }
            Err(e) => failures.push(json!({"source":source,"status":"failed","detail":e.0})),
        }
    }
    let mut plans = Vec::new();
    for (key, unit) in units {
        if token.is_cancelled() {
            break;
        }
        let sources: Vec<_> = unit.iter().map(|(i, _)| i["source"].clone()).collect();
        let made = (|| -> Result<Value> {
            let mut paths = Vec::new();
            let mut seen = HashSet::new();
            for (_, files) in &unit {
                for p in files {
                    if seen.insert(paths::normalized(p)) {
                        paths.push(p.clone());
                    }
                }
            }
            paths.sort();
            if paths.len() > MAX_FILES {
                return failure("原样导出组超过资源上限");
            }
            let mut resources = Vec::new();
            let mut names = HashSet::new();
            for p in paths {
                let r = if kind == "originals" {
                    unit.iter()
                        .flat_map(|(i, _)| {
                            i["verification"]["resources"]
                                .as_array()
                                .into_iter()
                                .flatten()
                        })
                        .find(|r| {
                            r["path"].as_str()
                                == Some(paths::normalized(&p).to_string_lossy().as_ref())
                        })
                        .cloned()
                        .ok_or_else(|| Error::new("原片冻结资源缺失"))?
                } else {
                    snapshot(&p, token)?
                };
                if !names.insert(text(&r, "name")?.to_lowercase()) {
                    return failure("同组原片文件名冲突，未覆盖或拆散资源");
                }
                resources.push(r);
            }
            let source = text(&unit[0].0, "source")?;
            let id = crate::sha256_hex(key.as_bytes());
            let name = if kind == "originals" {
                format!("{}-{}.originals", paths::stem(Path::new(source)), &id[..12])
            } else {
                let p = Path::new(source);
                let ext = paths::extension(p);
                format!(
                    "{}-{}{}",
                    paths::stem(p),
                    &id[..12],
                    if ext.is_empty() {
                        String::new()
                    } else {
                        format!(".{ext}")
                    }
                )
            };
            let photo_count = if kind == "originals" {
                sources.len()
            } else {
                unit[0].0["verification"]["burst"]["members"]
                    .as_array()
                    .map_or(1, |a| a.len().max(1))
            };
            let mut p = json!({"photo_count":photo_count,"revision":REVISION,"kind":kind,"source":source,"sources":sources,"target_name":name,"resources":resources,"verification":unit[0].0["verification"]});
            if kind == "originals" {
                p["original_checks"] = json!(unit
                    .iter()
                    .map(|(i, _)| i["verification"].clone())
                    .collect::<Vec<_>>());
                p["verification"] = Value::Null;
            }
            validate_resources(&p, token)?;
            p["signature"] = json!(fingerprint(&p));
            Ok(p)
        })();
        match made {
            Ok(p) => plans.push(p),
            Err(e) => {
                for source in sources {
                    failures.push(json!({"source":source,"status":"failed","detail":e.0}));
                }
            }
        }
    }
    Ok(json!({"kind":kind,"plans":plans,"files":failures,"cancelled":token.is_cancelled()}))
}

fn verify_copy(folder: &Path, plan: &Value, token: &StopToken) -> Result<()> {
    let actual: HashSet<_> = inventory(folder)?
        .iter()
        .map(|p| paths::file_name(p))
        .collect();
    let expected: HashSet<_> = resources(plan)?
        .iter()
        .filter_map(|r| r["name"].as_str().map(str::to_owned))
        .collect();
    if actual != expected {
        return failure("导出副本资源范围改变或附件缺失");
    }
    for r in resources(plan)? {
        let p = folder.join(text(r, "name")?);
        if SourceVersion::read(&p).is_none() || stream(&p, None, token)? != text(r, "sha256")? {
            return failure("导出副本校验失败或附件缺失");
        }
    }
    if plan["kind"] == "results" {
        output::verify_manifest(folder, &plan["verification"])?;
    }
    Ok(())
}
fn execute(
    events: &dyn Events,
    token: &StopToken,
    req: &Value,
    plan: &Value,
    root: &Path,
) -> Result<Value> {
    check(token)?;
    if plan["revision"] != REVISION
        || plan["kind"] != req["kind"]
        || text(plan, "signature")? != fingerprint(plan)
    {
        return failure("导出计划已变化，请重新检查");
    }
    let name = text(plan, "target_name")?;
    if !safe_name(name) {
        return failure("导出目标名称无效");
    }
    validate_resources(plan, token)?;
    let source = Path::new(text(plan, "source")?);
    let input_paths: Vec<_> = resources(plan)?
        .iter()
        .map(|r| PathBuf::from(r["path"].as_str().unwrap_or("")))
        .collect();
    paths::validate_output(&input_paths, root)?;
    let target = root.join(name);
    let mut overwrite = false;
    if std::fs::symlink_metadata(&target).is_ok() {
        let policy = if req["ask_existing_output"] == true {
            events.existing_output(source, &target)
        } else {
            None
        }
        .unwrap_or(match req["existing_output"].as_str() {
            Some("overwrite") => ExistingOutput::Overwrite,
            Some("resume") => ExistingOutput::Resume,
            Some("skip") => ExistingOutput::Skip,
            _ => ExistingOutput::Cancel,
        });
        check(token)?;
        match policy {
            ExistingOutput::Cancel => {
                token.cancel();
                return failure("已停止导出");
            }
            ExistingOutput::Skip => {
                return Ok(json!({"status":"skipped","detail":"已有同名输出，按本次选择跳过"}))
            }
            ExistingOutput::Overwrite => {
                if SourceVersion::read_directory(&target).is_none() {
                    return failure("同名目标不是普通目录，未覆盖");
                }
                overwrite = true;
            }
            ExistingOutput::Resume => {
                if SourceVersion::read_directory(&target).is_none() {
                    return failure("已有导出不是普通目录，未复用");
                }
                if SourceVersion::read(&target.join(RECEIPT)).is_none_or(|v| v.size > 16 << 20) {
                    return failure("已有导出校验记录不可读或超过安全上限");
                }
                let receipt: Value = serde_json::from_slice(&super::read(&target.join(RECEIPT))?)
                    .map_err(|_| Error::new("已有导出缺少有效校验记录"))?;
                if receipt["signature"] != plan["signature"] {
                    return failure("已有导出与本次来源或范围不同，未复用");
                }
                verify_copy(&target, plan, token)?;
                validate_resources(plan, token)?;
                return Ok(
                    json!({"status":"resumed","output":paths::json(&target),"detail":"已校验并复用完整导出，未重新复制"}),
                );
            }
        }
    }
    let stage = output::Stage::new(&target)?;
    for r in resources(plan)? {
        let input = Path::new(text(r, "path")?);
        let output = stage.path().join(text(r, "name")?);
        if stream(input, Some(&output), token)? != text(r, "sha256")? {
            return failure("复制期间来源变化，未发布导出");
        }
    }
    verify_copy(stage.path(), plan, token)?;
    validate_resources(plan, token)?;
    super::write_json(plan, &stage.path().join(RECEIPT))?;
    check(token)?;
    output::rename_dir(stage.path(), &target, overwrite).map_err(|e| io_error(&target, e))?;
    Ok(
        json!({"status":"copied","output":paths::json(&target),"detail":"完整资源已原样复制并校验；未计为转换成功"}),
    )
}

pub fn run(events: &dyn Events, token: &StopToken, req: &Value) -> Result<Value> {
    if !matches!(req["kind"].as_str(), Some("originals" | "results")) {
        return failure("未知导出类型");
    }
    let root = PathBuf::from(text(req, "output")?);
    if !root.is_absolute() {
        return failure("导出位置必须是绝对目录路径");
    }
    let plans = req["plans"]
        .as_array()
        .filter(|a| a.len() <= 100_000)
        .ok_or_else(|| Error::new("导出计划范围无效"))?;
    let mut files = Vec::new();
    let mut counts = BTreeMap::from([
        ("copied", 0usize),
        ("resumed", 0),
        ("skipped", 0),
        ("failed", 0),
    ]);
    let total: usize = plans
        .iter()
        .map(|p| {
            p["photo_count"]
                .as_u64()
                .filter(|n| (1..=MAX_FILES as u64).contains(n))
                .unwrap_or(1) as usize
        })
        .sum();
    let mut completed = 0;
    for plan in plans {
        if token.is_cancelled() {
            break;
        }
        let mut result = match execute(events, token, req, plan, &root) {
            Ok(v) => v,
            Err(e) => {
                if token.is_cancelled() {
                    break;
                }
                json!({"status":"failed","detail":e.0})
            }
        };
        result["source"] = plan["source"].clone();
        result["sources"] = plan["sources"].clone();
        let status = result["status"].as_str().unwrap_or("failed");
        let amount = plan["photo_count"]
            .as_u64()
            .filter(|n| (1..=MAX_FILES as u64).contains(n))
            .unwrap_or(1) as usize;
        if let Some(n) = counts.get_mut(status) {
            *n += amount;
        }
        completed += amount;
        events.progress(&Progress {
            index: completed,
            total,
            relative: plan["source"].as_str().unwrap_or("").into(),
            status: status.into(),
            detail: result["detail"].as_str().unwrap_or("").into(),
        });
        files.push(result);
    }
    Ok(
        json!({"counts":counts,"cancelled":token.is_cancelled(),"report_data":{"export_kind":req["kind"],"counts":counts,"files":files,"cancelled":token.is_cancelled()}}),
    )
}
