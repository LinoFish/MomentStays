//! Stable codes for the scan-row HDR labels. The labels stay user-facing
//! text; hosts branch on the code (`hdr_kind` in every scan row), so a
//! label can be reworded or translated without changing behaviour.

/// Every HDR label a scan row can carry, plus `Other` for any text this
/// table does not know (never a guess about HDR).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrKind {
    /// "HEIC（原样保留）": a HEIC still kept byte for byte.
    HeicPreserved,
    /// "Ultra HDR".
    UltraHdr,
    /// "Ultra HDR · 保留" (Google → Honor transfer).
    UltraHdrPreserved,
    /// "荣耀私有 HDR".
    HonorPrivate,
    /// "ISO HDR".
    IsoHdr,
    /// "旧 Apple HDR" (legacy JPEG gain map).
    AppleLegacy,
    /// "HEIC HDR" (static HEIC with an Apple gain map).
    HeicHdr,
    /// "OPPO UHDR（实验）".
    OppoUhdr,
    /// "Apple HDR" (Apple Live Photo still with a gain map, to Android).
    AppleHdr,
    /// "SDR / 其他", "SDR", "原始图像", "原始 JPEG".
    Sdr,
    /// "—": not inspected / not applicable.
    None,
    Other,
}

const TABLE: &[(&str, HdrKind)] = &[
    ("HEIC（原样保留）", HdrKind::HeicPreserved),
    ("Ultra HDR", HdrKind::UltraHdr),
    ("Ultra HDR · 保留", HdrKind::UltraHdrPreserved),
    ("荣耀私有 HDR", HdrKind::HonorPrivate),
    ("ISO HDR", HdrKind::IsoHdr),
    ("旧 Apple HDR", HdrKind::AppleLegacy),
    ("HEIC HDR", HdrKind::HeicHdr),
    ("OPPO UHDR（实验）", HdrKind::OppoUhdr),
    ("Apple HDR", HdrKind::AppleHdr),
    ("SDR / 其他", HdrKind::Sdr),
    ("SDR", HdrKind::Sdr),
    ("原始图像", HdrKind::Sdr),
    ("原始 JPEG", HdrKind::Sdr),
    ("—", HdrKind::None),
];

impl HdrKind {
    pub fn from_label(label: &str) -> HdrKind {
        TABLE
            .iter()
            .find(|(l, _)| *l == label)
            .map_or(HdrKind::Other, |(_, k)| *k)
    }

    /// Stable snake_case code (JSON `hdr_kind`).
    pub fn code(self) -> &'static str {
        match self {
            HdrKind::HeicPreserved => "heic_preserved",
            HdrKind::UltraHdr => "ultra_hdr",
            HdrKind::UltraHdrPreserved => "ultra_hdr_preserved",
            HdrKind::HonorPrivate => "honor_private",
            HdrKind::IsoHdr => "iso_hdr",
            HdrKind::AppleLegacy => "apple_legacy",
            HdrKind::HeicHdr => "heic_hdr",
            HdrKind::OppoUhdr => "oppo_uhdr",
            HdrKind::AppleHdr => "apple_hdr",
            HdrKind::Sdr => "sdr",
            HdrKind::None => "none",
            HdrKind::Other => "other",
        }
    }

    pub fn from_code(code: &str) -> Option<HdrKind> {
        ALL.iter().copied().find(|k| k.code() == code)
    }

    /// The labels `ScanRow.isStaticHDR` accepts (with `confirmed_static`).
    pub fn is_static_hdr(self) -> bool {
        matches!(
            self,
            HdrKind::UltraHdr
                | HdrKind::IsoHdr
                | HdrKind::AppleLegacy
                | HdrKind::HonorPrivate
                | HdrKind::HeicHdr
                | HdrKind::OppoUhdr
        )
    }

    /// The UI's `hdr.contains("HDR")` test, by code.
    pub fn mentions_hdr(self) -> bool {
        matches!(
            self,
            HdrKind::UltraHdr
                | HdrKind::UltraHdrPreserved
                | HdrKind::HonorPrivate
                | HdrKind::IsoHdr
                | HdrKind::AppleLegacy
                | HdrKind::HeicHdr
                | HdrKind::OppoUhdr
                | HdrKind::AppleHdr
        )
    }
}

pub const ALL: [HdrKind; 12] = [
    HdrKind::HeicPreserved,
    HdrKind::UltraHdr,
    HdrKind::UltraHdrPreserved,
    HdrKind::HonorPrivate,
    HdrKind::IsoHdr,
    HdrKind::AppleLegacy,
    HdrKind::HeicHdr,
    HdrKind::OppoUhdr,
    HdrKind::AppleHdr,
    HdrKind::Sdr,
    HdrKind::None,
    HdrKind::Other,
];

/// The table as JSON (`hdr-kinds` request), for host parity checks.
pub fn table_json() -> serde_json::Value {
    serde_json::Value::Array(
        TABLE
            .iter()
            .map(|(label, kind)| {
                serde_json::json!({"label": label, "code": kind.code(),
                    "static_hdr": kind.is_static_hdr(), "mentions_hdr": kind.mentions_hdr()})
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // The Swift sets these codes replace (BatchEngine.swift `isStaticHDR`,
    // WorkspaceModel `hdr.contains("HDR")`).
    const SWIFT_STATIC: [&str; 6] = [
        "Ultra HDR",
        "ISO HDR",
        "旧 Apple HDR",
        "荣耀私有 HDR",
        "HEIC HDR",
        "OPPO UHDR（实验）",
    ];

    #[test]
    fn codes_are_stable_unique_and_match_the_label_logic() {
        let codes: std::collections::HashSet<_> = ALL.iter().map(|k| k.code()).collect();
        assert_eq!(codes.len(), ALL.len());
        for k in ALL {
            assert_eq!(HdrKind::from_code(k.code()), Some(k));
        }
        for (label, kind) in TABLE {
            assert_eq!(HdrKind::from_label(label), *kind);
            assert_eq!(
                kind.is_static_hdr(),
                SWIFT_STATIC.contains(label),
                "{label}"
            );
            assert_eq!(kind.mentions_hdr(), label.contains("HDR"), "{label}");
        }
        assert_eq!(HdrKind::from_label("unknown"), HdrKind::Other);
        assert!(!HdrKind::Other.is_static_hdr());
        assert_eq!(HdrKind::HonorPrivate.code(), "honor_private");
    }

    #[test]
    fn every_classifier_label_has_a_code() {
        // Labels produced by scan::classify_hdr / static_hdr::inspect.
        for label in [
            "HEIC（原样保留）",
            "Ultra HDR",
            "荣耀私有 HDR",
            "ISO HDR",
            "旧 Apple HDR",
            "SDR / 其他",
            "HEIC HDR",
            "OPPO UHDR（实验）",
        ] {
            assert_ne!(HdrKind::from_label(label), HdrKind::Other, "{label}");
        }
    }
}
