//! C ABI parse/hash parity (core-replacement branch): `momentstays_parse`
//! matches `format::parse` field-for-field (old op-0 bridge semantics),
//! and `momentstays_coding_hash` / `momentstays_auxiliary_hashes` match
//! the direct `format::` functions the old op-1/op-2 bridge called.
//! The hash functions must NOT be served by `inspect`: the app calls
//! them on still-only bytes, where `inspect`'s parse fails.
//!
//! Inputs: every fixtures-pycli JPEG (motion or not; the assertion is
//! capi == format::parse, whatever the outcome) plus assembled
//! Google/Samsung HEIC motions and a Samsung sefd-form HEIC input.

mod common;

use base64::Engine;
use common::{build_still, build_still_spec, exif_item, sefd_box, HeicItem, HeicStillSpec};
use momentstays_core::capi;
use momentstays_core::format;
use momentstays_core::heifwrite::assemble_heic;
use momentstays_core::model::Vendor;
use std::ffi::CStr;

fn manifest() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn decode_b64(path: &std::path::Path) -> Vec<u8> {
    let text = std::fs::read_to_string(path).unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
}

fn real_mp4() -> Vec<u8> {
    decode_b64(&manifest().join("tests/fixtures-samples/real.mp4.b64"))
}

fn hvc1() -> Vec<u8> {
    b"fake-hvc1-coding-data-0123456789abcdef".to_vec()
}

/// Call `momentstays_parse` directly and return (rc, JSON, still bytes).
fn parse_via_capi(data: &[u8]) -> (i32, serde_json::Value, Vec<u8>) {
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut still: *const u8 = std::ptr::null();
    let mut still_len: usize = 0;
    let rc = capi::momentstays_parse(
        data.as_ptr(),
        data.len(),
        &mut json,
        &mut still,
        &mut still_len,
    );
    let value = if json.is_null() {
        serde_json::Value::Null
    } else {
        let s = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        capi::momentstays_free_string(json);
        serde_json::from_str(&s).unwrap()
    };
    let still_bytes = if still.is_null() || still_len == 0 {
        vec![]
    } else {
        let v = unsafe { std::slice::from_raw_parts(still, still_len) }.to_vec();
        capi::momentstays_free_bytes(still as *mut u8, still_len);
        v
    };
    (rc, value, still_bytes)
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let dir = manifest().join("tests/fixtures-pycli");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".b64"))
        .collect();
    names.sort();
    for n in names {
        out.push((n.clone(), decode_b64(&dir.join(&n))));
    }
    // Assembled HEIC motions (same builders as tests/heic_matrix.rs).
    let still = build_still(true, true);
    let video = real_mp4();
    out.push((
        "heic-google".to_string(),
        assemble_heic(&still, &video, Vendor::Google, 1_420_000).unwrap(),
    ));
    out.push((
        "heic-samsung".to_string(),
        assemble_heic(&still, &video, Vendor::Samsung, 1_420_000).unwrap(),
    ));
    // Samsung sefd-form input (no mime item, like the real S20 file).
    let spec = HeicStillSpec {
        iinf_version: 0,
        infe_version: 2,
        iloc_version: 0,
        offset_width: 4,
        length_width: 4,
        base_width: 0,
        base: 0,
        index_width: 0,
        with_iref: false,
    };
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
    ];
    let mut sefd = build_still_spec(&spec, &items);
    sefd.extend(sefd_box(&video));
    out.push(("heic-sefd".to_string(), sefd));
    out
}

#[test]
fn parse_matches_format_parse() {
    for (name, data) in inputs() {
        let (rc, json, still) = parse_via_capi(&data);
        match format::parse(&data) {
            Ok(p) => {
                assert_eq!(rc, 0, "{name}: parse rc");
                assert!(json["error"].is_null(), "{name}: unexpected error {json}");
                assert_eq!(json["video_start"].as_u64().unwrap() as usize, p.video.start, "{name}");
                assert_eq!(json["video_end"].as_u64().unwrap() as usize, p.video.end, "{name}");
                assert_eq!(json["timestamp"], serde_json::json!(p.timestamp), "{name}");
                assert_eq!(json["timestamp_tag"], serde_json::json!(p.timestamp_tag), "{name}");
                assert_eq!(json["ultra"].as_bool().unwrap(), p.ultra, "{name}");
                assert_eq!(json["cover_frame"], serde_json::json!(p.cover_frame), "{name}");
                assert_eq!(json["format"].as_str().unwrap(), p.format, "{name}");
                assert_eq!(still, p.still.as_ref(), "{name}: still bytes");
                assert_eq!(p.video(&data), &data[p.video.start..p.video.end], "{name}");
            }
            Err(e) => {
                assert_eq!(rc, 0, "{name}: error path still returns 0");
                assert_eq!(json["error"].as_str().unwrap(), e.0, "{name}");
                assert!(still.is_empty(), "{name}: no still on error");
            }
        }
    }
}

/// Call one of the C ABI hash functions and return (rc, JSON).
fn hash_via_capi(data: &[u8], which: u8) -> (i32, serde_json::Value) {
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let rc = if which == 0 {
        capi::momentstays_coding_hash(data.as_ptr(), data.len(), &mut json)
    } else {
        capi::momentstays_auxiliary_hashes(data.as_ptr(), data.len(), &mut json)
    };
    let value = if json.is_null() {
        serde_json::Value::Null
    } else {
        let s = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        capi::momentstays_free_string(json);
        serde_json::from_str(&s).unwrap()
    };
    (rc, value)
}

#[test]
fn hash_functions_match_direct_functions() {
    // Old op-1/op-2 called format::coding_hash / auxiliary_hashes on
    // the bytes the app passes — stills, not full motion files (every
    // app caller passes .still). The new hash functions must agree
    // with the direct functions on every input, and — unlike inspect,
    // whose parse fails on still-only bytes — must succeed on stills.
    let mut cases: Vec<(String, Vec<u8>)> = inputs();
    for (name, data) in inputs() {
        if let Ok(p) = format::parse(&data) {
            cases.push((format!("{name}-still"), p.still.into_owned()));
        }
    }
    cases.push(("garbage".to_string(), b"not a photo at all".to_vec()));
    cases.push(("empty".to_string(), vec![]));
    for (name, data) in cases {
        let (rc, json) = hash_via_capi(&data, 0);
        match format::coding_hash(&data) {
            Ok(h) => {
                assert_eq!(rc, 0, "{name}");
                assert_eq!(json["hash"].as_str().unwrap(), h, "{name}");
            }
            Err(e) => {
                assert_eq!(rc, 0, "{name}");
                assert_eq!(json["error"].as_str().unwrap(), e.0, "{name}");
            }
        }
        let (rc, json) = hash_via_capi(&data, 1);
        match format::auxiliary_hashes(&data) {
            Ok(hs) => {
                assert_eq!(rc, 0, "{name}");
                let got: Vec<&str> = json["hashes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_str().unwrap())
                    .collect();
                assert_eq!(
                    got,
                    hs.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                    "{name}"
                );
            }
            Err(e) => {
                assert_eq!(rc, 0, "{name}");
                assert_eq!(json["error"].as_str().unwrap(), e.0, "{name}");
            }
        }
    }
}

#[test]
fn parse_null_and_garbage_safety() {
    // NULL input with nonzero length is a hard failure.
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut still: *const u8 = std::ptr::null();
    let mut still_len: usize = 0;
    let rc = capi::momentstays_parse(
        std::ptr::null(),
        1,
        &mut json,
        &mut still,
        &mut still_len,
    );
    assert_ne!(rc, 0);
    assert!(json.is_null() && still.is_null() && still_len == 0);
    // NULL input with zero length is empty input: error JSON, rc 0.
    let (rc, json, still) = parse_via_capi(&[]);
    assert_eq!(rc, 0);
    assert!(json["error"].is_string());
    assert!(still.is_empty());
    // Garbage bytes: error JSON, no crash, no still.
    let (rc, json, still) = parse_via_capi(b"not a photo at all");
    assert_eq!(rc, 0);
    assert!(json["error"].is_string());
    assert!(still.is_empty());
    // Freeing NULL is a no-op.
    capi::momentstays_free_string(std::ptr::null_mut());
    capi::momentstays_free_bytes(std::ptr::null_mut(), 0);
}
