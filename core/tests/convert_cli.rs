//! `momentstays-convert` CLI tests (ROADMAP item 11).
//!
//! Runs the real binary via `CARGO_BIN_EXE_momentstays-convert`.

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
    // Cargo names the variable after the binary itself, hyphen included,
    // and builds the binary for every integration test run. (A lookup by
    // `momentstays_convert` never matched, and the old path-guessing
    // fallback had no `.exe`, which is why these tests failed on Windows.)
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_momentstays-convert"))
}

fn fixture(name: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures-pycli")
            .join(name),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
}

fn real_mp4() -> Vec<u8> {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
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
    let out = run(&[
        "vendor",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--to",
        "samsung",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    let parsed = format::parse(&bytes).expect("output parses");
    let input_bytes = fixture("motion.jpg.b64");
    let src = format::parse(&input_bytes).expect("input parses");
    assert_eq!(parsed.video(&bytes), src.video(&input_bytes));
}

#[test]
fn vendor_google_to_vivo_keeps_time() {
    let input = tmp("in-google2.jpg");
    let output = tmp("out-vivo.jpg");
    std::fs::write(&input, fixture("motion.jpg.b64")).unwrap();
    let out = run(&[
        "vendor",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--to",
        "vivo",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    let parsed = format::parse(&bytes).expect("output parses");
    let input_bytes = fixture("motion.jpg.b64");
    let src = format::parse(&input_bytes).expect("input parses");
    assert_eq!(parsed.timestamp, src.timestamp);
}

#[test]
fn to_live_writes_identifier() {
    let input = tmp("in.mp4");
    let output = tmp("out.mov");
    std::fs::write(&input, real_mp4()).unwrap();
    let uuid = "123E4567-E89B-12D3-A456-426614174000";
    let out = run(&[
        "to-live",
        input.to_str().unwrap(),
        "--uuid",
        uuid,
        "-o",
        output.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(
        read_content_identifier(&bytes).unwrap(),
        Some(uuid.to_string())
    );
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
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(
        read_content_identifier(&bytes).unwrap(),
        Some(uuid.to_string())
    );
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
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(format::parse(&bytes).unwrap().timestamp, Some(800000));
    // Same pair, Huawei target: 800000us is frame 2 on the media timeline
    // (raw PTS 8192 @10240) but frame 4 on the presentation timeline (the
    // edit list starts at media 0.4 s). Auto refuses; media picks frame 2.
    let output_hw = tmp("out-motion-hw.jpg");
    let hw = |extra: &[&str]| {
        let mut args = vec![
            "to-motion",
            still.to_str().unwrap(),
            movie.to_str().unwrap(),
            "--to",
            "huawei",
            "--cover-time",
            "800000",
            "-o",
            output_hw.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        run(&args)
    };
    let refused = hw(&[]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("歧义"));
    let out = hw(&["--time-domain", "media"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
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
    let out = run(&[
        "vendor",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--to",
        "google",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    let parsed = format::parse(&bytes).expect("output parses");
    assert_eq!(parsed.format, "google-heic");
    assert_eq!(parsed.timestamp, Some(1_420_000));
    assert_eq!(parsed.video(&bytes), real_mp4().as_slice());
}

#[test]
fn vendor_google_heic_to_samsung_heic() {
    let still = build_still(true, true);
    let input_bytes = assemble_heic(&still, &real_mp4(), Vendor::Google, 1_420_000).unwrap();
    let input = tmp("in-google.heic");
    let output = tmp("out-samsung.heic");
    std::fs::write(&input, &input_bytes).unwrap();
    let out = run(&[
        "vendor",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--to",
        "samsung",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&output).unwrap();
    let parsed = format::parse(&bytes).expect("output parses");
    assert_eq!(parsed.format, "samsung-heic-sef");
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

fn scan_skeleton() -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    let seg = |marker: u8, payload: &[u8]| {
        let mut v = vec![0xFF, marker];
        v.extend(((payload.len() + 2) as u16).to_be_bytes());
        v.extend(payload);
        v
    };
    out.extend(seg(0xDB, &[0x00, 0x11]));
    out.extend(seg(0xC0, &[0x08, 0x00, 0x10, 0x00, 0x10, 0x01]));
    out.extend(seg(0xC4, &[0x00, 0x11]));
    out.extend(seg(0xDA, &[0x00, 0x11]));
    out.push(0x42);
    out.extend([0xFF, 0xD9]);
    out
}

#[test]
fn scan_dir_rows_match_swift_shape() {
    let dir = tmp("scan-dir");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a-motion.jpg"), fixture("motion.jpg.b64")).unwrap();
    std::fs::write(dir.join("b-static.jpg"), scan_skeleton()).unwrap();
    let out = run(&["scan", dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["relative"], "a-motion.jpg");
    assert_eq!(rows[0]["motion"], true);
    assert_eq!(rows[0]["hdr"], "SDR / 其他");
    assert_eq!(rows[0]["cover"], "0.400000 秒");
    assert_eq!(rows[0]["manufacturer"], "未知");
    assert_eq!(rows[1]["relative"], "b-static.jpg");
    assert_eq!(rows[1]["motion"], false);
    assert_eq!(rows[1]["confirmed_static"], true);
    assert_eq!(rows[1]["detail"], "静态照片，不含动态内容，不参与转换");
}

#[test]
fn scan_single_file_and_usage_error() {
    let input = tmp("in-scan.jpg");
    std::fs::write(&input, fixture("motion.jpg.b64")).unwrap();
    let out = run(&["scan", input.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["relative"], "in-scan.jpg");
    assert_eq!(rows[0]["motion"], true);
    let out = run(&["scan"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn vendor_output_matches_library_writer_bytes() {
    // The CLI streams head + video + tail; the bytes must equal the
    // in-memory `model::write` result for the same input.
    let input = tmp("in-google-bytes.jpg");
    let output = tmp("out-samsung-bytes.jpg");
    let data = fixture("motion.jpg.b64");
    std::fs::write(&input, &data).unwrap();
    let out = run(&[
        "vendor",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--to",
        "samsung",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed = format::parse(&data).unwrap();
    let model = momentstays_core::model::MotionModel::from_parsed(&parsed, &data).unwrap();
    let want = momentstays_core::model::write(&model, Vendor::Samsung).unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), want);
}

#[test]
fn vendor_refuses_to_overwrite_its_input() {
    let input = tmp("in-place.jpg");
    let data = fixture("motion.jpg.b64");
    std::fs::write(&input, &data).unwrap();
    let out = run(&[
        "vendor",
        input.to_str().unwrap(),
        input.to_str().unwrap(),
        "--to",
        "samsung",
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("输出路径不能与输入相同"));
    assert_eq!(std::fs::read(&input).unwrap(), data);
}

#[test]
fn failed_conversion_leaves_existing_output_untouched() {
    // A still photo is not a Motion Photo, so the conversion fails; the
    // pre-existing output must survive unchanged.
    let input = tmp("in-not-motion.jpg");
    let output = tmp("out-existing.jpg");
    std::fs::write(&input, fixture("static.jpg.b64")).unwrap();
    std::fs::write(&output, b"keep me").unwrap();
    let out = run(&[
        "vendor",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--to",
        "google",
    ]);
    assert!(!out.status.success());
    assert_eq!(std::fs::read(&output).unwrap(), b"keep me");
}

#[test]
fn inspect_walks_directories_like_scan() {
    // `momentstays-core inspect <dir>` used to read only the top level and
    // every file regardless of type; it now matches `scan`.
    let d = std::env::temp_dir().join(format!("ms-inspect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("sub").join("m.jpg"), fixture("motion.jpg.b64")).unwrap();
    std::fs::write(d.join("readme.txt"), b"not a photo").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_momentstays-core"))
        .args(["inspect", d.to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(lines[0].contains("\"ok\":true") && lines[0].contains("m.jpg"));
}
