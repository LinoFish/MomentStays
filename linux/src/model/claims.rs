//! Feature claims shown in the UI. They must match what the code does;
//! the tests below tie each claim to the behaviour it describes.
//!
//! Strings are gettext message IDs, marked with `N_` for extraction and
//! translated where they are shown.

/// Marks a string for translation without translating it (gettext `N_`).
#[allow(non_snake_case)]
pub const fn N_(s: &'static str) -> &'static str {
    s
}

pub const TO_APPLE_SUMMARY: &str = N_(
    "Android Motion Photos become Apple Live Photos. Supported static HDR photos are exported as stills.",
);
pub const TO_ANDROID_SUMMARY: &str = N_(
    "Apple Live Photos become Motion Photos for the chosen Android brand. JPEG for every brand; HEIC for Samsung only. Bursts are not converted in this direction.",
);
pub const BURST_CLAIM: &str = N_(
    "Bursts (experimental): Android → Apple only, JPEG only. Google, Samsung and known Huawei/Honor burst formats.",
);
pub const PRIVATE_HDR_TITLE: &str = N_("Try Converting Private HDR");
pub const PRIVATE_HDR_CLAIM: &str = N_(
    "Experimental and off by default. Only supported configurations are tried; results are limited and the original display effect is not guaranteed. Standard HDR is handled as usual.",
);
pub const SOURCES_ARE_READ_ONLY: &str =
    N_("Source files are only read. Results are written to the output folder; keep a backup of your originals.");
pub const NO_NATIVE_CODECS: &str = N_(
    "This build has no Linux codec backend: static HDR checks, burst checks and image re-encoding are unavailable.",
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::queue::{classify, Category};
    use crate::model::settings::{Direction, Vendor};
    use serde_json::json;

    #[test]
    fn burst_claim_matches_directions() {
        assert!(BURST_CLAIM.contains("Android → Apple only"));
        assert!(BURST_CLAIM.contains("experimental"));
        assert!(TO_ANDROID_SUMMARY.contains("Bursts are not converted"));
        let burst =
            json!({"source":"/a.jpg","burst":{"supported":true,"source_format":"google-burst"}});
        assert_eq!(classify(&burst, Direction::ToApple), Category::Burst);
        assert_eq!(classify(&burst, Direction::ToAndroid), Category::Attention);
    }

    #[test]
    fn heic_claim_matches_vendors() {
        assert!(TO_ANDROID_SUMMARY.contains("HEIC for Samsung only"));
        let heic: Vec<_> = Vendor::ALL.iter().filter(|v| v.supports_heic()).collect();
        assert_eq!(heic, vec![&Vendor::Samsung]);
    }

    #[test]
    fn private_hdr_claim_matches_default() {
        assert!(PRIVATE_HDR_CLAIM.contains("off by default"));
        assert!(!crate::model::settings::ExportSettings::default().private_hdr);
    }
}
