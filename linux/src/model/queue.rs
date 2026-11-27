//! The photo queue: scan rows from the core, classified for the grid,
//! with the user's include choices, a category filter and search.

use super::settings::Direction;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Static HDR labels the core treats as convertible stills
/// (`ScanRow::is_static_hdr`). TODO(PR #15): prefer the stable
/// `hdr_kind` code once it is in every scan row on main.
const STATIC_HDR_LABELS: [&str; 6] = [
    "Ultra HDR",
    "ISO HDR",
    "旧 Apple HDR",
    "荣耀私有 HDR",
    "HEIC HDR",
    "OPPO UHDR（实验）",
];

/// What the grid shows a photo as, from the core's scan facts.
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

/// Burst family the core reports (`burst.source_format`; absent = Honor).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurstFormat {
    Google,
    Samsung,
    Huawei,
    Honor,
}

impl BurstFormat {
    fn from_row(burst: &Value) -> Self {
        match burst["source_format"].as_str() {
            Some("google-burst") => BurstFormat::Google,
            Some("samsung-burst") => BurstFormat::Samsung,
            Some("huawei-burst") => BurstFormat::Huawei,
            _ => BurstFormat::Honor,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BurstFacts {
    pub format: BurstFormat,
    pub group: String,
    pub index: Option<u64>,
    pub supported: bool,
    pub source_cover: bool,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct PhotoItem {
    /// The full scan row (passed back to the core's batch unchanged).
    pub row: Value,
    pub source: PathBuf,
    pub name: String,
    pub relative: String,
    pub category: Category,
    pub hdr: String,
    pub cover: String,
    pub manufacturer: String,
    pub manufacturer_detail: String,
    /// The core's explanation for this photo (Chinese, as the core writes it).
    pub detail: String,
    pub burst: Option<BurstFacts>,
    pub included: bool,
}

fn text(row: &Value, key: &str) -> String {
    row[key].as_str().unwrap_or("").to_string()
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
    let hdr = row["hdr"].as_str().unwrap_or("");
    if row["static"] == true && STATIC_HDR_LABELS.contains(&hdr) {
        return Category::Hdr;
    }
    Category::Attention
}

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
                format: BurstFormat::from_row(b),
                group: b["source_group"]
                    .as_str()
                    .or(b["group_id"].as_str())
                    .unwrap_or("")
                    .to_string(),
                index: b["index"].as_u64(),
                supported: b["supported"] == true,
                source_cover: b["source_cover"] == true,
                reason: b["reason"].as_str().unwrap_or("").to_string(),
            }
        });
        Some(PhotoItem {
            relative: text(&row, "relative"),
            hdr: text(&row, "hdr"),
            cover: text(&row, "cover"),
            manufacturer: text(&row, "manufacturer"),
            manufacturer_detail: text(&row, "manufacturer_detail"),
            detail: text(&row, "detail"),
            included: category.convertible(),
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Only(Category),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub all: usize,
    pub live: usize,
    pub hdr: usize,
    pub burst: usize,
    pub attention: usize,
    pub included: usize,
}

impl Counts {
    pub fn of(&self, filter: Filter) -> usize {
        match filter {
            Filter::All => self.all,
            Filter::Only(Category::Live) => self.live,
            Filter::Only(Category::Hdr) => self.hdr,
            Filter::Only(Category::Burst) => self.burst,
            Filter::Only(Category::Attention) => self.attention,
        }
    }
}

#[derive(Default)]
pub struct Queue {
    items: Vec<PhotoItem>,
    filter: Filter,
    search: String,
    /// Include choices the user made, kept across rescans by source path.
    choices: HashMap<PathBuf, bool>,
}

impl Queue {
    /// Replace the rows after a scan, keeping the user's include choices
    /// for photos that are still convertible.
    pub fn replace(&mut self, rows: Vec<Value>, direction: Direction) {
        self.items = rows
            .into_iter()
            .filter_map(|r| PhotoItem::from_row(r, direction))
            .collect();
        for item in &mut self.items {
            if item.category.convertible() {
                if let Some(choice) = self.choices.get(&item.source) {
                    item.included = *choice;
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
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

    pub fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
    }
    pub fn set_search(&mut self, search: &str) {
        self.search = search.trim().to_lowercase();
    }

    /// Indices into [`Queue::items`] that pass the filter and search.
    pub fn visible(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| match self.filter {
                Filter::All => true,
                Filter::Only(c) => item.category == c,
            })
            .filter(|(_, item)| item.matches(&self.search))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts {
            all: self.items.len(),
            ..Counts::default()
        };
        for item in &self.items {
            match item.category {
                Category::Live => c.live += 1,
                Category::Hdr => c.hdr += 1,
                Category::Burst => c.burst += 1,
                Category::Attention => c.attention += 1,
            }
            if item.included {
                c.included += 1;
            }
        }
        c
    }

    /// Include or exclude one photo. Photos that cannot be converted stay
    /// excluded; returns whether the state changed.
    pub fn set_included(&mut self, index: usize, included: bool) -> bool {
        let Some(item) = self.items.get_mut(index) else {
            return false;
        };
        if !item.category.convertible() || item.included == included {
            return false;
        }
        item.included = included;
        self.choices.insert(item.source.clone(), included);
        true
    }

    /// Include or exclude every visible convertible photo.
    pub fn set_visible_included(&mut self, included: bool) {
        for i in self.visible() {
            self.set_included(i, included);
        }
    }

    /// Sources and scan rows of the included photos, for `batch`.
    pub fn selection(&self) -> (Vec<PathBuf>, Vec<Value>) {
        self.items
            .iter()
            .filter(|i| i.included)
            .map(|i| (i.source.clone(), i.row.clone()))
            .unzip()
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

    fn rows() -> Vec<Value> {
        vec![
            json!({"source":"/p/IMG_1.jpg","relative":"IMG_1.jpg","motion":true,"hdr":"SDR","manufacturer":"Google","detail":"ok","static":false,"burst":null}),
            json!({"source":"/p/HDR_2.jpg","relative":"HDR_2.jpg","motion":false,"hdr":"Ultra HDR","manufacturer":"Google","static":true,"burst":null}),
            json!({"source":"/p/BURST001.jpg","relative":"b/BURST001.jpg","motion":false,"hdr":"SDR","manufacturer":"三星","static":true,
                   "burst":{"supported":true,"source_format":"samsung-burst","group_id":"g1","index":1,"source_cover":true}}),
            json!({"source":"/p/BURST002.jpg","relative":"b/BURST002.jpg","motion":false,"hdr":"SDR","manufacturer":"荣耀","static":true,
                   "burst":{"supported":false,"reason":"帧序号重复","group_id":"g2","index":2}}),
            json!({"source":"/p/plain.jpg","relative":"plain.jpg","motion":false,"hdr":"SDR","manufacturer":"未知","static":true,"burst":null}),
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
        assert_eq!(burst.format, BurstFormat::Samsung);
        assert!(burst.source_cover);
        assert_eq!(
            q.get(3).and_then(|i| i.burst.as_ref()).map(|b| b.format),
            Some(BurstFormat::Honor)
        );
    }

    #[test]
    fn bursts_are_not_convertible_towards_android() {
        let row = &rows()[2];
        assert_eq!(classify(row, Direction::ToAndroid), Category::Attention);
        assert_eq!(classify(row, Direction::ToApple), Category::Burst);
    }

    #[test]
    fn counts_and_default_inclusion() {
        let c = queue().counts();
        assert_eq!(
            (c.all, c.live, c.hdr, c.burst, c.attention),
            (5, 1, 1, 1, 2)
        );
        assert_eq!(c.included, 3);
        assert_eq!(c.of(Filter::Only(Category::Attention)), 2);
    }

    #[test]
    fn filter_and_search() {
        let mut q = queue();
        q.set_filter(Filter::Only(Category::Burst));
        assert_eq!(q.visible(), vec![2]);
        q.set_filter(Filter::All);
        q.set_search("  burst ");
        assert_eq!(q.visible(), vec![2, 3]);
        q.set_search("三星");
        assert_eq!(q.visible(), vec![2]);
        q.set_search("B/");
        assert_eq!(q.visible(), vec![2, 3]);
        q.set_search("");
        assert_eq!(q.visible().len(), 5);
    }

    #[test]
    fn attention_items_cannot_be_included() {
        let mut q = queue();
        assert!(!q.set_included(4, true));
        assert!(!q.get(4).map(|i| i.included).unwrap_or(true));
        assert!(q.set_included(0, false));
        assert!(!q.set_included(0, false));
        assert!(!q.set_included(99, true));
    }

    #[test]
    fn choices_survive_rescan() {
        let mut q = queue();
        q.set_included(1, false);
        q.replace(rows(), Direction::ToApple);
        assert!(!q.get(1).map(|i| i.included).unwrap_or(true));
        assert!(q.get(0).map(|i| i.included).unwrap_or(false));
        assert_eq!(q.position(Path::new("/p/HDR_2.jpg")), Some(1));
    }

    #[test]
    fn selection_returns_included_rows_only() {
        let mut q = queue();
        q.set_filter(Filter::Only(Category::Live));
        q.set_visible_included(false);
        let (sources, rows) = q.selection();
        assert_eq!(
            sources,
            vec![
                PathBuf::from("/p/HDR_2.jpg"),
                PathBuf::from("/p/BURST001.jpg")
            ]
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["source"], "/p/HDR_2.jpg");
    }
}
