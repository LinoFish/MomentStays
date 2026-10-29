//! A5 HEIC round-trip matrix (formal): every HEIC vendor form ->
//! every HEIC vendor form -> parse back, with video bytes, still
//! coding hash and cover time equal at every hop. This formalizes the
//! HEIC write + parse-back checks previously only run locally on the
//! /tmp public samples (S22 mpvd, Google largesize mpvd, S20 sefd —
//! registered in `tests/SOURCES.md`, too large to commit) into small
//! synthetic fixtures that live in the repo.
//!
//! Forms covered: Google mpvd, Samsung mpvd+SEF, Samsung sefd input,
//! and the `live_still_heic` clean still as an assembly input.

mod common;

use base64::Engine;
use common::{build_still, build_still_spec, bx, exif_item, motion_xmp, sefd_box, HeicItem, HeicStillSpec};
use momentstays_core::format;
use momentstays_core::heif;
use momentstays_core::heifwrite::{assemble_heic, live_still_heic};
use momentstays_core::model::Vendor;
use std::fs;

fn hvc1() -> Vec<u8> {
    b"fake-hvc1-coding-data-0123456789abcdef".to_vec()
}

fn real_mp4() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64");
    let text = fs::read_to_string(path).unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

/// Parse-back check for one hop: format, video bytes, still coding
/// hash (against the original plain still) and cover time.
fn check_hop(bytes: &[u8], format_want: &str, video: &[u8], coding_want: &str, time_want: i64) {
    let back = format::parse(bytes).expect("parse back");
    assert_eq!(back.format, format_want);
    assert_eq!(back.video(bytes), video);
    assert_eq!(heif::coding_hash(&back.still).unwrap(), coding_want, "still coding hash");
    assert_eq!(back.timestamp, Some(time_want), "cover time");
}

/// Top-level box types in order (largesize-aware), for asserting which
/// motion form a file uses (mpvd vs sefd as the last box).
fn top_types(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 8 <= bytes.len() {
        let mut size = u32::from_be_bytes(bytes[p..p + 4].try_into().unwrap()) as usize;
        let mut header = 8;
        if size == 1 {
            size = u64::from_be_bytes(bytes[p + 8..p + 16].try_into().unwrap()) as usize;
            header = 16;
        }
        if size < header || size > bytes.len() - p {
            break;
        }
        out.push(String::from_utf8_lossy(&bytes[p + 4..p + 8]).into_owned());
        p += size;
    }
    out
}

fn base_still() -> Vec<u8> {
    build_still(true, true)
}

#[test]
fn google_to_samsung() {
    let still = base_still();
    let video = real_mp4();
    let coding = heif::coding_hash(&still).unwrap();
    let first = assemble_heic(&still, &video, Vendor::Google, 1_420_000).unwrap();
    check_hop(&first, "google-heic", &video, &coding, 1_420_000);
    let back = format::parse(&first).unwrap();
    let second = assemble_heic(&back.still, back.video(&first), Vendor::Samsung, back.timestamp.unwrap()).unwrap();
    check_hop(&second, "samsung-heic-sef", &video, &coding, 1_420_000);
}

#[test]
fn samsung_to_google() {
    let still = base_still();
    let video = real_mp4();
    let coding = heif::coding_hash(&still).unwrap();
    let first = assemble_heic(&still, &video, Vendor::Samsung, 1_420_000).unwrap();
    check_hop(&first, "samsung-heic-sef", &video, &coding, 1_420_000);
    let back = format::parse(&first).unwrap();
    let second = assemble_heic(&back.still, back.video(&first), Vendor::Google, back.timestamp.unwrap()).unwrap();
    check_hop(&second, "google-heic", &video, &coding, 1_420_000);
}

#[test]
fn google_samsung_google_two_hop() {
    let still = base_still();
    let video = real_mp4();
    let coding = heif::coding_hash(&still).unwrap();
    let mut current = assemble_heic(&still, &video, Vendor::Google, 2_500_000).unwrap();
    for (vendor, want) in [(Vendor::Samsung, "samsung-heic-sef"), (Vendor::Google, "google-heic")] {
        let back = format::parse(&current).unwrap();
        current = assemble_heic(&back.still, back.video(&current), vendor, back.timestamp.unwrap()).unwrap();
        check_hop(&current, want, &video, &coding, 2_500_000);
    }
}

#[test]
fn sefd_input_to_mpvd_output() {
    // Sefd-form input with NO mime item (like the real S20 file):
    // still + sefd{video} -> parse -> assemble Samsung mpvd.
    let spec = HeicStillSpec { iinf_version: 0, infe_version: 2, iloc_version: 0, offset_width: 4, length_width: 4, base_width: 0, base: 0, index_width: 0, with_iref: false };
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
    ];
    let still = build_still_spec(&spec, &items);
    let video = real_mp4();
    let mut input = still.clone();
    input.extend(sefd_box(&video));
    assert_eq!(top_types(&input).last().map(|s| s.as_str()), Some("sefd"));
    let back = format::parse(&input).expect("parse sefd input");
    assert_eq!(back.format, "samsung-heic-sef");
    assert_eq!(back.timestamp, None, "sefd input has no XMP timestamp");
    let coding = heif::coding_hash(&still).unwrap();
    let out = assemble_heic(&back.still, back.video(&input), Vendor::Samsung, 3_100_000).unwrap();
    assert_eq!(top_types(&out).last().map(|s| s.as_str()), Some("mpvd"), "mpvd form out");
    check_hop(&out, "samsung-heic-sef", &video, &coding, 3_100_000);
}

#[test]
fn live_still_then_assemble() {
    // Live -> Motion HEIC path: motion input -> live_still_heic
    // (clean + new identifier) -> assemble Samsung -> parse back.
    let still = base_still();
    let video = real_mp4();
    let coding = heif::coding_hash(&still).unwrap();
    let motion = assemble_heic(&still, &video, Vendor::Google, 1_420_000).unwrap();
    let cleaned = live_still_heic(&motion, "123E4567-E89B-12D3-A456-426614174000").unwrap();
    assert!(format::parse(&cleaned).is_err(), "cleaned still is not motion");
    assert_eq!(heif::coding_hash(&cleaned).unwrap(), coding);
    let out = assemble_heic(&cleaned, &video, Vendor::Samsung, 1_420_000).unwrap();
    check_hop(&out, "samsung-heic-sef", &video, &coding, 1_420_000);
}

#[test]
fn motion_xmp_survives_vendor_hops() {
    // A still whose XMP already carries a Container directory (like a
    // re-exported still) keeps its coding hash across hops; only the
    // motion document is rewritten. The mime item must be cdsc-linked
    // to the primary (with_iref), otherwise the writer creates a
    // second mime item instead of replacing it and the parse-back
    // uniqueness gate correctly fails.
    let spec = HeicStillSpec { iinf_version: 0, infe_version: 2, iloc_version: 0, offset_width: 4, length_width: 4, base_width: 0, base: 0, index_width: 0, with_iref: true };
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
        HeicItem { id: 3, ty: "mime", data: motion_xmp(9_999_999), method: 0, extents: 1 },
    ];
    let still = build_still_spec(&spec, &items);
    let video = real_mp4();
    let coding = heif::coding_hash(&still).unwrap();
    let out = assemble_heic(&still, &video, Vendor::Google, 9_999_999).unwrap();
    check_hop(&out, "google-heic", &video, &coding, 9_999_999);
    let back = format::parse(&out).unwrap();
    let out2 = assemble_heic(&back.still, back.video(&out), Vendor::Samsung, back.timestamp.unwrap()).unwrap();
    check_hop(&out2, "samsung-heic-sef", &video, &coding, 9_999_999);
}
