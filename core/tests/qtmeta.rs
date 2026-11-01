//! Content-identifier (ROADMAP item 9b, first half) tests.
//!
//! The still-image-time mett track is intentionally NOT covered here: its
//! boxed sample bytes are not pinned by any verifiable source yet (see
//! ROADMAP item 9b), so no bytes are fabricated for it.

use base64::Engine;
use momentstays_core::remux::{
    check_remux, read_content_identifier, remux_to_mov, remux_to_mov_with_identifier,
};

fn real_mp4() -> Vec<u8> {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
}

const UUID: &str = "123E4567-E89B-12D3-A456-426614174000";

#[test]
fn identifier_roundtrip() {
    let src = real_mp4();
    let out = remux_to_mov_with_identifier(&src, 0, UUID).expect("remux with identifier");
    assert_eq!(
        read_content_identifier(&out).expect("read identifier"),
        Some(UUID.to_string())
    );
    check_remux(&src, &out, 0).expect("remux still verifies");
    // Debug artifact for local inspection; temp_dir() so this also
    // passes on Windows, where no /tmp directory exists.
    std::fs::write(std::env::temp_dir().join("real-identifier.mov"), &out).unwrap();
}

#[test]
fn identifier_absent_without_it() {
    let src = real_mp4();
    let out = remux_to_mov(&src, 0).expect("plain remux");
    assert_eq!(
        read_content_identifier(&out).expect("read identifier"),
        None
    );
}

#[test]
fn empty_identifier_is_refused() {
    let src = real_mp4();
    assert!(remux_to_mov_with_identifier(&src, 0, "").is_err());
}

/// (type, start, end) of the boxes in `d[from..to]`.
fn boxes(d: &[u8], from: usize, to: usize) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    let mut at = from;
    while at + 8 <= to {
        let size = u32::from_be_bytes(d[at..at + 4].try_into().unwrap()) as usize;
        out.push((d[at + 4..at + 8].try_into().unwrap(), at, at + size));
        at += size;
    }
    out
}

fn find(list: &[([u8; 4], usize, usize)], t: &[u8; 4]) -> (usize, usize) {
    let b = list.iter().find(|b| &b.0 == t).unwrap_or_else(|| {
        panic!("missing {}", String::from_utf8_lossy(t));
    });
    (b.1, b.2)
}

#[test]
fn identifier_uses_quicktime_keyed_metadata() {
    let out = remux_to_mov_with_identifier(&real_mp4(), 0, UUID).unwrap();
    let top = boxes(&out, 0, out.len());
    let (moov, moov_end) = find(&top, b"moov");
    let moov_kids = boxes(&out, moov + 8, moov_end);
    assert!(moov_kids.iter().all(|b| &b.0 != b"udta"), "no udta written");
    let (meta, meta_end) = find(&moov_kids, b"meta");
    // QuickTime meta: children start right after the header.
    let kids = boxes(&out, meta + 8, meta_end);
    let types: Vec<&[u8; 4]> = kids.iter().map(|b| &b.0).collect();
    assert_eq!(types, [b"hdlr", b"keys", b"ilst"]);
    let (hdlr, _) = find(&kids, b"hdlr");
    assert_eq!(&out[hdlr + 16..hdlr + 20], b"mdta");
    let (keys, keys_end) = find(&kids, b"keys");
    assert_eq!(&out[keys + 12..keys + 16], &1u32.to_be_bytes(), "one key");
    let key = &out[keys + 16..keys_end];
    assert_eq!(&key[4..8], b"mdta");
    assert_eq!(&key[8..], b"com.apple.quicktime.content.identifier");
    assert_eq!(
        u32::from_be_bytes(key[..4].try_into().unwrap()) as usize,
        key.len()
    );
    let (ilst, ilst_end) = find(&kids, b"ilst");
    let items = boxes(&out, ilst + 8, ilst_end);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].0, 1u32.to_be_bytes(), "item refers to key 1");
    let data = boxes(&out, items[0].1 + 8, items[0].2);
    assert_eq!(&data[0].0, b"data");
    let body = &out[data[0].1 + 8..data[0].2];
    assert_eq!(&body[..4], &[0, 0, 0, 1], "UTF-8 type");
    assert_eq!(&body[4..8], &[0, 0, 0, 0], "locale");
    assert_eq!(&body[8..], UUID.as_bytes());
}

/// Rebuild `out` with moov's `meta` replaced by `replacement` (fixing sizes
/// and chunk offsets is unnecessary: the reader only walks moov).
fn with_moov_meta(out: &[u8], replacement: &[u8]) -> Vec<u8> {
    let top = boxes(out, 0, out.len());
    let (moov, moov_end) = find(&top, b"moov");
    let (meta, meta_end) = find(&boxes(out, moov + 8, moov_end), b"meta");
    let mut moov_body = out[moov + 8..meta].to_vec();
    moov_body.extend(replacement);
    moov_body.extend(&out[meta_end..moov_end]);
    let mut v = out[..moov].to_vec();
    v.extend(((moov_body.len() + 8) as u32).to_be_bytes());
    v.extend(b"moov");
    v.extend(moov_body);
    v.extend(&out[moov_end..]);
    v
}

fn bx(t: &[u8], body: &[u8]) -> Vec<u8> {
    let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(t);
    v.extend(body);
    v
}

#[test]
fn legacy_freeform_udta_identifier_still_reads() {
    let out = remux_to_mov_with_identifier(&real_mp4(), 0, UUID).unwrap();
    let mut hdlr = vec![0u8; 8];
    hdlr[4..8].copy_from_slice(b"mdta");
    hdlr.extend([0u8; 13]);
    let mut item = bx(
        b"mean",
        &[&[0u8; 4][..], b"com.apple.quicktime.metadata"].concat(),
    );
    item.extend(bx(
        b"name",
        &[&[0u8; 4][..], b"com.apple.quicktime.content.identifier"].concat(),
    ));
    item.extend(bx(
        b"data",
        &[&[0, 0, 0, 1, 0, 0, 0, 0][..], UUID.as_bytes()].concat(),
    ));
    let mut meta_body = vec![0u8; 4];
    meta_body.extend(bx(b"hdlr", &hdlr));
    meta_body.extend(bx(b"ilst", &bx(b"----", &item)));
    let old = with_moov_meta(&out, &bx(b"udta", &bx(b"meta", &meta_body)));
    assert_eq!(
        read_content_identifier(&old).unwrap(),
        Some(UUID.to_string())
    );
}

#[test]
fn iso_full_box_meta_identifier_reads() {
    let out = remux_to_mov_with_identifier(&real_mp4(), 0, UUID).unwrap();
    let top = boxes(&out, 0, out.len());
    let (moov, moov_end) = find(&top, b"moov");
    let (meta, meta_end) = find(&boxes(&out, moov + 8, moov_end), b"meta");
    let mut body = vec![0u8; 4];
    body.extend(&out[meta + 8..meta_end]);
    let iso = with_moov_meta(&out, &bx(b"meta", &body));
    assert_eq!(
        read_content_identifier(&iso).unwrap(),
        Some(UUID.to_string())
    );
}
