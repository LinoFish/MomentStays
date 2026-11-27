//! Queue state of one photo in the macOS workspace: a port of
//! `PhotoQueueItem.initialState`, `isConvertible`, `ScanRow.isStaticHDR` and
//! `PhotoQueueItem.category(settings:)`. Pure function of the scan-row facts,
//! the item's result state and the settings that classify it, so the GNOME
//! app can share it (request mode `queue-state`).
use super::hdr_kind::HdrKind;
use serde_json::{json, Value};

/// Cover label of a motion item whose cover frame must be picked by hand.
pub const COVER_NEEDS_FRAME: &str = "未确定（需选帧）";
/// Cover label of a motion item without a recorded cover time.
pub const COVER_UNRECORDED: &str = "未记录";

/// Swift `PhotoState` raw values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ready,
    Manual,
    Inspect,
    Converted,
    Resumed,
    Copied,
    Skipped,
    Failed,
    Static,
}

impl State {
    pub const ALL: [State; 9] = [
        State::Ready,
        State::Manual,
        State::Inspect,
        State::Converted,
        State::Resumed,
        State::Copied,
        State::Skipped,
        State::Failed,
        State::Static,
    ];
    pub fn code(self) -> &'static str {
        match self {
            State::Ready => "ready",
            State::Manual => "manual",
            State::Inspect => "inspect",
            State::Converted => "converted",
            State::Resumed => "resumed",
            State::Copied => "copied",
            State::Skipped => "skipped",
            State::Failed => "failed",
            State::Static => "static",
        }
    }
    pub fn from_code(code: &str) -> Option<State> {
        State::ALL.iter().copied().find(|s| s.code() == code)
    }
}

/// Facts about one queue item.
#[derive(Clone, Debug, Default)]
pub struct Item {
    /// `hdr_kind` code; `Some(Other)` for a code this table does not know
    /// (Swift tests the code set whenever a code is present).
    pub hdr: Option<HdrKind>,
    /// Display label, used only when the row has no code.
    pub hdr_label: String,
    pub is_motion: bool,
    /// `Some(supported)` for a burst member, None otherwise.
    pub burst_supported: Option<bool>,
    pub confirmed_static: bool,
    /// Swift `PhotoQueueItem(_:allowsStaticHDR:)` (false for Photos-origin
    /// rows and for OPPO UHDR away from Apple).
    pub allows_static_hdr: bool,
    /// The item's result state (`resultState`), if it has one.
    pub result_state: Option<State>,
    /// Scan-row cover label.
    pub cover: String,
    /// `settings.manualCovers[id] != nil`.
    pub manual_cover: bool,
    /// `settings.coverIndex(for: source) != nil`.
    pub custom_cover: bool,
    /// `settings.manualMidpoints[id] == true`.
    pub manual_midpoint: bool,
}

/// The settings the category reads.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// "to-apple" | "to-android" | …
    pub direction: String,
    /// Swift `MissingTimePolicy` raw value: "error" | "midpoint".
    pub missing_time: String,
    /// Swift `ExportMode` raw value.
    pub export_mode: String,
}

/// The answer for one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub static_hdr: bool,
    pub convertible: bool,
    pub initial_state: State,
    pub category: State,
}

impl Decision {
    pub fn json(self) -> Value {
        json!({"static_hdr": self.static_hdr, "convertible": self.convertible,
            "initial_state": self.initial_state.code(), "category": self.category.code()})
    }
}

impl Item {
    /// Swift `ScanRow.isStaticHDR`.
    pub fn is_static_hdr(&self) -> bool {
        self.confirmed_static
            && self
                .hdr
                .unwrap_or_else(|| HdrKind::from_label(&self.hdr_label))
                .is_static_hdr()
    }

    pub fn from_json(v: &Value) -> Item {
        Item {
            hdr: v["hdr_kind"]
                .as_str()
                .map(|c| HdrKind::from_code(c).unwrap_or(HdrKind::Other)),
            hdr_label: v["hdr"].as_str().unwrap_or("").into(),
            is_motion: v["motion"].as_bool().unwrap_or(false),
            burst_supported: if v["burst"].is_object() {
                Some(v["burst"]["supported"].as_bool().unwrap_or(false))
            } else {
                v["burst_supported"].as_bool()
            },
            confirmed_static: v["static"].as_bool().unwrap_or(false),
            allows_static_hdr: v["allows_static_hdr"].as_bool().unwrap_or(true),
            result_state: v["result_state"].as_str().and_then(State::from_code),
            cover: v["cover"].as_str().unwrap_or("").into(),
            manual_cover: v["manual_cover"].as_bool().unwrap_or(false),
            custom_cover: v["custom_cover"].as_bool().unwrap_or(false),
            manual_midpoint: v["manual_midpoint"].as_bool().unwrap_or(false),
        }
    }
}

impl Settings {
    pub fn from_json(v: &Value) -> Settings {
        Settings {
            direction: v["direction"].as_str().unwrap_or("").into(),
            missing_time: v["missing_time"].as_str().unwrap_or("").into(),
            export_mode: v["export_mode"].as_str().unwrap_or("").into(),
        }
    }
}

/// Swift `isConvertible`.
pub fn convertible(item: &Item) -> bool {
    match item.burst_supported {
        Some(supported) => item.allows_static_hdr && supported,
        None => item.is_motion || (item.allows_static_hdr && item.is_static_hdr()),
    }
}

/// Swift `initialState`.
pub fn initial_state(item: &Item) -> State {
    match item.burst_supported {
        Some(supported) => {
            if item.allows_static_hdr && supported {
                State::Ready
            } else {
                State::Inspect
            }
        }
        None if item.is_motion || (item.allows_static_hdr && item.is_static_hdr()) => State::Ready,
        // `ScanRow.needsInspection` for a row without a burst.
        None if !item.is_motion && !item.confirmed_static => State::Inspect,
        None => State::Static,
    }
}

/// Swift `category(settings:)`: the queue scope the item is listed under.
pub fn category(item: &Item, s: &Settings) -> State {
    let to_apple = s.direction == "to-apple";
    if item.burst_supported.is_some() && !to_apple {
        return State::Inspect;
    }
    let state = item.result_state.unwrap_or_else(|| initial_state(item));
    match state {
        State::Converted | State::Resumed | State::Copied => return State::Converted,
        State::Static => return State::Static,
        State::Inspect | State::Failed => return State::Inspect,
        _ => {}
    }
    if item.cover == COVER_NEEDS_FRAME && to_apple && !item.manual_cover {
        return State::Manual;
    }
    if item.is_motion
        && item.cover == COVER_UNRECORDED
        && to_apple
        && s.missing_time == "error"
        && s.export_mode != "auxiliary-only"
        && !item.custom_cover
        && !item.manual_midpoint
    {
        return State::Manual;
    }
    State::Ready
}

pub fn decide(item: &Item, s: &Settings) -> Decision {
    Decision {
        static_hdr: item.is_static_hdr(),
        convertible: convertible(item),
        initial_state: initial_state(item),
        category: category(item, s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apple() -> Settings {
        Settings {
            direction: "to-apple".into(),
            missing_time: "error".into(),
            export_mode: "livephoto".into(),
        }
    }

    #[test]
    fn initial_state_and_convertible() {
        let motion = Item {
            is_motion: true,
            allows_static_hdr: true,
            ..Item::default()
        };
        assert_eq!(initial_state(&motion), State::Ready);
        assert!(convertible(&motion));
        let unknown = Item {
            allows_static_hdr: true,
            ..Item::default()
        };
        assert_eq!(initial_state(&unknown), State::Inspect);
        assert!(!convertible(&unknown));
        let still = Item {
            confirmed_static: true,
            hdr_label: "SDR".into(),
            allows_static_hdr: true,
            ..Item::default()
        };
        assert_eq!(initial_state(&still), State::Static);
        let ultra = Item {
            hdr_label: "Ultra HDR".into(),
            ..still.clone()
        };
        assert!(ultra.is_static_hdr() && convertible(&ultra));
        assert_eq!(initial_state(&ultra), State::Ready);
        let photos = Item {
            allows_static_hdr: false,
            ..ultra.clone()
        };
        assert!(!convertible(&photos));
        assert_eq!(initial_state(&photos), State::Static);
        // A present code wins over the label, even one the table does not know.
        let coded = Item {
            hdr: Some(HdrKind::Other),
            ..ultra.clone()
        };
        assert!(!coded.is_static_hdr());
        let burst = Item {
            burst_supported: Some(true),
            allows_static_hdr: true,
            ..Item::default()
        };
        assert_eq!(initial_state(&burst), State::Ready);
        assert!(convertible(&burst));
        let unsupported = Item {
            burst_supported: Some(false),
            is_motion: true,
            ..burst.clone()
        };
        assert_eq!(initial_state(&unsupported), State::Inspect);
        assert!(!convertible(&unsupported));
    }

    #[test]
    fn category_rules() {
        let s = apple();
        let motion = Item {
            is_motion: true,
            allows_static_hdr: true,
            cover: "0.4 秒".into(),
            ..Item::default()
        };
        assert_eq!(category(&motion, &s), State::Ready);
        let burst = Item {
            burst_supported: Some(true),
            ..motion.clone()
        };
        let android = Settings {
            direction: "to-android".into(),
            ..s.clone()
        };
        assert_eq!(category(&burst, &android), State::Inspect);
        for (result, expected) in [
            (State::Converted, State::Converted),
            (State::Resumed, State::Converted),
            (State::Copied, State::Converted),
            (State::Static, State::Static),
            (State::Failed, State::Inspect),
            (State::Skipped, State::Ready),
        ] {
            let item = Item {
                result_state: Some(result),
                ..motion.clone()
            };
            assert_eq!(category(&item, &s), expected, "{result:?}");
        }
        let frame = Item {
            cover: COVER_NEEDS_FRAME.into(),
            ..motion.clone()
        };
        assert_eq!(category(&frame, &s), State::Manual);
        assert_eq!(category(&frame, &android), State::Ready);
        let chosen = Item {
            manual_cover: true,
            ..frame
        };
        assert_eq!(category(&chosen, &s), State::Ready);
        let missing = Item {
            cover: COVER_UNRECORDED.into(),
            ..motion.clone()
        };
        assert_eq!(category(&missing, &s), State::Manual);
        let midpoint = Settings {
            missing_time: "midpoint".into(),
            ..s.clone()
        };
        assert_eq!(category(&missing, &midpoint), State::Ready);
        let auxiliary = Settings {
            export_mode: "auxiliary-only".into(),
            ..s.clone()
        };
        assert_eq!(category(&missing, &auxiliary), State::Ready);
        for item in [
            Item {
                custom_cover: true,
                ..missing.clone()
            },
            Item {
                manual_midpoint: true,
                ..missing.clone()
            },
        ] {
            assert_eq!(category(&item, &s), State::Ready);
        }
    }

    #[test]
    fn json_reads_the_swift_fields() {
        let item = Item::from_json(&json!({"hdr":"Ultra HDR","hdr_kind":"bogus","static":true,
            "burst":{"supported":true},"result_state":"copied","cover":"未记录"}));
        assert_eq!(item.hdr, Some(HdrKind::Other));
        assert_eq!(item.burst_supported, Some(true));
        assert!(item.allows_static_hdr);
        assert_eq!(item.result_state, Some(State::Copied));
        assert_eq!(
            decide(&item, &apple()).json(),
            json!({"static_hdr":false,"convertible":true,"initial_state":"ready","category":"converted"})
        );
    }
}
