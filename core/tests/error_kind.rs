//! `ErrorKind`: callers (host apps, the C ABI JSON) can branch on what
//! failed without matching message text.

use base64::Engine;
use momentstays_core::model::{self, Cover, MotionModel, Vendor};
use momentstays_core::{format, live, output, remux, ErrorKind};

fn b64(path: &str) -> Vec<u8> {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    base64::engine::general_purpose::STANDARD
        .decode(std::fs::read_to_string(p).unwrap().trim_ascii())
        .unwrap()
}

#[test]
fn kinds_for_representative_failures() {
    let still = b64("tests/fixtures-pycli/static.jpg.b64");
    assert_eq!(
        format::parse(&still).unwrap_err().kind(),
        ErrorKind::NotMotionPhoto
    );
    assert_eq!(
        format::parse(b"\xFF\xD8\xFF").unwrap_err().kind(),
        ErrorKind::Malformed
    );

    let mp4 = b64("tests/fixtures-samples/real.mp4.b64");
    assert_eq!(
        remux::remux_to_mov_with_identifier(&mp4, 0, "")
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidArgument
    );
    // 0.4 s is frame 0 on the media timeline, frame 2 on the presentation one.
    assert_eq!(
        live::resolve_cover(&mp4, Cover::Time(400_000), live::TimeDomain::Auto, 0)
            .unwrap_err()
            .kind(),
        ErrorKind::Cover
    );
    let m = MotionModel {
        still: b64("tests/fixtures-pycli/base.jpg.b64"),
        video: mp4,
        cover: Cover::Time(0),
        source_format: "t".into(),
    };
    assert_eq!(
        model::write(&m, Vendor::Huawei).unwrap_err().kind(),
        ErrorKind::Cover
    );
    let google = model::write(&m, Vendor::Google).unwrap();
    let p = format::parse(&google).unwrap();
    assert_eq!(
        momentstays_core::heifwrite::plan_heic(&p.still, p.video(&google), Vendor::Vivo, 0)
            .unwrap_err()
            .kind(),
        ErrorKind::Unsupported
    );

    let missing = std::env::temp_dir()
        .join("ms-no-such-dir-xyz")
        .join("o.bin");
    let parts: [&[u8]; 1] = [b"x"];
    assert_eq!(
        output::write_atomic(&missing, &parts, output::same_bytes(&parts))
            .unwrap_err()
            .kind(),
        ErrorKind::Io
    );
    let parts: [&[u8]; 1] = [b"x"];
    let ok = std::env::temp_dir().join(format!("ms-kind-{}", std::process::id()));
    let e = output::write_atomic(&ok, &parts, |_| {
        Err(momentstays_core::Error::with_kind(
            ErrorKind::VerifyFailed,
            "x",
        ))
    })
    .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::VerifyFailed);
}

#[test]
fn inspect_json_carries_the_kind() {
    let v = momentstays_core::inspect(&b64("tests/fixtures-pycli/static.jpg.b64"));
    assert_eq!(v["ok"], false);
    assert_eq!(v["error_kind"], "not_motion_photo");
    assert!(!v["error"].as_str().unwrap().is_empty());
}
