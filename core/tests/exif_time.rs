//! EXIF capture time lookup used for output file timestamps.

mod common;

use common::{build_still_spec, bx, motion_xmp, HeicItem, HeicStillSpec};
use momentstays_core::exif_time::capture_time_from_bytes;
use momentstays_core::format;

/// Big-endian TIFF with one IFD0 entry: ModifyDate "2021:03:04 05:06:07".
fn tiff() -> Vec<u8> {
    let mut v = b"MM\0\x2a".to_vec();
    v.extend(8u32.to_be_bytes());
    v.extend(1u16.to_be_bytes());
    v.extend(0x0132u16.to_be_bytes());
    v.extend(2u16.to_be_bytes());
    v.extend(20u32.to_be_bytes());
    v.extend(26u32.to_be_bytes());
    v.extend(0u32.to_be_bytes());
    v.extend(b"2021:03:04 05:06:07\0");
    v
}

fn app1(payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, 0xE1];
    v.extend(((payload.len() + 2) as u16).to_be_bytes());
    v.extend(payload);
    v
}

fn jpeg(xmp_first: bool) -> Vec<u8> {
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend(tiff());
    let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    xmp.extend(b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>");
    let mut v = vec![0xFF, 0xD8];
    if xmp_first {
        v.extend(app1(&xmp));
    }
    v.extend(app1(&exif));
    if !xmp_first {
        v.extend(app1(&xmp));
    }
    v.extend([0xFF, 0xD9]);
    v
}

#[test]
fn exif_after_an_xmp_app1_is_found() {
    let expected = capture_time_from_bytes(&jpeg(false)).expect("exif-first JPEG");
    // Used to look only at the first APP1 (here the XMP packet) and give up.
    assert_eq!(capture_time_from_bytes(&jpeg(true)), Some(expected));
}

#[test]
fn heic_motion_photos_keep_their_exif_time() {
    let spec = HeicStillSpec {
        iinf_version: 0,
        infe_version: 2,
        iloc_version: 0,
        offset_width: 4,
        length_width: 4,
        base_width: 0,
        base: 0,
        index_width: 0,
        with_iref: true,
    };
    let mut exif_item = vec![0u8; 4];
    exif_item.extend(tiff());
    let items = vec![
        HeicItem {
            id: 1,
            ty: "hvc1",
            data: b"fake-hvc1-coding-data-0123456789abcdef".to_vec(),
            method: 0,
            extents: 1,
        },
        HeicItem {
            id: 2,
            ty: "Exif",
            data: exif_item,
            method: 0,
            extents: 1,
        },
        HeicItem {
            id: 3,
            ty: "mime",
            data: motion_xmp(0),
            method: 0,
            extents: 1,
        },
    ];
    let still = build_still_spec(&spec, &items);
    let expected = capture_time_from_bytes(&still).expect("plain HEIC still");
    let video = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    use base64::Engine;
    let video = base64::engine::general_purpose::STANDARD
        .decode(video.trim_ascii())
        .unwrap();
    let mut motion = still.clone();
    motion.extend(bx("mpvd", &video));
    assert!(
        format::parse(&motion).is_ok(),
        "seed is a HEIC Motion Photo"
    );
    // The still-only HEIC layout refuses mpvd, so this used to be None.
    assert_eq!(capture_time_from_bytes(&motion), Some(expected));
}
