//! Swift `NativeBatch`: staging, verification, atomic publish, resume of
//! earlier results, the Motion Photo -> Live Photo conversion, the batch
//! runner (bounded workers, existing-output decisions, progress, report)
//! and scanning.

use crate::engine::candidates::{self, Session};
use crate::engine::movie::{self, MovieRequest};
use crate::engine::options::ConversionOptions;
use crate::engine::paths::{self, extension, file_name, stem};
use crate::engine::scanrow::{self, ScanRow, SourceVersion};
use crate::engine::still;
use crate::engine::{
    android, failure, hash_file, io_error, read, transfer, uuid, write, write_json, Native,
    StopToken, Timings,
};
use crate::{format, heif, jpeg, sha256_hex, Error, Result};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

pub const REVISION: &str = "gui-native-1.1.5";
/// Earlier to-apple revisions whose results are still accepted on resume.
const ACCEPTED: [&str; 10] = [
    REVISION,
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
const MANIFEST: &str = "manifest.json";

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
    verify(stage.path())?;
    if hash_file(source)? != source_hash {
        return failure("转换期间原片发生变化，未发布结果");
    }
    if token.is_cancelled() {
        return failure("已停止；当前暂存结果未发布");
    }
    if !export_data_files {
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
    let hashes = string_map(&manifest["output_sha256"]).ok_or_else(invalid)?;
    if !manifest["timing"].is_object() || !manifest["hdr"].is_object() {
        return Err(invalid());
    }
    let names = if manifest.get("output_files").is_some() {
        string_map(&manifest["output_files"]).ok_or_else(invalid)?
    } else {
        default_names()
    };
    let keys: HashSet<&str> = names.keys().map(String::as_str).collect();
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
    let known = [
        set(&["still", "video"]),
        set(&["still", "video", "auxiliary"]),
        set(&["auxiliary"]),
        set(&["motionphoto"]),
    ]
    .contains(&keys);
    let values: HashSet<&str> = names.values().map(String::as_str).collect();
    let hash_keys: HashSet<&str> = hashes.keys().map(String::as_str).collect();
    let names_ok = names
        .values()
        .all(|n| !n.is_empty() && !n.contains('/') && !n.contains('\0') && n != "." && n != "..");
    if !(known || candidate_set) || hash_keys != values || hashes.len() != names.len() || !names_ok
    {
        return failure("已有输出的配对文件名无效");
    }
    for (role, name) in &names {
        let ext = extension(Path::new(name));
        let image = role == "still" || role == "motionphoto" || is_candidate_photo_role(role);
        let ok = if image {
            paths::PHOTO_EXTENSIONS.contains(&ext.as_str())
        } else {
            ext == "mov"
        };
        if !ok {
            return failure("输出文件扩展名无效");
        }
        if hash_file(&folder.join(name))? != hashes[name] {
            return failure(format!("已有输出校验失败：{name}"));
        }
    }
    Ok(manifest)
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

fn names_json(names: &BTreeMap<String, String>) -> Value {
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

// ---------------------------------------------------------------------------
// Motion Photo -> Live Photo (Swift `NativeBatch.convert`)
// ---------------------------------------------------------------------------

fn s(p: &Path) -> Value {
    Value::from(p.to_string_lossy().into_owned())
}

fn resumed(source: &Path, target: &Path, names: Value, detail: &str) -> Value {
    json!({"status": "resumed", "source": s(source), "output": s(target), "output_files": names, "detail": detail})
}

fn movie_request<'a>(
    still: &'a [u8],
    identifier: &'a str,
    options: &'a ConversionOptions,
) -> MovieRequest<'a> {
    MovieRequest {
        still: Some(still),
        identifier,
        video_track: &options.video_track,
        missing_time: &options.missing_time,
        time_domain: &options.time_domain,
        ..Default::default()
    }
}

/// Swift `NativeBatch.convert`: one Motion Photo to a `.livephoto` folder.
pub fn convert(
    native: &dyn Native,
    source: &Path,
    target: &Path,
    options: &ConversionOptions,
    token: &StopToken,
    overwrite: bool,
    timings: &Timings,
) -> Result<Value> {
    let mut options = options.clone();
    if let Some(i) = options
        .cover_selections
        .get(&*paths::normalized(source).to_string_lossy())
    {
        options.cover_index = Some(*i);
    }
    let options = &options;
    let modes = [
        "livephoto",
        "livephoto+auxiliary",
        "auxiliary-only",
        "livephoto+candidates",
    ];
    if !["primary", "secondary"].contains(&options.video_track.as_str())
        || !modes.contains(&options.output_mode.as_str())
    {
        return failure("轨道或导出模式无效");
    }
    if options.cover_index.is_some() && options.output_mode == "auxiliary-only" {
        return failure("仅导出辅助轨时不能更换照片封面");
    }
    if !((options.cover_index.is_none() && options.output_mode != "livephoto+candidates")
        || options.video_track == "primary")
    {
        return failure("高清候选封面需搭配主运动轨");
    }
    if !["static", "livephoto"].contains(&options.candidate_delivery.as_str()) {
        return failure("候选照片的交付格式无效");
    }
    let source_hash = timings.measure("source_hash", || hash_file(source))?;
    if !overwrite && target.exists() {
        return resume(source, target, options, token, &source_hash);
    }
    let data = timings.measure("source_read", || read(source))?;
    let photo = timings.measure("source_parse", || format::parse(&data))?;
    if options.output_mode != "auxiliary-only"
        && options.cover_index.is_none()
        && photo.timestamp.is_none()
        && photo.cover_frame.is_none()
        && options.missing_time == "error"
    {
        return failure("原片没有有效封面时间戳或封面帧号，未使用推测位置");
    }
    if token.is_cancelled() {
        return failure("已停止");
    }
    let stage = Stage::new(target)?;
    let work = WorkDir::new()?;
    let video = photo.video(&data);
    let mut names = expected_names(source, options);
    let wants_candidates =
        options.cover_index.is_some() || options.output_mode == "livephoto+candidates";
    let session: Option<Session> = if wants_candidates {
        candidates::prepare(video, work.path())?
    } else {
        None
    };
    if let Some(index) = options.cover_index {
        match &session {
            Some(s) => {
                s.candidate(index)?;
            }
            None => return failure("原片没有辅助高清候选，请使用原始封面"),
        }
    }

    // Auxiliary track export.
    let mut auxiliary: Option<Value> = None;
    if let Some(aux_name) = names.get("auxiliary").cloned() {
        let aux_path = stage.path().join(&aux_name);
        let mut repair: Option<Value> = None;
        let mut input: Vec<u8> = video.to_vec();
        if options.repair_auxiliary_duration {
            if let Some((fixed, info)) = movie::repair_auxiliary_duration(video)? {
                input = fixed;
                repair = Some(info);
            }
        }
        let (mov, mut info) = movie::export_auxiliary(&input)?;
        if let Some(r) = repair {
            if info["compressed_sha256"] != r["original_sample_sha256"] {
                return failure("修复后的辅助轨压缩数据与原片不一致");
            }
            info["duration_repair"] = r;
        }
        write(&aux_path, &mov)?;
        let _ = crate::engine::copy_mtime(source, &aux_path);
        auxiliary = Some(info);
    }
    if options.output_mode == "auxiliary-only" {
        let aux = auxiliary.unwrap_or(Value::Null);
        let name = names["auxiliary"].clone();
        let manifest = json!({
            "version": "1.1.4", "implementation": REVISION, "source": s(source),
            "source_sha256": source_hash, "original_filename": file_name(source),
            "options": options.json(), "output_files": names_json(&names),
            "output_sha256": {name.clone(): hash_file(&stage.path().join(&name))?},
            "timing": aux, "auxiliary_video": aux, "hdr": {"status": "not-applicable"}, "warnings": [],
        });
        write_json(&manifest, &stage.path().join(MANIFEST))?;
        timings.measure("validation_and_publish", || {
            publish(
                &stage,
                target,
                source,
                &source_hash,
                token,
                overwrite,
                options.export_data_files,
            )
        })?;
        let repaired = if aux.get("duration_repair").is_some() {
            "；已按设置修复零时长，画面数据未改变"
        } else {
            ""
        };
        let num = |k: &str| aux[k].to_string();
        return Ok(json!({
            "status": "converted", "source": s(source), "output": s(target),
            "detail": format!("辅助轨已单独导出：{} × {}，{} 帧，{} 秒；无音频，未重新编码{repaired}", num("width"), num("height"), num("frames"), num("duration_seconds")),
            "auxiliary_video": aux,
        }));
    }

    // The still and its HDR handling.
    let mut image: Vec<u8> = photo.still.to_vec();
    let mut selected_candidate: Option<Value> = None;
    if let (Some(index), Some(sess)) = (options.cover_index, &session) {
        let selected = work.path().join("selected-cover.jpg");
        selected_candidate = Some(sess.write_jpeg(native, index, &selected)?);
        image = read(&selected)?;
    }
    let is_heic = heif::matches(&image);
    let mut hdr: Map<String, Value> = Map::new();
    hdr.insert(
        "input".into(),
        json!(if photo.ultra {
            "ultra-hdr"
        } else {
            "legacy-or-sdr"
        }),
    );
    hdr.insert("status".into(), json!("preserved"));
    hdr.insert("reason".into(), json!("Ultra HDR 主图及增益图原样保留"));
    if !is_heic && !photo.ultra && crate::scan::has_iso(&image)? {
        hdr.insert("input".into(), json!("iso-hdr"));
    }
    let mut warnings: Vec<String> = Vec::new();
    let honor = still::honor_private(&image)?;
    if let Some(ext) = &honor {
        let reason = "已保留荣耀私有 HDR 静态扩展；尚不能可靠转换为 Ultra HDR，Apple 设备显示 SDR。可先用荣耀相册转为通用 HDR 后再导入";
        hdr.insert("input".into(), json!("honor-private-ehdr"));
        hdr.insert("status".into(), json!("unsupported-preserved"));
        hdr.insert("reason".into(), json!(reason));
        hdr.insert("private_extension_sha256".into(), json!(sha256_hex(ext)));
        warnings.push(reason.into());
    }
    if auxiliary
        .as_ref()
        .is_some_and(|a| a.get("duration_repair").is_some())
    {
        warnings.push("已按高级设置修复独立辅助视频的零时长，压缩画面与主封面时间未改变".into());
    }
    if is_heic {
        hdr = json!({"input": "heic", "status": "preserved", "reason": "HEIC 图像编码、色彩与方向属性原样保留；未新增或转换 HDR"})
            .as_object()
            .cloned()
            .unwrap_or_default();
    }
    if options.cover_index.is_some() {
        hdr = json!({"input": "auxiliary-video", "status": "decoded-sdr", "reason": "辅助高清帧已解码为 JPEG 封面；主运动轨保持原编码，未复制原封面的 HDR 增益图"})
            .as_object()
            .cloned()
            .unwrap_or_default();
    }
    if options.cover_index.is_none() && !is_heic && !photo.ultra && honor.is_none() {
        hdr.insert("status".into(), json!("unchanged"));
        hdr.insert("reason".into(), json!("旧版 HDR 转换已关闭"));
        if options.legacy_hdr {
            if let Err(e) = convert_legacy_hdr(native, &mut image, &mut hdr) {
                warnings.push(e.0);
                hdr.insert(
                    "reason".into(),
                    json!("旧 HDR 尝试转换未完成，原格式已保留"),
                );
            }
        }
    }

    let identifier = uuid();
    let live = still::live_still(&image, &identifier, true)?;
    if still::honor_private(&live)? != honor {
        return failure("荣耀私有 HDR 静态扩展字节校验失败");
    }
    let static_ok = if is_heic {
        heif::layout(&live)?
            .boxes
            .iter()
            .all(|b| !b.is(b"mpvd") && !b.is(b"moov"))
    } else {
        jpeg::static_only(&live)? == live
    };
    if !static_ok {
        return failure("导出静态图片仍包含图片范围之外的数据");
    }
    if format::coding_hash(&live)? != format::coding_hash(&image)? {
        return failure("静态主图压缩数据发生变化");
    }
    let still_path = stage.path().join(&names["still"]);
    let mov_path = stage.path().join(&names["video"]);
    write(&still_path, &live)?;

    let mut request = movie_request(&live, &identifier, options);
    let candidate_time;
    if let (Some(index), Some(sess)) = (options.cover_index, &session) {
        candidate_time = sess.candidate(index)?.timestamp_us;
        request.timestamp_us = Some(candidate_time);
        request.time_domain = "presentation";
        request.timestamp_source = Some("high-res-candidate");
    } else {
        request.timestamp_us = photo.timestamp;
        request.cover_frame_index = photo.cover_frame;
    }
    if options.cover_index.is_none()
        && photo.timestamp_tag.as_deref() == Some("MP4:motionphoto-image-meta")
    {
        request.timestamp_source = Some("video-metadata");
        warnings.push("主 XMP 缺失；已从内嵌视频元数据恢复原始封面时间".into());
    }
    let (mov, timing) = timings.measure("media_remux_and_validation", || {
        movie::movie(video, &request)
    })?;
    write(&mov_path, &mov)?;

    // Extra candidate stills (and optional Live pairs for each).
    let mut candidate_photos: Vec<Value> = Vec::new();
    if options.output_mode == "livephoto+candidates" {
        let source_stem = stem(source);
        let pair_candidate = |names: &mut BTreeMap<String, String>,
                              role: &str,
                              name: &str,
                              timestamp: Option<i64>,
                              frame: Option<i64>,
                              domain: &str|
         -> Result<Option<Value>> {
            if options.candidate_delivery != "livephoto" {
                return Ok(None);
            }
            if token.is_cancelled() {
                return failure("已停止");
            }
            let path = stage.path().join(name);
            let id = uuid();
            let bytes = still::live_still(&read(&path)?, &id, true)?;
            write(&path, &bytes)?;
            let movie_name = format!("{}.mov", stem(Path::new(name)));
            let mut req = movie_request(&bytes, &id, options);
            req.time_domain = domain;
            req.video_track = "primary";
            req.timestamp_us = timestamp;
            req.cover_frame_index = frame;
            if role != "original" {
                req.timestamp_source = Some("high-res-candidate");
            }
            let (m, t) =
                timings.measure("media_remux_and_validation", || movie::movie(video, &req))?;
            write(&stage.path().join(&movie_name), &m)?;
            names.insert(candidate_video_role(role), movie_name);
            Ok(Some(t))
        };
        if options.cover_index.is_some() {
            let name = format!(
                "{source_stem}.original.{}",
                source.extension().and_then(|e| e.to_str()).unwrap_or("")
            );
            let original = still::live_still(&photo.still, &identifier, false)?;
            write(&stage.path().join(&name), &original)?;
            names.insert("original".into(), name.clone());
            let mut info =
                json!({"role": "original", "file": name, "encoding": "original-photo-preserved"});
            if let Some(t) = pair_candidate(
                &mut names,
                "original",
                &name,
                photo.timestamp,
                photo.cover_frame,
                &options.time_domain,
            )? {
                info["timing"] = t;
            }
            candidate_photos.push(info);
        }
        if let Some(sess) = &session {
            for c in &sess.candidates {
                if Some(c.index as i64) == options.cover_index {
                    continue;
                }
                if token.is_cancelled() {
                    return failure("已停止");
                }
                let role = format!("candidate_{}", c.index);
                let name = format!("{source_stem}.candidate-{:03}.jpg", c.index + 1);
                let mut info =
                    sess.write_jpeg(native, c.index as i64, &stage.path().join(&name))?;
                info["role"] = json!(role);
                info["file"] = json!(name);
                names.insert(role.clone(), name.clone());
                if let Some(t) = pair_candidate(
                    &mut names,
                    &role,
                    &name,
                    Some(c.timestamp_us),
                    None,
                    "presentation",
                )? {
                    info["timing"] = t;
                }
                candidate_photos.push(info);
            }
        }
    }
    let _ = crate::engine::copy_mtime(source, &still_path);
    let _ = crate::engine::copy_mtime(source, &mov_path);
    let mut hashes = BTreeMap::new();
    for name in names.values() {
        hashes.insert(name.clone(), hash_file(&stage.path().join(name))?);
    }
    let null_or = |v: Option<Value>| v.unwrap_or(Value::Null);
    let mut manifest = json!({
        "version": "1.1.4", "implementation": REVISION, "source": s(source),
        "source_sha256": source_hash, "identifier": identifier, "options": options.json(),
        "output_files": names_json(&names), "original_filename": file_name(source),
        "source_timestamp_us": null_or(photo.timestamp.map(Value::from)),
        "source_timestamp_tag": null_or(photo.timestamp_tag.clone().map(Value::from)),
        "source_cover_frame_index": null_or(photo.cover_frame.map(Value::from)),
        "source_format": photo.format, "still_contains_embedded_media": false,
        "timing": timing, "hdr": Value::Object(hdr.clone()), "warnings": warnings,
        "primary_coding_sha256": format::coding_hash(&live)?,
        "auxiliary_sha256": format::auxiliary_hashes(&live)?,
        "output_sha256": names_json(&hashes),
    });
    if let Some(a) = &auxiliary {
        manifest["auxiliary_video"] = a.clone();
    }
    if let Some(c) = &selected_candidate {
        manifest["selected_cover"] = c.clone();
    }
    if options.output_mode == "livephoto+candidates" {
        manifest["candidate_photos"] = Value::from(candidate_photos.clone());
    }
    write_json(&manifest, &stage.path().join(MANIFEST))?;
    timings.measure("validation_and_publish", || {
        publish(
            &stage,
            target,
            source,
            &source_hash,
            token,
            overwrite,
            options.export_data_files,
        )
    })?;
    let mut detail = if warnings.is_empty() {
        hdr.get("reason")
            .and_then(Value::as_str)
            .unwrap_or("已转换并校验")
            .to_string()
    } else {
        warnings.join("；")
    };
    if options.output_mode == "livephoto+candidates" {
        detail += &format!(
            "；另存 {} 张{}",
            candidate_photos.len(),
            if options.candidate_delivery == "livephoto" {
                "独立实况照片"
            } else {
                "独立高清静态照片"
            }
        );
    }
    Ok(json!({
        "status": "converted", "source": s(source), "output": s(target), "timing": timing,
        "hdr": Value::Object(hdr), "warnings": warnings, "output_files": names_json(&names),
        "detail": detail,
    }))
}

/// The legacy-HDR branch of Swift `convert` (ISO upgrade or old Apple gain
/// map to Ultra HDR). Errors become warnings in the caller.
fn convert_legacy_hdr(
    native: &dyn Native,
    image: &mut Vec<u8>,
    hdr: &mut Map<String, Value>,
) -> Result<()> {
    if crate::scan::has_iso(image)? {
        if let Some(upgraded) = still::upgrade_iso(image)? {
            *image = upgraded;
            hdr.insert("status".into(), json!("converted"));
            hdr.insert("output".into(), json!("ultra-hdr+iso"));
            hdr.insert(
                "reason".into(),
                json!("已有 ISO 增益图已补充 Ultra HDR 元数据；主图及增益图均未重新编码"),
            );
        } else {
            hdr.insert(
                "reason".into(),
                json!("ISO HDR 数据已保留；缺少可验证的单增益图布局，未推测转换"),
            );
        }
        return Ok(());
    }
    let (legacy, reason) = crate::apple_hdr::legacy(image)?;
    hdr.insert("reason".into(), json!(reason));
    let Some(legacy) = legacy else {
        return Ok(());
    };
    match still::legacy_gain(native, &legacy.gain, legacy.stops)? {
        Some((gain, width, height)) => {
            let icc = native.srgb_icc()?;
            let tagged = still::color_profile(image, &icc)?;
            *image = still::ultra(&tagged, &gain, legacy.stops)?;
            hdr.insert("status".into(), json!("converted"));
            hdr.insert("stops".into(), json!(legacy.stops));
            hdr.insert("width".into(), json!(width));
            hdr.insert("height".into(), json!(height));
            hdr.insert(
                "reason".into(),
                json!("已有旧 Apple 增益图已转换，主图未重新编码"),
            );
            hdr.insert("output".into(), json!("ultra-hdr"));
        }
        None => {
            hdr.insert(
                "reason".into(),
                json!("Legacy map cannot be decoded as an 8-bit monochrome JPEG"),
            );
        }
    }
    Ok(())
}

/// Resume an existing `.livephoto` result (Swift `convert`'s first branch).
fn resume(
    source: &Path,
    target: &Path,
    options: &ConversionOptions,
    token: &StopToken,
    source_hash: &str,
) -> Result<Value> {
    let manifest = verify(target)?;
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
    if implementation != REVISION && options.output_mode != "auxiliary-only" {
        let data = read(source)?;
        let original = format::parse(&data)?;
        if let Some(ext) = still::honor_private(&original.still)? {
            if manifest["hdr"]["private_extension_sha256"].as_str()
                != Some(sha256_hex(&ext).as_str())
            {
                return failure(
                    "旧版本输出未保留荣耀私有 HDR 静态扩展，请在界面选择覆盖后重新转换",
                );
            }
        }
    }
    let stored = string_map(&manifest["output_files"]).unwrap_or_default();
    let wanted = expected_names(source, options);
    let names_match = if options.output_mode == "livephoto+candidates" {
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
    Ok(resumed(
        source,
        target,
        names_json(&stored),
        "已验证，跳过重复转换",
    ))
}

// ---------------------------------------------------------------------------
// Scheduling (Swift `BatchScheduling`)
// ---------------------------------------------------------------------------

/// Worker count: 4 for scanning, 2 for converting, at most half the CPUs.
pub fn worker_limit(scanning: bool) -> usize {
    let cpus = std::thread::available_parallelism().map_or(2, |n| n.get());
    (if scanning { 4 } else { 2 }).min(cpus / 2).max(1)
}

/// Memory admitted at once across workers (Swift: physical memory / 8,
/// at least 256 MiB). Hosts without that figure use 1 GiB.
///
/// Fixed workers with weighted admission: a large input waits until enough
/// budget is free (and runs alone if needed). `receive` is serialized.
/// Returns (peak workers in flight, peak reserved bytes).
pub fn perform<T: Send>(
    files: &[PathBuf],
    limit: usize,
    heavy: bool,
    token: &StopToken,
    work: impl Fn(usize) -> T + Sync,
    receive: impl FnMut(usize, T) + Send,
) -> (usize, u64) {
    if files.is_empty() {
        return (0, 0);
    }
    let budget = crate::platform::memory_budget();
    let factor = if heavy { 24 } else { 12 };
    let costs: Vec<u64> = files
        .iter()
        .map(|f| {
            let size = std::fs::metadata(f).map_or(budget, |m| m.len());
            (size.min(budget) * factor).clamp(64 << 20, budget)
        })
        .collect();
    struct State {
        next: usize,
        used: u64,
        running: usize,
        peak: usize,
        peak_bytes: u64,
    }
    let state = Mutex::new(State {
        next: 0,
        used: 0,
        running: 0,
        peak: 0,
        peak_bytes: 0,
    });
    let changed = Condvar::new();
    let receive = Mutex::new(receive);
    let workers = limit.min(files.len()).max(1);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let (index, cost) = {
                    let mut st = state.lock().unwrap_or_else(|p| p.into_inner());
                    while st.next < files.len()
                        && !token.is_cancelled()
                        && st.used > 0
                        && st.used + costs[st.next] > budget
                    {
                        st = changed
                            .wait_timeout(st, std::time::Duration::from_millis(100))
                            .unwrap_or_else(|p| p.into_inner())
                            .0;
                    }
                    if st.next >= files.len() || token.is_cancelled() {
                        return;
                    }
                    let index = st.next;
                    st.next += 1;
                    st.used += costs[index];
                    st.running += 1;
                    st.peak = st.peak.max(st.running);
                    st.peak_bytes = st.peak_bytes.max(st.used);
                    (index, costs[index])
                };
                let value = work(index);
                (receive.lock().unwrap_or_else(|p| p.into_inner()))(index, value);
                let mut st = state.lock().unwrap_or_else(|p| p.into_inner());
                st.used -= cost;
                st.running -= 1;
                changed.notify_all();
            });
        }
    });
    let st = state.into_inner().unwrap_or_else(|p| p.into_inner());
    (st.peak, st.peak_bytes)
}

// ---------------------------------------------------------------------------
// Scan and run
// ---------------------------------------------------------------------------

/// Swift `NativeBatch.scan(sources:direction:…)`.
pub fn scan(
    native: &dyn Native,
    sources: &[PathBuf],
    direction: &str,
    token: &StopToken,
    concurrency: Option<usize>,
    mut rows_ready: impl FnMut(&[ScanRow]) + Send,
    mut progress: impl FnMut(usize, usize) + Send,
) -> Result<Vec<ScanRow>> {
    let (base, entries) = if direction == "to-android" {
        android::selection(sources)?
    } else {
        paths::selection(sources)?
    };
    let relatives: Vec<String> = entries
        .iter()
        .map(|e| paths::relative(e, &base))
        .collect::<Result<_>>()?;
    let mut rows: BTreeMap<usize, ScanRow> = BTreeMap::new();
    let mut pending: Vec<ScanRow> = Vec::new();
    let mut last = Instant::now() - std::time::Duration::from_secs(1);
    let total = entries.len();
    let mut completed = 0;
    perform(
        &entries,
        concurrency.unwrap_or_else(|| worker_limit(true)),
        false,
        token,
        |i| match direction {
            "to-android" => android::scan_file(&entries[i], &relatives[i]),
            "android-to-android" => transfer::scan_file(native, &entries[i], &relatives[i], None),
            _ => scanrow::scan_file(native, &entries[i], &relatives[i], None),
        },
        |i, row| {
            completed += 1;
            pending.push(row.clone());
            rows.insert(i, row);
            if completed == total || last.elapsed().as_secs_f64() >= 0.1 {
                rows_ready(&pending);
                pending.clear();
                last = Instant::now();
            }
            progress(completed, total);
        },
    );
    if !pending.is_empty() {
        rows_ready(&pending);
    }
    Ok(rows.into_values().collect())
}

/// What to do with an output folder that already exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingOutput {
    Skip,
    Overwrite,
    Cancel,
}

/// Swift `BatchProgress`.
#[derive(Clone, Debug)]
pub struct Progress {
    pub index: usize,
    pub total: usize,
    pub relative: String,
    pub status: String,
    pub detail: String,
}

/// Swift `BatchOutcome`.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub counts: BTreeMap<String, usize>,
    pub report: Option<PathBuf>,
    pub report_data: Value,
    pub cancelled: bool,
}

pub struct RunRequest<'a> {
    pub sources: &'a [PathBuf],
    pub output: &'a Path,
    pub options: &'a ConversionOptions,
    /// Rows from an earlier scan; reused while a file's version is unchanged.
    pub scan_rows: Option<&'a [ScanRow]>,
    pub concurrency: Option<usize>,
}

/// Swift `NativeBatch.run(sources:…)`.
pub fn run(
    native: &dyn Native,
    req: &RunRequest,
    token: &StopToken,
    existing_output: Option<&(dyn Fn(&Path, &Path) -> ExistingOutput + Sync)>,
    mut progress: impl FnMut(&Progress) + Send,
) -> Result<Outcome> {
    let started = Instant::now();
    let options = req.options;
    let destination = paths::normalized(req.output);
    paths::validate_output(req.sources, &destination)?;
    let (base, entries) = if options.direction == "to-android" {
        android::selection(req.sources)?
    } else {
        paths::selection(req.sources)?
    };
    let enumerated = crate::engine::uptime(started);
    if let Some(rows) = req.scan_rows {
        let a: HashSet<PathBuf> = rows.iter().map(|r| paths::normalized(&r.source)).collect();
        let b: HashSet<PathBuf> = entries.iter().cloned().collect();
        if a != b {
            return failure("来源清单已变化，请重新扫描后转换");
        }
    }
    let relatives: Vec<String> = entries
        .iter()
        .map(|e| paths::relative(e, &base))
        .collect::<Result<_>>()?;
    let snapshot: HashMap<PathBuf, &ScanRow> = req
        .scan_rows
        .unwrap_or(&[])
        .iter()
        .map(|r| (paths::normalized(&r.source), r))
        .collect();
    let mut static_indices: HashSet<usize> = HashSet::new();
    if options.direction != "to-android" {
        perform(
            &entries,
            worker_limit(true),
            false,
            token,
            |i| {
                if let Some(cached) = snapshot.get(&entries[i]) {
                    if cached.version.is_some()
                        && cached.version == SourceVersion::read(&entries[i])
                    {
                        return cached.confirmed_static;
                    }
                }
                if options.direction == "android-to-android" {
                    transfer::scan_file(native, &entries[i], &relatives[i], None).confirmed_static
                } else {
                    scanrow::scan_file(native, &entries[i], &relatives[i], None).confirmed_static
                }
            },
            |i, is_static| {
                if is_static {
                    static_indices.insert(i);
                }
            },
        );
    }
    let planned = crate::engine::uptime(started);
    let total = entries.len() - static_indices.len();
    let mut results: BTreeMap<usize, Value> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = ["converted", "resumed", "skipped", "failed"]
        .iter()
        .map(|k| (k.to_string(), 0))
        .collect();
    let mut sorted_static: Vec<usize> = static_indices.iter().copied().collect();
    sorted_static.sort_unstable();
    for i in &sorted_static {
        results.insert(*i, json!({"status": "skipped", "classification": "static", "source": s(&entries[*i]), "detail": "静态照片，已跳过"}));
        *counts.entry("skipped".into()).or_default() += 1;
        progress(&Progress {
            index: 0,
            total,
            relative: relatives[*i].clone(),
            status: "static".into(),
            detail: "静态照片，已跳过".into(),
        });
    }
    let active: Vec<usize> = (0..entries.len())
        .filter(|i| !static_indices.contains(i))
        .collect();
    let suffix = if options.direction != "to-apple" {
        ".motionphoto"
    } else if options.output_mode == "auxiliary-only" {
        ".auxiliary-export"
    } else {
        ".livephoto"
    };
    let targets: Vec<PathBuf> = active
        .iter()
        .map(|i| destination.join(format!("{}{suffix}", relatives[*i])))
        .collect();
    let folded: HashSet<String> = targets
        .iter()
        .map(|t| t.to_string_lossy().to_lowercase())
        .collect();
    if folded.len() != targets.len() {
        return failure("多个来源映射到同一输出路径，请分批转换");
    }
    if !active.is_empty() {
        std::fs::create_dir_all(&destination).map_err(|e| io_error(&destination, e))?;
    }
    let decision_lock = Mutex::new(());
    let workers = req.concurrency.unwrap_or_else(|| worker_limit(false));
    let heavy = options.cover_index.is_some()
        || !options.cover_selections.is_empty()
        || options.output_mode == "livephoto+candidates";
    let active_files: Vec<PathBuf> = active.iter().map(|i| entries[*i].clone()).collect();
    let mut completed = 0usize;
    let (peak, reserved) = perform(
        &active_files,
        workers,
        heavy,
        token,
        |offset| {
            let file = &active_files[offset];
            let target = &targets[offset];
            let mut overwrite = false;
            let mut wait = 0.0;
            if target.exists() {
                if let Some(decide) = existing_output {
                    let waiting = Instant::now();
                    let decision = {
                        let _g = decision_lock.lock().unwrap_or_else(|p| p.into_inner());
                        if token.is_cancelled() {
                            ExistingOutput::Cancel
                        } else {
                            decide(file, target)
                        }
                    };
                    wait = waiting.elapsed().as_secs_f64();
                    match decision {
                        ExistingOutput::Skip => {
                            return json!({"status": "skipped", "source": s(file), "output": s(target), "detail": "输出已存在，已按选择跳过；已有文件未改动", "decision_wait_seconds": wait})
                        }
                        ExistingOutput::Overwrite => overwrite = true,
                        ExistingOutput::Cancel => token.cancel(),
                    }
                }
            }
            if token.is_cancelled() {
                return json!({"status": "cancelled"});
            }
            let began = Instant::now();
            let timings = Timings::default();
            let outcome = match options.direction.as_str() {
                "android-to-android" => {
                    transfer::convert(native, file, target, options, token, overwrite, &timings)
                }
                "to-android" => {
                    android::convert(native, file, target, options, token, overwrite, &timings)
                }
                _ => convert(native, file, target, options, token, overwrite, &timings),
            };
            let mut result = outcome.unwrap_or_else(
                |e| json!({"status": "failed", "source": s(file), "error": e.0, "detail": e.0}),
            );
            result["stage_seconds"] = timings.seconds();
            result["conversion_seconds"] = json!(began.elapsed().as_secs_f64());
            result["decision_wait_seconds"] = json!(wait);
            result
        },
        |offset, result| {
            let status = result["status"].as_str().unwrap_or("failed").to_string();
            if status == "cancelled"
                || (token.is_cancelled()
                    && !["converted", "resumed", "skipped"].contains(&status.as_str()))
            {
                return;
            }
            let index = active[offset];
            let detail = result["detail"].as_str().unwrap_or("").to_string();
            results.insert(index, result);
            *counts.entry(status.clone()).or_default() += 1;
            completed += 1;
            progress(&Progress {
                index: completed,
                total,
                relative: relatives[index].clone(),
                status,
                detail,
            });
        },
    );
    let stamp = iso8601_now().replace(':', "-");
    let report = options
        .export_data_files
        .then(|| destination.join(format!("report-{stamp}-{}.json", &uuid()[..6])));
    let metrics = json!({
        "enumeration_seconds": enumerated,
        "planning_seconds": planned - enumerated,
        "total_seconds": crate::engine::uptime(started),
        "worker_limit": workers,
        "peak_in_flight": peak,
        "peak_reserved_bytes": reserved,
        "static_skipped": static_indices.len(),
        "conversion_entries": total,
        "completed_entries": completed,
    });
    let data = json!({
        "counts": counts,
        "cancelled": token.is_cancelled(),
        "files": results.into_values().collect::<Vec<_>>(),
        "performance": metrics,
    });
    if let Some(r) = &report {
        std::fs::create_dir_all(&destination).map_err(|e| io_error(&destination, e))?;
        write_json(&data, r)?;
    }
    Ok(Outcome {
        counts,
        report,
        report_data: data,
        cancelled: token.is_cancelled(),
    })
}

/// UTC time as `YYYY-MM-DDTHH:MM:SSZ` (Swift `ISO8601DateFormatter`).
pub fn iso8601_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
