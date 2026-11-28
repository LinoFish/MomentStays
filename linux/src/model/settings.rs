//! Conversion route, the per-run feature choices of the workflow, and the
//! core requests they produce. Keys and defaults follow
//! `momentstays_core::engine::options` (`ConversionOptions::from_request`)
//! and the macOS app's `ConversionSettings.options`.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// Android Motion Photos (and static HDR, bursts) → Apple Live Photos.
    #[default]
    ToApple,
    /// Apple Live Photos → Android Motion Photos for one vendor.
    ToAndroid,
    /// Verified Google Motion Photo JPEG → HONOR.
    AndroidToAndroid,
    /// Recognized HONOR Motion Photo JPEG → Google.
    HonorToGoogle,
}

impl Direction {
    /// The core's `direction` value.
    pub fn engine(self) -> &'static str {
        match self {
            Direction::ToApple => "to-apple",
            Direction::ToAndroid => "to-android",
            Direction::AndroidToAndroid => "android-to-android",
            Direction::HonorToGoogle => "honor-to-google",
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
    #[cfg(test)]
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

/// The direction cards of step 1 (macOS `MigrationTargetPreset`), in the
/// order the macOS app shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Apple,
    Android(Vendor),
    GoogleToHonor,
    HonorToGoogle,
}

impl Preset {
    pub const ALL: [Preset; 10] = [
        Preset::Apple,
        Preset::Android(Vendor::Google),
        Preset::Android(Vendor::Xiaomi),
        Preset::Android(Vendor::Samsung),
        Preset::Android(Vendor::Huawei),
        Preset::Android(Vendor::Honor),
        Preset::Android(Vendor::Oppo),
        Preset::Android(Vendor::Vivo),
        Preset::GoogleToHonor,
        Preset::HonorToGoogle,
    ];
    pub fn direction(self) -> Direction {
        match self {
            Preset::Apple => Direction::ToApple,
            Preset::Android(_) => Direction::ToAndroid,
            Preset::GoogleToHonor => Direction::AndroidToAndroid,
            Preset::HonorToGoogle => Direction::HonorToGoogle,
        }
    }
    pub fn vendor(self) -> Option<Vendor> {
        match self {
            Preset::Android(v) => Some(v),
            _ => None,
        }
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
    pub const ALL: [ExistingOutput; 3] = [
        ExistingOutput::Resume,
        ExistingOutput::Skip,
        ExistingOutput::Overwrite,
    ];
    pub fn engine(self) -> &'static str {
        match self {
            ExistingOutput::Resume => "resume",
            ExistingOutput::Skip => "skip",
            ExistingOutput::Overwrite => "overwrite",
        }
    }
}

/// Live Photo output content towards Apple (macOS `ExportMode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportMode {
    #[default]
    LivePhoto,
    WithAuxiliary,
    AuxiliaryOnly,
    WithCandidates,
}

impl ExportMode {
    pub const ALL: [ExportMode; 4] = [
        ExportMode::LivePhoto,
        ExportMode::WithAuxiliary,
        ExportMode::AuxiliaryOnly,
        ExportMode::WithCandidates,
    ];
    pub fn engine(self) -> &'static str {
        match self {
            ExportMode::LivePhoto => "livephoto",
            ExportMode::WithAuxiliary => "livephoto+auxiliary",
            ExportMode::AuxiliaryOnly => "auxiliary-only",
            ExportMode::WithCandidates => "livephoto+candidates",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VideoTrack {
    #[default]
    Primary,
    Secondary,
}

impl VideoTrack {
    pub const ALL: [VideoTrack; 2] = [VideoTrack::Primary, VideoTrack::Secondary];
    pub fn engine(self) -> &'static str {
        match self {
            VideoTrack::Primary => "primary",
            VideoTrack::Secondary => "secondary",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CandidateDelivery {
    #[default]
    StaticPhotos,
    LivePhotos,
}

impl CandidateDelivery {
    pub fn engine(self) -> &'static str {
        match self {
            CandidateDelivery::StaticPhotos => "static",
            CandidateDelivery::LivePhotos => "livephoto",
        }
    }
}

/// Export the photos of this run that are not converted, unchanged
/// (macOS `UnconvertedExportChoice`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unconverted {
    #[default]
    None,
    SameLocation,
    SeparateLocation,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportSettings {
    pub direction: Direction,
    pub vendor: Vendor,
    pub container: Container,
    pub preserve_jpeg: bool,
    pub image_quality: f64,
    pub gain_quality: f64,
    /// HDR feature of step 3 (core `legacy_hdr`). Off by default.
    pub legacy_hdr: bool,
    /// Write the manifest / report JSON next to the results.
    pub export_data_files: bool,
    /// Stamp output files with the EXIF capture time. Off by default.
    pub exif_file_times: bool,
    pub existing_output: ExistingOutput,
    /// Step 3 features. `true` until the user turns them off.
    pub live_conversion: bool,
    pub burst_conversion: bool,
    pub export_mode: ExportMode,
    pub track: VideoTrack,
    pub candidate_delivery: CandidateDelivery,
    /// "Photo Dates" master switch and its options.
    pub time_options_enabled: bool,
    pub photo_time_repair: bool,
    pub photo_time_synthetic: bool,
    /// IANA zone used for filename dates and synthetic dates.
    pub photo_time_zone: String,
    /// Frozen base for synthetic dates, set when date repair is turned on.
    pub photo_time_reference: f64,
    /// Normalized source path → chosen high-resolution cover candidate.
    pub cover_selections: BTreeMap<String, i64>,
    /// Burst group id → `{signature, representative, independent}`.
    pub burst_choices: BTreeMap<String, Value>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        ExportSettings {
            direction: Direction::ToApple,
            vendor: Vendor::Google,
            container: Container::Jpeg,
            preserve_jpeg: true,
            image_quality: 0.95,
            gain_quality: 0.95,
            legacy_hdr: false,
            export_data_files: true,
            exif_file_times: false,
            existing_output: ExistingOutput::Resume,
            live_conversion: true,
            burst_conversion: true,
            export_mode: ExportMode::LivePhoto,
            track: VideoTrack::Primary,
            candidate_delivery: CandidateDelivery::StaticPhotos,
            time_options_enabled: false,
            photo_time_repair: false,
            photo_time_synthetic: false,
            photo_time_zone: "UTC".into(),
            photo_time_reference: 0.0,
            cover_selections: BTreeMap::new(),
            burst_choices: BTreeMap::new(),
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

    pub fn preset(&self) -> Preset {
        match self.direction {
            Direction::ToApple => Preset::Apple,
            Direction::ToAndroid => Preset::Android(self.vendor),
            Direction::AndroidToAndroid => Preset::GoogleToHonor,
            Direction::HonorToGoogle => Preset::HonorToGoogle,
        }
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        self.direction = preset.direction();
        if let Some(vendor) = preset.vendor() {
            self.set_vendor(vendor);
        }
    }

    /// The video track the core gets: a chosen high-resolution cover or
    /// extra covers need the motion video (macOS `normalize`).
    pub fn effective_track(&self) -> VideoTrack {
        if self.export_mode == ExportMode::WithCandidates || !self.cover_selections.is_empty() {
            VideoTrack::Primary
        } else {
            self.track
        }
    }

    /// The "Photo Dates" switch: turning it on enables date repair where
    /// the route can write metadata times; turning it off clears its options.
    pub fn set_time_options(&mut self, enabled: bool, metadata_time_available: bool, now: f64) {
        self.time_options_enabled = enabled;
        self.set_photo_time_repair(enabled && metadata_time_available, now);
        if !enabled {
            self.exif_file_times = false;
            self.photo_time_synthetic = false;
        }
    }

    pub fn set_photo_time_repair(&mut self, on: bool, now: f64) {
        if on && (!self.photo_time_repair || self.photo_time_reference == 0.0) {
            self.photo_time_reference = now;
        }
        self.photo_time_repair = on;
    }

    /// `photo_time` options (macOS `photoTimeOptions`).
    pub fn photo_time(&self) -> Option<Value> {
        if !self.photo_time_repair
            || (self.direction == Direction::ToApple
                && self.export_mode == ExportMode::AuxiliaryOnly)
        {
            return None;
        }
        Some(json!({
            "policy": "phototime-1",
            "synthetic": self.photo_time_synthetic,
            "zone": self.photo_time_zone,
            "epoch": self.photo_time_reference,
            "file_times": self.exif_file_times,
        }))
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

/// `batch` request for the photos expected to run. `rows` are the scan
/// rows of exactly those photos; the core rejects the run if they differ.
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
        "legacy_hdr": settings.legacy_hdr,
        "export_data_files": settings.export_data_files,
        "exif_file_times": settings.exif_file_times,
        "existing_output": settings.existing_output.engine(),
        "include_report_data": true,
    });
    if let Some(time) = settings.photo_time() {
        req["photo_time"] = time;
    }
    match settings.direction {
        Direction::ToApple => {
            req["output_mode"] = json!(settings.export_mode.engine());
            req["video_track"] = json!(settings.effective_track().engine());
            req["missing_time"] = json!("error");
            req["time_domain"] = json!("auto");
            req["candidate_delivery"] = json!(settings.candidate_delivery.engine());
            if settings.export_mode != ExportMode::AuxiliaryOnly
                && !settings.cover_selections.is_empty()
            {
                req["cover_selections"] = Value::Object(
                    settings
                        .cover_selections
                        .iter()
                        .map(|(k, v)| (k.clone(), json!(v)))
                        .collect::<Map<_, _>>(),
                );
            }
            if !settings.burst_choices.is_empty() {
                req["burst_choices"] = Value::Object(
                    settings
                        .burst_choices
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<Map<_, _>>(),
                );
            }
        }
        Direction::ToAndroid => {
            req["vendor"] = json!(settings.vendor.engine());
            req["container"] = json!(match settings.effective_container() {
                Container::Jpeg => "jpeg",
                Container::Heic => "heic",
            });
            req["preserve_jpeg"] = json!(settings.preserve_jpeg);
            req["image_quality"] = json!(settings.image_quality.clamp(0.01, 1.0));
            req["gain_quality"] = json!(settings.gain_quality.clamp(0.01, 1.0));
        }
        Direction::AndroidToAndroid | Direction::HonorToGoogle => {}
    }
    req
}

/// `export-plan` request that freezes the complete resources of photos
/// exported unchanged (`kind: originals`).
pub fn originals_plan_request(direction: Direction, rows: &[Value]) -> Value {
    json!({
        "mode": "export-plan",
        "kind": "originals",
        "direction": direction.engine(),
        "items": rows.iter().map(|row| json!({
            "source": row["source"],
            "scan_row": row,
            "source_kind": "local",
        })).collect::<Vec<_>>(),
    })
}

/// `export` request that copies the frozen plans into `output`.
pub fn originals_export_request(plans: &Value, output: &Path, existing: ExistingOutput) -> Value {
    json!({
        "mode": "export",
        "kind": "originals",
        "plans": plans,
        "output": output.to_string_lossy(),
        "existing_output": existing.engine(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use momentstays_core::engine::options::ConversionOptions;

    #[test]
    fn defaults_keep_experimental_and_time_options_off() {
        let s = ExportSettings::default();
        assert!(!s.legacy_hdr);
        assert!(!s.exif_file_times);
        assert!(!s.photo_time_repair);
        assert!(s.photo_time().is_none());
        assert!(s.export_data_files);
        assert!(s.live_conversion && s.burst_conversion);
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
        assert!(!Direction::HonorToGoogle.supports_burst());
    }

    #[test]
    fn presets_round_trip() {
        let mut s = ExportSettings::default();
        for preset in Preset::ALL {
            s.apply_preset(preset);
            assert_eq!(s.preset(), preset);
        }
        assert_eq!(Preset::ALL[0], Preset::Apple);
    }

    #[test]
    fn batch_request_round_trips_through_core_options() {
        let settings = ExportSettings {
            direction: Direction::ToAndroid,
            container: Container::Heic,
            vendor: Vendor::Samsung,
            legacy_hdr: true,
            image_quality: 0.8,
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
        assert_eq!(o.android.image_quality, 0.8);
        assert!(o.legacy_hdr);
        assert!(!o.exif_file_times);
        assert_eq!(req["existing_output"], "resume");
        assert_eq!(req["inputs"][0], "/in/a.heic");
    }

    #[test]
    fn apple_request_carries_live_options_and_covers() {
        let mut s = ExportSettings {
            export_mode: ExportMode::WithAuxiliary,
            track: VideoTrack::Secondary,
            ..Default::default()
        };
        let req = batch_request(
            &s,
            &[],
            &[json!({"source": "/in/a.jpg"})],
            Path::new("/out"),
        );
        let o = ConversionOptions::from_request(&req);
        assert_eq!(o.output_mode, "livephoto+auxiliary");
        assert_eq!(o.video_track, "secondary");
        assert_eq!(req["scan_rows"].as_array().map(Vec::len), Some(1));
        s.cover_selections.insert("/in/a.jpg".into(), 2);
        let req = batch_request(&s, &[], &[], Path::new("/out"));
        let o = ConversionOptions::from_request(&req);
        assert_eq!(o.video_track, "primary");
        assert_eq!(o.cover_selections.get("/in/a.jpg"), Some(&2));
    }

    #[test]
    fn photo_time_follows_the_dates_switch() {
        let mut s = ExportSettings::default();
        s.set_time_options(true, true, 1000.0);
        assert!(s.photo_time_repair);
        let req = batch_request(&s, &[], &[], Path::new("/out"));
        let o = ConversionOptions::from_request(&req);
        let time = o.photo_time.expect("photo_time");
        assert_eq!(time["policy"], "phototime-1");
        assert_eq!(time["epoch"], 1000.0);
        s.exif_file_times = true;
        s.photo_time_synthetic = true;
        s.set_time_options(false, true, 2000.0);
        assert!(!s.photo_time_repair && !s.exif_file_times && !s.photo_time_synthetic);
        // Auxiliary-only Apple output writes no capture times.
        s.set_time_options(true, true, 3000.0);
        s.export_mode = ExportMode::AuxiliaryOnly;
        assert!(s.photo_time().is_none());
    }

    #[test]
    fn originals_requests() {
        let row = json!({"source": "/in/a.jpg", "relative": "a.jpg"});
        let plan = originals_plan_request(Direction::ToApple, std::slice::from_ref(&row));
        assert_eq!(plan["mode"], "export-plan");
        assert_eq!(plan["items"][0]["scan_row"], row);
        assert_eq!(plan["items"][0]["source_kind"], "local");
        let run = originals_export_request(&json!([]), Path::new("/out/o"), ExistingOutput::Skip);
        assert_eq!(run["kind"], "originals");
        assert_eq!(run["existing_output"], "skip");
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
