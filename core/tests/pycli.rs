//! Parity tests against outputs of the former Python reference (`motionlive`).
//!
//! Fixtures and expected outputs in `fixtures-pycli/` were generated from the
//! Python reference implementation (`motionlive`) by /tmp/gen_pycli.py; the
//! expected scan JSON came from running the Python CLI itself. These tests
//! need no Python at run time.

use base64::Engine;
use momentstays_core::{format, pyjpeg, pymotion};
use std::fs;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-pycli")
}

/// Fixtures are stored base64-encoded (`.b64` suffix) so they can be pushed
/// through the text-only file API; decode on read.
fn read(name: &str) -> Vec<u8> {
    let text = fs::read_to_string(dir().join(format!("{name}.b64")))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"));
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

#[test]
fn parse_values_match_python() {
    let cases: &[(&str, Option<i64>, Option<&str>, &str)] = &[
        (
            "motion.jpg",
            Some(400000),
            Some("MotionPhotoPresentationTimestampUs"),
            "legacy-or-sdr",
        ),
        (
            "motion-element.jpg",
            Some(400000),
            Some("MotionPhotoPresentationTimestampUs"),
            "legacy-or-sdr",
        ),
        (
            "motion-legacy.jpg",
            Some(1234567),
            Some("MicroVideoPresentationTimestampUs"),
            "legacy-or-sdr",
        ),
        ("motion-notime.jpg", None, None, "legacy-or-sdr"),
        (
            "ultra.jpg",
            Some(400000),
            Some("MotionPhotoPresentationTimestampUs"),
            "ultra-hdr",
        ),
    ];
    // `format::parse` (the only parser now) gives the Python reference's
    // values on these files.
    for (name, ts, tag, hdr) in cases {
        let data = read(name);
        let p = format::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(p.timestamp, *ts, "{name} timestamp");
        assert_eq!(p.timestamp_tag.as_deref(), *tag, "{name} tag");
        assert_eq!(p.ultra, *hdr == "ultra-hdr", "{name} hdr");
    }
    assert!(format::parse(&read("static.jpg")).is_err());
}

#[test]
fn live_still_bytes_match_python() {
    const ID: &str = "01234567-89AB-CDEF-0123-456789ABCDEF";
    let cases: &[(&str, &str, &str)] = &[
        ("motion.jpg", "expected/live-still-motion.bin", ID),
        ("motion-element.jpg", "expected/live-still-element.bin", ID),
        ("ultra.jpg", "expected/live-still-ultra.bin", ID),
        ("extended.jpg", "expected/live-still-extended.bin", "TEST"),
        (
            "extended-motion.jpg",
            "expected/live-still-extended-motion.bin",
            "TEST",
        ),
    ];
    for (fixture, expected, id) in cases {
        let data = read(fixture);
        let photo = format::parse(&data).unwrap_or_else(|e| panic!("{fixture}: {e}"));
        let out =
            pymotion::live_still(&photo.still, id).unwrap_or_else(|e| panic!("{fixture}: {e}"));
        assert_eq!(out, read(expected), "{fixture} live_still bytes");
        // Coding data must survive the rewrite.
        assert_eq!(
            pyjpeg::image_hash(&photo.still).unwrap(),
            pyjpeg::image_hash(&out).unwrap(),
            "{fixture} coding hash"
        );
    }
}

#[test]
fn ultra_from_apple_bytes_match_python() {
    let base = read("base.jpg");
    assert_eq!(
        pymotion::ultra_from_apple(&base, &base, 2.0).unwrap(),
        read("expected/ultra-2.0.bin")
    );
    assert_eq!(
        pymotion::ultra_from_apple(&base, &base, 1.8).unwrap(),
        read("expected/ultra-1.8.bin")
    );
}

#[test]
fn legacy_apple_matches_python() {
    let (info, _reason) = momentstays_core::apple_hdr::legacy(&read("legacy-mpf.jpg")).unwrap();
    let info = info.expect("legacy gain map expected");
    assert!((info.stops - 1.5909).abs() < 1e-9, "stops {}", info.stops);
    assert_eq!(info.gain, read("expected/legacy-gain.bin"));
}

#[test]
fn legacy_color_profile_bytes_match_python() {
    assert_eq!(
        pymotion::legacy_color_profile(&read("srgb.jpg"), b"FAKEICC").unwrap(),
        read("expected/srgb-icc.bin")
    );
}
