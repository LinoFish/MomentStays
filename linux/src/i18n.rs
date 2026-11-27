//! gettext for the UI. Lookups go through GLib (`g_dgettext`), and the
//! domain is bound with the C library's `bindtextdomain`, both provided by
//! the runtime and linked dynamically. No gettext implementation is
//! bundled or statically linked (see AGENTS.md, dependency rules).
//!
//! Message IDs are English; catalogs live in `linux/po` (zh_CN = zh-Hans,
//! zh_TW = zh-Hant). Extract with
//! `xgettext --language=C --from-code=UTF-8 -k -kgettext -kngettext:1,2`.

use crate::config::{GETTEXT_PACKAGE, LOCALEDIR};
use std::ffi::CString;
use std::os::raw::c_char;

extern "C" {
    fn bindtextdomain(domain: *const c_char, dir: *const c_char) -> *mut c_char;
    fn bind_textdomain_codeset(domain: *const c_char, codeset: *const c_char) -> *mut c_char;
}

/// Bind the app's domain. GTK sets the locale (`setlocale`) when it starts.
pub fn init() {
    let (Ok(domain), Ok(dir), Ok(utf8)) = (
        CString::new(GETTEXT_PACKAGE),
        CString::new(LOCALEDIR),
        CString::new("UTF-8"),
    ) else {
        return;
    };
    // SAFETY: valid NUL-terminated strings; glibc copies them.
    unsafe {
        bindtextdomain(domain.as_ptr(), dir.as_ptr());
        bind_textdomain_codeset(domain.as_ptr(), utf8.as_ptr());
    }
}

/// Translate `msgid` in the app's domain.
pub fn gettext(msgid: &str) -> String {
    gtk::glib::dgettext(Some(GETTEXT_PACKAGE), msgid).into()
}

/// Plural form of [`gettext`].
pub fn ngettext(singular: &str, plural: &str, n: u32) -> String {
    gtk::glib::dngettext(Some(GETTEXT_PACKAGE), singular, plural, n as _).into()
}

/// Replace `{name}` placeholders, so translators can reorder them.
pub fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in args {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fill;

    #[test]
    fn fill_replaces_named_placeholders() {
        assert_eq!(
            fill("{done} of {total}", &[("done", "3"), ("total", "9")]),
            "3 of 9"
        );
        assert_eq!(fill("{x}{x}", &[("x", "a")]), "aa");
        assert_eq!(fill("no args", &[]), "no args");
    }
}
