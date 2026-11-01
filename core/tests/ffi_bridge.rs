//! The `ms_core_*` ABI the Swift apps call (`apple/shared/RustCore.swift`):
//! op 0 parse (+ still bytes in slot 1), op 1 coding hash, op 2 auxiliary
//! hashes. The JSON keys here are what `RustCore.Reply` decodes.

use base64::Engine;
use momentstays_core::ffi::{ms_core_bytes, ms_core_call, ms_core_free};
use momentstays_core::format;

fn fixture(name: &str) -> Vec<u8> {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures-pycli")
        .join(format!("{name}.b64"));
    base64::engine::general_purpose::STANDARD
        .decode(std::fs::read_to_string(p).unwrap().trim_ascii())
        .unwrap()
}

/// Call the ABI and return (JSON, still bytes); the response is freed.
fn call(op: u32, input: &[u8]) -> (serde_json::Value, Vec<u8>) {
    unsafe {
        let r = ms_core_call(op, input.as_ptr(), input.len());
        let slot = |n: u32| {
            let mut len = 0usize;
            let p = ms_core_bytes(r, n, &mut len);
            if p.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(p, len).to_vec()
            }
        };
        let out = (serde_json::from_slice(&slot(0)).unwrap(), slot(1));
        ms_core_free(r);
        out
    }
}

#[test]
fn parse_matches_the_library_for_every_motion_fixture() {
    for name in [
        "motion.jpg",
        "motion-element.jpg",
        "motion-legacy.jpg",
        "ultra.jpg",
        "extended.jpg",
    ] {
        let data = fixture(name);
        let want = format::parse(&data).unwrap();
        let (json, still) = call(0, &data);
        assert!(json.get("error").is_none(), "{name}: {json}");
        assert_eq!(json["video_start"], want.video.start, "{name}");
        assert_eq!(json["video_end"], want.video.end, "{name}");
        assert_eq!(
            json["timestamp"],
            serde_json::json!(want.timestamp),
            "{name}"
        );
        assert_eq!(
            json["timestamp_tag"],
            serde_json::json!(want.timestamp_tag),
            "{name}"
        );
        assert_eq!(json["ultra"], want.ultra, "{name}");
        assert_eq!(
            json["cover_frame"],
            serde_json::json!(want.cover_frame),
            "{name}"
        );
        assert_eq!(json["format"], want.format.as_str(), "{name}");
        assert_eq!(still, &want.still[..], "{name}: still bytes");
    }
}

#[test]
fn hashes_match_the_library() {
    let data = fixture("ultra.jpg");
    let still = format::parse(&data).unwrap().still.into_owned();
    let (json, _) = call(1, &still);
    assert_eq!(json["hash"], format::coding_hash(&still).unwrap());
    let (json, _) = call(2, &still);
    assert_eq!(
        json["hashes"],
        serde_json::json!(format::auxiliary_hashes(&still).unwrap())
    );
}

#[test]
fn bad_input_is_an_error_reply_not_a_crash() {
    let (json, still) = call(0, &fixture("static.jpg"));
    assert!(json["error"].is_string());
    assert!(still.is_empty());
    let (json, _) = call(99, b"x");
    assert!(json["error"].is_string());
}

#[test]
fn op3_reads_apple_hdr_and_returns_the_legacy_gain_map() {
    let data = fixture("legacy-mpf.jpg");
    let (json, gain) = call(3, &data);
    assert!(json.get("error").is_none(), "{json}");
    assert_eq!(json["status"], "legacy");
    assert_eq!(json["container"], "jpeg");
    assert!((json["stops"].as_f64().unwrap() - 1.5909).abs() < 1e-9);
    assert_eq!(gain, fixture("expected/legacy-gain.bin"));
    // A file with nothing to recover: status none, empty slot 1.
    let (json, gain) = call(3, &fixture("ultra.jpg"));
    assert_eq!(json["status"], "none");
    assert!(gain.is_empty());
    // Not an image at all: an error reply, not a crash.
    let (json, _) = call(3, b"not an image");
    assert!(json["error"].is_string());
}
