//! Link the Linux codec libraries for `--features linux-native`.
//! Default builds (and every non-Linux target) do nothing here.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "linux-native")]
    linux_native();
}

#[cfg(feature = "linux-native")]
fn linux_native() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    // Same libraries the GNOME 50 runtime (freedesktop-sdk 25.08) ships, all
    // dynamically linked. pkg-config only checks versions and supplies the
    // search paths; the `-l` flags are emitted here so nothing beyond the
    // three libraries is linked (Debian's lcms2.pc also lists the
    // GPL-3.0 fast_float/threaded plugins, which this crate never uses).
    for (name, min, lib) in [
        ("libturbojpeg", "2.0", "turbojpeg"),
        ("lcms2", "2.9", "lcms2"),
        ("libheif", "1.16", "heif"),
    ] {
        match pkg_config::Config::new()
            .atleast_version(min)
            .cargo_metadata(false)
            .probe(name)
        {
            Ok(found) => {
                for dir in found.link_paths {
                    println!("cargo:rustc-link-search=native={}", dir.display());
                }
                println!("cargo:rustc-link-lib=dylib={lib}");
            }
            Err(e) => panic!("linux-native needs {name} >= {min} (pkg-config): {e}"),
        }
    }
}
