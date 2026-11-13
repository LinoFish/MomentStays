mod engine_support;
use engine_support::{b64, Dir};
use momentstays_core::{
    engine::{
        batch::{self, RunRequest},
        hdr,
        options::ConversionOptions,
        output, scanrow, static_hdr, NoNative, StopToken,
    },
    format,
};
fn image() -> Vec<u8> {
    let base = b64("tests/fixtures-pycli/base.jpg.b64");
    hdr::ultra(&base, &base, 2.0).unwrap()
}
#[test]
fn existing_standard_gainmap_is_delivered_without_rebuilding_or_counting_conversion() {
    let dir = Dir::new("static-hdr-noop");
    let source = dir.0.join("hdr.jpg");
    let bytes = image();
    std::fs::write(&source, &bytes).unwrap();
    for direction in ["to-apple", "to-android"] {
        let opts = ConversionOptions {
            direction: direction.into(),
            ..Default::default()
        };
        let out = dir.0.join(direction);
        let result = static_hdr::convert(
            &NoNative,
            &source,
            &out,
            &opts,
            &StopToken::new(),
            false,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(result["status"], "converted");
        assert_eq!(result["processing_kind"], "original-copy");
        assert_eq!(result["hdr"]["outcome"], "preserved");
        assert_eq!(std::fs::read(out.join("hdr.jpg")).unwrap(), bytes);
        assert_eq!(
            output::verify(&out).unwrap()["processing_kind"],
            "original-copy"
        );
        let resumed = static_hdr::convert(
            &NoNative,
            &source,
            &out,
            &opts,
            &StopToken::new(),
            false,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(resumed["status"], "resumed");
        assert_eq!(resumed["processing_kind"], "original-copy");
    }
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
}

#[test]
fn honor_editor_standard_gainmap_keeps_empty_footer_and_counts_only_copy() {
    let dir = Dir::new("honor-editor-hdr");
    std::fs::create_dir_all(dir.0.join("input")).unwrap();
    let source = dir.0.join("input/hdr.jpg");
    let mut bytes = image();
    bytes.extend(b"{}\x02\0\0\0HiHonor_ExtInfo\0\0\0\0\0");
    std::fs::write(&source, &bytes).unwrap();
    assert_eq!(
        static_hdr::inspect(&NoNative, &bytes).unwrap(),
        Some("Ultra HDR")
    );
    assert_eq!(momentstays_core::jpeg::static_only(&bytes).unwrap(), bytes);
    let sources = [source.clone()];
    let options = ConversionOptions::default();
    let target = dir.0.join("output");
    let request = RunRequest {
        sources: &sources,
        output: &target,
        options: &options,
        scan_rows: None,
        concurrency: Some(1),
    };
    for _ in 0..2 {
        let result = batch::run(&NoNative, &request, &StopToken::new(), None, |_| {}).unwrap();
        assert_eq!(result.counts["copied"], 1);
        assert_eq!(result.counts["converted"], 0);
        assert_eq!(result.counts["failed"], 0);
    }
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    let mut unknown = bytes.clone();
    let at = unknown.len() - 24;
    unknown[at] = 3;
    assert!(!momentstays_core::scan::confirmed_static(&unknown));
    let mut sdr = b64("tests/fixtures-pycli/base.jpg.b64");
    sdr.extend(b"{}\x02\0\0\0HiHonor_ExtInfo\0\0\0\0\0");
    assert!(momentstays_core::scan::confirmed_static(&sdr));
    assert_eq!(static_hdr::inspect(&NoNative, &sdr).unwrap(), None);
}
#[test]
fn static_hdr_rejects_motion_options_and_malformed_gainmap() {
    let dir = Dir::new("static-options");
    std::fs::create_dir_all(dir.0.join("input")).unwrap();
    let source = dir.0.join("input/hdr.jpg");
    std::fs::write(&source, image()).unwrap();
    let opts = ConversionOptions {
        output_mode: "auxiliary-only".into(),
        ..Default::default()
    };
    assert!(static_hdr::convert(
        &NoNative,
        &source,
        &dir.0.join("bad"),
        &opts,
        &StopToken::new(),
        false,
        &Default::default()
    )
    .is_err());
    let mut broken = image();
    broken.truncate(broken.len() - 8);
    let row = scanrow::scan_file(&NoNative, &source, "hdr.jpg", Some(&broken));
    assert!(!row.is_static_hdr());
    assert!(static_hdr::convert(
        &NoNative,
        &source,
        &dir.0.join("cancel"),
        &Default::default(),
        &{
            let t = StopToken::new();
            t.cancel();
            t
        },
        false,
        &Default::default()
    )
    .is_err());
}
#[test]
fn ordinary_static_sdr_remains_skipped() {
    let dir = Dir::new("static-sdr");
    std::fs::create_dir_all(dir.0.join("input")).unwrap();
    let source = dir.0.join("input/sdr.jpg");
    std::fs::write(&source, b64("tests/fixtures-pycli/base.jpg.b64")).unwrap();
    let row = scanrow::scan_file(&NoNative, &source, "sdr.jpg", None);
    assert!(row.confirmed_static && !row.is_static_hdr());
    let opts = Default::default();
    let sources = [source];
    let out = dir.0.join("out");
    let req = RunRequest {
        sources: &sources,
        output: &out,
        options: &opts,
        scan_rows: None,
        concurrency: None,
    };
    let result = batch::run(&NoNative, &req, &StopToken::new(), None, |_| {}).unwrap();
    assert_eq!(result.counts["skipped"], 1);
    assert_eq!(result.counts["converted"], 0);
    assert!(format::parse(&image()).is_err());
}

#[test]
fn live_identifier_cannot_fall_back_to_standalone_hdr_when_pair_is_missing() {
    use momentstays_core::engine::live::{honor_to_google, pairing};
    let d = Dir::new("hdr-live-pair-required");
    let path = d.0.join("source.jpg");
    let data = momentstays_core::engine::still::live_still(
        &image(),
        "F5185798-5479-4BFB-A28F-5BCE0DD48263",
        true,
    )
    .unwrap();
    std::fs::write(&path, &data).unwrap();
    assert!(static_hdr::inspect(&NoNative, &data).is_err());
    let row = pairing::scan_file_with_native(&NoNative, &path, "");
    assert!(!row.is_motion && !row.is_static_hdr());
    assert!(!honor_to_google::scan_file(&NoNative, &path, "").is_static_hdr());
    assert!(static_hdr::convert(
        &NoNative,
        &path,
        &d.0.join("out"),
        &ConversionOptions::default(),
        &StopToken::new(),
        false,
        &Default::default()
    )
    .is_err());
    assert!(!d.0.join("out").exists());
    assert_eq!(std::fs::read(path).unwrap(), data);
}
