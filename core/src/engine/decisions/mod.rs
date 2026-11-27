//! Platform-independent decisions that the Apple UI used to compute in
//! Swift, shared with the GNOME app (request modes in `request.rs`):
//!
//! * [`hdr_kind`]: stable codes for scan-row HDR labels (`hdr_kind`).
//! * [`filename_time`]: capture time from an original filename.
//! * [`track_info`]: per-track sample facts from MP4/MOV bytes.
//! * [`workspace`]: the per-item conversion blocking reason.
//! * [`live_selection`]: the still paired with a selected Live Photo movie.
pub mod filename_time;
pub mod hdr_kind;
pub mod track_info;
pub mod workspace;

use crate::{fail, Result};
use std::path::{Path, PathBuf};

const STILLS: [&str; 4] = ["jpg", "jpeg", "heic", "heif"];

fn extension(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Swift `AndroidExport.selection`'s movie branch: the one sibling still
/// whose Apple content identifier equals the movie's.
pub fn live_selection(movie: &Path) -> Result<PathBuf> {
    let id = crate::engine::live::pairing::video_identifier(&crate::engine::read(movie)?)?;
    let dir = movie.parent().unwrap_or(Path::new("."));
    let mut matching = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| crate::engine::io_error(dir, e))?;
    for entry in entries.flatten() {
        let p = entry.path();
        if !STILLS.contains(&extension(&p).as_str()) || !p.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        if crate::engine::live::pairing::image_identifier(&bytes)
            .ok()
            .as_deref()
            == Some(&id)
        {
            matching.push(p);
        }
    }
    if matching.len() != 1 {
        return fail("所选视频缺少唯一匹配的静态照片");
    }
    Ok(matching.pop().unwrap())
}
