//! The conversion settings model of the Apple apps (port of the rules in
//! `apple/macos/Sources/ConversionSettings.swift`): the defaults of a new
//! settings value, the fixed facts of each choice (which routes export to
//! Android, which vendor writes HEIC, which destinations import into Photos
//! or share, which direction and vendor each target preset stands for),
//! `normalize()` (the constraints between options), the private-HDR
//! experiment's availability, the engine options a settings value produces,
//! the destinations a route offers and the kind of its primary action.
//!
//! Values travel as the Swift raw values (`to-apple`, `samsung`,
//! `livephoto+candidates`, ...). Doubles (qualities, the photo-time epoch)
//! travel as decimal strings in both directions (Swift `description`, Rust
//! `Debug`/`parse`, both exact), because `JSONSerialization` rounds them. Request modes: `conversion-settings-tables`
//! (defaults and choice facts, static) and `conversion-settings` (one
//! settings value).
use serde_json::{json, Map, Value};

pub const DIRECTIONS: [&str; 4] = [
    "to-apple",
    "to-android",
    "android-to-android",
    "honor-to-google",
];
pub const VENDORS: [&str; 7] = [
    "google", "xiaomi", "samsung", "huawei", "honor", "oppo", "vivo",
];
pub const DESTINATIONS: [&str; 6] = [
    "ask",
    "folder",
    "photos",
    "folderAndPhotos",
    "share",
    "folderAndShare",
];
/// Swift `MigrationTargetPreset` raw values, in the order the app shows them.
pub const PRESETS: [&str; 10] = [
    "apple",
    "google",
    "xiaomi",
    "samsung",
    "huawei",
    "honor",
    "oppo",
    "vivo",
    "googleToHonor",
    "honorToGoogle",
];
pub const DEFAULT_ALBUM: &str = "MomentStays";
const DEFAULT_QUALITY: f64 = 0.95;

/// Every route but `to-apple` writes Android files.
pub fn exports_android(direction: &str) -> bool {
    direction != "to-apple"
}

/// Only the Samsung writer has a HEIC target.
pub fn supports_heic(vendor: &str) -> bool {
    vendor == "samsung"
}

/// Results go through a cache folder (no folder of the user's own).
pub fn uses_cache(destination: &str) -> bool {
    destination == "photos" || destination == "share"
}

pub fn imports_photos(destination: &str) -> bool {
    destination == "photos" || destination == "folderAndPhotos"
}

pub fn shares(destination: &str) -> bool {
    destination == "share" || destination == "folderAndShare"
}

/// The engine direction a target preset stands for.
pub fn preset_direction(preset: &str) -> &'static str {
    match preset {
        "apple" => "to-apple",
        "googleToHonor" => "android-to-android",
        "honorToGoogle" => "honor-to-google",
        _ => "to-android",
    }
}

/// The Android vendor a target preset stands for (Google for the others).
pub fn preset_vendor(preset: &str) -> &'static str {
    match preset {
        "xiaomi" => "xiaomi",
        "samsung" => "samsung",
        "huawei" => "huawei",
        "honor" => "honor",
        "oppo" => "oppo",
        "vivo" => "vivo",
        _ => "google",
    }
}

/// Swift `MigrationTargetPreset.current(settings)`.
pub fn current_preset(direction: &str, vendor: &str) -> &'static str {
    match direction {
        "to-apple" => "apple",
        "android-to-android" => "googleToHonor",
        "honor-to-google" => "honorToGoogle",
        _ => PRESETS
            .iter()
            .copied()
            .find(|p| *p == vendor)
            .unwrap_or("google"),
    }
}

/// A settings value as the Swift struct stores it (scalars only; the
/// per-source maps stay with the caller, which learns from
/// [`Options::carry`] which of them reach the engine).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub direction: String,
    pub destination: String,
    pub album_title: String,
    pub vendor: String,
    pub container: String,
    pub image_quality: f64,
    pub gain_quality: f64,
    pub preserve_jpeg: bool,
    pub legacy_hdr: bool,
    pub export_data_files: bool,
    pub track: String,
    pub export_mode: String,
    pub missing_time: String,
    pub time_domain: String,
    pub repair_auxiliary_duration: bool,
    pub exif_file_times: Option<bool>,
    pub photo_time_repair: Option<bool>,
    pub photo_time_synthetic: Option<bool>,
    pub photo_time_zone: Option<String>,
    pub photo_time_reference: Option<f64>,
    pub candidate_delivery: Option<String>,
    /// `coverSelections` is non-empty.
    pub has_cover_selections: bool,
}

impl Default for Settings {
    /// Swift `ConversionSettings()`.
    fn default() -> Self {
        Settings {
            direction: "to-apple".into(),
            destination: "ask".into(),
            album_title: DEFAULT_ALBUM.into(),
            vendor: "google".into(),
            container: "jpeg".into(),
            image_quality: DEFAULT_QUALITY,
            gain_quality: DEFAULT_QUALITY,
            preserve_jpeg: true,
            legacy_hdr: false,
            export_data_files: false,
            track: "primary".into(),
            export_mode: "livephoto".into(),
            missing_time: "error".into(),
            time_domain: "auto".into(),
            repair_auxiliary_duration: false,
            exif_file_times: None,
            photo_time_repair: None,
            photo_time_synthetic: None,
            photo_time_zone: None,
            photo_time_reference: None,
            candidate_delivery: None,
            has_cover_selections: false,
        }
    }
}

fn text(v: &Value, key: &str, default: &str) -> String {
    v[key].as_str().unwrap_or(default).to_string()
}

/// A double sent as a decimal string (or, from other callers, a number).
fn double(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => s.parse().ok(),
        _ => v.as_f64(),
    }
}

/// A double as a decimal string that Swift `Double(_:)` reads back exactly.
pub fn double_text(value: f64) -> String {
    if value.is_nan() {
        "nan".into()
    } else if value.is_infinite() {
        if value > 0.0 { "inf" } else { "-inf" }.into()
    } else {
        format!("{value:?}")
    }
}

impl Settings {
    pub fn from_json(v: &Value) -> Settings {
        let d = Settings::default();
        Settings {
            direction: text(v, "direction", &d.direction),
            destination: text(v, "destination", &d.destination),
            album_title: text(v, "album_title", &d.album_title),
            vendor: text(v, "vendor", &d.vendor),
            container: text(v, "container", &d.container),
            image_quality: double(&v["image_quality"]).unwrap_or(d.image_quality),
            gain_quality: double(&v["gain_quality"]).unwrap_or(d.gain_quality),
            preserve_jpeg: v["preserve_jpeg"].as_bool().unwrap_or(d.preserve_jpeg),
            legacy_hdr: v["legacy_hdr"].as_bool().unwrap_or(d.legacy_hdr),
            export_data_files: v["export_data_files"]
                .as_bool()
                .unwrap_or(d.export_data_files),
            track: text(v, "track", &d.track),
            export_mode: text(v, "export_mode", &d.export_mode),
            missing_time: text(v, "missing_time", &d.missing_time),
            time_domain: text(v, "time_domain", &d.time_domain),
            repair_auxiliary_duration: v["repair_auxiliary_duration"].as_bool().unwrap_or(false),
            exif_file_times: v["exif_file_times"].as_bool(),
            photo_time_repair: v["photo_time_repair"].as_bool(),
            photo_time_synthetic: v["photo_time_synthetic"].as_bool(),
            photo_time_zone: v["photo_time_zone"].as_str().map(str::to_string),
            photo_time_reference: double(&v["photo_time_reference"]),
            candidate_delivery: v["candidate_delivery"].as_str().map(str::to_string),
            has_cover_selections: v["has_cover_selections"].as_bool().unwrap_or(false),
        }
    }

    fn auxiliary_only_to_apple(&self) -> bool {
        self.direction == "to-apple" && self.export_mode == "auxiliary-only"
    }

    /// Swift `privateHDRExperimentAvailable`: the stored preference is kept
    /// even where this output cannot use it.
    pub fn private_hdr_available(&self) -> bool {
        self.direction != "android-to-android" && !self.auxiliary_only_to_apple()
    }

    pub fn private_hdr_enabled(&self) -> bool {
        self.legacy_hdr && self.private_hdr_available()
    }

    /// Swift `normalize()`: the constraints between options.
    pub fn normalize(&mut self) {
        let android = exports_android(&self.direction);
        if android && self.destination == "ask" {
            self.destination = "folder".into();
        }
        if android && imports_photos(&self.destination) {
            self.destination = "folder".into();
        }
        if self.direction == "to-apple" && shares(&self.destination) {
            self.destination = "folder".into();
        }
        if self.auxiliary_only_to_apple() && imports_photos(&self.destination) {
            self.destination = "folder".into();
        }
        if self.export_mode == "livephoto+candidates" || self.has_cover_selections {
            self.track = "primary".into();
        }
        if self.album_title_blank() {
            self.album_title = DEFAULT_ALBUM.into();
        }
        if !supports_heic(&self.vendor) {
            self.container = "jpeg".into();
        }
        self.image_quality = clamp_quality(self.image_quality);
        self.gain_quality = clamp_quality(self.gain_quality);
    }

    /// Swift `albumTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty`.
    pub fn album_title_blank(&self) -> bool {
        self.album_title.chars().all(swift_whitespace_or_newline)
    }

    /// The destinations this route offers, in menu order.
    pub fn destinations(&self) -> &'static [&'static str] {
        if exports_android(&self.direction) {
            &["folder", "share", "folderAndShare"]
        } else if self.export_mode == "auxiliary-only" {
            &["ask", "folder"]
        } else {
            &["ask", "photos", "folder", "folderAndPhotos"]
        }
    }

    /// The kind of the primary action (Swift `actionTitle`).
    pub fn action(&self) -> &'static str {
        if imports_photos(&self.destination) {
            "import-photos"
        } else if shares(&self.destination) {
            "share"
        } else if exports_android(&self.direction) {
            "export-android"
        } else if self.export_mode == "auxiliary-only" {
            "export-auxiliary"
        } else {
            "convert"
        }
    }

    /// Swift `photoTimeOptions`; `current_zone` is the device's zone.
    pub fn photo_time(&self, current_zone: &str) -> Option<Value> {
        if self.photo_time_repair != Some(true) || self.auxiliary_only_to_apple() {
            return None;
        }
        Some(json!({
            "policy": "phototime-1",
            "synthetic": self.photo_time_synthetic.unwrap_or(false),
            "zone": self.photo_time_zone.as_deref().unwrap_or(current_zone),
            "epoch": double_text(self.photo_time_reference.unwrap_or(0.0)),
            "file_times": self.exif_file_times.unwrap_or(false),
        }))
    }

    /// Swift `ConversionSettings.options`.
    pub fn options(&self, current_zone: &str) -> Options {
        let photo_time = self.photo_time(current_zone);
        let exif_file_times = self.exif_file_times.unwrap_or(false);
        if self.direction == "android-to-android" || self.direction == "honor-to-google" {
            return Options {
                direction: self.direction.clone(),
                legacy_hdr: self.legacy_hdr,
                missing_time: "error".into(),
                time_domain: "auto".into(),
                video_track: "primary".into(),
                output_mode: "livephoto".into(),
                repair_auxiliary_duration: false,
                export_data_files: self.export_data_files,
                exif_file_times,
                photo_time,
                candidate_delivery: "static".into(),
                android: None,
                carry_cover_selections: false,
                carry_manual_covers: false,
                carry_burst_choices: false,
            };
        }
        Options {
            direction: self.direction.clone(),
            legacy_hdr: self.legacy_hdr,
            missing_time: self.missing_time.clone(),
            time_domain: self.time_domain.clone(),
            video_track: self.track.clone(),
            output_mode: self.export_mode.clone(),
            repair_auxiliary_duration: self.repair_auxiliary_duration,
            export_data_files: self.export_data_files,
            exif_file_times,
            photo_time,
            candidate_delivery: self
                .candidate_delivery
                .clone()
                .unwrap_or_else(|| "static".into()),
            android: Some(AndroidChoice {
                vendor: self.vendor.clone(),
                container: self.container.clone(),
                hdr: if self.container == "heic" {
                    "preserve"
                } else {
                    "ultra"
                }
                .into(),
                preserve_jpeg: self.preserve_jpeg,
            }),
            carry_cover_selections: self.export_mode != "auxiliary-only",
            carry_manual_covers: self.direction == "to-apple",
            carry_burst_choices: true,
        }
    }

    /// The scalar fields; the album title is not echoed back (`album_title_blank`
    /// says whether it becomes the default), qualities as decimal strings.
    pub fn json(&self) -> Value {
        json!({
            "direction": self.direction,
            "destination": self.destination,
            "album_title_blank": self.album_title_blank(),
            "vendor": self.vendor,
            "container": self.container,
            "image_quality": double_text(self.image_quality),
            "gain_quality": double_text(self.gain_quality),
            "preserve_jpeg": self.preserve_jpeg,
            "legacy_hdr": self.legacy_hdr,
            "export_data_files": self.export_data_files,
            "track": self.track,
            "export_mode": self.export_mode,
            "missing_time": self.missing_time,
            "time_domain": self.time_domain,
            "repair_auxiliary_duration": self.repair_auxiliary_duration,
        })
    }
}

fn clamp_quality(value: f64) -> f64 {
    let v = if value.is_finite() {
        value
    } else {
        DEFAULT_QUALITY
    };
    // Swift `min(1, max(0.01, v))`.
    let lower = if 0.01 > v { 0.01 } else { v };
    if lower < 1.0 {
        lower
    } else {
        1.0
    }
}

/// Foundation `CharacterSet.whitespacesAndNewlines`: Unicode General
/// Category Z*, U+0009–U+000D, U+0085 and U+200B (Foundation still counts
/// the zero width space; U+180E and U+FEFF are not in it).
pub fn swift_whitespace_or_newline(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{D}'
            | '\u{20}'
            | '\u{85}'
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200B}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// The Android writer choices of an options value (its qualities are the
/// settings' own, passed through unchanged by the caller).
#[derive(Clone, Debug, PartialEq)]
pub struct AndroidChoice {
    pub vendor: String,
    pub container: String,
    pub hdr: String,
    pub preserve_jpeg: bool,
}

/// Swift `ConversionOptions` fields that a settings value decides.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub direction: String,
    pub legacy_hdr: bool,
    pub missing_time: String,
    pub time_domain: String,
    pub video_track: String,
    pub output_mode: String,
    pub repair_auxiliary_duration: bool,
    pub export_data_files: bool,
    pub exif_file_times: bool,
    pub photo_time: Option<Value>,
    pub candidate_delivery: String,
    /// None: the default `AndroidOptions()`.
    pub android: Option<AndroidChoice>,
    pub carry_cover_selections: bool,
    pub carry_manual_covers: bool,
    pub carry_burst_choices: bool,
}

impl Options {
    pub fn json(&self) -> Value {
        json!({
            "direction": self.direction,
            "legacy_hdr": self.legacy_hdr,
            "missing_time": self.missing_time,
            "time_domain": self.time_domain,
            "video_track": self.video_track,
            "output_mode": self.output_mode,
            "repair_auxiliary_duration": self.repair_auxiliary_duration,
            "export_data_files": self.export_data_files,
            "exif_file_times": self.exif_file_times,
            "photo_time": self.photo_time,
            "candidate_delivery": self.candidate_delivery,
            "android": self.android.as_ref().map(|a| json!({
                "vendor": a.vendor, "container": a.container, "hdr": a.hdr, "preserve_jpeg": a.preserve_jpeg,
            })),
            "carry": {
                "cover_selections": self.carry_cover_selections,
                "manual_covers": self.carry_manual_covers,
                "burst_choices": self.carry_burst_choices,
            },
        })
    }
}

/// `conversion-settings-tables`: defaults and the fixed facts of each choice.
pub fn tables_json() -> Value {
    let directions: Map<String, Value> = DIRECTIONS
        .iter()
        .map(|d| {
            (
                d.to_string(),
                json!({"exports_android": exports_android(d)}),
            )
        })
        .collect();
    let vendors: Map<String, Value> = VENDORS
        .iter()
        .map(|v| (v.to_string(), json!({"supports_heic": supports_heic(v)})))
        .collect();
    let destinations: Map<String, Value> = DESTINATIONS
        .iter()
        .map(|d| {
            (d.to_string(), json!({"uses_cache": uses_cache(d), "imports_photos": imports_photos(d), "shares": shares(d)}))
        })
        .collect();
    let presets: Map<String, Value> = PRESETS
        .iter()
        .map(|p| {
            (
                p.to_string(),
                json!({"direction": preset_direction(p), "vendor": preset_vendor(p)}),
            )
        })
        .collect();
    let mut current = Map::new();
    for d in DIRECTIONS {
        for v in VENDORS {
            current.insert(format!("{d}/{v}"), json!(current_preset(d, v)));
        }
    }
    let mut defaults = Settings::default().json();
    defaults["album_title"] = json!(DEFAULT_ALBUM);
    json!({
        "defaults": defaults,
        "directions": directions,
        "vendors": vendors,
        "destinations": destinations,
        "presets": presets,
        "preset_order": PRESETS,
        "current_preset": current,
    })
}

/// `conversion-settings`: the decisions for one settings value.
pub fn decide_json(req: &Value) -> Value {
    let settings = Settings::from_json(&req["settings"]);
    let zone = req["current_zone"].as_str().unwrap_or("UTC");
    let mut normalized = settings.clone();
    normalized.normalize();
    let mut normalized_json = normalized.json();
    // Whether the stored title becomes the default (the normalized title no longer is blank).
    normalized_json["album_title_blank"] = json!(settings.album_title_blank());
    json!({
        "normalized": normalized_json,
        "options": settings.options(zone).json(),
        "private_hdr_available": settings.private_hdr_available(),
        "private_hdr_enabled": settings.private_hdr_enabled(),
        "destinations": settings.destinations(),
        "action": settings.action(),
        "preset": current_preset(&settings.direction, &settings.vendor),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> Settings {
        Settings::default()
    }

    #[test]
    fn heic_only_for_samsung() {
        for v in VENDORS {
            let mut x = Settings {
                vendor: v.into(),
                container: "heic".into(),
                ..s()
            };
            x.normalize();
            assert_eq!(x.container == "heic", v == "samsung", "{v}");
        }
    }

    #[test]
    fn android_routes_never_ask_or_import() {
        for d in ["to-android", "android-to-android", "honor-to-google"] {
            for dest in ["ask", "photos", "folderAndPhotos"] {
                let mut x = Settings {
                    direction: d.into(),
                    destination: dest.into(),
                    ..s()
                };
                x.normalize();
                assert_eq!(x.destination, "folder");
            }
            let mut x = Settings {
                direction: d.into(),
                destination: "share".into(),
                ..s()
            };
            x.normalize();
            assert_eq!(x.destination, "share");
        }
        let mut x = Settings {
            destination: "folderAndShare".into(),
            ..s()
        };
        x.normalize();
        assert_eq!(x.destination, "folder");
        let mut x = Settings {
            destination: "photos".into(),
            export_mode: "auxiliary-only".into(),
            ..s()
        };
        x.normalize();
        assert_eq!(x.destination, "folder");
    }

    #[test]
    fn candidates_force_primary_track_and_album_defaults() {
        let mut x = Settings {
            track: "secondary".into(),
            export_mode: "livephoto+candidates".into(),
            ..s()
        };
        x.normalize();
        assert_eq!(x.track, "primary");
        let mut x = Settings {
            track: "secondary".into(),
            has_cover_selections: true,
            ..s()
        };
        x.normalize();
        assert_eq!(x.track, "primary");
        let mut x = Settings {
            track: "secondary".into(),
            album_title: " \u{3000}\n".into(),
            ..s()
        };
        x.normalize();
        assert_eq!(
            (x.track.as_str(), x.album_title.as_str()),
            ("secondary", DEFAULT_ALBUM)
        );
    }

    #[test]
    fn decide_reports_the_blank_title_of_the_input() {
        let v = decide_json(
            &json!({"settings": {"album_title": "\u{3000}\n", "image_quality": "0.9500000000000001"}}),
        );
        assert_eq!(v["normalized"]["album_title_blank"], true);
        assert_eq!(v["normalized"]["image_quality"], "0.9500000000000001");
        let v = decide_json(&json!({"settings": {"album_title": "\u{FEFF}"}}));
        assert_eq!(v["normalized"]["album_title_blank"], false);
    }

    #[test]
    fn qualities_clamp() {
        assert_eq!(clamp_quality(f64::NAN), 0.95);
        assert_eq!(clamp_quality(f64::NEG_INFINITY), 0.95);
        assert_eq!(clamp_quality(0.0), 0.01);
        assert_eq!(clamp_quality(7.0), 1.0);
        assert_eq!(clamp_quality(0.5), 0.5);
        for text in [
            "0.9500000000000001",
            "0.7777777777777777",
            "5e-324",
            "-0.0",
            "1.0",
            "0.01",
            "1e300",
        ] {
            let v: f64 = text.parse().unwrap();
            assert_eq!(
                double_text(v).parse::<f64>().unwrap().to_bits(),
                v.to_bits(),
                "{text}"
            );
        }
        assert_eq!(double(&json!("inf")), Some(f64::INFINITY));
        assert_eq!(double(&json!("-inf")), Some(f64::NEG_INFINITY));
        assert!(double(&json!("nan")).unwrap().is_nan());
        assert!(
            swift_whitespace_or_newline('\u{200B}') && !swift_whitespace_or_newline('\u{FEFF}')
        );
    }

    #[test]
    fn private_hdr_defaults_off_and_availability() {
        assert!(!s().private_hdr_enabled());
        let x = Settings {
            legacy_hdr: true,
            ..s()
        };
        assert!(x.private_hdr_enabled());
        let x = Settings {
            legacy_hdr: true,
            export_mode: "auxiliary-only".into(),
            ..s()
        };
        assert!(!x.private_hdr_available());
        let x = Settings {
            legacy_hdr: true,
            direction: "android-to-android".into(),
            ..s()
        };
        assert!(!x.private_hdr_enabled());
        let x = Settings {
            legacy_hdr: true,
            direction: "honor-to-google".into(),
            ..s()
        };
        assert!(x.private_hdr_enabled());
    }

    #[test]
    fn options_by_route() {
        let x = Settings {
            direction: "honor-to-google".into(),
            track: "secondary".into(),
            ..s()
        };
        let o = x.options("UTC");
        assert_eq!(
            (
                o.video_track.as_str(),
                o.android.is_none(),
                o.carry_burst_choices
            ),
            ("primary", true, false)
        );
        let x = Settings {
            container: "heic".into(),
            photo_time_repair: Some(true),
            ..s()
        };
        let o = x.options("Asia/Shanghai");
        assert_eq!(o.android.unwrap().hdr, "preserve");
        assert_eq!(o.photo_time.unwrap()["zone"], "Asia/Shanghai");
        let x = Settings {
            export_mode: "auxiliary-only".into(),
            photo_time_repair: Some(true),
            ..s()
        };
        let o = x.options("UTC");
        assert!(o.photo_time.is_none() && !o.carry_cover_selections && o.carry_manual_covers);
    }

    #[test]
    fn presets_round_trip() {
        for p in PRESETS {
            assert_eq!(current_preset(preset_direction(p), preset_vendor(p)), p);
        }
        assert_eq!(current_preset("to-android", "bogus"), "google");
    }
}
