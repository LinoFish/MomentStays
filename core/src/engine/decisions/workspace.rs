//! Per-item "will this be converted" decision of the macOS workspace:
//! a port of `WorkspaceModel.actionBlockingReason(_:)`, with the HDR label
//! tests done on stable codes (`HdrKind`). Pure function of item facts and
//! the chosen settings, so the GNOME app can share it.
use super::hdr_kind::HdrKind;
use serde_json::Value;

const HONOR_UNCHECKED: &str = "荣耀私有 HDR 未通过配置检查，不能转换";
const LIVE_COMBO: &str = "本轮实况附加输出选项与静态 HDR 的组合尚未支持；请调整实况选项或分批处理";
const JPEG_REBUILD: &str = "目标 JPEG 需要 HDR 重建，未选择 HDR 时不能安全保留；请调整格式或特性";
const BURST_HDR_ONLY: &str = "仅 HDR 尚不能安全保留原连拍封装；请启用连拍或原样输出";
const BURST_OFF: &str = "未选择连拍转换；原连拍资源保持不变";
const LIVE_HDR_ONLY: &str = "仅 HDR 尚不能安全保留此原动态封装；请启用实况或原样输出";
const LIVE_OFF: &str = "未选择实况转换；原动态资源保持不变";
const HDR_OFF: &str = "未选择 HDR 转换；原 HDR 资源保持不变";
const HONOR_UNVERIFIED: &str = "当前荣耀私有 HDR 不在已验证配置内，不能转换 HDR";
const COVER_HDR: &str = "自选封面不能保留未选择转换的 HDR；请恢复原封面或调整特性";

/// Facts about one queue item.
#[derive(Clone, Debug, Default)]
pub struct Item {
    pub hdr: Option<HdrKind>,
    /// Display label, used only when `hdr` is unknown (rows made by hosts).
    pub hdr_label: String,
    pub is_motion: bool,
    pub has_burst: bool,
    pub confirmed_static: bool,
    pub private_hdr_supported: Option<bool>,
    pub private_hdr_reason: Option<String>,
    /// Lower-case source file extension.
    pub extension: String,
    /// A user-chosen cover is set for this source.
    pub custom_cover: bool,
}

/// The settings the decision reads.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub legacy_hdr: bool,
    /// "to-apple" | "to-android" | …
    pub direction: String,
    /// "jpeg" | "heic" …
    pub container: String,
    pub burst_conversion: Option<bool>,
    pub live_conversion: Option<bool>,
    /// "livePhoto" | "auxiliaryOnly" | …
    pub export_mode: String,
    /// "primary" | …
    pub track: String,
    /// `availableLive`: the scope contains a non-burst motion item.
    pub available_live: bool,
}

impl Item {
    fn kind(&self) -> HdrKind {
        self.hdr
            .unwrap_or_else(|| HdrKind::from_label(&self.hdr_label))
    }
    fn mentions_hdr(&self) -> bool {
        match self.hdr {
            Some(k) => k.mentions_hdr(),
            None => self.hdr_label.contains("HDR"),
        }
    }
    fn is_static_hdr(&self) -> bool {
        self.confirmed_static && self.kind().is_static_hdr()
    }

    pub fn from_json(v: &Value) -> Item {
        Item {
            hdr: v["hdr_kind"].as_str().and_then(HdrKind::from_code),
            hdr_label: v["hdr"].as_str().unwrap_or("").into(),
            is_motion: v["motion"].as_bool().unwrap_or(false),
            has_burst: v["burst"].is_object() || v["has_burst"].as_bool() == Some(true),
            confirmed_static: v["static"].as_bool().unwrap_or(false),
            private_hdr_supported: v["private_hdr_supported"].as_bool(),
            private_hdr_reason: v["private_hdr_reason"].as_str().map(String::from),
            extension: v["extension"].as_str().unwrap_or("").to_lowercase(),
            custom_cover: v["custom_cover"].as_bool().unwrap_or(false),
        }
    }
}

impl Settings {
    pub fn from_json(v: &Value) -> Settings {
        Settings {
            legacy_hdr: v["legacy_hdr"].as_bool().unwrap_or(false),
            direction: v["direction"].as_str().unwrap_or("").into(),
            container: v["container"].as_str().unwrap_or("").into(),
            burst_conversion: v["burst_conversion"].as_bool(),
            live_conversion: v["live_conversion"].as_bool(),
            export_mode: v["export_mode"].as_str().unwrap_or("").into(),
            track: v["track"].as_str().unwrap_or("").into(),
            available_live: v["available_live"].as_bool().unwrap_or(false),
        }
    }
}

/// Why `item` will not be converted under `s`, or None. Same order and
/// texts as the Swift implementation.
pub fn blocking_reason(item: &Item, s: &Settings) -> Option<String> {
    let honor = item.kind() == HdrKind::HonorPrivate;
    let single = !item.is_motion && !item.has_burst;
    if honor && single && item.private_hdr_supported != Some(true) {
        return Some(
            item.private_hdr_reason
                .clone()
                .unwrap_or_else(|| HONOR_UNCHECKED.into()),
        );
    }
    if item.is_static_hdr()
        && single
        && s.available_live
        && s.live_conversion != Some(false)
        && (s.export_mode != "livePhoto" || s.track != "primary")
    {
        return Some(LIVE_COMBO.into());
    }
    if !s.legacy_hdr
        && s.direction == "to-android"
        && s.container == "jpeg"
        && ["heic", "heif"].contains(&item.extension.as_str())
        && item.mentions_hdr()
    {
        return Some(JPEG_REBUILD.into());
    }
    if item.has_burst {
        if s.burst_conversion == Some(false) {
            return Some(
                if s.legacy_hdr {
                    BURST_HDR_ONLY
                } else {
                    BURST_OFF
                }
                .into(),
            );
        }
    } else if item.is_motion {
        if s.live_conversion == Some(false) {
            return Some(
                if s.legacy_hdr {
                    LIVE_HDR_ONLY
                } else {
                    LIVE_OFF
                }
                .into(),
            );
        }
    } else if item.is_static_hdr() && !s.legacy_hdr {
        return Some(HDR_OFF.into());
    }
    if s.legacy_hdr && honor && item.private_hdr_supported != Some(true) && single {
        return Some(
            item.private_hdr_reason
                .clone()
                .unwrap_or_else(|| HONOR_UNVERIFIED.into()),
        );
    }
    if !s.legacy_hdr && item.mentions_hdr() && item.custom_cover {
        return Some(COVER_HDR.into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings {
            direction: "to-apple".into(),
            container: "jpeg".into(),
            export_mode: "livePhoto".into(),
            track: "primary".into(),
            ..Settings::default()
        }
    }

    #[test]
    fn honor_private_needs_verified_profile() {
        let mut item = Item {
            hdr: Some(HdrKind::HonorPrivate),
            confirmed_static: true,
            ..Item::default()
        };
        assert_eq!(
            blocking_reason(&item, &settings()).unwrap(),
            HONOR_UNCHECKED
        );
        item.private_hdr_reason = Some("配置不符".into());
        assert_eq!(blocking_reason(&item, &settings()).unwrap(), "配置不符");
        item.private_hdr_supported = Some(true);
        // Verified, but HDR conversion not chosen.
        assert_eq!(blocking_reason(&item, &settings()).unwrap(), HDR_OFF);
        let legacy = Settings {
            legacy_hdr: true,
            ..settings()
        };
        assert_eq!(blocking_reason(&item, &legacy), None);
        // Motion Honor items are decided by the live switch, not the profile.
        item.is_motion = true;
        item.private_hdr_supported = None;
        assert_eq!(blocking_reason(&item, &legacy), None);
    }

    #[test]
    fn switches_and_combinations() {
        let s = settings();
        let motion = Item {
            hdr: Some(HdrKind::Sdr),
            is_motion: true,
            ..Item::default()
        };
        let off = Settings {
            live_conversion: Some(false),
            ..s.clone()
        };
        assert_eq!(blocking_reason(&motion, &off).unwrap(), LIVE_OFF);
        let off_legacy = Settings {
            legacy_hdr: true,
            ..off.clone()
        };
        assert_eq!(
            blocking_reason(&motion, &off_legacy).unwrap(),
            LIVE_HDR_ONLY
        );
        assert_eq!(blocking_reason(&motion, &s), None);
        let burst = Item {
            has_burst: true,
            ..Item::default()
        };
        let no_burst = Settings {
            burst_conversion: Some(false),
            ..s.clone()
        };
        assert_eq!(blocking_reason(&burst, &no_burst).unwrap(), BURST_OFF);
        // Static HDR + live extra outputs in the same scope.
        let ultra = Item {
            hdr: Some(HdrKind::UltraHdr),
            confirmed_static: true,
            ..Item::default()
        };
        let combo = Settings {
            available_live: true,
            legacy_hdr: true,
            export_mode: "auxiliaryOnly".into(),
            ..s.clone()
        };
        assert_eq!(blocking_reason(&ultra, &combo).unwrap(), LIVE_COMBO);
        // HEIC HDR to Android JPEG without HDR conversion.
        let heic = Item {
            hdr: Some(HdrKind::HeicHdr),
            extension: "heic".into(),
            confirmed_static: true,
            ..Item::default()
        };
        let android = Settings {
            direction: "to-android".into(),
            ..s.clone()
        };
        assert_eq!(blocking_reason(&heic, &android).unwrap(), JPEG_REBUILD);
        // A custom cover cannot keep an unconverted HDR.
        let covered = Item {
            hdr: Some(HdrKind::UltraHdr),
            is_motion: true,
            custom_cover: true,
            ..Item::default()
        };
        assert_eq!(blocking_reason(&covered, &s).unwrap(), COVER_HDR);
    }

    #[test]
    fn label_fallback_matches_codes() {
        for (label, kind) in [
            ("荣耀私有 HDR", HdrKind::HonorPrivate),
            ("Ultra HDR", HdrKind::UltraHdr),
        ] {
            let by_label = Item {
                hdr_label: label.into(),
                confirmed_static: true,
                ..Item::default()
            };
            let by_code = Item {
                hdr: Some(kind),
                ..by_label.clone()
            };
            assert_eq!(
                blocking_reason(&by_label, &settings()),
                blocking_reason(&by_code, &settings())
            );
        }
    }
}
