//! Output transactions and integrity contracts; no queue or live-photo workflow.
//! Publication, overwrite, cancellation and resume rules are preserved verbatim.
use crate::engine::contracts::LEGACY_FORMAT;
use crate::engine::options::ConversionOptions;
use crate::engine::paths::{self, extension, file_name, json as s, stem};
use crate::engine::{failure, hash_file, io_error, read, uuid, write_json, StopToken};
use crate::{format, jpeg, sha256_hex, Error, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub use crate::engine::contracts::TO_APPLE_REVISION as REVISION;
/// Earlier to-apple revisions whose results are still accepted on resume.
const ACCEPTED: [&str; 12] = [
    REVISION,
    "gui-native-1.1.6",
    "gui-native-1.1.5",
    "gui-native-1.1.4",
    "gui-native-1.1.3",
    "gui-native-1.1.2",
    "gui-native-1.1.1",
    "gui-native-1.1.0",
    "gui-native-1.0.6",
    "gui-native-1.0.5",
    "gui-native-1.0.4",
    "gui-native-1.0.3",
];
pub(crate) const MANIFEST: &str = "manifest.json";

// ---------------------------------------------------------------------------
// Staging and publishing
// ---------------------------------------------------------------------------

/// A hidden staging folder next to the target, removed on drop.
pub struct Stage(PathBuf);
impl Stage {
    pub fn new(target: &Path) -> Result<Stage> {
        let parent = target.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        let dir = parent.join(format!(".motionlive-{}", uuid()));
        std::fs::create_dir(&dir).map_err(|e| io_error(&dir, e))?;
        Ok(Stage(dir))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A private temporary folder, removed on drop.
pub struct WorkDir(PathBuf);
impl WorkDir {
    pub fn new() -> Result<WorkDir> {
        let dir = std::env::temp_dir().join(format!("motionlive-{}", uuid()));
        std::fs::create_dir(&dir).map_err(|e| io_error(&dir, e))?;
        Ok(WorkDir(dir))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Move `from` to `to`: exchanged with an existing `to` when `swap`, else
/// only when `to` does not exist. Atomic where the OS supports it
/// (macOS `renamex_np`, Linux `renameat2`).
pub fn rename_dir(from: &Path, to: &Path, swap: bool) -> std::io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let a = CString::new(from.as_os_str().as_bytes())?;
        let b = CString::new(to.as_os_str().as_bytes())?;
        let flags = if swap {
            libc::RENAME_SWAP
        } else {
            libc::RENAME_EXCL
        };
        // SAFETY: both arguments are valid NUL-terminated paths.
        let rc = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), flags) };
        if rc == 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error())
    }
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let a = CString::new(from.as_os_str().as_bytes())?;
        let b = CString::new(to.as_os_str().as_bytes())?;
        let flags = if swap {
            libc::RENAME_EXCHANGE
        } else {
            libc::RENAME_NOREPLACE
        };
        // SAFETY: both arguments are valid NUL-terminated paths.
        let rc = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                flags,
            )
        };
        if rc == 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        if !matches!(e.raw_os_error(), Some(libc::EINVAL) | Some(libc::ENOSYS)) {
            return Err(e);
        }
        rename_fallback(from, to, swap)
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "linux")))]
    {
        rename_fallback(from, to, swap)
    }
}

/// Non-atomic fallback: the old target is moved aside first and restored
/// if the second rename fails.
#[allow(dead_code)]
fn rename_fallback(from: &Path, to: &Path, swap: bool) -> std::io::Result<()> {
    if !swap {
        if to.exists() {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
        }
        return std::fs::rename(from, to);
    }
    let aside = to.with_file_name(format!(".motionlive-old-{}", uuid()));
    std::fs::rename(to, &aside)?;
    if let Err(e) = std::fs::rename(from, to) {
        let _ = std::fs::rename(&aside, to);
        return Err(e);
    }
    // `from` now holds the old target, as with an atomic exchange.
    std::fs::rename(&aside, from)
}

/// Swift `NativeBatch.publish`.
pub fn publish(
    stage: &Stage,
    target: &Path,
    source: &Path,
    source_hash: &str,
    token: &StopToken,
    overwrite: bool,
    export_data_files: bool,
) -> Result<()> {
    let verified = verify(stage.path())?;
    if hash_file(source)? != source_hash {
        return failure("转换期间原片发生变化，未发布结果");
    }
    if token.is_cancelled() {
        return failure("已停止；当前暂存结果未发布");
    }
    if !export_data_files
        && verified["source_format"].as_str() != Some(LEGACY_FORMAT)
        && verified["source_format"].as_str() != Some("static-hdr")
    {
        let m = stage.path().join(MANIFEST);
        std::fs::remove_file(&m).map_err(|e| io_error(&m, e))?;
    }
    let replacing = overwrite && target.exists();
    rename_dir(stage.path(), target, replacing)
        .map_err(|e| Error::new(format!("无法发布结果（可能有同名文件或权限不足）：{e}")))
}

// ---------------------------------------------------------------------------
// Manifest verification and names
// ---------------------------------------------------------------------------

pub fn is_candidate_photo_role(role: &str) -> bool {
    role == "original"
        || role
            .strip_prefix("candidate_")
            .and_then(|n| n.parse::<i64>().ok())
            .is_some_and(|n| n >= 0)
}

pub fn candidate_video_role(photo_role: &str) -> String {
    if photo_role == "original" {
        "original_video".into()
    } else {
        format!(
            "candidate_video_{}",
            &photo_role["candidate_".len().min(photo_role.len())..]
        )
    }
}

fn string_map(v: &Value) -> Option<BTreeMap<String, String>> {
    let m = v.as_object()?;
    let mut out = BTreeMap::new();
    for (k, v) in m {
        out.insert(k.clone(), v.as_str()?.to_string());
    }
    Some(out)
}

fn default_names() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("still".to_string(), "photo.jpg".to_string()),
        ("video".to_string(), "photo.mov".to_string()),
    ])
}

/// Swift `NativeBatch.verify`: the manifest's files exist and hash right.
pub fn verify(folder: &Path) -> Result<Value> {
    let invalid = || Error::new("已有输出的校验清单无效");
    let text = read(&folder.join(MANIFEST))?;
    let manifest: Value = serde_json::from_slice(&text).map_err(|_| invalid())?;
    verify_manifest(folder, &manifest)?;
    Ok(manifest)
}

/// The same integrity contract using a manifest retained in the task entry
/// when the user chose not to write JSON sidecars beside their media.
pub fn verify_manifest(folder: &Path, manifest: &Value) -> Result<()> {
    let invalid = || Error::new("已有输出的校验清单无效");
    let hashes = string_map(&manifest["output_sha256"]).ok_or_else(invalid)?;
    if !manifest["timing"].is_object() || !manifest["hdr"].is_object() {
        return Err(invalid());
    }
    let names = if manifest.get("output_files").is_some() {
        string_map(&manifest["output_files"]).ok_or_else(invalid)?
    } else {
        default_names()
    };
    let legacy = manifest["source_format"].as_str() == Some(LEGACY_FORMAT);
    let legacy_names: BTreeMap<String, String> = names
        .iter()
        .filter(|(r, _)| r.starts_with("legacy_"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if legacy {
        for (role, name) in [
            ("legacy_original", "source-original.bin"),
            ("legacy_private", "source-private.bin"),
            ("legacy_tail", "source-tail.bin"),
        ] {
            if names.get(role).map(String::as_str) != Some(name) {
                return failure("旧华为原始附属数据清单缺失");
            }
        }
        if !manifest["manual_cover"].is_object()
            || hashes.get("source-original.bin").map(String::as_str)
                != manifest["source_sha256"].as_str()
        {
            return failure("旧华为原片备份或人工策略校验失败");
        }
        for (r, n) in &legacy_names {
            let expected = match r.as_str() {
                "legacy_original" => "source-original.bin".to_string(),
                "legacy_private" => "source-private.bin".to_string(),
                "legacy_tail" => "source-tail.bin".to_string(),
                _ => {
                    let i = r
                        .strip_prefix("legacy_exif_")
                        .and_then(|v| v.parse::<usize>().ok())
                        .ok_or_else(invalid)?;
                    format!("source-exif-{i}.bin")
                }
            };
            if n != &expected {
                return Err(invalid());
            }
        }
    } else if !legacy_names.is_empty() {
        return Err(invalid());
    }
    let keys: HashSet<&str> = names
        .keys()
        .filter(|k| !k.starts_with("legacy_"))
        .map(String::as_str)
        .collect();
    let extra: HashSet<&str> = keys
        .iter()
        .copied()
        .filter(|k| *k != "still" && *k != "video")
        .collect();
    let photo_roles: Vec<&str> = extra
        .iter()
        .copied()
        .filter(|r| is_candidate_photo_role(r))
        .collect();
    let mut allowed: HashSet<String> = HashSet::new();
    for r in &photo_roles {
        allowed.insert(r.to_string());
        let v = candidate_video_role(r);
        if keys.contains(v.as_str()) {
            allowed.insert(v);
        }
    }
    let candidate_set = keys.contains("still")
        && keys.contains("video")
        && !photo_roles.is_empty()
        && extra.iter().map(|s| s.to_string()).collect::<HashSet<_>>() == allowed;
    fn set<'a>(v: &[&'a str]) -> HashSet<&'a str> {
        v.iter().copied().collect()
    }
    let burst = matches!(
        manifest["source_format"].as_str(),
        Some("honor-burst" | "google-burst" | "huawei-burst" | "samsung-burst")
    );
    let known = [
        set(&["still", "video"]),
        set(&["still", "video", "auxiliary"]),
        set(&["auxiliary"]),
        set(&["motionphoto"]),
        set(&["static"]),
    ]
    .contains(&keys);
    if manifest["source_format"].as_str() == Some("static-hdr") && keys != set(&["static"]) {
        return failure("静态 HDR 输出不能包含视频或配对角色");
    }
    if keys.contains("static")
        && (manifest["source_format"].as_str() != Some("static-hdr")
            || manifest.get("identifier").is_some()
            || manifest["timing"] != json!({"applicable":false}))
    {
        return failure("静态 HDR 输出清单无效");
    }
    let values: HashSet<&str> = names.values().map(String::as_str).collect();
    let hash_keys: HashSet<&str> = hashes.keys().map(String::as_str).collect();
    let names_ok = names
        .values()
        .all(|n| !n.is_empty() && !n.contains(['/', '\\', '\0']) && n != "." && n != "..");
    if !(known || candidate_set || burst)
        || hash_keys != values
        || hashes.len() != names.len()
        || !names_ok
    {
        return failure("已有输出的配对文件名无效");
    }
    for (role, name) in &names {
        let ext = extension(Path::new(name));
        let image = role == "static"
            || role == "still"
            || role == "motionphoto"
            || is_candidate_photo_role(role);
        let ok = if burst {
            (role.starts_with("burst_frame_") && ["jpg", "jpeg"].contains(&ext.as_str()))
                || (role.starts_with("burst_original_") && ext == "bin")
        } else if role.starts_with("legacy_") {
            legacy && ext == "bin"
        } else if image {
            paths::PHOTO_EXTENSIONS.contains(&ext.as_str())
        } else {
            ext == "mov"
        };
        if !ok {
            return failure("输出文件扩展名无效");
        }
        if crate::engine::identity::SourceVersion::read(&folder.join(name)).is_none()
            || hash_file(&folder.join(name))? != hashes[name]
        {
            return failure(format!("已有输出校验失败：{name}"));
        }
    }
    if burst {
        crate::engine::burst::verify(folder, manifest)?;
    }
    Ok(())
}

pub fn output_names(source: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("still".to_string(), file_name(source)),
        ("video".to_string(), format!("{}.mov", stem(source))),
    ])
}

pub fn expected_names(source: &Path, options: &ConversionOptions) -> BTreeMap<String, String> {
    let mut names = if options.output_mode == "auxiliary-only" {
        BTreeMap::new()
    } else {
        output_names(source)
    };
    if options.cover_index.is_some() {
        names.insert("still".into(), format!("{}.jpg", stem(source)));
    }
    if options.output_mode == "livephoto+auxiliary" || options.output_mode == "auxiliary-only" {
        names.insert(
            "auxiliary".into(),
            format!("{}.auxiliary.mov", stem(source)),
        );
    }
    names
}

pub(crate) fn names_json(names: &BTreeMap<String, String>) -> Value {
    Value::Object(
        names
            .iter()
            .map(|(k, v)| (k.clone(), Value::from(v.clone())))
            .collect(),
    )
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir(to).map_err(|e| io_error(to, e))?;
    for e in std::fs::read_dir(from).map_err(|e| io_error(from, e))? {
        let e = e.map_err(|e| io_error(from, e))?;
        let dest = to.join(e.file_name());
        if e.file_type().map_err(|x| io_error(&e.path(), x))?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            std::fs::copy(e.path(), &dest).map_err(|x| io_error(&dest, x))?;
        }
    }
    Ok(())
}

/// Swift `NativeBatch.restoreNames`: rename a 1.0.3 pair to the source's
/// names in a staged copy, verify it, then swap the folders.
pub fn restore_names(
    target: &Path,
    source: &Path,
    original: &Value,
    token: &StopToken,
) -> Result<()> {
    let names = output_names(source);
    let old_names = string_map(&original["output_files"]).unwrap_or_else(default_names);
    let old_hashes = string_map(&original["output_sha256"]).unwrap_or_default();
    let parent = target.parent().unwrap_or(Path::new("."));
    let stage = Stage(parent.join(format!(".motionlive-name-{}", uuid())));
    copy_dir(target, stage.path())?;
    let mut hashes = BTreeMap::new();
    for key in ["still", "video"] {
        let (Some(before), Some(after)) = (old_names.get(key), names.get(key)) else {
            return failure("已有输出的配对文件名无效");
        };
        if before != after {
            let (a, b) = (stage.path().join(before), stage.path().join(after));
            rename_dir(&a, &b, false).map_err(|e| io_error(&a, e))?;
        }
        let Some(h) = old_hashes.get(before) else {
            return failure("已有输出的配对文件名无效");
        };
        hashes.insert(after.clone(), h.clone());
    }
    let mut manifest = original.clone();
    manifest["implementation"] = json!(REVISION);
    manifest["version"] = json!("1.1.4");
    manifest["output_files"] = names_json(&names);
    manifest["original_filename"] = json!(file_name(source));
    manifest["output_sha256"] = names_json(&hashes);
    write_json(&manifest, &stage.path().join(MANIFEST))?;
    verify(stage.path())?;
    if token.is_cancelled() {
        return failure("已停止；原有文件名未改动");
    }
    rename_dir(stage.path(), target, true)
        .map_err(|e| Error::new(format!("无法恢复输出文件名；原结果已保留：{e}")))
}

fn resumed(source: &Path, target: &Path, names: Value, detail: &str) -> Value {
    json!({"status": "resumed", "source": s(source), "output": s(target), "output_files": names, "detail": detail})
}

/// Resume an existing `.livephoto` result (Swift `convert`'s first branch).
pub(crate) fn resume(
    source: &Path,
    target: &Path,
    options: &ConversionOptions,
    token: &StopToken,
    source_hash: &str,
) -> Result<Value> {
    let manifest = verify(target)?;
    if options.output_mode != "auxiliary-only" && manifest["hdr"]["policy"] != super::hdr::POLICY {
        return failure("已有实况结果未执行当前 HDR 策略，请显式覆盖");
    }
    let mut previous = manifest["options"].as_object().cloned().unwrap_or_default();
    for (k, v) in [
        ("video_track", json!("primary")),
        ("output_mode", json!("livephoto")),
        ("repair_auxiliary_duration", json!(false)),
        ("export_data_files", json!(true)),
    ] {
        previous.entry(k.to_string()).or_insert(v);
    }
    let implementation = manifest["implementation"].as_str().unwrap_or("");
    let same = manifest["source_sha256"].as_str() == Some(source_hash)
        && ACCEPTED.contains(&implementation)
        && crate::engine::same_json(&Value::Object(previous), &options.json());
    if !same {
        return failure("同名输出已存在且内容或设置不同；未覆盖，请选择新的输出目录");
    }
    if options.legacy_hdr
        && manifest["hdr"]["status"].as_str() == Some("converted-experimental")
        && manifest["hdr"]["profile"].as_str() != Some(crate::engine::hdr::honor::PROFILE)
    {
        return failure("已有输出使用其他荣耀 EHDR 实验配置，请选择覆盖以应用当前配置");
    }
    if implementation != REVISION && options.output_mode != "auxiliary-only" {
        let data = read(source)?;
        let original = format::parse(&data)?;
        if crate::mp4::Movie::parse(original.video(&data))?
            .tracks
            .iter()
            .any(|t| t.samples.handler == "soun")
        {
            return failure(
                "旧版实况未通过当前 Apple 音频兼容校验，请显式覆盖重新处理；未复用旧音频封装",
            );
        }
        if let Some(ext) = crate::engine::hdr::honor_private(&original.still)? {
            if manifest["hdr"]["private_extension_sha256"].as_str()
                != Some(sha256_hex(&ext).as_str())
            {
                return failure(
                    "旧版本输出未保留荣耀私有 HDR 静态扩展，请在界面选择覆盖后重新转换",
                );
            }
            if options.legacy_hdr
                && options.cover_index.is_none()
                && crate::hdr::honor_ehdr(&original.still, jpeg::image_end(&original.still)?)?
                    .is_some_and(|h| !h.luma.is_empty())
            {
                return failure("已有输出由旧版 HDR 流程生成，请选择覆盖以尝试荣耀 EHDR 实验转换");
            }
        }
    }
    let stored = string_map(&manifest["output_files"]).unwrap_or_default();
    let wanted = expected_names(source, options);
    let names_match = if manifest["source_format"].as_str() == Some(LEGACY_FORMAT)
        || options.output_mode == "livephoto+candidates"
    {
        wanted.iter().all(|(k, v)| stored.get(k) == Some(v))
    } else {
        stored == wanted
    };
    if !names_match {
        if options.output_mode != "livephoto" {
            return failure("已有输出文件名与所选模式不同，请使用新的输出位置");
        }
        restore_names(target, source, &manifest, token)?;
        return Ok(resumed(
            source,
            target,
            names_json(&wanted),
            "已恢复原始文件名，图片和视频未重新编码",
        ));
    }
    let mut result = resumed(
        source,
        target,
        names_json(&stored),
        &format!("已验证；{}", super::hdr::summary(&manifest["hdr"])),
    );
    result["hdr"] = manifest["hdr"].clone();
    result["verification"] = manifest;
    Ok(result)
}
