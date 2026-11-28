//! Build-time identity of the GNOME app. Every place that needs the
//! application ID (GApplication, resources, desktop file, metainfo, icon,
//! Flatpak manifest) uses this value.

/// Reverse-DNS application ID (owner domain: linofish.com).
pub const APP_ID: &str = "com.linofish.MomentStays";

/// GResource base path derived from [`APP_ID`]; libadwaita loads
/// `style.css` / `style-dark.css` from here automatically.
pub const RESOURCE_BASE: &str = "/com/linofish/MomentStays";

/// gettext domain; the `.mo` files are installed as `momentstays.mo`.
pub const GETTEXT_PACKAGE: &str = "momentstays";

/// Where the translations are installed. Flatpak builds use `/app`;
/// `MOMENTSTAYS_LOCALEDIR` at build time overrides it for local installs.
pub const LOCALEDIR: &str = match option_env!("MOMENTSTAYS_LOCALEDIR") {
    Some(dir) => dir,
    None => "/app/share/locale",
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether this build carries the Linux system codec backend
/// (`linux-native`, a default feature). Without it, codec steps (pixel checks,
/// re-encoding, video frames) are refused by the core.
pub const NATIVE_CODECS: bool = cfg!(feature = "linux-native");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_base_matches_app_id() {
        let expected = format!("/{}", APP_ID.replace('.', "/"));
        assert_eq!(RESOURCE_BASE, expected);
    }
}
