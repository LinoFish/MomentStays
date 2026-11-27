//! Bridge to `momentstays-core`: the same JSON request layer the macOS app
//! and `momentstays-driver` use (`engine::request::handle`). Calls block;
//! the UI runs them on a worker thread (`gio::spawn_blocking`).

use crate::model::summary::ProgressSnapshot;
use momentstays_core::engine::batch::Progress;
use momentstays_core::engine::request::{handle, Events};
use momentstays_core::engine::StopToken;
use momentstays_core::platform::SystemNative;
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Progress shared between the worker thread and the UI, which polls it.
#[derive(Default)]
pub struct SharedProgress(Mutex<ProgressSnapshot>);

impl SharedProgress {
    pub fn snapshot(&self) -> ProgressSnapshot {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn set(&self, p: &Progress) {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        s.done = p.index;
        s.total = p.total;
        if !p.relative.is_empty() {
            s.current = p.relative.clone();
        }
    }
}

struct Host(Arc<SharedProgress>);
impl Events for Host {
    fn progress(&self, p: &Progress) {
        self.0.set(p);
    }
}

/// Run one core request (blocking). Errors carry the core's message.
pub fn run(
    req: &Value,
    progress: Arc<SharedProgress>,
    token: Arc<StopToken>,
) -> Result<Value, String> {
    handle(&SystemNative, &Host(progress), &token, req).map_err(|e| e.to_string())
}

/// Scan rows from a `scan` reply.
pub fn rows(reply: Value) -> Vec<Value> {
    match reply {
        Value::Object(mut m) => match m.remove("rows") {
            Some(Value::Array(rows)) => rows,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Host facts the macOS app adds to scan rows (`ConversionService`):
/// whether the still has a capture date (`capture_time_missing`), whether
/// its file name carries one (`filename_time`), and the HONOR private HDR
/// preflight. The core ignores these keys when the rows come back.
pub fn annotate(mut row: Value) -> Value {
    let Some(source) = row["source"].as_str().map(std::path::PathBuf::from) else {
        return row;
    };
    let photo = matches!(
        source
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg" | "heic" | "heif")
    );
    if photo {
        row["capture_time_missing"] =
            Value::from(momentstays_core::exif_time::capture_time_from_file(&source).is_none());
    }
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    row["filename_time"] =
        Value::from(momentstays_core::engine::decisions::filename_time::infer(&name).is_some());
    if row["hdr"] == crate::model::queue::HONOR_PRIVATE_HDR {
        let req = serde_json::json!({"mode": "honor-hdr-preflight", "source": source});
        match run(&req, Arc::default(), Arc::new(StopToken::new())) {
            Ok(v) => {
                row["private_hdr_supported"] = v["supported"].clone();
                row["private_hdr_reason"] = v["reason"].clone();
            }
            Err(_) => row["private_hdr_supported"] = Value::from(false),
        }
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::settings::{scan_request, Direction};
    use serde_json::json;

    #[test]
    fn scan_of_empty_folder_returns_no_rows() {
        let dir = std::env::temp_dir().join(format!("ms-gnome-scan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("notes.txt"), b"not a photo").expect("write");
        let reply = run(
            &scan_request(std::slice::from_ref(&dir), Direction::ToApple),
            Arc::default(),
            Arc::new(StopToken::new()),
        )
        .expect("scan");
        assert!(rows(reply).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_reports_unrecognized_jpeg_with_source() {
        let dir = std::env::temp_dir().join(format!("ms-gnome-row-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("a.jpg"), [0xFF, 0xD8, 0xFF, 0xD9]).expect("write");
        let reply = run(
            &scan_request(std::slice::from_ref(&dir), Direction::ToApple),
            Arc::default(),
            Arc::new(StopToken::new()),
        )
        .expect("scan");
        let rows = rows(reply);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["motion"], false);
        assert!(rows[0]["source"]
            .as_str()
            .is_some_and(|s| s.ends_with("a.jpg")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn annotate_adds_host_time_facts() {
        let dir = std::env::temp_dir().join(format!("ms-gnome-time-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("IMG_20240102_030405.jpg");
        std::fs::write(&file, [0xFF, 0xD8, 0xFF, 0xD9]).expect("write");
        let row = annotate(json!({"source": file.to_string_lossy(), "hdr": "SDR"}));
        assert_eq!(row["capture_time_missing"], true);
        assert_eq!(row["filename_time"], true);
        assert!(row.get("private_hdr_supported").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_request_is_an_error() {
        let err = run(&json!({}), Arc::default(), Arc::new(StopToken::new()));
        assert!(err.is_err());
        assert!(rows(json!({"rows": 3})).is_empty());
    }
}
