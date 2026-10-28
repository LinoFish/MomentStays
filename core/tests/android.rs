//! Vendor round-trip tests for `model` (ROADMAP A5, JPEG path):
//! parse a Motion Photo, write it for another vendor, parse it back and
//! require video bytes, still coding hash and cover to be unchanged.

use base64::Engine;
use momentstays_core::format::{self, coding_hash};
use momentstays_core::model::{Cover, MotionModel, Vendor};
use std::fs;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-pycli")
}

fn read(name: &str) -> Vec<u8> {
    let text = fs::read_to_string(dir().join(format!("{name}.b64"))).unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
}

fn google_model() -> MotionModel {
    let data = read("motion.jpg");
    let p = format::parse(&data).expect("motion.jpg parses");
    assert_eq!(p.format, "google-compatible");
    assert_eq!(p.timestamp, Some(400000));
    MotionModel::from_parsed(&p, &data).unwrap()
}

fn check_roundtrip(model: &MotionModel, target: Vendor, format: &str) {
    let out = momentstays_core::model::write(model, target)
        .unwrap_or_else(|e| panic!("write {}: {}", target.label(), e));
    let back = format::parse(&out).expect("written file parses");
    assert_eq!(back.format, format, "target {}", target.label());
    assert_eq!(back.video(&out), model.video.as_slice(), "video bytes");
    assert_eq!(
        coding_hash(&back.still).unwrap(),
        coding_hash(&model.still).unwrap(),
        "still coding hash"
    );
    match model.cover {
        Cover::Time(t) => assert_eq!(back.timestamp, Some(t), "cover time"),
        Cover::Frame(f) => assert_eq!(back.cover_frame, Some(f), "cover frame"),
    }
}

#[test]
fn google_roundtrip() {
    let m = google_model();
    check_roundtrip(&m, Vendor::Google, "google-compatible");
}

#[test]
fn samsung_roundtrip() {
    let m = google_model();
    check_roundtrip(&m, Vendor::Samsung, "samsung-sef");
}

#[test]
fn oppo_roundtrip() {
    let m = google_model();
    check_roundtrip(&m, Vendor::Oppo, "oppo");
}

#[test]
fn vivo_roundtrip() {
    let m = google_model();
    check_roundtrip(&m, Vendor::Vivo, "vivo-v2");
}

fn mp4() -> Vec<u8> {
    let b = |t: &[u8], p: &[u8]| {
        let mut v = ((p.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend(t);
        v.extend(p);
        v
    };
    let mut v = b(b"ftyp", b"isom\0\0\0\0isom");
    v.extend(b(b"moov", &[0; 16]));
    v.extend(b(b"mdat", b"\xff\xd9ftyp-decoy"));
    v
}

fn huawei_model() -> MotionModel {
    let video = mp4();
    let mut d = vec![0xff, 0xd8];
    let seg = |marker: u8, payload: &[u8]| {
        let mut v = vec![0xff, marker];
        v.extend(((payload.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(payload);
        v
    };
    d.extend(seg(0xe1, b"Exif\0\0"));
    d.extend(seg(0xdb, &[0u8; 65]));
    d.extend(seg(0xc0, b"\x08\0\x10\0\x10\x01\x01\x11\0"));
    d.extend(seg(0xda, b"\x01\x01\0\0\0\0"));
    d.extend(b"\x12\xff\0\x34\xff\xd0\x56");
    d.extend([0xff, 0xd9]);
    d.extend(&video);
    let pad = |s: &str| format!("{s:<20}").into_bytes();
    d.extend(pad("v2_f12"));
    d.extend(pad("anything"));
    d.extend(pad(&format!("LIVE_{}", video.len() + 20)));
    let p = format::parse(&d).expect("huawei parses");
    assert_eq!(p.format, "huawei-honor");
    assert_eq!(p.cover_frame, Some(12));
    MotionModel::from_parsed(&p, &d).unwrap()
}

#[test]
fn huawei_frame_roundtrip() {
    let m = huawei_model();
    check_roundtrip(&m, Vendor::Huawei, "huawei-honor");
}

#[test]
fn huawei_from_time_is_refused() {
    let m = google_model();
    let err = momentstays_core::model::write(&m, Vendor::Huawei).unwrap_err();
    assert!(err.0.contains("帧号"), "unexpected error: {}", err.0);
}

#[test]
fn google_from_frame_is_refused() {
    let m = huawei_model();
    let err = momentstays_core::model::write(&m, Vendor::Google).unwrap_err();
    assert!(err.0.contains("封面时间"), "unexpected error: {}", err.0);
}

#[test]
fn ultra_gain_map_survives() {
    let data = read("ultra.jpg");
    let p = format::parse(&data).expect("ultra parses");
    assert!(p.ultra);
    let m = MotionModel::from_parsed(&p, &data).unwrap();
    let out = momentstays_core::model::write(&m, Vendor::Google).unwrap();
    let back = format::parse(&out).expect("ultra output parses");
    assert!(back.ultra);
    assert_eq!(back.timestamp, Some(400000));
    let aux_before = format::auxiliary_hashes(&m.still).unwrap();
    let aux_after = format::auxiliary_hashes(&back.still).unwrap();
    assert_eq!(aux_before, aux_after, "gain map bytes");
    assert!(!aux_before.is_empty(), "gain map present");
}

#[test]
fn extended_xmp_motion_keys_roundtrip() {
    let data = read("extended-motion.jpg");
    let p = format::parse(&data).expect("extended-motion parses");
    let m = MotionModel::from_parsed(&p, &data).unwrap();
    assert_eq!(m.cover, Cover::Time(400000));
    check_roundtrip(&m, Vendor::Google, "google-compatible");
}

#[test]
fn cross_vendor_chain() {
    let m = google_model();
    let samsung = momentstays_core::model::write(&m, Vendor::Samsung).unwrap();
    let p = format::parse(&samsung).expect("samsung output parses");
    let m2 = MotionModel::from_parsed(&p, &samsung).unwrap();
    assert_eq!(m2.cover, Cover::Time(400000));
    let vivo = momentstays_core::model::write(&m2, Vendor::Vivo).unwrap();
    let p2 = format::parse(&vivo).expect("vivo output parses");
    assert_eq!(p2.format, "vivo-v2");
    assert_eq!(p2.video(&vivo), m.video.as_slice());
    assert_eq!(p2.timestamp, Some(400000));
}
