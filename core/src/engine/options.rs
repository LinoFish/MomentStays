//! Swift `ConversionOptions` / `AndroidOptions`, including their `json`
//! form, which is stored in manifests and compared on resume, so it must
//! stay value-identical to the Swift engine's.

use crate::engine::contracts;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct AndroidOptions {
    pub vendor: String,
    pub container: String,
    pub hdr: String,
    pub image_quality: f64,
    pub gain_quality: f64,
    pub preserve_jpeg: bool,
}

impl Default for AndroidOptions {
    fn default() -> Self {
        AndroidOptions {
            vendor: "google".into(),
            container: "jpeg".into(),
            hdr: "ultra".into(),
            image_quality: 0.95,
            gain_quality: 0.95,
            preserve_jpeg: true,
        }
    }
}

impl AndroidOptions {
    pub fn vendor(vendor: &str) -> Self {
        AndroidOptions {
            vendor: vendor.into(),
            ..Default::default()
        }
    }
    pub fn json(&self) -> Value {
        json!({
            "vendor": self.vendor,
            "container": self.container,
            "hdr": self.hdr,
            "image_quality": self.image_quality,
            "gain_quality": self.gain_quality,
            "preserve_jpeg": self.preserve_jpeg,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConversionOptions {
    pub direction: String,
    pub android: AndroidOptions,
    pub legacy_hdr: bool,
    pub missing_time: String,
    pub time_domain: String,
    pub video_track: String,
    pub output_mode: String,
    pub repair_auxiliary_duration: bool,
    pub export_data_files: bool,
    pub cover_index: Option<i64>,
    /// Normalized source path -> candidate index.
    pub cover_selections: BTreeMap<String, i64>,
    pub candidate_delivery: String,
    pub burst_plans: Vec<Value>,
    pub burst_choices: BTreeMap<String, Value>,
    pub manual_cover: Option<crate::engine::manual::Selection>,
    pub manual_covers: BTreeMap<String, crate::engine::manual::Selection>,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        ConversionOptions {
            direction: "to-apple".into(),
            android: AndroidOptions::default(),
            legacy_hdr: false,
            missing_time: "error".into(),
            time_domain: "auto".into(),
            video_track: "primary".into(),
            output_mode: "livephoto".into(),
            repair_auxiliary_duration: false,
            export_data_files: true,
            cover_index: None,
            cover_selections: BTreeMap::new(),
            candidate_delivery: "static".into(),
            burst_plans: Vec::new(),
            burst_choices: BTreeMap::new(),
            manual_cover: None,
            manual_covers: BTreeMap::new(),
        }
    }
}

impl ConversionOptions {
    pub fn json(&self) -> Value {
        if self.direction == "honor-to-google" {
            return json!({"direction":self.direction,"legacy_hdr":self.legacy_hdr,"hdr_policy":super::hdr::POLICY,"export_data_files":self.export_data_files});
        }
        if self.direction == "android-to-android" {
            return json!({
                "direction": self.direction,
                "target": "honor",
                "layout": contracts::TRANSFER_REVISION,
                "vendor": "google",
                "container": "jpeg",
                "export_data_files": self.export_data_files,
            });
        }
        if self.direction == "to-android" {
            return json!({
                "direction": self.direction,
                "android": self.android.json(),
                "legacy_hdr":self.legacy_hdr, "hdr_policy":super::hdr::POLICY,
                "time_domain": self.time_domain,
                "export_data_files": self.export_data_files,
            });
        }
        let mut m = Map::new();
        m.insert(
            "legacy_hdr".into(),
            Value::from(if self.legacy_hdr { "auto" } else { "preserve" }),
        );
        m.insert(
            "missing_time".into(),
            Value::from(self.missing_time.clone()),
        );
        m.insert("time_domain".into(), Value::from(self.time_domain.clone()));
        m.insert("video_track".into(), Value::from(self.video_track.clone()));
        m.insert("output_mode".into(), Value::from(self.output_mode.clone()));
        m.insert(
            "repair_auxiliary_duration".into(),
            Value::from(self.output_mode != "livephoto" && self.repair_auxiliary_duration),
        );
        m.insert(
            "export_data_files".into(),
            Value::from(self.export_data_files),
        );
        if let Some(i) = self.cover_index {
            m.insert("cover_candidate_index".into(), Value::from(i));
        }
        if self.output_mode == "livephoto+candidates" && self.candidate_delivery != "static" {
            m.insert(
                "candidate_delivery".into(),
                Value::from(self.candidate_delivery.clone()),
            );
        }
        if let Some(c) = &self.manual_cover {
            m.insert("manual_cover".into(), c.json());
        }
        Value::Object(m)
    }

    /// Options from a NativeDriver `batch` request (same keys and defaults
    /// as `apple/shared/tests/NativeDriver.swift`).
    pub fn from_request(req: &Value) -> Self {
        let s = |k: &str, d: &str| req[k].as_str().unwrap_or(d).to_string();
        let b = |k: &str, d: bool| req[k].as_bool().unwrap_or(d);
        let f = |k: &str, d: f64| req[k].as_f64().unwrap_or(d);
        let mut selections = BTreeMap::new();
        if let Some(m) = req["cover_selections"].as_object() {
            for (k, v) in m {
                if let Some(i) = v.as_i64() {
                    selections.insert(
                        crate::engine::paths::normalized(std::path::Path::new(k))
                            .to_string_lossy()
                            .into_owned(),
                        i,
                    );
                }
            }
        }
        ConversionOptions {
            direction: s("direction", "to-apple"),
            android: AndroidOptions {
                vendor: s("vendor", "google"),
                container: s("container", "jpeg"),
                hdr: "ultra".into(),
                image_quality: f("image_quality", 0.95),
                gain_quality: f("gain_quality", 0.95),
                preserve_jpeg: b("preserve_jpeg", true),
            },
            legacy_hdr: b("legacy_hdr", false),
            missing_time: s("missing_time", "error"),
            time_domain: s("time_domain", "auto"),
            video_track: s("video_track", "primary"),
            output_mode: s("output_mode", "livephoto"),
            repair_auxiliary_duration: b("repair_auxiliary_duration", false),
            export_data_files: b("export_data_files", true),
            cover_index: req["cover_candidate_index"].as_i64(),
            cover_selections: selections,
            candidate_delivery: s("candidate_delivery", "static"),
            burst_plans: req["burst_plans"].as_array().cloned().unwrap_or_default(),
            burst_choices: req["burst_choices"]
                .as_object()
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default(),
            manual_cover: crate::engine::manual::Selection::from_json(&req["manual_cover"]),
            manual_covers: req["manual_covers"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| {
                            crate::engine::manual::Selection::from_json(v).map(|s| {
                                (
                                    crate::engine::paths::normalized(std::path::Path::new(k))
                                        .to_string_lossy()
                                        .into_owned(),
                                    s,
                                )
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}
