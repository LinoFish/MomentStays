//! Feature claims shown in the UI. They must match what the code does;
//! the tests below tie each claim to the behaviour it describes.
//!
//! Strings are gettext message IDs (English, the macOS app's wording),
//! marked with `N_` for extraction and translated where they are shown.

/// Marks a string for translation without translating it (gettext `N_`).
#[allow(non_snake_case)]
pub const fn N_(s: &'static str) -> &'static str {
    s
}

/// Step 1 card: Android → Apple.
pub const APPLE_ROUTE: &str = N_(
    "Motion photos; local static HDR; HONOR/Huawei JPEG bursts. Google/Samsung bursts are experimental. Private HDR supports only specific profiles.",
);
/// Step 1 cards: Apple → Android vendors (after the format list).
pub const ANDROID_ROUTE_TAIL: &str =
    N_("; local static HDR supports verified types only; burst conversion is unsupported.");
/// Step 1 note on the experimental private HDR route.
pub const ROUTES_NOTE: &str = N_(
    "Only the verified routes below are supported. Available Live Photo, HDR, and burst actions depend on inspection and preview. Private HDR is experimental and off by default. Arbitrary Android-to-Android transfers are unsupported.",
);
pub const READ_ONLY_NOTE: &str = N_(
    "Conversion changes the format or metadata of output copies. Keep source photos archived and backed up. Source files remain read-only.",
);
pub const NO_NATIVE_CODECS: &str = N_(
    "This build has no Linux codec backend: static HDR checks, burst checks and image re-encoding are unavailable.",
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::queue::{classify, Category};
    use crate::model::settings::{Direction, ExportSettings, Vendor};
    use serde_json::json;

    #[test]
    fn burst_claims_match_directions() {
        assert!(APPLE_ROUTE.contains("bursts"));
        assert!(ANDROID_ROUTE_TAIL.contains("burst conversion is unsupported"));
        let burst =
            json!({"source":"/a.jpg","burst":{"supported":true,"source_format":"google-burst"}});
        assert_eq!(classify(&burst, Direction::ToApple), Category::Burst);
        assert_eq!(classify(&burst, Direction::ToAndroid), Category::Attention);
    }

    #[test]
    fn heic_only_where_the_card_says_so() {
        // The vendor cards list "JPEG / HEIC" only for vendors with a HEIC writer.
        let heic: Vec<_> = Vendor::ALL.iter().filter(|v| v.supports_heic()).collect();
        assert_eq!(heic, vec![&Vendor::Samsung]);
    }

    #[test]
    fn private_hdr_claim_matches_default() {
        assert!(ROUTES_NOTE.contains("off by default"));
        assert!(!ExportSettings::default().legacy_hdr);
        assert!(READ_ONLY_NOTE.contains("read-only"));
    }
}
