//! Conversion direction and export settings, and the core request they
//! produce. Keys and defaults follow `momentstays_core::engine::options`
//! (`ConversionOptions::from_request`).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// Android Motion Photos (and static HDR, bursts) → Apple Live Photos.
    #[default]
    ToApple,
    /// Apple Live Photos → Android Motion Photos for one vendor.
    ToAndroid,
}

impl Direction {
    /// The core's `direction` value.
    pub fn engine(self) -> &'static str {
        match self {
            Direction::ToApple => "to-apple",
            Direction::ToAndroid => "to-android",
        }
    }
    /// Burst groups are converted only towards Apple (core: "连拍目前仅支持 Apple 目标").
    pub fn supports_burst(self) -> bool {
        self == Direction::ToApple
    }
}

/// Android target vendors (Apple → Android), as the core writes them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vendor {
    #[default]
    Google,
    Xiaomi,
    Samsung,
    Huawei,
    Honor,
    Oppo,
    Vivo,
}

impl Vendor {
    pub const ALL: [Vendor; 7] = [
        Vendor::Google,
        Vendor::Xiaomi,
        Vendor::Samsung,
        Vendor::Huawei,
        Vendor::Honor,
        Vendor::Oppo,
        Vendor::Vivo,
    ];
    pub fn engine(self) -> &'static str {
        match self {
            Vendor::Google => "google",
            Vendor::Xiaomi => "xiaomi",
            Vendor::Samsung => "samsung",
            Vendor::Huawei => "huawei",
            Vendor::Honor => "honor",
            Vendor::Oppo => "oppo",
            Vendor::Vivo => "vivo",
        }
    }
    /// Only the Samsung writer has a HEIC target (core `heifwrite`).
    pub fn supports_heic(self) -> bool {
        self == Vendor::Samsung
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Container {
    #[default]
    Jpeg,
    Heic,
}

/// What to do when a result for a photo already exists in the output folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExistingOutput {
    /// Verify and keep finished results; redo incomplete ones (core default).
    #[default]
    Resume,
    Skip,
    Overwrite,
}

impl ExistingOutput {
    pub fn engine(self) -> &'static str {
        match self {
            ExistingOutput::Resume => "resume",
            ExistingOutput::Skip => "skip",
            ExistingOutput::Overwrite => "overwrite",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportSettings {
    pub direction: Direction,
    pub vendor: Vendor,
    pub container: Container,
    pub preserve_jpeg: bool,
    /// Experimental private HDR conversion (core `legacy_hdr`). Off by default.
    pub private_hdr: bool,
    /// Write the manifest / report JSON next to the results.
    pub export_data_files: bool,
    /// Stamp output files with the EXIF capture time. Off by default.
    pub exif_file_times: bool,
    pub existing_output: ExistingOutput,
}

impl Default for ExportSettings {
    fn default() -> Self {
        ExportSettings {
            direction: Direction::ToApple,
            vendor: Vendor::Google,
            container: Container::Jpeg,
            preserve_jpeg: true,
            private_hdr: false,
            export_data_files: true,
            exif_file_times: false,
            existing_output: ExistingOutput::Resume,
        }
    }
}

impl ExportSettings {
    /// The container actually written: HEIC falls back to JPEG for vendors
    /// without a HEIC writer.
    pub fn effective_container(&self) -> Container {
        if self.container == Container::Heic && self.vendor.supports_heic() {
            Container::Heic
        } else {
            Container::Jpeg
        }
    }

    pub fn set_vendor(&mut self, vendor: Vendor) {
        self.vendor = vendor;
        self.container = self.effective_container();
    }
}

fn paths_json(paths: &[PathBuf]) -> Value {
    Value::from(
        paths
            .iter()
            .map(|p| Value::from(p.to_string_lossy().into_owned()))
            .collect::<Vec<_>>(),
    )
}

/// `scan` request for the source folders (full rows, so they can be passed
/// back to `batch` unchanged).
pub fn scan_request(folders: &[PathBuf], direction: Direction) -> Value {
    json!({
        "mode": "scan",
        "inputs": paths_json(folders),
        "direction": direction.engine(),
        "full_rows": true,
    })
}

/// `batch` request for the included photos. `rows` are the scan rows of
/// exactly those photos; the core rejects the run if they differ.
pub fn batch_request(
    settings: &ExportSettings,
    sources: &[PathBuf],
    rows: &[Value],
    output: &Path,
) -> Value {
    let mut req = json!({
        "mode": "batch",
        "inputs": paths_json(sources),
        "output": output.to_string_lossy(),
        "scan_rows": rows,
        "direction": settings.direction.engine(),
        "legacy_hdr": settings.private_hdr,
        "export_data_files": settings.export_data_files,
        "exif_file_times": settings.exif_file_times,
        "existing_output": settings.existing_output.engine(),
    });
    match settings.direction {
        Direction::ToApple => {
            // This milestone exposes the plain Live Photo route only.
            req["output_mode"] = json!("livephoto");
            req["video_track"] = json!("primary");
            req["missing_time"] = json!("error");
        }
        Direction::ToAndroid => {
            req["vendor"] = json!(settings.vendor.engine());
            req["container"] = json!(match settings.effective_container() {
                Container::Jpeg => "jpeg",
                Container::Heic => "heic",
            });
            req["preserve_jpeg"] = json!(settings.preserve_jpeg);
        }
    }
    req
}

#[cfg(test)]
mod tests {
    use super::*;
    use momentstays_core::engine::options::ConversionOptions;

    #[test]
    fn defaults_keep_experimental_and_time_options_off() {
        let s = ExportSettings::default();
        assert!(!s.private_hdr);
        assert!(!s.exif_file_times);
        assert!(s.export_data_files);
        assert_eq!(s.direction, Direction::ToApple);
    }

    #[test]
    fn heic_only_for_samsung() {
        let mut s = ExportSettings {
            container: Container::Heic,
            ..Default::default()
        };
        s.set_vendor(Vendor::Samsung);
        assert_eq!(s.effective_container(), Container::Heic);
        s.set_vendor(Vendor::Google);
        assert_eq!(s.container, Container::Jpeg);
        assert!(Vendor::ALL
            .iter()
            .all(|v| v.supports_heic() == (*v == Vendor::Samsung)));
    }

    #[test]
    fn burst_only_towards_apple() {
        assert!(Direction::ToApple.supports_burst());
        assert!(!Direction::ToAndroid.supports_burst());
    }

    #[test]
    fn batch_request_round_trips_through_core_options() {
        let settings = ExportSettings {
            direction: Direction::ToAndroid,
            container: Container::Heic,
            vendor: Vendor::Samsung,
            private_hdr: true,
            ..Default::default()
        };
        let req = batch_request(
            &settings,
            &[PathBuf::from("/in/a.heic")],
            &[],
            Path::new("/out"),
        );
        let o = ConversionOptions::from_request(&req);
        assert_eq!(o.direction, "to-android");
        assert_eq!(o.android.vendor, "samsung");
        assert_eq!(o.android.container, "heic");
        assert!(o.legacy_hdr);
        assert!(!o.exif_file_times);
        assert_eq!(req["existing_output"], "resume");
        assert_eq!(req["inputs"][0], "/in/a.heic");
    }

    #[test]
    fn apple_request_is_plain_live_photo() {
        let req = batch_request(
            &ExportSettings::default(),
            &[PathBuf::from("/in/a.jpg")],
            &[json!({"source": "/in/a.jpg"})],
            Path::new("/out"),
        );
        let o = ConversionOptions::from_request(&req);
        assert_eq!(o.direction, "to-apple");
        assert_eq!(o.output_mode, "livephoto");
        assert!(!o.legacy_hdr);
        assert_eq!(req["scan_rows"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn scan_request_asks_for_full_rows() {
        let req = scan_request(
            &[PathBuf::from("/a"), PathBuf::from("/b")],
            Direction::ToAndroid,
        );
        assert_eq!(req["mode"], "scan");
        assert_eq!(req["direction"], "to-android");
        assert_eq!(req["full_rows"], true);
        assert_eq!(req["inputs"].as_array().map(Vec::len), Some(2));
    }
}
