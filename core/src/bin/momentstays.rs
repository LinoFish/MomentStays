//! `momentstays` CLI: scan / convert / verify — Rust port of the Python
//! `motionlive` CLI (`motionlive/cli.py`).
//!
//! Behaviour, output shapes and error strings follow the Python CLI. The
//! video work (MOV remux, legacy gain-map conversion) still runs through the
//! Swift helper `native/MotionLive.swift`, compiled on first use exactly like
//! the Python CLI does, so `convert` needs macOS 12+ and Xcode Command Line
//! Tools; `scan` and `verify` run anywhere.

use momentstays_core::pyjpeg;
use momentstays_core::pymotion;
use momentstays_core::{Error, Result};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const VERSION: &str = "0.1.0";
const NOT_MARKED: &str = "Not a marked Google Motion Photo";

fn err(msg: impl Into<String>) -> Error {
    Error::new(msg.into())
}

fn usage() -> String {
    "usage:\n  momentstays scan <input>\n  momentstays convert <input> -o <output> [--missing-time error|midpoint] [--legacy-hdr auto|preserve] [--time-domain auto|media|presentation]\n  momentstays verify <folder>".to_string()
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn last_chars(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        s.to_string()
    } else {
        s.chars().skip(count - n).collect()
    }
}

fn random_bytes(n: usize) -> Vec<u8> {
    if let Ok(bytes) = fs::read("/dev/urandom") {
        if bytes.len() >= n {
            return bytes[..n].to_vec();
        }
    }
    // Fallback: time + pid mixed (only used for temp names and UUIDs).
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (std::process::id() as u64) << 32
        ^ COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E3779B97F4A7C15);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        out.push((seed & 0xFF) as u8);
    }
    out
}

fn uuid_v4() -> String {
    let b = random_bytes(16);
    let hex = |x: u8| format!("{x:02x}");
    format!(
        "{}{}{}{}-{}{}-{}{}-{}{}-{}{}{}{}{}{}",
        hex(b[0]), hex(b[1]), hex(b[2]), hex(b[3]),
        hex(b[4]), hex(b[5]),
        hex((b[6] & 0x0F) | 0x40), hex(b[7]),
        hex((b[8] & 0x3F) | 0x80), hex(b[9]),
        hex(b[10]), hex(b[11]), hex(b[12]), hex(b[13]), hex(b[14]), hex(b[15]),
    )
}

fn uuid_hex6() -> String {
    uuid_v4().replace('-', "")[..6].to_string()
}

#[cfg(unix)]
fn local_time_str() -> String {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return "19700101-000000".to_string();
        }
        let mut buf = [0i8; 32];
        if libc::strftime(buf.as_mut_ptr(), buf.len(), c"%Y%m%d-%H%M%S".as_ptr(), &tm) == 0 {
            return "19700101-000000".to_string();
        }
        let cstr = std::ffi::CStr::from_ptr(buf.as_ptr());
        cstr.to_string_lossy().into_owned()
    }
}

#[cfg(not(unix))]
fn local_time_str() -> String {
    // UTC fallback (the CLI's primary targets are macOS/Linux).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let tod = secs.rem_euclid(86400);
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

struct TempGuard(PathBuf);
impl Drop for TempGuard {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn mkdtemp(parent: &Path, prefix: &str) -> Result<(PathBuf, TempGuard)> {
    for _ in 0..100 {
        let suffix: String = random_bytes(8).iter().map(|b| format!("{b:02x}")).collect();
        let path = parent.join(format!("{prefix}{suffix}"));
        match fs::create_dir(&path) {
            Ok(()) => return Ok((path.clone(), TempGuard(path))),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(err(e.to_string())),
        }
    }
    Err(err("cannot create a temporary directory"))
}

fn digest(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path).map_err(|e| err(e.to_string()))?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    use std::io::Read;
    loop {
        let n = f.read(&mut buf).map_err(|e| err(e.to_string()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(momentstays_core::hex(&h.finalize()))
}

fn sha256_bytes(data: &[u8]) -> String {
    momentstays_core::hex(&Sha256::digest(data))
}

// ---------------------------------------------------------------------------
// Repository root / Swift helper
// ---------------------------------------------------------------------------

fn absolutize(p: &Path) -> PathBuf {
    let expanded = if let Ok(rest) = p.strip_prefix("~") {
        if let Some(home) = env::var_os("HOME") {
            PathBuf::from(home).join(rest)
        } else {
            p.to_path_buf()
        }
    } else {
        p.to_path_buf()
    };
    if expanded.is_absolute() {
        return expanded;
    }
    match env::current_dir() {
        Ok(cwd) => cwd.join(expanded),
        Err(_) => expanded,
    }
}

fn find_repo_root() -> Result<PathBuf> {
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    starts.push(env::current_dir().map_err(|e| err(e.to_string()))?);
    for start in starts {
        let mut cur: Option<PathBuf> = Some(start);
        while let Some(dir) = cur {
            if dir.join("native/MotionLive.swift").is_file() {
                return Ok(dir);
            }
            cur = dir.parent().map(|p| p.to_path_buf());
        }
    }
    Err(err("找不到仓库根目录（native/MotionLive.swift）"))
}

fn implementation_id(root: &Path) -> Result<String> {
    let mut files: Vec<PathBuf> = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().map_or(false, |e| e == "rs") {
                    out.push(path);
                }
            }
        }
    }
    walk(&root.join("core/src"), &mut files);
    files.sort();
    files.push(root.join("native/MotionLive.swift"));
    let mut h = Sha256::new();
    for f in &files {
        let bytes = fs::read(f).map_err(|e| err(e.to_string()))?;
        h.update(&bytes);
    }
    Ok(momentstays_core::hex(&h.finalize()))
}

/// Wait for a child with piped output and a timeout, like
/// subprocess.run(timeout=): stdout/stderr are drained on threads while the
/// parent polls, so a large output cannot deadlock the wait.
fn wait_output_timeout(mut child: Child, secs: u64) -> Result<std::process::Output> {
    let stdout_handle = std::thread::spawn({
        let mut pipe = child.stdout.take();
        move || {
            let mut buf = Vec::new();
            if let Some(p) = pipe.as_mut() {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        }
    });
    let stderr_handle = std::thread::spawn({
        let mut pipe = child.stderr.take();
        move || {
            let mut buf = Vec::new();
            if let Some(p) = pipe.as_mut() {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        }
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_handle.join().unwrap_or_default();
                let stderr = stderr_handle.join().unwrap_or_default();
                return Ok(std::process::Output { status, stdout, stderr });
            }
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(secs) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(err("原生组件调用超时（300 秒）"));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(err(e.to_string())),
        }
    }
}

fn native(binary: &Path, request: &Value) -> Result<Value> {
    let text = serde_json::to_string(request).map_err(|e| err(e.to_string()))?;
    let mut cmd = Command::new(binary);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| err(e.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let output = wait_output_timeout(child, 300)?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let parsed: Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return Err(err(format!("原生组件未返回 JSON: {}", last_chars(&stderr, 2000)))),
    };
    if !output.status.success() || parsed.get("error").is_some() {
        let message = parsed
            .get("error")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| last_chars(&stderr, 2000));
        return Err(err(message));
    }
    Ok(parsed)
}

fn native_binary(root: &Path) -> Result<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Err(err("Conversion requires macOS 12 or later"));
    }
    let source = root.join("native/MotionLive.swift");
    let build = root.join(".build");
    fs::create_dir_all(&build).map_err(|e| err(e.to_string()))?;
    let arch = match env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => other,
    };
    let source_bytes = fs::read(&source).map_err(|e| err(e.to_string()))?;
    let mut h = Sha256::new();
    h.update(&source_bytes);
    h.update(arch.as_bytes());
    let fingerprint = momentstays_core::hex(&h.finalize())[..16].to_string();
    let target = build.join(format!("motionlive-native-{fingerprint}"));
    if target.is_file() {
        return Ok(target);
    }
    eprintln!("首次运行：编译 macOS 12 原生组件…");
    let (temp, _guard) = mkdtemp(&build, "motionlive-build-")?;
    let executable = temp.join("native");
    let cache = build.join("swift-cache");
    let output = Command::new("xcrun")
        .args([
            "swiftc", "-parse-as-library", "-O", "-target",
            &format!("{arch}-apple-macosx12.0"),
            "-module-cache-path", &cache.to_string_lossy(),
            &source.to_string_lossy(), "-o", &executable.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| err(e.to_string()))?;
    if !output.status.success() {
        return Err(err(format!(
            "Swift 编译失败；需要 Xcode Command Line Tools。\n{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    fs::rename(&executable, &target).map_err(|e| err(e.to_string()))?;
    Ok(target)
}

fn publish(stage: &Path, target: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let from = CString::new(stage.as_os_str().as_bytes()).map_err(|e| err(e.to_string()))?;
        let to = CString::new(target.as_os_str().as_bytes()).map_err(|e| err(e.to_string()))?;
        // RENAME_EXCL: one atomic directory publication, no overwrite.
        let rc = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), 0x00000004) };
        if rc != 0 {
            return Err(err(std::io::Error::last_os_error().to_string()));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        if target.exists() {
            return Err(err(format!("目标已存在: {}", target.display())));
        }
        fs::rename(stage, target).map_err(|e| err(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// verify_pair (motionlive/cli.py)
// ---------------------------------------------------------------------------

fn verify_pair(folder: &Path) -> Result<Value> {
    let text = fs::read_to_string(folder.join("manifest.json")).map_err(|e| err(e.to_string()))?;
    let manifest: Value = serde_json::from_str(&text).map_err(|e| err(e.to_string()))?;
    let malformed = || err("Missing or malformed conversion manifest");
    let obj = manifest.as_object().ok_or_else(malformed)?;
    let output_sha = obj.get("output_sha256").and_then(|v| v.as_object()).ok_or_else(malformed)?;
    let keys: HashSet<&str> = output_sha.keys().map(|k| k.as_str()).collect();
    if keys != HashSet::from(["photo.jpg", "photo.mov"]) {
        return Err(malformed());
    }
    if !obj.get("timing").map_or(false, |v| v.is_object()) || !obj.get("hdr").map_or(false, |v| v.is_object()) {
        return Err(malformed());
    }
    for (name, expected) in output_sha {
        let base = Path::new(name).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let expected_str = expected.as_str().unwrap_or("");
        if base != *name || digest(&folder.join(name))? != expected_str {
            return Err(err(format!("Output integrity check failed: {name}")));
        }
    }
    Ok(manifest)
}

// ---------------------------------------------------------------------------
// convert
// ---------------------------------------------------------------------------

struct ConvertArgs {
    missing_time: String,
    legacy_hdr: String,
    time_domain: String,
}

fn options_json(args: &ConvertArgs) -> Value {
    let mut m = Map::new();
    m.insert("missing_time".to_string(), json!(args.missing_time));
    m.insert("legacy_hdr".to_string(), json!(args.legacy_hdr));
    m.insert("time_domain".to_string(), json!(args.time_domain));
    Value::Object(m)
}

fn options_equal(manifest_options: &Value, expected: &Value) -> bool {
    let a = match manifest_options.as_object() {
        Some(o) => o,
        None => return false,
    };
    let b = match expected.as_object() {
        Some(o) => o,
        None => return false,
    };
    if a.len() != b.len() {
        return false;
    }
    b.iter().all(|(k, v)| a.get(k) == Some(v))
}

/// The legacy-HDR block of convert_one; on Err the caller records "unchanged"
/// plus a warning, exactly like the Python try/except.
fn legacy_convert(
    image: &mut Vec<u8>,
    hdr: &mut Value,
    binary: &Path,
    work: &Path,
) -> Result<()> {
    let (legacy, reason) = pymotion::legacy_apple(image)?;
    if let Some(lg) = legacy {
        let old_gain = work.join("old-gain.jpg");
        fs::write(&old_gain, &lg.gain).map_err(|e| err(e.to_string()))?;
        let gain_out = work.join("gain.jpg");
        let mut result = native(
            binary,
            &json!({
                "mode": "legacy-gain",
                "input": old_gain.to_string_lossy(),
                "gain_output": gain_out.to_string_lossy(),
                "stops": lg.stops,
            }),
        )?;
        let icc = result.as_object_mut().and_then(|o| o.remove("srgb_icc_base64"));
        for (k, v) in result.as_object().cloned().unwrap_or_default() {
            hdr[k] = v;
        }
        if result.get("status").and_then(|v| v.as_str()) == Some("converted") {
            let icc_b64 = icc.and_then(|v| v.as_str().map(|s| s.to_string()))
                .ok_or_else(|| err("Legacy HDR conversion: missing sRGB ICC"))?;
            let icc_bytes = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                icc_b64.as_bytes(),
            )
            .map_err(|e| err(format!("Legacy HDR conversion: {e}")))?;
            let tagged = pymotion::legacy_color_profile(image, &icc_bytes)?;
            let gain_bytes = fs::read(&gain_out).map_err(|e| err(e.to_string()))?;
            let stops = result.get("stops").and_then(|v| v.as_f64())
                .ok_or_else(|| err("Legacy HDR conversion: missing stops"))?;
            *image = pymotion::ultra_from_apple(&tagged, &gain_bytes, stops)?;
            hdr["output"] = json!("ultra-hdr");
        }
    } else {
        hdr["status"] = json!("unchanged");
        hdr["reason"] = json!(reason);
    }
    Ok(())
}

fn convert_one(source: &Path, target: &Path, binary: &Path, args: &ConvertArgs, impl_id: &str) -> Result<Value> {
    let source_hash = digest(source)?;
    let options = options_json(args);
    if target.exists() {
        let manifest = verify_pair(target)?;
        let same = manifest.get("source_sha256").and_then(|v| v.as_str()) == Some(source_hash.as_str())
            && manifest.get("implementation").and_then(|v| v.as_str()) == Some(impl_id)
            && options_equal(manifest.get("options").unwrap_or(&Value::Null), &options);
        if same {
            return Ok(json!({
                "status": "resumed",
                "source": source.to_string_lossy(),
                "output": target.to_string_lossy(),
            }));
        }
        return Err(err(format!(
            "输出已存在且与当前输入/设置不一致；请选择新的输出目录: {}",
            target.display()
        )));
    }
    let source_bytes = fs::read(source).map_err(|e| err(e.to_string()))?;
    let photo = pymotion::parse(&source_bytes)?;
    if photo.timestamp_us.is_none() && args.missing_time == "error" {
        return Err(err("原片没有有效封面时间戳；不自动使用视频中点"));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| err(e.to_string()))?;
    }
    let parent = target.parent().ok_or_else(|| err("输出路径没有父目录"))?.to_path_buf();
    let (stage, _stage_guard) = mkdtemp(&parent, ".motionlive-")?;
    let (work, _work_guard) = mkdtemp(&env::temp_dir(), "motionlive-work-")?;
    let mut image = photo.still.clone();
    let video_path = work.join("source.mp4");
    fs::write(&video_path, &photo.video).map_err(|e| err(e.to_string()))?;
    let mut hdr = json!({
        "input": photo.hdr,
        "status": "preserved",
        "reason": "Ultra HDR bytes retained",
    });
    let mut warnings = photo.warnings.clone();
    if photo.hdr != "ultra-hdr" {
        if args.legacy_hdr == "auto" {
            if let Err(e) = legacy_convert(&mut image, &mut hdr, binary, &work) {
                hdr["status"] = json!("unchanged");
                hdr["reason"] = json!("Legacy HDR conversion could not be completed");
                warnings.push(format!("Legacy HDR conversion: {e}"));
            }
        } else {
            hdr["status"] = json!("unchanged");
            hdr["reason"] = json!("Legacy HDR conversion disabled");
        }
    }
    let identifier = uuid_v4().to_uppercase();
    let still = pymotion::live_still(&image, &identifier)?;
    if pyjpeg::image_hash(&photo.still)? != pyjpeg::image_hash(&still)? {
        return Err(err("Static JPEG compressed coding data changed"));
    }
    fs::write(stage.join("photo.jpg"), &still).map_err(|e| err(e.to_string()))?;
    let timing = native(
        binary,
        &json!({
            "mode": "movie",
            "input": video_path.to_string_lossy(),
            "output": stage.join("photo.mov").to_string_lossy(),
            "still": stage.join("photo.jpg").to_string_lossy(),
            "identifier": identifier,
            "timestamp_us": photo.timestamp_us,
            "missing_time": args.missing_time,
            "time_domain": args.time_domain,
        }),
    )?;
    let meta = fs::metadata(source).map_err(|e| err(e.to_string()))?;
    let times = fs::FileTimes::new()
        .set_accessed(meta.accessed().map_err(|e| err(e.to_string()))?)
        .set_modified(meta.modified().map_err(|e| err(e.to_string()))?);
    for name in ["photo.jpg", "photo.mov"] {
        let f = fs::File::options().write(true).open(stage.join(name)).map_err(|e| err(e.to_string()))?;
        f.set_times(times).map_err(|e| err(e.to_string()))?;
    }
    let aux = pyjpeg::mpf_images(&still)?;
    let mut manifest = Map::new();
    manifest.insert("version".to_string(), json!(VERSION));
    manifest.insert("implementation".to_string(), json!(impl_id));
    manifest.insert("source".to_string(), json!(source.to_string_lossy()));
    manifest.insert("source_sha256".to_string(), json!(source_hash));
    manifest.insert("identifier".to_string(), json!(identifier));
    manifest.insert("options".to_string(), options);
    manifest.insert("source_timestamp_us".to_string(), json!(photo.timestamp_us));
    manifest.insert("source_timestamp_tag".to_string(), json!(photo.timestamp_tag));
    manifest.insert("timing".to_string(), timing.clone());
    manifest.insert("hdr".to_string(), hdr.clone());
    manifest.insert("warnings".to_string(), json!(warnings));
    manifest.insert("primary_coding_sha256".to_string(), json!(pyjpeg::image_hash(&still)?));
    manifest.insert(
        "auxiliary_sha256".to_string(),
        json!(aux.get(1..).unwrap_or(&[]).iter().map(|r| sha256_bytes(&still[r.clone()])).collect::<Vec<_>>()),
    );
    let mut out_sha = Map::new();
    out_sha.insert("photo.jpg".to_string(), json!(digest(&stage.join("photo.jpg"))?));
    out_sha.insert("photo.mov".to_string(), json!(digest(&stage.join("photo.mov"))?));
    manifest.insert("output_sha256".to_string(), Value::Object(out_sha));
    fs::write(
        stage.join("manifest.json"),
        serde_json::to_string_pretty(&Value::Object(manifest)).map_err(|e| err(e.to_string()))? + "\n",
    )
    .map_err(|e| err(e.to_string()))?;
    verify_pair(&stage)?;
    // Catch an input changed by another application during conversion.
    if digest(source)? != source_hash {
        return Err(err("Source changed during conversion; result not published"));
    }
    publish(&stage, target)?;
    Ok(json!({
        "status": "converted",
        "source": source.to_string_lossy(),
        "output": target.to_string_lossy(),
        "timing": timing,
        "hdr": hdr,
        "warnings": warnings,
    }))
}

// ---------------------------------------------------------------------------
// inputs / commands
// ---------------------------------------------------------------------------

fn is_jpeg_path(p: &Path) -> bool {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .map_or(false, |e| e == "jpg" || e == "jpeg")
}

fn inputs(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        if !is_jpeg_path(path) {
            return Err(err("目前输入支持 Google Motion Photo JPEG (.jpg/.jpeg)"));
        }
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        return Err(err(format!("输入不存在: {}", path.display())));
    }
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        let entries = fs::read_dir(dir).map_err(|e| err(e.to_string()))?;
        for entry in entries.flatten() {
            let p = entry.path();
            let md = fs::symlink_metadata(&p).map_err(|e| err(e.to_string()))?;
            if md.file_type().is_symlink() {
                continue;
            }
            if md.is_dir() {
                walk(&p, out)?;
            } else if md.is_file() && is_jpeg_path(&p) {
                out.push(p);
            }
        }
        Ok(())
    }
    walk(path, &mut out)?;
    // Python sorted() on Path objects compares component tuples.
    out.sort_by(|a, b| {
        let ka: Vec<String> = a.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let kb: Vec<String> = b.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        ka.cmp(&kb)
    });
    Ok(out)
}

fn cmd_scan(input: &Path) -> Result<()> {
    let files = inputs(input)?;
    let mut results = Vec::new();
    for file in &files {
        let row = match fs::read(file) {
            Ok(bytes) => match pymotion::parse(&bytes) {
                Ok(photo) => json!({
                    "source": file.to_string_lossy(),
                    "status": "motion-photo",
                    "hdr": photo.hdr,
                    "timestamp_us": photo.timestamp_us,
                    "timestamp_tag": photo.timestamp_tag,
                    "warnings": photo.warnings,
                }),
                Err(e) => json!({
                    "source": file.to_string_lossy(),
                    "status": "not-convertible",
                    "reason": e.to_string(),
                }),
            },
            Err(e) => json!({
                "source": file.to_string_lossy(),
                "status": "not-convertible",
                "reason": e.to_string(),
            }),
        };
        results.push(row);
    }
    println!("{}", serde_json::to_string_pretty(&Value::Array(results)).map_err(|e| err(e.to_string()))?);
    Ok(())
}

fn cmd_verify(folder: &Path) -> Result<()> {
    let manifest = verify_pair(folder)?;
    let out = json!({
        "status": "verified",
        "timing": manifest.get("timing").cloned().unwrap_or(Value::Null),
        "hdr": manifest.get("hdr").cloned().unwrap_or(Value::Null),
    });
    println!("{}", serde_json::to_string_pretty(&out).map_err(|e| err(e.to_string()))?);
    Ok(())
}

fn cmd_convert(input: &Path, output: &Path, args: &ConvertArgs) -> Result<i32> {
    let source = absolutize(input);
    let source = if source.exists() {
        fs::canonicalize(&source).map_err(|e| err(e.to_string()))?
    } else {
        source
    };
    let destination = absolutize(output);
    let input_root = if source.is_dir() {
        source.clone()
    } else {
        source.parent().map(|p| p.to_path_buf()).unwrap_or(source.clone())
    };
    if destination.starts_with(&input_root) {
        return Err(err("输出目录必须位于输入目录之外"));
    }
    fs::create_dir_all(&destination).map_err(|e| err(e.to_string()))?;
    let root = find_repo_root()?;
    let binary = native_binary(&root)?;
    let impl_id = implementation_id(&root)?;
    let files = inputs(&source)?;
    let mut results = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let relative = file.strip_prefix(&input_root).map_err(|e| err(e.to_string()))?.to_path_buf();
        let target = destination
            .join(relative.parent().unwrap_or(Path::new("")))
            .join(format!("{}.livephoto", relative.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
        let result = match convert_one(file, &target, &binary, args, &impl_id) {
            Ok(r) => r,
            Err(e) => {
                let status = if e.to_string() == NOT_MARKED { "skipped" } else { "failed" };
                json!({
                    "status": status,
                    "source": file.to_string_lossy(),
                    "error": e.to_string(),
                })
            }
        };
        eprintln!("[{}/{}] {} {}", index + 1, files.len(), result["status"].as_str().unwrap_or(""), relative.display());
        results.push(result);
    }
    let mut counts = Map::new();
    for status in ["converted", "resumed", "skipped", "failed"] {
        let n = results.iter().filter(|r| r.get("status").and_then(|v| v.as_str()) == Some(status)).count();
        counts.insert(status.to_string(), json!(n));
    }
    let failed = counts["failed"].as_u64().unwrap_or(0);
    let report = destination.join(format!("report-{}-{}.json", local_time_str(), uuid_hex6()));
    let report_doc = json!({ "counts": counts, "files": results });
    fs::write(
        &report,
        serde_json::to_string_pretty(&report_doc).map_err(|e| err(e.to_string()))? + "\n",
    )
    .map_err(|e| err(e.to_string()))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({ "counts": counts, "report": report.to_string_lossy() }))
            .map_err(|e| err(e.to_string()))?
    );
    Ok(if failed > 0 { 1 } else { 0 })
}

// ---------------------------------------------------------------------------
// Argument parsing (argparse-shaped, exit code 2 on usage errors)
// ---------------------------------------------------------------------------

fn arg_error(msg: &str) -> ExitCode {
    eprintln!("{}\nerror: {msg}", usage());
    ExitCode::from(2)
}

fn choice(value: &str, allowed: &[&str], flag: &str) -> std::result::Result<String, String> {
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(format!(
            "argument {flag}: invalid choice: '{value}' (choose from {})",
            allowed.iter().map(|a| format!("'{a}'")).collect::<Vec<_>>().join(", ")
        ))
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.first().map(|s| s.as_str()) == Some("--version") {
        println!("{VERSION}");
        return ExitCode::SUCCESS;
    }
    if argv.is_empty() {
        return arg_error("the following arguments are required: command");
    }
    let command = argv[0].as_str();
    let rest = &argv[1..];
    let run = || -> Result<i32> {
        match command {
            "scan" => {
                if rest.len() != 1 {
                    return Err(err("scan 需要一个输入路径"));
                }
                cmd_scan(&absolutize(Path::new(&rest[0])))?;
                Ok(0)
            }
            "verify" => {
                if rest.len() != 1 {
                    return Err(err("verify 需要一个文件夹路径"));
                }
                cmd_verify(&absolutize(Path::new(&rest[0])))?;
                Ok(0)
            }
            "convert" => {
                let mut input: Option<String> = None;
                let mut output: Option<String> = None;
                let mut missing_time = "error".to_string();
                let mut legacy_hdr = "auto".to_string();
                let mut time_domain = "auto".to_string();
                let mut i = 0;
                while i < rest.len() {
                    let arg = &rest[i];
                    let take = |i: &mut usize| -> std::result::Result<String, Error> {
                        *i += 1;
                        rest.get(*i).cloned().ok_or_else(|| err(format!("argument {arg}: expected one argument")))
                    };
                    match arg.as_str() {
                        "-o" | "--output" => output = Some(take(&mut i)?),
                        "--missing-time" => missing_time = take(&mut i)?,
                        "--legacy-hdr" => legacy_hdr = take(&mut i)?,
                        "--time-domain" => time_domain = take(&mut i)?,
                        _ if arg.starts_with('-') => return Err(err(format!("unrecognized arguments: {arg}"))),
                        _ => {
                            if input.is_some() {
                                return Err(err(format!("unrecognized arguments: {arg}")));
                            }
                            input = Some(arg.clone());
                        }
                    }
                    i += 1;
                }
                let input = input.ok_or_else(|| err("convert 需要输入路径"))?;
                let output = output.ok_or_else(|| err("convert 需要 -o/--output"))?;
                let check = |value: &str, allowed: &[&str], flag: &str| -> std::result::Result<String, Error> {
                    choice(value, allowed, flag).map_err(|msg| {
                        eprintln!("{}\nerror: {msg}", usage());
                        err(msg)
                    })
                };
                // Invalid choices exit 2 like argparse; print the usage first.
                let missing_time = match check(&missing_time, &["error", "midpoint"], "--missing-time") {
                    Ok(v) => v,
                    Err(_) => return Ok(2),
                };
                let legacy_hdr = match check(&legacy_hdr, &["auto", "preserve"], "--legacy-hdr") {
                    Ok(v) => v,
                    Err(_) => return Ok(2),
                };
                let time_domain = match check(&time_domain, &["auto", "media", "presentation"], "--time-domain") {
                    Ok(v) => v,
                    Err(_) => return Ok(2),
                };
                let args = ConvertArgs { missing_time, legacy_hdr, time_domain };
                cmd_convert(&absolutize(Path::new(&input)), &absolutize(Path::new(&output)), &args)
            }
            _ => Err(err(format!("invalid choice: '{command}' (choose from 'scan', 'convert', 'verify')"))),
        }
    };
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("错误: {e}");
            ExitCode::FAILURE
        }
    }
}
