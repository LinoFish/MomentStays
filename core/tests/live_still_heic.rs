//! HEIC `live_still` tests (ROADMAP milestone 3): same order as the
//! JPEG version — static truncation, motion XMP strip, MakerNote
//! identifier inject — with parity on the coding hash plus semantic
//! fields (motion keys gone, MakerNote tag 17 reads back the uuid,
//! tag 23 kept).

mod common;

use base64::Engine;
use common::{build_still_spec, bx, exif_item, exif_item_with_makernote, motion_xmp, HeicItem, HeicStillSpec};
use momentstays_core::format;
use momentstays_core::heif;
use momentstays_core::heifwrite::live_still_heic;
use momentstays_core::tiff::Tiff;
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

/// (tag 17 uuid, tag 23 video index) read back from a HEIC still's
/// Exif item, via the same TIFF walk the JPEG tests use.
fn makernote_fields(heic: &[u8]) -> (Option<String>, Option<u64>) {
    makernote_fields_inner(heic).unwrap_or((None, None))
}

fn makernote_fields_inner(heic: &[u8]) -> Option<(Option<String>, Option<u64>)> {
    let layout = heif::layout(heic).ok()?;
    let exif = layout.items.iter().find(|i| i.ty == "Exif")?;
    let raw = heif::item_bytes(heic, exif);
    let tiff = &raw[4..];
    let t = Tiff::new(tiff).ok()?;
    let (root, _) = t.root().ok()?;
    let ptr = root.iter().find(|f| f.tag == 0x8769)?;
    let (exif_fields, _) = t.directory(ptr.offset(t.little).ok()?).ok()?;
    let note_field = exif_fields.iter().find(|f| f.tag == 0x927C)?;
    let note_off = note_field.offset(t.little).ok()?;
    let note = &tiff[note_off..note_off + note_field.count];
    if !note.starts_with(b"Apple iOS\0\x00\x01") || note.len() < 16 {
        return Some((None, None));
    }
    let little = note[12] == b'I';
    let rd16 = |p: usize| -> u16 {
        if little {
            u16::from_le_bytes([note[p], note[p + 1]])
        } else {
            u16::from_be_bytes([note[p], note[p + 1]])
        }
    };
    let rd32 = |p: usize| -> u32 {
        if little {
            u32::from_le_bytes([note[p], note[p + 1], note[p + 2], note[p + 3]])
        } else {
            u32::from_be_bytes([note[p], note[p + 1], note[p + 2], note[p + 3]])
        }
    };
    let n = rd16(14) as usize;
    let mut uuid = None;
    let mut index = None;
    for i in 0..n {
        let p = 16 + i * 12;
        if p + 12 > note.len() {
            break;
        }
        match (rd16(p), rd16(p + 2)) {
            (0x11, 2) => {
                let count = rd32(p + 4) as usize;
                let off = rd32(p + 8) as usize;
                let mut s = note.get(off..off + count)?.to_vec();
                while s.last() == Some(&0) {
                    s.pop();
                }
                uuid = Some(String::from_utf8_lossy(&s).into_owned());
            }
            (0x17, 16) => {
                let off = rd32(p + 8) as usize;
                let raw8: [u8; 8] = note.get(off..off + 8)?.try_into().ok()?;
                index = Some(if little {
                    u64::from_le_bytes(raw8)
                } else {
                    u64::from_be_bytes(raw8)
                });
            }
            _ => {}
        }
    }
    Some((uuid, index))
}

fn mime_text(heic: &[u8]) -> String {
    let layout = heif::layout(heic).unwrap();
    let mime = layout.items.iter().find(|i| i.ty == "mime").unwrap();
    String::from_utf8_lossy(&heif::item_bytes(heic, mime)).into_owned()
}

fn motion_still() -> Vec<u8> {
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item_with_makernote("AAAAAAAA-1111-2222-3333-444444444444", 7), method: 0, extents: 1 },
        HeicItem { id: 3, ty: "mime", data: motion_xmp(1_420_000), method: 0, extents: 1 },
    ];
    build_still_spec(&spec_v0(), &items)
}

#[test]
fn strips_motion_and_injects_identifier() {
    let src = motion_still();
    let uuid = "123E4567-E89B-12D3-A456-426614174000";
    let out = live_still_heic(&src, uuid).expect("live_still_heic");
    assert_eq!(makernote_fields(&out).0.as_deref(), Some(uuid));
    assert_eq!(makernote_fields(&out).1, Some(7), "tag 23 kept");
    assert_eq!(heif::coding_hash(&src).unwrap(), heif::coding_hash(&out).unwrap(), "coding unchanged");
    assert!(!mime_text(&out).contains("MotionPhoto"), "motion keys stripped");
    assert!(format::parse(&out).is_err(), "no longer a motion photo");
}

#[test]
fn replaces_identifier_keeps_coding() {
    let first = live_still_heic(&motion_still(), "AAAAAAAA-0000-0000-0000-000000000001").unwrap();
    let second = live_still_heic(&first, "BBBBBBBB-0000-0000-0000-000000000002").unwrap();
    assert_eq!(makernote_fields(&second).0.as_deref(), Some("BBBBBBBB-0000-0000-0000-000000000002"));
    assert_eq!(makernote_fields(&second).1, Some(7), "tag 23 kept across replace");
    assert_eq!(heif::coding_hash(&first).unwrap(), heif::coding_hash(&second).unwrap());
}

#[test]
fn truncates_motion_input() {
    let still = motion_still();
    let mut motion = still.clone();
    motion.extend(bx("mpvd", &real_mp4()));
    assert!(format::parse(&motion).is_ok(), "fixture is a motion photo");
    let uuid = "123E4567-E89B-12D3-A456-426614174000";
    let out = live_still_heic(&motion, uuid).expect("truncate + clean");
    assert!(!out.windows(4).any(|w| w == b"mpvd"), "no mpvd in output");
    assert_eq!(heif::coding_hash(&still).unwrap(), heif::coding_hash(&out).unwrap());
    assert_eq!(makernote_fields(&out).0.as_deref(), Some(uuid));
    assert!(!mime_text(&out).contains("MotionPhoto"));
}

#[test]
fn creates_exif_when_absent() {
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "mime", data: motion_xmp(1_420_000), method: 0, extents: 1 },
    ];
    let src = build_still_spec(&spec_v0(), &items);
    let uuid = "123E4567-E89B-12D3-A456-426614174000";
    let out = live_still_heic(&src, uuid).expect("create exif");
    let layout = heif::layout(&out).unwrap();
    assert_eq!(layout.items.iter().filter(|i| i.ty == "Exif").count(), 1);
    assert_eq!(makernote_fields(&out).0.as_deref(), Some(uuid));
    assert_eq!(heif::coding_hash(&src).unwrap(), heif::coding_hash(&out).unwrap());
    assert!(!mime_text(&out).contains("MotionPhoto"));
}

#[test]
fn refuses_non_heic_and_multiple_exif() {
    assert!(live_still_heic(b"\xff\xd8\xff\xe0fake-jpeg", "123E4567-E89B-12D3-A456-426614174000").is_err());
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: hvc1(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
        HeicItem { id: 3, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
    ];
    let src = build_still_spec(&spec_v0(), &items);
    assert!(live_still_heic(&src, "123E4567-E89B-12D3-A456-426614174000").is_err(), "multiple Exif refused");
}
