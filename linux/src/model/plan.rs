//! This run's plan: which photos in the scope are expected to run, which
//! are exported unchanged, and why. Mirrors the macOS `WorkspaceModel`
//! presentation plan (`isMigrationReady`, `actionBlockingReason`,
//! `availableLive` …). Media decisions stay with the core.
//!
//! Reasons and summaries are gettext message IDs (English, the macOS
//! wording), translated where they are shown.

use super::claims::N_;
use super::queue::{PhotoItem, Queue, HONOR_PRIVATE_HDR};
use super::settings::{Direction, ExportMode, ExportSettings, Unconverted, VideoTrack};
use serde_json::Value;
use std::collections::BTreeMap;

/// One `burst-plan` group from the core.
#[derive(Clone, Debug, PartialEq)]
pub struct BurstGroup {
    pub id: String,
    pub label: String,
    pub signature: String,
    pub supported: bool,
    pub ordered: bool,
    pub reason: String,
    pub members: Vec<String>,
    /// Source cover the core found (one original COVER), if any.
    pub representative: Option<String>,
}

impl BurstGroup {
    pub fn from_json(v: &Value) -> Option<BurstGroup> {
        Some(BurstGroup {
            id: v["id"].as_str()?.to_string(),
            label: v["label"].as_str().unwrap_or("").to_string(),
            signature: v["signature"].as_str().unwrap_or("").to_string(),
            supported: v["supported"] == true,
            ordered: v["ordered"] != false,
            reason: v["reason"].as_str().unwrap_or("").to_string(),
            members: v["members"]
                .as_array()
                .map(|m| {
                    m.iter()
                        .filter_map(|m| m["source"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            representative: v["representative"].as_str().map(str::to_string),
        })
    }
}

/// The user's choice for a burst group (macOS `BurstChoice`).
#[derive(Clone, Debug, PartialEq)]
pub struct BurstChoice {
    pub signature: String,
    pub representative: Option<String>,
    pub independent: bool,
}

impl BurstChoice {
    pub fn json(&self) -> Value {
        serde_json::json!({
            "signature": self.signature,
            "representative": self.representative,
            "independent": self.independent,
        })
    }
}

pub fn burst_choice(settings: &ExportSettings, group: &BurstGroup) -> BurstChoice {
    if let Some(c) = settings.burst_choices.get(&group.id) {
        if c["signature"].as_str() == Some(group.signature.as_str()) {
            return BurstChoice {
                signature: group.signature.clone(),
                representative: c["representative"].as_str().map(str::to_string),
                independent: c["independent"] == true,
            };
        }
    }
    BurstChoice {
        signature: group.signature.clone(),
        representative: group.representative.clone(),
        independent: false,
    }
}

/// The plan for the current queue, settings and burst groups.
pub struct Plan<'a> {
    pub queue: &'a Queue,
    pub settings: &'a ExportSettings,
    pub groups: &'a [BurstGroup],
    pub unconverted: Unconverted,
}

/// Where the preview puts a photo (macOS `previewBucket`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bucket {
    Ready,
    Risk,
    NotConverted,
}

impl<'a> Plan<'a> {
    pub fn scope(&self) -> Vec<&'a PhotoItem> {
        self.queue.scope()
    }

    fn group_of(&self, item: &PhotoItem) -> Option<&'a BurstGroup> {
        let source = item.source.to_string_lossy();
        self.groups
            .iter()
            .find(|g| g.members.iter().any(|m| *m == source))
    }

    pub fn burst_selected_count(&self, group: &BurstGroup) -> usize {
        group
            .members
            .iter()
            .filter(|m| {
                !self.queue.has_explicit_selection()
                    || self
                        .queue
                        .position(std::path::Path::new(m))
                        .is_some_and(|i| self.queue.is_checked(i))
            })
            .count()
    }

    /// macOS `burstBlockingReason`.
    pub fn burst_blocking_reason(&self, group: &BurstGroup) -> Option<String> {
        let count = self.burst_selected_count(group);
        let choice = burst_choice(self.settings, group);
        if count == 0 {
            return None;
        }
        if self.settings.direction != Direction::ToApple {
            return Some("连拍目前仅支持 Apple 目标".into());
        }
        if !group.supported {
            return Some(group.reason.clone());
        }
        if choice.independent {
            return None;
        }
        if count != group.members.len() {
            return Some(
                N_("Only some members are selected. Select the whole group or explicitly export independent photos")
                    .into(),
            );
        }
        if !group.ordered {
            return Some(group.reason.clone());
        }
        if count > 1
            && !choice
                .representative
                .as_ref()
                .is_some_and(|r| group.members.contains(r))
        {
            return Some(N_("Select a representative frame").into());
        }
        None
    }

    pub fn available_live(&self) -> bool {
        self.scope()
            .iter()
            .any(|i| i.is_motion && i.burst.is_none())
    }
    pub fn available_burst(&self) -> bool {
        self.settings.direction == Direction::ToApple
            && self
                .scope()
                .iter()
                .any(|i| i.burst.as_ref().is_some_and(|b| b.supported))
    }
    pub fn available_hdr(&self) -> bool {
        self.scope().iter().any(|i| {
            i.category.convertible()
                && (i.is_static_hdr || i.hdr.contains("HDR"))
                && (i.hdr != HONOR_PRIVATE_HDR || i.private_hdr_supported == Some(true))
        })
    }
    fn plain_still(i: &PhotoItem) -> bool {
        i.confirmed_static && !i.is_static_hdr && !i.is_motion && i.burst.is_none()
    }
    pub fn available_time(&self) -> bool {
        self.scope()
            .iter()
            .any(|i| Self::plain_still(i) || self.is_ready(i))
    }
    pub fn available_metadata_time(&self) -> bool {
        self.settings.direction == Direction::ToApple
            || self
                .scope()
                .iter()
                .any(|i| !i.is_motion && i.burst.is_none())
    }
    fn live_aux_only(&self) -> bool {
        self.settings.live_conversion
            && self.available_live()
            && self.settings.export_mode == ExportMode::AuxiliaryOnly
    }
    /// Only stills that need missing dates (macOS `isTimeOnlyWorkspace`).
    pub fn time_only(&self) -> bool {
        let scope = self.scope();
        self.settings.photo_time_repair
            && self.settings.direction == Direction::ToApple
            && !self.live_aux_only()
            && !scope.is_empty()
            && scope.iter().all(|i| Self::plain_still(i))
    }
    pub fn is_time_repair_candidate(&self, i: &PhotoItem) -> bool {
        self.settings.photo_time_repair
            && !self.live_aux_only()
            && i.confirmed_static
            && i.burst.is_none()
            && i.capture_time_missing == Some(true)
            && !i.is_motion
            && (self.settings.photo_time_synthetic || i.filename_time)
    }

    /// macOS `actionBlockingReason`.
    pub fn blocking_reason(&self, i: &PhotoItem) -> Option<String> {
        let s = self.settings;
        let plain_hdr_still = !i.is_motion && i.burst.is_none();
        if i.hdr == HONOR_PRIVATE_HDR && plain_hdr_still && i.private_hdr_supported != Some(true) {
            return Some(if i.private_hdr_reason.is_empty() {
                "荣耀私有 HDR 未通过配置检查，不能转换".into()
            } else {
                i.private_hdr_reason.clone()
            });
        }
        if i.is_static_hdr
            && plain_hdr_still
            && self.available_live()
            && s.live_conversion
            && (s.export_mode != ExportMode::LivePhoto || s.track != VideoTrack::Primary)
        {
            return Some(N_("The selected Live Photo auxiliary-output options cannot currently be combined with static HDR. Adjust those options or process separately").into());
        }
        let ext = i
            .source
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !s.legacy_hdr
            && s.direction == Direction::ToAndroid
            && s.effective_container() == super::settings::Container::Jpeg
            && (ext == "heic" || ext == "heif")
            && i.hdr.contains("HDR")
        {
            return Some(N_("Target JPEG requires HDR reconstruction. HDR cannot be safely preserved while its action is off; change the format or selected features").into());
        }
        if i.burst.is_some() {
            if !s.burst_conversion {
                return Some(if s.legacy_hdr {
                    N_("HDR-only processing cannot safely preserve the original burst container yet. Enable burst conversion or export unchanged resources")
                } else {
                    N_("Burst conversion is not selected; original burst resources are preserved")
                }.into());
            }
        } else if i.is_motion {
            if !s.live_conversion {
                return Some(if s.legacy_hdr {
                    N_("HDR-only processing cannot safely preserve this motion container yet. Enable Live Photo conversion or export unchanged resources")
                } else {
                    N_("Live Photo conversion is not selected; original motion resources are preserved")
                }.into());
            }
        } else if i.is_static_hdr && !s.legacy_hdr {
            return Some(
                N_("HDR conversion is not selected; original HDR resources are preserved").into(),
            );
        }
        if !s.legacy_hdr
            && i.hdr.contains("HDR")
            && s.cover_selections
                .contains_key(i.source.to_string_lossy().as_ref())
        {
            return Some(N_("A custom cover cannot preserve unselected HDR. Restore the original cover or change the selected features").into());
        }
        None
    }

    /// macOS `isMigrationReady`.
    pub fn is_ready(&self, i: &PhotoItem) -> bool {
        if self.blocking_reason(i).is_some() {
            return false;
        }
        if self.settings.photo_time_repair
            && self.settings.direction != Direction::ToApple
            && i.is_motion
            && i.capture_time_missing == Some(true)
        {
            return false;
        }
        if !(self.is_time_repair_candidate(i) || i.category.convertible()) {
            return false;
        }
        if let Some(group) = self.group_of(i) {
            return self.burst_blocking_reason(group).is_none();
        }
        if i.burst.is_some() {
            // Burst rows without a group plan cannot run.
            return false;
        }
        true
    }

    pub fn ready(&self) -> Vec<&'a PhotoItem> {
        self.scope()
            .into_iter()
            .filter(|i| self.is_ready(i))
            .collect()
    }
    pub fn unconverted_items(&self) -> Vec<&'a PhotoItem> {
        self.scope()
            .into_iter()
            .filter(|i| !self.is_ready(i))
            .collect()
    }

    pub fn risk(&self, i: &PhotoItem) -> bool {
        self.is_ready(i)
            && (self.settings.photo_time_repair
                || self.settings.legacy_hdr && i.hdr.contains("HDR")
                || i.hdr == HONOR_PRIVATE_HDR)
    }
    pub fn bucket(&self, i: &PhotoItem) -> Bucket {
        if !self.is_ready(i) {
            Bucket::NotConverted
        } else if self.risk(i) {
            Bucket::Risk
        } else {
            Bucket::Ready
        }
    }

    /// Status line of a photo in the preview (macOS `anticipatedStatus`).
    /// Returns a message ID and, for blocked photos, the reason message ID.
    pub fn anticipated_status(&self, i: &PhotoItem) -> (&'static str, Option<String>) {
        if let Some(reason) = self.blocking_reason(i) {
            return (N_("Will not convert: "), Some(reason));
        }
        if self.risk(i) {
            return (
                if i.hdr == HONOR_PRIVATE_HDR && i.private_hdr_supported != Some(true) {
                    N_("Partly executable: HDR cannot be converted")
                } else {
                    N_("May fail; execution-time verification required")
                },
                None,
            );
        }
        if self.is_ready(i) {
            return (N_("Expected to Run"), None);
        }
        if self.time_only() && i.capture_time_missing == Some(false) {
            return (N_("Date already present; no repair needed"), None);
        }
        if self.unconverted != Unconverted::None {
            return (N_("Planned unchanged export"), None);
        }
        (N_("Not converted in this run"), None)
    }

    /// macOS `expectedResources`.
    pub fn expected_resources(&self, i: &PhotoItem) -> &'static str {
        if !self.is_ready(i) {
            return if self.unconverted != Unconverted::None {
                N_("Plan to export complete original resources; required resources still need readability checks.")
            } else {
                N_("No converted output will be generated. See the plan for the reason.")
            };
        }
        if self.is_time_repair_candidate(i) {
            N_("Planned output: date-repaired still copy in the original format")
        } else if i.is_motion {
            N_("Planned output: target motion format and required paired resources")
        } else if i.burst.is_some() {
            N_("Planned output: selected burst members and group resources")
        } else {
            N_("Planned output: one static HDR image, without Live Photo video")
        }
    }

    /// macOS `migrationPlanSummary`: message IDs joined with "; ", or the
    /// core's own (Chinese) explanation.
    pub fn summary(&self, i: &PhotoItem) -> Vec<String> {
        if let Some(reason) = self.blocking_reason(i) {
            return vec![reason];
        }
        if let Some(b) = &i.burst {
            if !b.supported {
                return vec![b.reason.clone()];
            }
            if let Some(group) = self.group_of(i) {
                if let Some(reason) = self.burst_blocking_reason(group) {
                    return vec![reason];
                }
                return vec![if burst_choice(self.settings, group).independent {
                    N_("Export selected photos independently without writing burst markers")
                } else {
                    N_("Convert burst markers; preserve existing indices and all selected frames. Missing frames are allowed; the original total is not inferred")
                }
                .into()];
            }
        }
        if self.is_time_repair_candidate(i) {
            return vec![N_("PhotoTime fills only missing capture dates in output copies, preserving image encoding and other features. Without a reliable date or explicit synthetic-time option, no capture date is written.").into()];
        }
        if self.settings.photo_time_repair
            && self.settings.direction != Direction::ToApple
            && i.is_motion
            && i.capture_time_missing == Some(true)
        {
            return vec![N_("Capture-date rewriting cannot safely rebuild this target motion container yet. Disable date repair or choose another route; it will not be silently flattened into a still image.").into()];
        }
        if !i.category.convertible() {
            return vec![if i.detail.is_empty() {
                N_("Cannot convert with the current settings. Unchanged export may be available; target compatibility is not guaranteed").into()
            } else {
                i.detail.clone()
            }];
        }
        if !i.is_motion {
            return vec![N_(
                "Static HDR: process as one image, without video, cover time, or Live Photo pairing",
            )
            .into()];
        }
        let mut parts: Vec<String> = vec![
            if self.settings.direction == Direction::ToApple {
                N_("Convert motion photos into Apple paired resources")
            } else {
                N_("Convert Live Photos into the target motion format")
            }
            .into(),
            N_("Preserve motion, audio, and a trusted cover").into(),
        ];
        if i.hdr.to_lowercase().contains("hdr") {
            parts.push(if self.settings.legacy_hdr {
                N_("HDR follows available capabilities. Private HDR conversion is experimental; unsupported HDR is reported and ignored without blocking other supported actions")
            } else {
                N_("HDR follows the standard compatibility path. Private HDR conversion is off or does not apply to this output")
            }.into());
        }
        if self
            .settings
            .cover_selections
            .contains_key(i.source.to_string_lossy().as_ref())
        {
            parts.push(N_("The custom high-resolution cover is JPEG and does not preserve the original cover's HDR").into());
        }
        parts
    }
}

/// Burst choices to send: only groups with selected members, keyed by id.
pub fn burst_choices(plan: &Plan) -> BTreeMap<String, Value> {
    plan.groups
        .iter()
        .filter(|g| plan.burst_selected_count(g) > 0 && g.supported)
        .map(|g| (g.id.clone(), burst_choice(plan.settings, g).json()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::queue::Queue;
    use serde_json::json;

    fn rows() -> Vec<Value> {
        vec![
            json!({"source":"/p/live.jpg","motion":true,"hdr":"SDR","static":false}),
            json!({"source":"/p/hdr.jpg","motion":false,"hdr":"Ultra HDR","hdr_kind":"ultra_hdr","static":true}),
            json!({"source":"/p/still.jpg","motion":false,"hdr":"SDR","static":true,"capture_time_missing":true,"filename_time":true}),
            json!({"source":"/p/b1.jpg","motion":false,"hdr":"SDR","static":true,"burst":{"supported":true,"group_id":"g","index":1}}),
            json!({"source":"/p/b2.jpg","motion":false,"hdr":"SDR","static":true,"burst":{"supported":true,"group_id":"g","index":2}}),
        ]
    }
    fn group() -> BurstGroup {
        BurstGroup {
            id: "g".into(),
            label: "g".into(),
            signature: "sig".into(),
            supported: true,
            ordered: true,
            reason: String::new(),
            members: vec!["/p/b1.jpg".into(), "/p/b2.jpg".into()],
            representative: None,
        }
    }
    fn queue() -> Queue {
        let mut q = Queue::default();
        q.replace(rows(), Direction::ToApple);
        q
    }

    #[test]
    fn default_plan_runs_live_only() {
        let q = queue();
        let s = ExportSettings::default();
        let groups = [group()];
        let p = Plan {
            queue: &q,
            settings: &s,
            groups: &groups,
            unconverted: Unconverted::None,
        };
        let ready: Vec<_> = p.ready().iter().map(|i| i.name.clone()).collect();
        // HDR off by default, still has no date repair, burst lacks a representative.
        assert_eq!(ready, ["live.jpg"]);
        assert_eq!(p.unconverted_items().len(), 4);
        assert!(
            p.available_live() && p.available_burst() && p.available_hdr() && p.available_time()
        );
        assert_eq!(
            p.burst_blocking_reason(&groups[0]).as_deref(),
            Some("Select a representative frame")
        );
    }

    #[test]
    fn features_change_readiness() {
        let q = queue();
        let mut s = ExportSettings {
            legacy_hdr: true,
            live_conversion: false,
            ..Default::default()
        };
        s.set_time_options(true, true, 1.0);
        s.burst_choices.insert(
            "g".into(),
            BurstChoice {
                signature: "sig".into(),
                representative: Some("/p/b1.jpg".into()),
                independent: false,
            }
            .json(),
        );
        let groups = [group()];
        let p = Plan {
            queue: &q,
            settings: &s,
            groups: &groups,
            unconverted: Unconverted::SameLocation,
        };
        let ready: Vec<_> = p.ready().iter().map(|i| i.name.clone()).collect();
        assert_eq!(ready, ["hdr.jpg", "still.jpg", "b1.jpg", "b2.jpg"]);
        let live = q.get(0).expect("live");
        assert_eq!(
            p.blocking_reason(live).as_deref(),
            Some("HDR-only processing cannot safely preserve this motion container yet. Enable Live Photo conversion or export unchanged resources")
        );
        assert_eq!(p.anticipated_status(live).0, "Will not convert: ");
        assert_eq!(p.bucket(q.get(2).expect("still")), Bucket::Risk);
        assert_eq!(burst_choices(&p).len(), 1);
    }

    #[test]
    fn partial_burst_selection_blocks_the_group() {
        let mut q = queue();
        q.toggle(4);
        let s = ExportSettings::default();
        let groups = [group()];
        let p = Plan {
            queue: &q,
            settings: &s,
            groups: &groups,
            unconverted: Unconverted::None,
        };
        assert_eq!(p.burst_selected_count(&groups[0]), 1);
        assert!(p
            .burst_blocking_reason(&groups[0])
            .is_some_and(|r| r.starts_with("Only some members")));
    }

    #[test]
    fn time_only_workspace() {
        let mut q = queue();
        q.deselect_all();
        q.toggle(2);
        let mut s = ExportSettings::default();
        s.set_time_options(true, true, 1.0);
        let p = Plan {
            queue: &q,
            settings: &s,
            groups: &[],
            unconverted: Unconverted::None,
        };
        assert!(p.time_only());
        assert_eq!(p.ready().len(), 1);
        assert_eq!(
            p.expected_resources(q.get(2).expect("still")),
            "Planned output: date-repaired still copy in the original format"
        );
    }

    #[test]
    fn group_parses_core_json() {
        let g = BurstGroup::from_json(
            &json!({"id":"g","signature":"s","supported":true,"ordered":true,
            "members":[{"source":"/a"},{"source":"/b"}],"representative":"/a","reason":""}),
        )
        .expect("group");
        assert_eq!(g.members, ["/a", "/b"]);
        assert_eq!(g.representative.as_deref(), Some("/a"));
    }
}
