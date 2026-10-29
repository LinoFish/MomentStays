//! HEIF fixture tests (ROADMAP item 3): synthetic stills covering
//! iloc v0/v1/v2, mdat vs idat storage, multi-extent items, base
//! offsets, mpvd+SEF and sefd motion forms, and coding-hash stability
//! under Exif edits. No pixel decode anywhere: fake `hvc1` bytes stand
//! in for the coded image data, which the container logic never reads.
//!
//! The sefd box shape mirrors the Galaxy S20 sample registered in
//! `tests/SOURCES.md` (MIT); its parser support lands in milestone 2,
//! so the sefd test here pins the shape and the current rejection.

mod common;

use base64::Engine;
use common::{build_still_spec, bx, exif_item, motion_xmp, plain_xmp, sefd_box, sef_tail_mpv2, HeicItem, HeicStillSpec};
use momentstays_core::format;
use momentstays_core::heif;
use momentstays_core::vendor;
use std::fs;

fn real_mp4() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64");
    let text = fs::read_to_string(path).unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

fn hvc1() -> Vec<u8> {
    b"fake-hvc1-coding-data-0123456789abcdef".to_vec()
}

fn spec_v0() -> HeicStillSpec {
    HeicStillSpec { iinf_version: 0, infe_version: 2, iloc_version: 0, offset_width: 4, length_width: 4, base_width: 0, base: 0, index_width: 0, with_iref: false }
}

fn item_bytes(still: &[u8], ty: &str) -> Vec<u8> {
    let layout = heif::layout(still).unwrap();
    let item = layout.items.iter().find(|i| i.ty == ty).expect("item");
    heif::item_bytes(still, item)
}

#[test]
fn iloc_v1_width8_mdat_multiextent() {
    let spec = HeicStillSpec { iinf_version: 0, infe_version: 2, iloc_version: 1, offset_width: 8, length_width: 8, base_width: 0, base: 0, index_width: 0, with_iref: false };
    let data = hvc1();
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: data.clone(), method: 0, extents: 2 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
        HeicItem { id: 3, ty: "mime", data: plain_xmp(), method: 0, extents: 1 },
    ];
    let still = build_still_spec(&spec, &items);
    assert_eq!(item_bytes(&still, "hvc1"), data, "multi-extent concatenation");
    assert_eq!(item_bytes(&still, "mime"), plain_xmp());
    heif::coding_hash(&still).unwrap();
}

#[test]
fn iloc_v2_base4_mdat() {
    let spec = HeicStillSpec { iinf_version: 1, infe_version: 3, iloc_version: 2, offset_width: 8, length_width: 8, base_width: 4, base: 32, index_width: 0, with_iref: false };
    let data = hvc1();
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: data.clone(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
        HeicItem { id: 3, ty: "mime", data: plain_xmp(), method: 0, extents: 1 },
    ];
    let still = build_still_spec(&spec, &items);
    assert_eq!(item_bytes(&still, "hvc1"), data, "base offset applied");
    assert_eq!(item_bytes(&still, "Exif"), exif_item());
}

#[test]
fn iloc_v1_idat_only() {
    // Note: idat needs iloc v1+ (v0 has no method field; method is
    // implicitly mdat there).
    let spec = HeicStillSpec { iinf_version: 0, infe_version: 2, iloc_version: 1, offset_width: 4, length_width: 4, base_width: 0, base: 0, index_width: 0, with_iref: false };
    let data = hvc1();
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: data.clone(), method: 1, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 1, extents: 1 },
        HeicItem { id: 3, ty: "mime", data: plain_xmp(), method: 1, extents: 1 },
    ];
    let still = build_still_spec(&spec, &items);
    assert_eq!(item_bytes(&still, "hvc1"), data);
    assert_eq!(item_bytes(&still, "mime"), plain_xmp());
    // Pure idat still: top level is ftyp + meta, no mdat.
    assert!(!still.windows(4).any(|w| w == b"mdat"), "no mdat box");
    heif::coding_hash(&still).unwrap();
}

#[test]
fn mixed_mdat_idat() {
    let spec = HeicStillSpec { iinf_version: 0, infe_version: 2, iloc_version: 1, offset_width: 4, length_width: 4, base_width: 0, base: 0, index_width: 0, with_iref: true };
    let data = hvc1();
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: data.clone(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 1, extents: 1 },
        HeicItem { id: 3, ty: "mime", data: plain_xmp(), method: 1, extents: 1 },
    ];
    let still = build_still_spec(&spec, &items);
    assert_eq!(item_bytes(&still, "hvc1"), data);
    assert_eq!(item_bytes(&still, "Exif"), exif_item());
    assert_eq!(item_bytes(&still, "mime"), plain_xmp());
}

#[test]
fn iloc_v1_index_nonzero_rejected() {
    let spec = HeicStillSpec { iinf_version: 0, infe_version: 2, iloc_version: 1, offset_width: 4, length_width: 4, base_width: 0, base: 0, index_width: 4, with_iref: false };
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
    ];
    let still = build_still_spec(&spec, &items);
    let layout = heif::layout(&still).unwrap();
    let hvc1 = layout.items.iter().find(|i| i.ty == "hvc1").unwrap();
    // The extent index field sits immediately before the offset field.
    let idx_at = hvc1.offset_at - 4;
    assert_eq!(still[idx_at], 0, "index starts zero");
    let mut patched = still.clone();
    patched[idx_at] = 1;
    assert!(heif::layout(&patched).is_err(), "nonzero extent index refused");
}

fn motion_still() -> Vec<u8> {
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
        HeicItem { id: 3, ty: "mime", data: motion_xmp(1_420_000), method: 0, extents: 1 },
    ];
    build_still_spec(&spec_v0(), &items)
}

#[test]
fn google_heic_fixture_parses() {
    let still = motion_still();
    let video = real_mp4();
    let mut file = still.clone();
    file.extend(bx("mpvd", &video));
    let back = format::parse(&file).expect("parse google fixture");
    assert_eq!(back.format, "google-heic");
    assert_eq!(back.video(&file), video.as_slice());
    assert_eq!(back.timestamp, Some(1_420_000));
}

#[test]
fn samsung_mpvd_sef_fixture_parses() {
    let still = motion_still();
    let video = real_mp4();
    let video_start = still.len() + 8; // right after the mpvd header
    let tail = sef_tail_mpv2(video_start, video.len());
    let mut mpvd_body = video.clone();
    mpvd_body.extend(&tail);
    let mut file = still.clone();
    file.extend(bx("mpvd", &mpvd_body));
    let back = format::parse(&file).expect("parse samsung fixture");
    assert_eq!(back.format, "samsung-heic-sef");
    assert_eq!(back.video(&file), video.as_slice());
    assert_eq!(back.timestamp, Some(1_420_000));
}

#[test]
fn sefd_fixture_parses() {
    // Still with NO mime item, mirroring the real S20 file: the SEF
    // record, not XMP, declares the motion photo.
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
    ];
    let still = build_still_spec(&spec_v0(), &items);
    heif::layout(&still).unwrap();
    let video = real_mp4();
    let mut file = still.clone();
    file.extend(sefd_box(&video));
    // The SEF directory locates the video.
    let loc = vendor::locate(&file, 0).expect("locate").expect("sef record");
    assert_eq!(loc.format, "samsung-sef");
    assert_eq!(&file[loc.start..loc.end], video.as_slice());
    // sefd is the last top-level box and runs to EOF.
    assert!(file.ends_with(b"SEFT"));
    // Parser acceptance for sefd (milestone 2): no XMP gate, no
    // timestamp without a mime item.
    let back = format::parse(&file).expect("parse sefd fixture");
    assert_eq!(back.format, "samsung-heic-sef");
    assert_eq!(back.video(&file), video.as_slice());
    assert_eq!(back.timestamp, None);
}

#[test]
fn coding_hash_stable_under_exif_edits() {
    let base_items = |exif: Option<Vec<u8>>, mime: Vec<u8>| {
        let mut items = vec![HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 }];
        if let Some(e) = exif {
            items.push(HeicItem { id: 2, ty: "Exif", data: e, method: 0, extents: 1 });
            items.push(HeicItem { id: 3, ty: "mime", data: mime, method: 0, extents: 1 });
        } else {
            items.push(HeicItem { id: 2, ty: "mime", data: mime, method: 0, extents: 1 });
        }
        build_still_spec(&spec_v0(), &items)
    };
    let a = base_items(Some(exif_item()), plain_xmp());
    // Different Exif content AND length: coding hash must not move.
    let mut exif_b = exif_item();
    exif_b.extend(b"extra-exif-payload-bytes-0123456789");
    let b = base_items(Some(exif_b), plain_xmp());
    // No Exif at all: same hash again.
    let c = base_items(None, plain_xmp());
    // Different XMP bytes: mime is excluded from the hash too.
    let d = base_items(Some(exif_item()), motion_xmp(9_999_999));
    let ha = heif::coding_hash(&a).unwrap();
    assert_eq!(heif::coding_hash(&b).unwrap(), ha);
    assert_eq!(heif::coding_hash(&c).unwrap(), ha);
    assert_eq!(heif::coding_hash(&d).unwrap(), ha);
}
