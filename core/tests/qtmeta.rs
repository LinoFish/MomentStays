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
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

const UUID: &str = "123E4567-E89B-12D3-A456-426614174000";

#[test]
fn identifier_roundtrip() {
    let src = real_mp4();
    let out = remux_to_mov_with_identifier(&src, 0, UUID).expect("remux with identifier");
    assert_eq!(read_content_identifier(&out).expect("read identifier"), Some(UUID.to_string()));
    check_remux(&src, &out, 0).expect("remux still verifies");
    // Debug artifact for local inspection; temp_dir() so this also
    // passes on Windows, where no /tmp directory exists.
    std::fs::write(std::env::temp_dir().join("real-identifier.mov"), &out).unwrap();
}

#[test]
fn identifier_absent_without_it() {
    let src = real_mp4();
    let out = remux_to_mov(&src, 0).expect("plain remux");
    assert_eq!(read_content_identifier(&out).expect("read identifier"), None);
}

#[test]
fn empty_identifier_is_refused() {
    let src = real_mp4();
    assert!(remux_to_mov_with_identifier(&src, 0, "").is_err());
}
