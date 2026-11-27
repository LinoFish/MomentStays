//! The photo queue: scan rows from the core, classified for the browser,
//! with this run's selection (macOS `selectedIDs` / `hasExplicitSelection`),
//! the feature tabs, the source filter and search.

use super::settings::Direction;
use momentstays_core::engine::decisions::hdr_kind::HdrKind;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What a photo is for the current direction, from the core's scan facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    /// Motion Photo (towards Apple) or Live Photo pair (towards Android).
    Live,
    /// Static HDR still.
    Hdr,
    /// Member of a supported burst group.
    Burst,
    /// Recognized but not convertible here, or not recognized.
    Attention,
}

impl Category {
    pub fn convertible(self) -> bool {
        self != Category::Attention
    }
}

/// Feature tabs and the results step's Filter menu (macOS
/// `MigrationPhotoFeature`). Filtering never changes the run's scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Feature {
    Live,
    Hdr,
    Burst,
    MissingTime,
}

impl Feature {
    pub const ALL: [Feature; 4] = [
        Feature::Live,
        Feature::Hdr,
        Feature::Burst,
        Feature::MissingTime,
    ];
    pub fn includes(self, item: &PhotoItem) -> bool {
        match self {
            Feature::Live => item.is_motion,
            Feature::Hdr => item.hdr.to_lowercase().contains("hdr"),
            Feature::Burst => item.burst.is_some(),
            Feature::MissingTime => item.capture_time_missing == Some(true),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BurstFacts {
    /// Original frame index within the group.
    pub index: Option<u64>,
    pub supported: bool,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct PhotoItem {
    /// The full scan row (passed back to the core unchanged).
    pub row: Value,
    pub source: PathBuf,
    pub name: String,
    pub relative: String,
    pub category: Category,
    pub is_motion: bool,
    pub is_static_hdr: bool,
    pub confirmed_static: bool,
    pub hdr: String,
    pub cover: String,
    pub manufacturer: String,
    pub manufacturer_detail: String,
    /// The core's explanation for this photo (Chinese, as the core writes it).
    pub detail: String,
    pub burst: Option<BurstFacts>,
    /// No capture date in the still's metadata (`None`: not checked).
    pub capture_time_missing: Option<bool>,
    /// A date or Unix time can be read from the file name.
    pub filename_time: bool,
    /// HONOR private HDR preflight (`None`: not that kind of HDR).
    pub private_hdr_supported: Option<bool>,
    pub private_hdr_reason: String,
}

fn text(row: &Value, key: &str) -> String {
    row[key].as_str().unwrap_or("").to_string()
}

fn hdr_kind(row: &Value) -> HdrKind {
    // The stable `hdr_kind` code; rows without it (older callers) fall
    // back to the label, as the core itself does.
    row["hdr_kind"]
        .as_str()
        .and_then(HdrKind::from_code)
        .unwrap_or_else(|| HdrKind::from_label(row["hdr"].as_str().unwrap_or("")))
}

/// Classify one scan row for `direction`.
pub fn classify(row: &Value, direction: Direction) -> Category {
    if row["burst"].is_object() {
        return if direction.supports_burst() && row["burst"]["supported"] == true {
            Category::Burst
        } else {
            Category::Attention
        };
    }
    if row["motion"] == true {
        return Category::Live;
    }
    if row["static"] == true && hdr_kind(row).is_static_hdr() {
        return Category::Hdr;
    }
    Category::Attention
}

/// HONOR private HDR label as the core writes it.
pub const HONOR_PRIVATE_HDR: &str = "荣耀私有 HDR";

impl PhotoItem {
    pub fn from_row(row: Value, direction: Direction) -> Option<PhotoItem> {
        let source = PathBuf::from(row["source"].as_str()?);
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let category = classify(&row, direction);
        let burst = row["burst"].as_object().map(|_| {
            let b = &row["burst"];
            BurstFacts {
                index: b["index"].as_u64(),
                supported: b["supported"] == true,
                reason: b["reason"].as_str().unwrap_or("").to_string(),
            }
        });
        let is_static_hdr = row["static"] == true && hdr_kind(&row).is_static_hdr();
        Some(PhotoItem {
            relative: text(&row, "relative"),
            hdr: text(&row, "hdr"),
            cover: text(&row, "cover"),
            manufacturer: text(&row, "manufacturer"),
            manufacturer_detail: text(&row, "manufacturer_detail"),
            detail: text(&row, "detail"),
            is_motion: row["motion"] == true,
            confirmed_static: row["static"] == true,
            is_static_hdr,
            capture_time_missing: row["capture_time_missing"].as_bool(),
            filename_time: row["filename_time"] == true,
            private_hdr_supported: row["private_hdr_supported"].as_bool(),
            private_hdr_reason: text(&row, "private_hdr_reason"),
            source,
            name,
            category,
            burst,
            row,
        })
    }

    fn matches(&self, needle: &str) -> bool {
        needle.is_empty()
            || [
                &self.name,
                &self.relative,
                &self.manufacturer,
                &self.manufacturer_detail,
            ]
            .iter()
            .any(|field| field.to_lowercase().contains(needle))
    }
}

/// Result states for the results step's tabs (macOS `PhotoState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResultState {
    Converted,
    Resumed,
    Copied,
    Failed,
    Skipped,
}

impl ResultState {
    pub const ALL: [ResultState; 5] = [
        ResultState::Converted,
        ResultState::Resumed,
        ResultState::Copied,
        ResultState::Failed,
        ResultState::Skipped,
    ];
    pub fn from_status(status: &str) -> Option<ResultState> {
        match status {
            "converted" => Some(ResultState::Converted),
            "resumed" => Some(ResultState::Resumed),
            "copied" => Some(ResultState::Copied),
            "failed" => Some(ResultState::Failed),
            "skipped" => Some(ResultState::Skipped),
            _ => None,
        }
    }
}

/// Which items the browser starts from (macOS `baseItems`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Base {
    /// Steps 1–3: every photo.
    #[default]
    All,
    /// Export preview: this run's scope.
    Scope,
    /// Results: the photos of the last run.
    Results,
}

/// What the browser filters by: feature tabs (steps 1–4) or the Filter
/// menu (results), and the results step's state tabs. Filtering never
/// changes the run's scope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tab {
    /// Photos must have every feature in the set.
    pub features: BTreeSet<Feature>,
    /// Results step only.
    pub result: Option<ResultState>,
}

impl Tab {
    pub fn all() -> Tab {
        Tab::default()
    }
    pub fn feature(f: Feature) -> Tab {
        Tab {
            features: [f].into(),
            result: None,
        }
    }
    pub fn result(state: ResultState) -> Tab {
        Tab {
            features: BTreeSet::new(),
            result: Some(state),
        }
    }
}

#[derive(Default)]
pub struct Queue {
    items: Vec<PhotoItem>,
    pub tab: Tab,
    search: String,
    source: Option<PathBuf>,
    explicit: bool,
    selected: HashSet<PathBuf>,
    /// Per-photo outcome of the last run, and the photos it covered.
    results: HashMap<PathBuf, (ResultState, String)>,
    result_scope: Option<HashSet<PathBuf>>,
}

impl Queue {
    /// Replace the rows after a scan, keeping the selection of photos that
    /// are still there.
    pub fn replace(&mut self, rows: Vec<Value>, direction: Direction) {
        self.items = rows
            .into_iter()
            .filter_map(|r| PhotoItem::from_row(r, direction))
            .collect();
        let present: HashSet<&PathBuf> = self.items.iter().map(|i| &i.source).collect();
        self.selected.retain(|p| present.contains(p));
    }

    /// Forget every photo and the selection (sources cleared).
    pub fn clear(&mut self) {
        self.items.clear();
        self.selected.clear();
        self.explicit = false;
        self.results.clear();
        self.result_scope = None;
    }

    #[cfg(test)]
    pub fn items(&self) -> &[PhotoItem] {
        &self.items
    }
    pub fn get(&self, index: usize) -> Option<&PhotoItem> {
        self.items.get(index)
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn set_search(&mut self, search: &str) {
        self.search = search.trim().to_lowercase();
    }
    pub fn set_source(&mut self, source: Option<PathBuf>) {
        self.source = source;
    }
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    // ---- Selection (this run's scope) --------------------------------

    pub fn has_explicit_selection(&self) -> bool {
        self.explicit
    }
    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }
    /// Selected, or in the default scope when nothing was chosen.
    pub fn in_scope(&self, item: &PhotoItem) -> bool {
        !self.explicit || self.selected.contains(&item.source)
    }
    pub fn is_checked(&self, index: usize) -> bool {
        self.items.get(index).is_some_and(|i| self.in_scope(i))
    }
    fn make_explicit(&mut self) {
        if !self.explicit {
            self.selected = self.items.iter().map(|i| i.source.clone()).collect();
            self.explicit = true;
        }
    }
    /// The check mark (or Ctrl-click): include or exclude one photo.
    pub fn toggle(&mut self, index: usize) {
        let Some(source) = self.items.get(index).map(|i| i.source.clone()) else {
            return;
        };
        self.make_explicit();
        if !self.selected.remove(&source) {
            self.selected.insert(source);
        }
    }
    pub fn set_checked(&mut self, index: usize, on: bool) {
        if self.is_checked(index) != on {
            self.toggle(index);
        }
    }
    /// Select Filtered Photos: add every shown photo.
    pub fn select_visible(&mut self, base: Base) {
        let shown: Vec<PathBuf> = self
            .visible(base)
            .into_iter()
            .filter_map(|i| self.items.get(i).map(|i| i.source.clone()))
            .collect();
        self.make_explicit();
        self.selected.extend(shown);
    }
    pub fn deselect_all(&mut self) {
        self.explicit = true;
        self.selected.clear();
    }
    /// Restore Default Scope.
    pub fn clear_selection(&mut self) {
        self.explicit = false;
        self.selected.clear();
    }
    /// Selected photos not shown by the current tab, source or search.
    pub fn hidden_selected(&self, base: Base) -> usize {
        if !self.explicit {
            return 0;
        }
        let shown: HashSet<usize> = self.visible(base).into_iter().collect();
        self.items
            .iter()
            .enumerate()
            .filter(|(i, item)| self.selected.contains(&item.source) && !shown.contains(i))
            .count()
    }

    /// This run's scope, in queue order.
    pub fn scope(&self) -> Vec<&PhotoItem> {
        self.items.iter().filter(|i| self.in_scope(i)).collect()
    }

    // ---- Results ------------------------------------------------------

    pub fn set_results(&mut self, scope: HashSet<PathBuf>, files: &[Value]) {
        self.results.clear();
        for f in files {
            let (Some(source), Some(state)) = (
                f["source"].as_str(),
                f["status"].as_str().and_then(ResultState::from_status),
            ) else {
                continue;
            };
            let detail = f["detail"].as_str().unwrap_or("").to_string();
            self.results.insert(PathBuf::from(source), (state, detail));
        }
        self.result_scope = Some(scope);
    }
    pub fn result(&self, item: &PhotoItem) -> Option<&(ResultState, String)> {
        self.results.get(&item.source)
    }
    pub fn has_results(&self) -> bool {
        self.result_scope.is_some()
    }

    // ---- Browsing -----------------------------------------------------

    fn in_base(&self, item: &PhotoItem, base: Base) -> bool {
        match base {
            Base::All => true,
            Base::Scope => self.in_scope(item),
            Base::Results => self
                .result_scope
                .as_ref()
                .is_none_or(|s| s.contains(&item.source)),
        }
    }
    pub fn base_count(&self, base: Base) -> usize {
        self.items.iter().filter(|i| self.in_base(i, base)).count()
    }
    fn in_tab(&self, item: &PhotoItem, tab: &Tab) -> bool {
        tab.features.iter().all(|f| f.includes(item))
            && tab
                .result
                .is_none_or(|state| self.result(item).is_some_and(|(s, _)| *s == state))
    }
    fn in_source(&self, item: &PhotoItem) -> bool {
        self.source
            .as_ref()
            .is_none_or(|folder| item.source.starts_with(folder))
    }
    /// Count for one tab, over the base items (ignores search and source).
    pub fn tab_count(&self, base: Base, tab: &Tab) -> usize {
        self.items
            .iter()
            .filter(|i| self.in_base(i, base) && self.in_tab(i, tab))
            .count()
    }

    /// Indices into [`Queue::items`] that pass the base, tab, source and search.
    pub fn visible(&self, base: Base) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                self.in_base(item, base)
                    && self.in_tab(item, &self.tab)
                    && self.in_source(item)
                    && item.matches(&self.search)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Index of the photo at `source`, if it is in the queue.
    pub fn position(&self, source: &Path) -> Option<usize> {
        self.items.iter().position(|i| i.source == source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn rows() -> Vec<Value> {
        vec![
            json!({"source":"/p/IMG_1.jpg","relative":"IMG_1.jpg","motion":true,"hdr":"SDR","manufacturer":"Google","detail":"ok","static":false,"burst":null}),
            json!({"source":"/p/HDR_2.jpg","relative":"HDR_2.jpg","motion":false,"hdr":"Ultra HDR","hdr_kind":"ultra_hdr","manufacturer":"Google","static":true,"burst":null}),
            json!({"source":"/p/b/BURST001.jpg","relative":"b/BURST001.jpg","motion":false,"hdr":"SDR","manufacturer":"三星","static":true,
                   "burst":{"supported":true,"source_format":"samsung-burst","group_id":"g1","index":1,"source_cover":true}}),
            json!({"source":"/p/b/BURST002.jpg","relative":"b/BURST002.jpg","motion":false,"hdr":"SDR","manufacturer":"荣耀","static":true,
                   "burst":{"supported":false,"reason":"帧序号重复","group_id":"g2","index":2}}),
            json!({"source":"/p/plain.jpg","relative":"plain.jpg","motion":false,"hdr":"SDR","manufacturer":"未知","static":true,"burst":null,"capture_time_missing":true,"filename_time":false}),
            json!({"relative":"no-source.jpg"}),
        ]
    }

    fn queue() -> Queue {
        let mut q = Queue::default();
        q.replace(rows(), Direction::ToApple);
        q
    }

    #[test]
    fn classifies_core_rows() {
        let q = queue();
        let cats: Vec<_> = q.items().iter().map(|i| i.category).collect();
        assert_eq!(
            cats,
            [
                Category::Live,
                Category::Hdr,
                Category::Burst,
                Category::Attention,
                Category::Attention
            ]
        );
        let burst = q.get(2).and_then(|i| i.burst.clone()).expect("burst facts");
        assert_eq!(burst.index, Some(1));
        assert!(burst.supported);
        assert_eq!(
            q.get(3)
                .and_then(|i| i.burst.as_ref())
                .map(|b| b.reason.as_str()),
            Some("帧序号重复")
        );
        assert_eq!(q.get(4).and_then(|i| i.capture_time_missing), Some(true));
    }

    #[test]
    fn hdr_kind_code_wins_over_label() {
        let coded =
            json!({"source":"/a.jpg","static":true,"hdr":"renamed label","hdr_kind":"iso_hdr"});
        assert_eq!(classify(&coded, Direction::ToApple), Category::Hdr);
        let label_only = json!({"source":"/a.jpg","static":true,"hdr":"荣耀私有 HDR"});
        assert_eq!(classify(&label_only, Direction::ToApple), Category::Hdr);
        let preserved = json!({"source":"/a.jpg","static":true,"hdr_kind":"ultra_hdr_preserved"});
        assert_eq!(
            classify(&preserved, Direction::ToApple),
            Category::Attention
        );
    }

    #[test]
    fn bursts_are_not_convertible_towards_android() {
        let row = &rows()[2];
        assert_eq!(classify(row, Direction::ToAndroid), Category::Attention);
        assert_eq!(classify(row, Direction::ToApple), Category::Burst);
    }

    #[test]
    fn default_scope_is_every_photo_until_one_is_toggled() {
        let mut q = queue();
        assert!(!q.has_explicit_selection());
        assert_eq!(q.scope().len(), 5);
        q.toggle(0);
        assert!(q.has_explicit_selection());
        assert_eq!(q.selected_count(), 4);
        assert!(!q.is_checked(0));
        q.toggle(0);
        assert!(q.is_checked(0));
        q.deselect_all();
        assert!(q.scope().is_empty());
        q.clear_selection();
        assert_eq!(q.scope().len(), 5);
    }

    #[test]
    fn feature_tabs_count_and_filter() {
        let mut q = queue();
        let live = Tab::feature(Feature::Live);
        assert_eq!(q.tab_count(Base::All, &live), 1);
        assert_eq!(q.tab_count(Base::All, &Tab::feature(Feature::Hdr)), 1);
        assert_eq!(q.tab_count(Base::All, &Tab::feature(Feature::Burst)), 2);
        assert_eq!(
            q.tab_count(Base::All, &Tab::feature(Feature::MissingTime)),
            1
        );
        q.tab = Tab::feature(Feature::Burst);
        assert_eq!(q.visible(Base::All), vec![2, 3]);
        q.tab = Tab::all();
        q.set_search("  burst ");
        assert_eq!(q.visible(Base::All), vec![2, 3]);
        q.set_search("三星");
        assert_eq!(q.visible(Base::All), vec![2]);
        q.set_search("");
        q.set_source(Some(PathBuf::from("/p/b")));
        assert_eq!(q.visible(Base::All), vec![2, 3]);
    }

    #[test]
    fn filtering_does_not_change_selection() {
        let mut q = queue();
        q.tab = Tab::feature(Feature::Live);
        q.deselect_all();
        q.select_visible(Base::All);
        assert_eq!(q.selected_count(), 1);
        assert_eq!(q.hidden_selected(Base::All), 0);
        q.tab = Tab::feature(Feature::Hdr);
        assert_eq!(q.hidden_selected(Base::All), 1);
        assert_eq!(q.base_count(Base::Scope), 1);
    }

    #[test]
    fn selection_survives_rescan() {
        let mut q = queue();
        q.toggle(1);
        q.replace(rows(), Direction::ToApple);
        assert!(!q.is_checked(1));
        assert!(q.is_checked(0));
        assert_eq!(q.position(Path::new("/p/HDR_2.jpg")), Some(1));
    }

    #[test]
    fn result_tabs() {
        let mut q = queue();
        q.set_results(
            [PathBuf::from("/p/IMG_1.jpg"), PathBuf::from("/p/plain.jpg")].into(),
            &[
                json!({"source":"/p/IMG_1.jpg","status":"converted"}),
                json!({"source":"/p/plain.jpg","status":"copied","detail":"x"}),
            ],
        );
        assert_eq!(q.base_count(Base::Results), 2);
        assert_eq!(
            q.tab_count(Base::Results, &Tab::result(ResultState::Copied)),
            1
        );
        q.tab = Tab::result(ResultState::Converted);
        assert_eq!(q.visible(Base::Results), vec![0]);
    }
}
