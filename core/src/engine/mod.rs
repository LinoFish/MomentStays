//! Batch conversion engine: the Rust replacement for the Swift engine in
//! `apple/shared` (`BatchEngine`, `MotionLive`, `AndroidExport`,
//! `AndroidTransfer`, `AndroidHDR`, `PhotoHDR`, `CoverCandidates`).
//!
//! Compatibility is the rule: request keys, option JSON, manifest and
//! report keys, `implementation` revisions, output names and user-facing
//! (Chinese) texts follow the Swift engine, so results written by the
//! Swift app are resumed, verified and delivered unchanged.
//!
//! `batch` owns queue/scheduling; `live` owns per-item conversion routes;
//! `output` owns publication/integrity; `hdr` owns static HDR processing.
//! `native` supplies shared codec types and the replaceable backend interface.
//!
//! Byte and container work is done here. Pixel and codec work (decoding a
//! gain map, re-encoding a JPEG, decoding a video frame, ImageIO checks)
//! goes through [`Native`], implemented by the host (ImageIO/AVFoundation
//! on Apple). [`NoNative`] refuses those steps with a clear message, while the standalone JSON driver uses `platform::SystemNative` with the
//! codecs available on its platform.

pub mod android;
pub mod batch;
pub mod burst;
pub mod cabi;
pub mod candidates;
pub mod contracts;
pub mod export;
pub mod hdr;
pub mod honor;
pub mod identity;
pub mod live;
pub mod manual;
pub mod movie;
pub mod native;
pub mod options;
pub mod output;
pub mod paths;
pub mod request;
pub mod scanrow;
pub mod static_hdr;
pub mod still;
pub mod transfer;

pub use native::{needs_native, Gray, HeicGain, Native, NoNative, Rgba};

use crate::{Error, ErrorKind, Result};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cooperative cancellation, like Swift `StopToken`.
#[derive(Default)]
pub struct StopToken(AtomicBool);
impl StopToken {
    pub fn new() -> Self {
        StopToken(AtomicBool::new(false))
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Swift `Failure(message)`: a plain error carrying the user-facing text.
pub(crate) fn failure<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::new(msg))
}

/// Streaming SHA-256 of a file (Swift `NativeBatch.hashFile`).
pub fn hash_file(path: &Path) -> Result<String> {
    use sha2::Digest;
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| io_error(path, e))?;
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| io_error(path, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(crate::hex(&h.finalize()))
}

pub(crate) fn io_error(path: &Path, e: std::io::Error) -> Error {
    Error::with_kind(ErrorKind::Io, format!("{}：{e}", path.display()))
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| io_error(path, e))
}

pub(crate) fn write(path: &Path, data: &[u8]) -> Result<()> {
    std::fs::write(path, data).map_err(|e| io_error(path, e))
}

/// Recursively sort object keys (Swift `JSONSerialization .sortedKeys`).
pub fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                out.insert(k.clone(), sorted(&m[k]));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// Write pretty, key-sorted JSON atomically (Swift `NativeBatch.json`).
pub fn write_json(value: &Value, path: &Path) -> Result<()> {
    let mut text = serde_json::to_vec_pretty(&sorted(value))
        .map_err(|e| Error::new(format!("JSON 编码失败：{e}")))?;
    text.push(b'\n');
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("json"),
        std::process::id()
    ));
    write(&tmp, &text)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_error(path, e)
    })
}

/// Numeric-tolerant JSON equality, like `NSDictionary.isEqual`: numbers
/// compare by value (1 == 1.0), objects ignore key order.
pub fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => x == y,
        },
        // NSNumber bridges Bool and 0/1 the same way.
        (Value::Bool(x), Value::Number(y)) | (Value::Number(y), Value::Bool(x)) => {
            y.as_f64() == Some(if *x { 1.0 } else { 0.0 })
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| same_json(v, w)))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(v, w)| same_json(v, w))
        }
        _ => a == b,
    }
}

/// A random uppercase UUID (Swift `UUID().uuidString`).
pub fn uuid() -> String {
    crate::live::new_content_identifier().unwrap_or_else(|_| {
        // new_content_identifier cannot fail; keep a deterministic fallback.
        "00000000-0000-4000-8000-000000000000".to_string()
    })
}

/// File modification time copy (Swift sets `.modificationDate` from the source).
pub(crate) fn copy_mtime(from: &Path, to: &Path) -> Result<()> {
    let modified = std::fs::metadata(from)
        .and_then(|m| m.modified())
        .map_err(|e| io_error(from, e))?;
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(to)
        .map_err(|e| io_error(to, e))?;
    f.set_modified(modified).map_err(|e| io_error(to, e))
}

/// Seconds since an arbitrary start, monotonic (Swift `systemUptime`).
pub(crate) fn uptime(start: std::time::Instant) -> f64 {
    start.elapsed().as_secs_f64()
}

/// Per-stage timings (Swift `BatchTimings`).
#[derive(Default)]
pub struct Timings(std::sync::Mutex<Map<String, Value>>);
impl Timings {
    pub fn measure<T>(&self, stage: &str, work: impl FnOnce() -> T) -> T {
        let start = std::time::Instant::now();
        let out = work();
        let spent = start.elapsed().as_secs_f64();
        let mut m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let prev = m.get(stage).and_then(Value::as_f64).unwrap_or(0.0);
        m.insert(stage.to_string(), Value::from(prev + spent));
        out
    }
    pub fn seconds(&self) -> Value {
        Value::Object(self.0.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }
}
