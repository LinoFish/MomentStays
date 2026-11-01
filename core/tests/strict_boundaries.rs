mod common;
use common::{build_still, bx};
use momentstays_core::{format, heifwrite};

#[test]
fn reject_hidden_unknown_heif_payload() {
    let mut still = build_still(true, true);
    still.extend(4u32.to_be_bytes());
    still.extend(b"uuid");
    still.extend(b"private-metadata-must-not-disappear");
    let out = heifwrite::write_xmp(&still, b"<x/>");
    assert!(
        out.is_err(),
        "malformed unknown box was silently accepted and discarded"
    );
}

#[test]
fn reject_truncated_video_box() {
    // Minimal container declaration: mdat promises 108 bytes, only 8 exist.
    let mut video = bx("ftyp", b"isom0000");
    video.extend(bx("moov", b""));
    video.extend(108u32.to_be_bytes());
    video.extend(b"mdat");
    let still = build_still(true, true);
    let mut motion = heifwrite::assemble_heic(
        &still,
        &{
            let mut valid = bx("ftyp", b"isom0000");
            valid.extend(bx("moov", b""));
            valid.extend(bx("mdat", b""));
            valid
        },
        momentstays_core::model::Vendor::Google,
        42,
    )
    .unwrap();
    // Locate mpvd by walking valid top-level boxes, then replace payload.
    let boxes = momentstays_core::mp4::boxes(&motion, 0, motion.len()).unwrap();
    let at = boxes.iter().find(|b| b.is(b"mpvd")).unwrap().start;
    motion.truncate(at);
    motion.extend(bx("mpvd", &video));
    assert!(
        format::parse(&motion).is_err(),
        "truncated mdat accepted as valid motion video"
    );
}

#[test]
fn explicit_tolerance_reports_partial_walks_without_weakening_default() {
    use momentstays_core::mp4::{boxes, boxes_with_mode, BoxParseMode};
    let valid = bx("free", b"payload");
    let strict = boxes_with_mode(&valid, 0, valid.len(), BoxParseMode::Strict).unwrap();
    let tolerant = boxes_with_mode(&valid, 0, valid.len(), BoxParseMode::Tolerant).unwrap();
    assert!(strict.complete && tolerant.complete);
    assert_eq!(strict.boxes, tolerant.boxes);
    let mut cases = Vec::new();
    for tail in [
        vec![1, 2, 3],
        [4u32.to_be_bytes().as_slice(), b"uuid"].concat(),
        [1u32.to_be_bytes().as_slice(), b"uuid"].concat(),
    ] {
        let mut bad = valid.clone();
        bad.extend(tail);
        cases.push(bad);
    }
    let mut clipped = valid.clone();
    clipped.extend(100u32.to_be_bytes());
    clipped.extend(b"mdat");
    cases.push(clipped);
    for input in cases {
        assert!(boxes(&input, 0, input.len()).is_err());
        assert!(boxes_with_mode(&input, 0, input.len(), BoxParseMode::Strict).is_err());
        let walk = boxes_with_mode(&input, 0, input.len(), BoxParseMode::Tolerant).unwrap();
        assert!(!walk.complete);
        assert_eq!(walk.boxes[0], strict.boxes[0]);
        assert!(walk
            .boxes
            .iter()
            .all(|b| b.start <= b.payload && b.payload <= b.end && b.end <= input.len()));
    }
    assert!(boxes_with_mode(&valid, 1, valid.len() + 1, BoxParseMode::Tolerant).is_err());
}
