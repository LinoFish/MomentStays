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
pub mod static_time;
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
/// Set an output file's modification time, and on Apple platforms its
/// creation time, from the source photo's EXIF capture time when
/// `use_exif` is set (Advanced Settings, default off); otherwise copy the
/// source file's modification time, as before. Failures are silent: call
/// sites ignore the result (`let _ =`), so a missing EXIF time or an
/// unwritable timestamp never fails a conversion.
pub(crate) fn stamp_output_time(source: &Path, to: &Path, use_exif: bool) -> Result<()> {
    let time = if use_exif {
        crate::exif_time::capture_time_from_file(source)
            .or_else(|| std::fs::metadata(source).and_then(|m| m.modified()).ok())
    } else {
        std::fs::metadata(source).and_then(|m| m.modified()).ok()
    };
    let Some(time) = time else {
        return Ok(());
    };
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(to)
        .map_err(|e| io_error(to, e))?;
    f.set_modified(time).map_err(|e| io_error(to, e))?;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let _ = set_birthtime(to, time);
    }
    Ok(())
}

/// Apple platforms only: `setattrlist` + `ATTR_CMN_CRTIME` sets the birth
/// time alone (documented since OS X 10.5; no public std API exists, and
/// Linux exposes no writable birth time at all). Creation and modification
/// times are set to the same value in one call, so the kernel rule that
/// clamps creation time to modification time cannot bite. Best effort:
/// future times (more than 24h ahead) are rejected by the kernel and
/// volumes without `setattrlist` support fail here; the modification time
/// above already landed, so the caller ignores this result.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn set_birthtime(path: &Path, time: std::time::SystemTime) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let duration = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::new("EXIF 拍摄时间早于 Unix 纪元"))?;
    let stamp = libc::timespec {
        tv_sec: duration.as_secs() as libc::time_t,
        tv_nsec: duration.subsec_nanos() as libc::c_long,
    };
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_CRTIME | libc::ATTR_CMN_MODTIME,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    // Buffer order follows attribute bit order: creation time, then
    // modification time.
    let buf = [stamp, stamp];
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Error::new("输出路径包含 NUL"))?;
    let rc = unsafe {
        libc::setattrlist(
            cpath.as_ptr(),
            &mut list as *mut libc::attrlist as *mut libc::c_void,
            buf.as_ptr() as *mut libc::c_void,
            std::mem::size_of_val(&buf),
            0,
        )
    };
    if rc != 0 {
        return Err(io_error(path, std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod stamp_tests {
    use super::*;
    use crate::exif_time::capture_time_from_bytes;

    fn test_jpeg() -> Vec<u8> {
        // Minimal JPEG carrying DateTimeOriginal "2024:05:06 07:08:09",
        // built with the same TIFF helpers as the exif_time tests.
        let tiff = {
            let root: &[(usize, usize, &[u8])] = &[];
            let exif: &[(usize, usize, &[u8])] = &[(0x9003, 2, b"2024:05:06 07:08:09\0")];
            let ifd0_n = root.len() + 1;
            let ifd0_size = 2 + ifd0_n * 12 + 4;
            let sub_off = 8 + ifd0_size;
            let sub_size = 2 + exif.len() * 12 + 4;
            let mut blob_at = sub_off + sub_size;
            let mut out = Vec::new();
            out.extend_from_slice(b"II");
            out.extend_from_slice(&42u16.to_le_bytes());
            out.extend_from_slice(&8u32.to_le_bytes());
            let root_entries = vec![crate::tiff::entry(true, 0x8769, 4, 1, sub_off)];
            let mut sub_entries: Vec<[u8; 12]> = Vec::new();
            let mut blobs: Vec<Vec<u8>> = Vec::new();
            for (tag, ty, value) in exif {
                let mut e = crate::tiff::entry(true, *tag, *ty, value.len(), 0);
                if value.len() <= 4 {
                    e[8..8 + value.len()].copy_from_slice(value);
                } else {
                    e[8..12].copy_from_slice(&(blob_at as u32).to_le_bytes());
                    blob_at += value.len();
                    blobs.push(value.to_vec());
                }
                sub_entries.push(e);
            }
            out.extend_from_slice(&crate::tiff::serialize(true, &root_entries, 0));
            out.extend_from_slice(&crate::tiff::serialize(true, &sub_entries, 0));
            for blob in blobs {
                out.extend_from_slice(&blob);
            }
            out
        };
        let mut payload = crate::jpeg::EXIF.to_vec();
        payload.extend_from_slice(&tiff);
        let len = u16::try_from(payload.len() + 2).expect("small test payload");
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&payload);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    #[test]
    fn stamp_prefers_exif_time_when_enabled() {
        let dir =
            std::env::temp_dir().join(format!("momentstays-stamp-exif-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let source = dir.join("source.jpg");
        let dest = dir.join("dest.jpg");
        std::fs::write(&source, test_jpeg()).expect("write source");
        std::fs::write(&dest, b"output").expect("write dest");
        let expected = capture_time_from_bytes(&test_jpeg()).expect("exif time");
        stamp_output_time(&source, &dest, true).expect("stamp");
        let modified = std::fs::metadata(&dest)
            .expect("dest metadata")
            .modified()
            .expect("dest mtime");
        assert_eq!(modified, expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamp_copies_source_mtime_when_disabled() {
        let dir =
            std::env::temp_dir().join(format!("momentstays-stamp-mtime-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let source = dir.join("source.jpg");
        let dest = dir.join("dest.jpg");
        std::fs::write(&source, test_jpeg()).expect("write source");
        std::fs::write(&dest, b"output").expect("write dest");
        let source_mtime = std::fs::metadata(&source)
            .expect("source metadata")
            .modified()
            .expect("source mtime");
        stamp_output_time(&source, &dest, false).expect("stamp");
        let modified = std::fs::metadata(&dest)
            .expect("dest metadata")
            .modified()
            .expect("dest mtime");
        assert_eq!(modified, source_mtime);
        let _ = std::fs::remove_dir_all(&dir);
    }
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
