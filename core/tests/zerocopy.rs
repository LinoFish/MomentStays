//! P2 zero-copy tests (ROADMAP milestone 7): `format::MotionPhoto`
//! borrows its still from the input whenever no rebuild is needed,
//! and the borrowed bytes are exactly what the owned rebuild would
//! produce. The owned path (MPF re-indexing) must stay byte-equal
//! to `jpeg::static_only` too.

mod common;

use base64::Engine;
use common::build_still;
use momentstays_core::format;
use momentstays_core::heifwrite::assemble_heic;
use momentstays_core::jpeg;
use momentstays_core::model::Vendor;
use momentstays_core::pyjpeg;
use momentstays_core::pymotion::XMP_HDR;
use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;

fn real_mp4() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64");
    let text = fs::read_to_string(path).unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
    // APPn/SOS segment header: marker + 2-byte length incl. itself.
    let mut v = vec![0xFF, marker];
    v.extend(((payload.len() + 2) as u16).to_be_bytes());
    v.extend(payload);
    v
}

fn read_fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures-pycli")
        .join(format!("{name}.b64"));
    let text = fs::read_to_string(path).unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

/// Single-image Google motion JPEG, no MPF segment: SOI, Exif,
/// motion XMP, one SOS scan byte, EOI, real MP4. Same shape as the
/// fuzz seed, built locally so this file stays self-contained.
fn single_image_motion_jpeg() -> Vec<u8> {
    let video = real_mp4();
    let xml = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
        <rdf:Description rdf:about=\"\" xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\" \
        xmlns:Container=\"http://ns.google.com/photos/1.0/container/\" \
        xmlns:Item=\"http://ns.google.com/photos/1.0/container/item/\" \
        GCamera:MotionPhoto=\"1\" GCamera:MotionPhotoVersion=\"1\" \
        GCamera:MotionPhotoPresentationTimestampUs=\"1420000\">\
        <Container:Directory><rdf:Seq>\
        <rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"Primary\" Item:Mime=\"image/jpeg\" Item:Length=\"0\"/></rdf:li>\
        <rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"MotionPhoto\" Item:Mime=\"video/mp4\" Item:Length=\"{}\"/></rdf:li>\
        </rdf:Seq></Container:Directory></rdf:Description></rdf:RDF></x:xmpmeta>",
        video.len()
    );
    let mut xmp = XMP_HDR.to_vec();
    xmp.extend(xml.as_bytes());
    let mut tiff = b"MM".to_vec();
    tiff.extend([0u8, 42]);
    tiff.extend(8u32.to_be_bytes());
    tiff.extend(0u16.to_be_bytes());
    tiff.extend(0u32.to_be_bytes());
    let mut exif = pyjpeg::EXIF_HDR.to_vec();
    exif.extend(tiff);
    let mut v = vec![0xFF, 0xD8];
    v.extend(seg(0xE1, &exif));
    v.extend(seg(0xE1, &xmp));
    v.extend(seg(0xDA, &[0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]));
    v.push(0x42);
    v.extend([0xFF, 0xD9]);
    v.extend(&video);
    v
}

#[test]
fn heic_still_is_borrowed() {
    let still = build_still(true, true);
    let video = real_mp4();
    let motion = assemble_heic(&still, &video, Vendor::Google, 1_420_000).unwrap();
    let p = format::parse(&motion).expect("motion heic parses");
    assert!(matches!(p.still, Cow::Borrowed(_)), "HEIC still must borrow");
    // The still boundary is the mpvd box start: assemble_heic
    // repacks the still (rewritten XMP), so the parsed-back still
    // is the repacked image, not the `build_still` input. Google
    // output is image + 8-byte mpvd header + video, no tail.
    let boundary = motion.len() - 8 - video.len();
    assert_eq!(&p.still[..], &motion[..boundary], "borrowed slice is the still");
}

#[test]
fn jpeg_single_image_borrows_and_matches_rebuild() {
    let data = single_image_motion_jpeg();
    let p = format::parse(&data).expect("single-image motion parses");
    assert!(matches!(p.still, Cow::Borrowed(_)), "single image, no MPF: must borrow");
    assert_eq!(&p.still[..], &data[..p.video.start], "borrowed slice is the still");
    // The borrow is only valid because the rebuild is identical.
    assert_eq!(
        &p.still[..],
        &jpeg::static_only(&data[..p.video.start]).unwrap()[..],
        "borrowed still equals owned rebuild"
    );
}

#[test]
fn jpeg_mpf_still_is_owned_but_matches_rebuild() {
    let data = read_fixture("ultra.jpg");
    let p = format::parse(&data).expect("ultra parses");
    assert!(p.ultra, "fixture has a gain map");
    assert!(matches!(p.still, Cow::Owned(_)), "MPF gain map forces a rebuild");
    assert_eq!(
        &p.still[..],
        &jpeg::static_only(&data[..p.video.start]).unwrap()[..],
        "owned still equals the rebuild"
    );
}
