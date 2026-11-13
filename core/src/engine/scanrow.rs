//! Scan rows (Swift `ScanRow`, `SourceVersion`, `NativeBatch.scanFile`).

use crate::engine::Native;
use crate::{format, scan};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub use crate::engine::identity::SourceVersion;

#[derive(Clone, Debug)]
pub struct ScanRow {
    pub source: PathBuf,
    pub relative: String,
    pub is_motion: bool,
    pub hdr: String,
    pub cover: String,
    pub detail: String,
    pub manufacturer: String,
    pub manufacturer_detail: String,
    pub confirmed_static: bool,
    pub version: Option<SourceVersion>,
    pub burst: Option<Value>,
}

impl Default for ScanRow {
    fn default() -> Self {
        ScanRow {
            source: PathBuf::new(),
            relative: String::new(),
            is_motion: false,
            hdr: String::new(),
            cover: String::new(),
            detail: String::new(),
            manufacturer: "未知".into(),
            manufacturer_detail: String::new(),
            confirmed_static: false,
            version: None,
            burst: None,
        }
    }
}

impl ScanRow {
    pub fn is_static_hdr(&self) -> bool {
        self.confirmed_static
            && matches!(
                self.hdr.as_str(),
                "Ultra HDR" | "ISO HDR" | "旧 Apple HDR" | "荣耀私有 HDR" | "HEIC HDR"
            )
    }
    pub fn needs_inspection(&self) -> bool {
        !self.is_motion && !self.confirmed_static && self.burst.is_none()
    }
    /// The NativeDriver `scan` row shape.
    pub fn json(&self) -> Value {
        json!({
            "relative": self.relative,
            "motion": self.is_motion,
            "hdr": self.hdr,
            "cover": self.cover,
            "manufacturer": self.manufacturer,
            "manufacturer_detail": self.manufacturer_detail,
            "detail": self.detail,
            "static": self.confirmed_static,
            "burst": self.burst,
        })
    }
    pub(crate) fn changed(source: &Path, relative: &str) -> Self {
        ScanRow {
            source: source.to_path_buf(),
            relative: relative.to_string(),
            hdr: "—".into(),
            cover: "—".into(),
            detail: "扫描期间来源发生变化，请重新扫描".into(),
            ..ScanRow::default()
        }
    }
}

/// Swift `NativeBatch.scanFile` (Android Motion Photo -> Apple direction).
pub fn scan_file(
    native: &dyn Native,
    file: &Path,
    relative: &str,
    bytes: Option<&[u8]>,
) -> ScanRow {
    let before = SourceVersion::read(file);
    if before.is_some_and(|v| v.size > super::burst::MAX_IMAGE)
        && super::burst::filename_candidate(&super::paths::file_name(file))
    {
        let mut burst = super::burst::inspect(file, &[]);
        if let Some(frame) = burst.as_mut() {
            frame["reason"] = json!("连拍单图超过 256 MiB 安全上限");
        }
        return ScanRow {
            source: file.to_path_buf(),
            relative: relative.into(),
            version: before,
            burst,
            detail: "连拍单图超过安全上限，未读取或转换".into(),
            ..ScanRow::default()
        };
    }
    let owned;
    let data: Result<&[u8], String> = match bytes {
        Some(b) => Ok(b),
        None => match std::fs::read(file) {
            Ok(b) => {
                owned = b;
                Ok(&owned)
            }
            Err(e) => Err(e.to_string()),
        },
    };
    let mut row = match data {
        Ok(data) => {
            let mut r = scan::scan_row(data);
            if !r.is_motion {
                if let Ok(photo) = crate::engine::manual::parse(data) {
                    if photo.format == crate::engine::manual::FORMAT
                        && native.image_decodes(&photo.still) != Some(false)
                    {
                        r.is_motion = true;
                        r.confirmed_static = false;
                        r.cover = crate::engine::manual::COVER.into();
                        r.hdr = "SDR".into();
                        r.manufacturer = "华为".into();
                        r.detail = "旧式华为动态照片；原封面时间未确定，请人工选择主视频帧；保留原静态照片及原始附属数据".into();
                    }
                }
            }
            let mut row = ScanRow {
                source: file.to_path_buf(),
                relative: relative.to_string(),
                is_motion: r.is_motion,
                hdr: r.hdr,
                cover: r.cover,
                detail: r.detail,
                manufacturer: r.manufacturer,
                manufacturer_detail: r.manufacturer_detail,
                confirmed_static: r.confirmed_static,
                version: None,
                burst: None,
            };
            if !row.is_motion {
                match crate::engine::static_hdr::inspect(native, data) {
                    Ok(Some(kind)) => {
                        row.confirmed_static = true;
                        row.hdr = kind.into();
                        row.cover = "不适用".into();
                        row.detail = "静态 HDR，可处理并导出静态图片".into();
                    }
                    Err(e) => {
                        row.confirmed_static = false;
                        row.detail = e.0;
                    }
                    Ok(None) => {}
                }
            }
            row.burst = super::burst::inspect(file, data);
            if let Some(burst) = &row.burst {
                row.detail = if burst["supported"] == true {
                    match burst["source_format"].as_str() {
                        Some("google-burst") => {
                            "Google JPEG 连拍成员（实验支持）；按原帧序保留，需检查整组与代表帧"
                                .into()
                        }
                        Some("huawei-burst") => {
                            "华为 JPEG 连拍成员（已知荣耀同格式）；未承诺新版本兼容".into()
                        }
                        Some("samsung-burst") => {
                            "三星 JPEG 连拍成员（实验支持）；请明确选择代表帧".into()
                        }
                        _ => "荣耀 JPEG 连拍成员；按原帧序保留，需检查整组与代表帧".into(),
                    }
                } else {
                    burst["reason"]
                        .as_str()
                        .unwrap_or("不支持的连拍组合")
                        .into()
                };
            }
            // The pixel half of Swift `confirmedStatic` (ImageIO decode).
            if row.confirmed_static && native.image_decodes(data) == Some(false) {
                row.confirmed_static = false;
                row.detail = format::NOT_MOTION.to_string();
                if let Some(burst) = row.burst.as_mut() {
                    burst["supported"] = json!(false);
                    burst["reason"] = json!("JPEG 原图无法解码");
                }
            }
            row
        }
        Err(message) => {
            let maker = scan::identify(&[], "");
            ScanRow {
                source: file.to_path_buf(),
                relative: relative.to_string(),
                hdr: "—".into(),
                cover: "—".into(),
                detail: message,
                manufacturer: maker.name,
                manufacturer_detail: maker.detail,
                ..ScanRow::default()
            }
        }
    };
    match before {
        Some(v) if Some(v) == SourceVersion::read(file) => {
            row.version = Some(v);
            row
        }
        _ => ScanRow::changed(file, relative),
    }
}
