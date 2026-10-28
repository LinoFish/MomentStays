//! HEIC writers tests (ROADMAP A3): XMP rewrite and Motion Photo
//! assembly for Google and Samsung, mirroring Swift `HEIFMetadata.write`
//!
//! Inputs are synthetic HEIC stills built here (no pixel decode anywhere
//! in the core, so fake `hvc1` bytes are enough for the container logic).
//! The real-file MP4 fixture is reused as the motion video.

mod common;

use base64::Engine;
use common::{build_still, bx};
use momentstays_core::format;
use momentstays_core::heif;
use momentstays_core::heifwrite::{assemble_heic, write_xmp};
use momentstays_core::model::Vendor;
use std::fs;

fn real_mp4() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64");
    let text = fs::read_to_string(path).unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

fn mime_bytes(still: &[u8]) -> Vec<u8> {
    let layout = heif::layout(still).unwrap();
    let item = layout.items.iter().find(|i| i.ty == "mime").expect("mime item");
    heif::item_bytes(still, item)
}

#[test]
fn write_xmp_replaces_primary_mime() {
    let still = build_still(true, true);
    let before = heif::coding_hash(&still).unwrap();
    let new_xmp = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description GCamera:MotionPhoto=\"1\"/></rdf:RDF></x:xmpmeta>";
    let out = write_xmp(&still, new_xmp).expect("write");
    assert_eq!(heif::coding_hash(&out).unwrap(), before, "coding hash unchanged");
    assert_eq!(mime_bytes(&out), new_xmp, "mime replaced");
    // The rewritten file still parses as a clean still.
    heif::layout(&out).unwrap();
}

#[test]
fn write_xmp_creates_mime_and_iref() {
    let still = build_still(false, false);
    let before = heif::coding_hash(&still).unwrap();
    let new_xmp = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description GCamera:MotionPhoto=\"1\"/></rdf:RDF></x:xmpmeta>";
    let out = write_xmp(&still, new_xmp).expect("write");
    assert_eq!(heif::coding_hash(&out).unwrap(), before, "coding hash unchanged");
    assert_eq!(mime_bytes(&out), new_xmp, "mime created");
    let layout = heif::layout(&out).unwrap();
    assert!(layout.children.iter().any(|b| b.is(b"iref")), "iref created");
}

#[test]
fn assemble_google_heic_roundtrip() {
    let still = build_still(true, true);
    let video = real_mp4();
    let out = assemble_heic(&still, &video, Vendor::Google, 1_420_000).expect("assemble");
    let back = format::parse(&out).expect("parse back");
    assert_eq!(back.format, "google-heic");
    assert_eq!(back.video(&out), video.as_slice());
    assert_eq!(back.timestamp, Some(1_420_000));
    // mpvd is the last top-level box and holds exactly the video.
    let tail = &out[out.len() - video.len()..];
    assert_eq!(tail, video.as_slice());
}

#[test]
fn assemble_samsung_heic_roundtrip() {
    let still = build_still(true, true);
    let video = real_mp4();
    let out = assemble_heic(&still, &video, Vendor::Samsung, 1_420_000).expect("assemble");
    let back = format::parse(&out).expect("parse back");
    assert_eq!(back.format, "samsung-heic-sef");
    assert_eq!(back.video(&out), video.as_slice());
    assert_eq!(back.timestamp, Some(1_420_000));
    assert!(out.ends_with(b"SEFT"), "SEF footer at end");
}

#[test]
fn assemble_refuses_other_vendors_and_bad_inputs() {
    let still = build_still(true, true);
    let video = real_mp4();
    for vendor in [Vendor::Huawei, Vendor::Oppo, Vendor::Vivo] {
        assert!(assemble_heic(&still, &video, vendor, 1_420_000).is_err(), "{vendor:?} refused");
    }
    // A still that already carries video (mpvd) is refused.
    let mut with_video = still.clone();
    with_video.extend(bx("mpvd", b"xxxx"));
    assert!(write_xmp(&with_video, b"<x:xmpmeta/>").is_err());
    // Unknown top-level box is refused.
    let mut with_unknown = still.clone();
    with_unknown.extend(bx("xxxx", b"xxxx"));
    assert!(write_xmp(&with_unknown, b"<x:xmpmeta/>").is_err());
}
