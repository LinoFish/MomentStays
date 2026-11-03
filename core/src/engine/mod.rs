//! Batch conversion engine: the Rust replacement for the Swift engine in
//! `apple/shared` (`BatchEngine`, `MotionLive`, `AndroidExport`,
//! `AndroidTransfer`, `AndroidHDR`, `PhotoHDR`, `CoverCandidates`).
//!
//! Compatibility is the rule: request keys, option JSON, manifest and
//! report keys, `implementation` revisions, output names and user-facing
//! (Chinese) texts follow the Swift engine, so results written by the
//! Swift app are resumed, verified and delivered unchanged.
//!
//! Byte and container work is done here. Pixel and codec work (decoding a
//! gain map, re-encoding a JPEG, decoding a video frame, ImageIO checks)
//! goes through [`Native`], implemented by the host (ImageIO/AVFoundation
//! on Apple). [`NoNative`] refuses those steps with a clear message, while the standalone JSON driver uses `platform::SystemNative` with the
//! codecs available on its platform.

pub mod android;
pub mod batch;
pub mod cabi;
pub mod candidates;
pub mod manual;
pub mod movie;
pub mod options;
pub mod paths;
pub mod request;
pub mod scanrow;
pub mod still;
pub mod transfer;

use crate::{Error, ErrorKind, Result};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Error for a step that needs the host's image or video codecs.
pub fn needs_native<T>(what: &str) -> Result<T> {
    Err(Error::with_kind(
        ErrorKind::Unsupported,
        format!("需要原生处理：{what}"),
    ))
}

/// An 8-bit single-channel image (gain maps).
#[derive(Clone, Debug, PartialEq)]
pub struct Gray {
    pub width: usize,
    pub height: usize,
    /// Bytes per row (>= width).
    pub stride: usize,
    pub pixels: Vec<u8>,
}

/// The HDR gain map ImageIO exposes for a HEIC (`kCGImageAuxiliaryDataTypeHDRGainMap`).
#[derive(Clone, Debug)]
pub struct HeicGain {
    /// `HDRGainMapVersion` values in the auxiliary metadata (Apple namespace).
    pub versions: Vec<String>,
    /// `HDRGainMapHeadroom` values.
    pub headrooms: Vec<f64>,
    /// None when the pixel format is not 8-bit one-component.
    pub image: Option<Gray>,
}

/// Host codecs and services. Every method has a refusing default, so a
/// host implements only what it supports.
pub trait Native: Sync {
    /// ImageIO-style full decode check of a still (complete, positive size).
    /// None = not available; callers then rely on the byte checks alone.
    fn image_decodes(&self, _data: &[u8]) -> Option<bool> {
        None
    }
    /// The sRGB ICC profile tagged onto a legacy-HDR still without one.
    fn srgb_icc(&self) -> Result<Vec<u8>> {
        needs_native("sRGB ICC 色彩描述")
    }
    /// Decode a JPEG gain map; Ok(None) when it is not 8-bit monochrome.
    fn decode_gray_jpeg(&self, _jpeg: &[u8]) -> Result<Option<Gray>> {
        needs_native("解码旧 Apple 增益图")
    }
    /// Encode an 8-bit gray image as JPEG at `quality` (0..=1).
    fn encode_gray_jpeg(&self, _image: &Gray, _quality: f64) -> Result<Vec<u8>> {
        needs_native("编码增益图 JPEG")
    }
    /// Decode the still at `path` and re-encode it as JPEG at `quality`,
    /// keeping its image properties (EXIF, orientation, colour).
    fn reencode_jpeg(&self, _path: &Path, _quality: f64) -> Result<Vec<u8>> {
        needs_native("重新编码静态图")
    }
    /// The HDR gain map of a HEIC still, if ImageIO exposes one.
    fn heic_gain_map(&self, _path: &Path) -> Result<Option<HeicGain>> {
        needs_native("读取 HEIC HDR 增益图")
    }
    /// Decode one frame of video track `track` (0-based among video tracks)
    /// of the movie at `video`, at media time `time_seconds`, with the track
    /// transform applied, and encode it as JPEG at quality 1.0.
    /// Returns (jpeg, width, height).
    fn frame_jpeg(
        &self,
        _video: &Path,
        _track: usize,
        _time_seconds: f64,
    ) -> Result<(Vec<u8>, u32, u32)> {
        needs_native("解码辅助轨高清画面")
    }
}

/// No host codecs: pixel steps are refused, byte steps still run.
pub struct NoNative;
impl Native for NoNative {}

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
