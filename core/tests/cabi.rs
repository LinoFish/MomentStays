//! C ABI smoke test (ROADMAP item 10): compile `cabi_smoke.c` against
//! the committed header, link the release staticlib, and run it.
//!
//! The staticlib is a declared crate-type (`crate-type = ["rlib",
//! "staticlib"]`), so a plain `cargo build` / `cargo test` produces it
//! next to the rlib; this test fails clearly if it is absent instead of
//! silently skipping, because the C ABI is a shipped surface.

use base64::Engine;
use std::process::Command;

fn manifest() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn target_dir() -> std::path::PathBuf {
    if let Ok(t) = std::env::var("CARGO_TARGET_DIR") {
        return std::path::PathBuf::from(t);
    }
    manifest().join("target")
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ms-cabi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// Newest mtime among the lib sources (recursively under src/ plus
/// Cargo.toml, which feeds the version string).
fn newest_source_mtime(dir: &std::path::Path) -> std::time::SystemTime {
    let mut newest = std::time::SystemTime::UNIX_EPOCH;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(t) = e.metadata() else { continue };
            if t.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                if let Ok(m) = t.modified() {
                    newest = newest.max(m);
                }
            }
        }
    }
    newest
}

/// The release staticlib, built on demand when missing or older than
/// the lib sources (`cargo test` only builds the rlib). The nested build
/// is bounded: if it cannot finish in time (e.g. a target-dir lock),
/// the test fails with a clear message instead of hanging.
fn ensure_staticlib() -> std::path::PathBuf {
    let lib = target_dir().join("release").join("libmomentstays_core.a");
    let stale = match std::fs::metadata(&lib) {
        Err(_) => true,
        Ok(m) => match m.modified() {
            Err(_) => true,
            Ok(t) => {
                let src = newest_source_mtime(&manifest().join("src")).max(
                    std::fs::metadata(manifest().join("Cargo.toml"))
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                );
                src > t
            }
        },
    };
    if stale {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let mut child = Command::new(cargo)
            .arg("build")
            .arg("--release")
            .arg("--lib")
            .arg("--manifest-path")
            .arg(manifest().join("Cargo.toml"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("failed to run cargo build for the staticlib");
        let mut waited = 0;
        let status = loop {
            match child.try_wait().expect("cargo build wait failed") {
                Some(s) => break s,
                None => {
                    if waited >= 2400 {
                        let _ = child.kill();
                        panic!("cargo build --release --lib timed out; run it manually, then rerun the test");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    waited += 1;
                }
            }
        };
        assert!(
            status.success() && lib.exists(),
            "cargo build --release --lib failed; run it manually, then rerun the test"
        );
    }
    lib
}

fn build_smoke() -> std::path::PathBuf {
    let lib = ensure_staticlib();
    let out = tmp("cabi_smoke");
    let mut cmd = Command::new("cc");
    cmd.arg("-O2")
        .arg("-I")
        .arg(manifest().join("include"))
        .arg(manifest().join("tests").join("cabi_smoke.c"))
        .arg(&lib)
        .arg("-o")
        .arg(&out);
    if cfg!(target_os = "linux") {
        cmd.args(["-lpthread", "-ldl", "-lm"]);
    }
    let res = cmd.output().unwrap();
    assert!(
        res.status.success(),
        "cc failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    out
}

fn fixture(name: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(manifest().join("tests/fixtures-pycli").join(name)).unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

// Keep missing Windows coverage visible as ignored, not as a passing test.
// This harness expects a Unix C compiler and .a archive; an MSVC/MinGW
// linker harness still needs separate verification. The production Swift
// ms_core_* ABI has its own ownership tests and is distinct from this C API.
#[test]
#[cfg_attr(target_os = "windows", ignore = "Windows C linker harness not yet verified")]
fn c_abi_smoke() {
    let exe = build_smoke();
    // Version round-trip.
    let v = Command::new(&exe).arg("--version").output().unwrap();
    assert!(v.status.success(), "{}", String::from_utf8_lossy(&v.stderr));
    assert_eq!(String::from_utf8_lossy(&v.stdout).trim(), env!("CARGO_PKG_VERSION"));
    // Real Motion Photo fixture: JSON ok, format present, video present.
    let input = tmp("in.jpg");
    std::fs::write(&input, fixture("motion.jpg.b64")).unwrap();
    let out = Command::new(&exe).arg(&input).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("C program prints JSON");
    assert_eq!(json["ok"], true);
    assert!(json["format"].as_str().is_some_and(|f| !f.is_empty()));
    assert!(json["video_bytes"].as_u64().is_some_and(|n| n > 0));
    // Invalid bytes are JSON `ok: false`, not a crash.
    let bad = tmp("bad.bin");
    std::fs::write(&bad, b"not a photo at all").unwrap();
    let out = Command::new(&exe).arg(&bad).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("C program prints JSON");
    assert_eq!(json["ok"], false);
}
