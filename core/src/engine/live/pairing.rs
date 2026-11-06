//! Apple still/movie pairing, source selection and exact cover-time resolution.
use crate::engine::paths::{self, extension};
use crate::engine::scanrow::ScanRow;
use crate::engine::{failure, read};
use crate::live::{self, TimeDomain};
use crate::model::Cover;
use crate::mp4::Movie;
use crate::{apple_hdr, live2motion, remux, Error, Result};
use std::path::{Path, PathBuf};

const VIDEO_EXTENSIONS: [&str; 2] = ["mov", "mp4"];

/// A Live Photo still and its movie, with the cover frame.
#[derive(Clone, Debug)]
pub struct Pair {
    pub still: PathBuf,
    pub movie: PathBuf,
    pub identifier: String,
    /// Cover frame's media time, microseconds (rounded).
    pub cover_us: i64,
    /// Cover frame's media time, seconds (exact ratio).
    pub cover_seconds: f64,
    /// Presentation-sorted frame index of the cover.
    pub frame: usize,
}

pub fn image_identifier(still: &[u8]) -> Result<String> {
    match live::still_content_identifier(still) {
        Ok(Some(id)) if !id.is_empty() => Ok(id),
        _ => failure("照片缺少 Apple 实况内容标识；请从「照片」导出完整原片与配对视频"),
    }
}

pub fn video_identifier(movie: &[u8]) -> Result<String> {
    match remux::read_content_identifier(movie) {
        Ok(Some(id)) => Ok(id),
        _ => failure("视频缺少唯一实况内容标识"),
    }
}

fn siblings(of: &Path, exts: &[&str]) -> Result<Vec<PathBuf>> {
    let dir = of.parent().unwrap_or(Path::new("."));
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| crate::engine::io_error(dir, e))? {
        let Ok(e) = e else { continue };
        let p = e.path();
        if exts.contains(&extension(&p).as_str()) && p.is_file() {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Swift `AndroidExport.pair`: the sibling movie with the same content
/// identifier, and the still-image-time marker matched to a video frame.
pub fn pair(still_path: &Path, domain: &str) -> Result<Pair> {
    let id = image_identifier(&read(still_path)?)?;
    let mut matching = Vec::new();
    for candidate in siblings(still_path, &VIDEO_EXTENSIONS)? {
        if let Ok(bytes) = std::fs::read(&candidate) {
            if video_identifier(&bytes).ok().as_deref() == Some(id.as_str()) {
                matching.push((candidate, bytes));
            }
        }
    }
    if matching.len() != 1 {
        return failure(if matching.is_empty() {
            "未找到内容标识匹配的视频；仅分享静态图不能恢复实况"
        } else {
            "多个视频使用同一内容标识，请将正确配对单独放入文件夹"
        });
    }
    let (movie_path, mov) = matching.remove(0);
    let parsed = Movie::parse(&mov)?;
    let marker = match live2motion::mett_cover_time_in(&mov, &parsed) {
        Ok(Some(t)) => t,
        _ => return failure("实况视频缺少唯一有效 still-image-time 封面标记"),
    };
    let video = parsed.video(0).map_err(|_| Error::new("实况缺少视频轨"))?;
    if video.samples.samples.is_empty() {
        return failure("视频没有有效画面");
    }
    let domain = match domain {
        "media" => TimeDomain::Media,
        "presentation" => TimeDomain::Presentation,
        _ => TimeDomain::Auto,
    };
    let cover =
        live::resolve_cover_in(&mov, &parsed, Cover::Time(marker), domain, 0).map_err(|_| {
            Error::new("封面时间越界或编辑时间线存在歧义；未使用中点猜测，请检查高级时间域选项")
        })?;
    let sorted = crate::cover::sorted_presentation(&video.samples);
    let pts = sorted[cover.frame].0;
    let ts = video.samples.timescale.max(1) as f64;
    Ok(Pair {
        still: still_path.to_path_buf(),
        movie: movie_path,
        identifier: id,
        cover_us: cover.frame_time_us,
        cover_seconds: pts as f64 / ts,
        frame: cover.frame,
    })
}

/// Swift `AndroidExport.selection`: a chosen movie stands for its still.
pub fn selection(sources: &[PathBuf]) -> Result<(PathBuf, Vec<PathBuf>)> {
    let mut images = Vec::new();
    for source in sources {
        if VIDEO_EXTENSIONS.contains(&extension(source).as_str()) {
            let id = video_identifier(&read(source)?)?;
            let matching: Vec<PathBuf> = siblings(source, &paths::PHOTO_EXTENSIONS)?
                .into_iter()
                .filter(|p| {
                    std::fs::read(p)
                        .ok()
                        .and_then(|b| image_identifier(&b).ok())
                        .as_deref()
                        == Some(id.as_str())
                })
                .collect();
            if matching.len() != 1 {
                return failure("所选视频缺少唯一匹配的静态照片");
            }
            images.extend(matching);
        } else {
            images.extend(paths::files(source)?);
        }
    }
    if images.is_empty() {
        return failure("未找到完整实况照片");
    }
    paths::selection(&images)
}

/// Swift `AndroidExport.scanFile`.
pub fn scan_file(path: &Path, relative: &str) -> ScanRow {
    scan_file_with_native(&crate::platform::SystemNative, path, relative)
}

pub fn scan_file_with_native(
    native: &dyn crate::engine::Native,
    path: &Path,
    relative: &str,
) -> ScanRow {
    let result = (|| -> Result<(Pair, bool)> {
        let p = pair(path, "auto")?;
        let has_map = apple_hdr::read(&read(path)?)
            .map(|r| r.maker.has_33)
            .unwrap_or(false);
        Ok((p, has_map))
    })();
    match result {
        Ok((p, has_map)) => ScanRow {
            source: path.to_path_buf(),
            relative: relative.to_string(),
            is_motion: true,
            hdr: if has_map { "Apple HDR" } else { "原始图像" }.into(),
            cover: format!("{:.6} 秒", p.cover_seconds),
            detail: "实况内容标识匹配；封面来自视频 timed metadata".into(),
            manufacturer: "苹果".into(),
            manufacturer_detail: "配对依据：Apple ContentIdentifier".into(),
            ..ScanRow::default()
        },
        Err(e) => {
            let row = crate::engine::scanrow::scan_file(native, path, relative, None);
            if row.is_static_hdr() || row.burst.is_some() {
                return row;
            }
            ScanRow {
                source: path.to_path_buf(),
                relative: relative.to_string(),
                is_motion: false,
                hdr: "—".into(),
                cover: "—".into(),
                detail: e.0,
                manufacturer: "苹果 / 待检查".into(),
                ..ScanRow::default()
            }
        }
    }
}
