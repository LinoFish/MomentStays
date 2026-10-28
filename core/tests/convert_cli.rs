//! `momentstays-convert` CLI tests (ROADMAP item 11).
//!
//! Runs the real binary via CARGO_BIN_EXE_momentstays-convert. The existing
//! `momentstays` binary's Python-compatibility tests are untouched.

mod common;

use base64::Engine;
use common::build_still;
use momentstays_core::format;
use momentstays_core::heifwrite::assemble_heic;
use momentstays_core::live2motion::mett_cover_time;
use momentstays_core::model::Vendor;
use momentstays_core::remux::{read_content_identifier, remux_to_mov};
use std::process::{Command, Output};

fn bin() -> std::path::PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_momentstays-convert") {
        return std::path::PathBuf::from(p);
    }
    // Fallback for single-target runs (`cargo test --test convert_cli`),
    // where Cargo may not build the binary: look in likely target dirs.
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = Vec::new();
    if let Ok(t) = std::env::var("CARGO_TARGET_DIR") {
        candidates.push(std::path::PathBuf::from(t).join("release").join("momentstays-convert"));
    }
    candidates.push(manifest.join("target").join("release").join("momentstays-convert"));
    candidates.push(manifest.join("..").join("target").join("release").join("momentstays-convert"));
    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    panic!("momentstays-convert binary not built; run `cargo build --release --bin momentstays-convert` first");
}

fn fixture(name: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-pycli").join(name),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

fn real_mp4() -> Vec<u8> {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ms-convert-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn vendor_google_to_samsung() {
    let input = tmp("in-google.jpg");
    let output = tmp("out-samsung.jpg");
    std::fs::write(&input, fixture("motion.jpg.b64")).unwrap();
    let out = run(&["vendor", input.to_str().unwrap(), output.to_str().unwrap(), "--to", "samsung"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let bytes = std::fs::read(&output).unwrap();
    let parsed = format::parse(&bytes).expect("output parses");
    let src = format::parse(&fixture("motion.jpg.b64")).expect("input parses");
    assert_eq!(parsed.video(&bytes), src.video(&fixture("motion.jpg.b64")));
}

#[test]
fn vendor_google_to_vivo_keeps_time() {
    let input = tmp("in-google2.jpg");
    let output = tmp("out-vivo.jpg");
    std::fs::write(&input, fixture("motion.jpg.b64")).unwrap();
    let out = run(&["vendor", input.to_str().unwrap(), output.to_str().unwrap(), "--to", "vivo"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let bytes = std::fs::read(&output).unwrap();
    let parsed = format::parse(&bytes).expect("output parses");
    let src = format::parse(&fixture("motion.jpg.b64")).expect("input parses");
    assert_eq!(parsed.timestamp, src.timestamp);
}

#[test]
fn to_live_writes_identifier() {
    let input = tmp("in.mp4");
    let output = tmp("out.mov");
    std::fs::write(&input, real_mp4()).unwrap();
    let uuid = "123E4567-E89B-12D3-A456-426614174000";
    let out = run(&["to-live", input.to_str().unwrap(), "--uuid", uuid, "-o", output.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(read_content_identifier(&bytes).unwrap(), Some(uuid.to_string()));
    assert!(String::from_utf8_lossy(&out.stderr).contains("still-image-time"));
}

#[test]
fn to_live_with_cover_time_writes_mett() {
    let input = tmp("in-mett.mp4");
    let output = tmp("out-mett.mov");
    std::fs::write(&input, real_mp4()).unwrap();
    let uuid = "123E4567-E89B-12D3-A456-426614174000";
    let out = run(&[
        "to-live",
        input.to_str().unwrap(),
        "--uuid",
        uuid,
        "--cover-time-us",
        "1420000",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(read_content_identifier(&bytes).unwrap(), Some(uuid.to_string()));
    assert_eq!(mett_cover_time(&bytes).unwrap(), Some(1_420_000));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("尚不是完整的"));
}

#[test]
fn to_motion_google_and_huawei_frame() {
    let still = tmp("still.jpg");
    let movie = tmp("movie.mov");
    std::fs::write(&still, fixture("static.jpg.b64")).unwrap();
    let mov = remux_to_mov(&real_mp4(), 0).unwrap();
    std::fs::write(&movie, &mov).unwrap();
    let output = tmp("out-motion.jpg");
    let out = run(&[
        "to-motion",
        still.to_str().unwrap(),
        movie.to_str().unwrap(),
        "--to",
        "google",
        "--cover-time",
        "800000",
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(format::parse(&bytes).unwrap().timestamp, Some(800000));
    // Same pair, Huawei target: time 800000us is exactly frame 2 of the
    // real file (presentation 800000us @10240), so conversion succeeds.
    let output_hw = tmp("out-motion-hw.jpg");
    let out = run(&[
        "to-motion",
        still.to_str().unwrap(),
        movie.to_str().unwrap(),
        "--to",
        "huawei",
        "--cover-time",
        "800000",
        "-o",
        output_hw.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let bytes = std::fs::read(&output_hw).unwrap();
    assert_eq!(format::parse(&bytes).unwrap().cover_frame, Some(2));
}

#[test]
fn vendor_samsung_heic_to_google_heic() {
    let still = build_still(true, true);
    let input_bytes = assemble_heic(&still, &real_mp4(), Vendor::Samsung, 1_420_000).unwrap();
    let input = tmp("in-samsung.heic");
    let output = tmp("out-google.heic");
    std::fs::write(&input, &input_bytes).unwrap();
    let out = run(&["vendor", input.to_str().unwrap(), output.to_str().unwrap(), "--to", "google"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let bytes = std::fs::read(&output).unwrap();
    let parsed = format::parse(&bytes).expect("output parses");
    assert_eq!(parsed.format, "google-heic");
    assert_eq!(parsed.timestamp, Some(1_420_000));
    assert_eq!(parsed.video(&bytes), real_mp4().as_slice());
}

#[test]
fn bad_usage_exits_2() {
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(2));
    let out = run(&["vendor", "only-one-arg"]);
    assert_eq!(out.status.code(), Some(2));
}
