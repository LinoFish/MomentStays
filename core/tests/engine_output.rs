//! Existing output restoration collision check, through the output domain API.
mod engine_support;
use engine_support::Dir;
use momentstays_core::engine::{output, StopToken};
use serde_json::json;
use std::path::Path;

#[test]
fn legacy_name_restore_never_overwrites_an_unlisted_user_file() {
    let dir = Dir::new("rename-collision");
    let pair = dir.0.join("pair");
    std::fs::create_dir(&pair).unwrap();
    std::fs::write(pair.join("photo.jpg"), b"old still").unwrap();
    std::fs::write(pair.join("photo.mov"), b"old movie").unwrap();
    std::fs::write(pair.join("source.jpg"), b"user file").unwrap();
    let original = json!({"timing":{},"hdr":{},"implementation":"gui-native-1.0.3",
        "output_sha256":{"photo.jpg":momentstays_core::sha256_hex(b"old still"),"photo.mov":momentstays_core::sha256_hex(b"old movie")}});
    let bytes = serde_json::to_vec(&original).unwrap();
    std::fs::write(pair.join("manifest.json"), &bytes).unwrap();
    assert!(
        output::restore_names(&pair, Path::new("source.jpg"), &original, &StopToken::new())
            .is_err()
    );
    assert_eq!(
        std::fs::read(pair.join("source.jpg")).unwrap(),
        b"user file"
    );
    assert_eq!(std::fs::read(pair.join("photo.jpg")).unwrap(), b"old still");
    assert_eq!(std::fs::read(pair.join("photo.mov")).unwrap(), b"old movie");
    assert_eq!(std::fs::read(pair.join("manifest.json")).unwrap(), bytes);
    assert_eq!(
        std::fs::read_dir(&dir.0).unwrap().count(),
        1,
        "no staging left behind"
    );
}
