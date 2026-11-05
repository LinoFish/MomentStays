mod engine_support;
use engine_support::{b64, Dir};
use momentstays_core::{
    engine::{
        batch::{self, ExistingOutput, RunRequest},
        hdr,
        options::ConversionOptions,
        output, scanrow, static_hdr, NoNative, StopToken,
    },
    format,
};
use serde_json::json;
fn image() -> Vec<u8> {
    let base = b64("tests/fixtures-pycli/base.jpg.b64");
    hdr::ultra(&base, &base, 2.0).unwrap()
}
#[test]
fn static_hdr_batch_delivers_without_movie_and_resumes() {
    let dir = Dir::new("static-hdr");
    std::fs::create_dir_all(dir.0.join("input")).unwrap();
    let source = dir.0.join("input/hdr.jpg");
    let bytes = image();
    std::fs::write(&source, &bytes).unwrap();
    let row = scanrow::scan_file(&NoNative, &source, "hdr.jpg", None);
    assert!(!row.is_motion && row.confirmed_static && row.is_static_hdr());
    for direction in ["to-apple", "to-android", "android-to-android"] {
        let opts = ConversionOptions {
            direction: direction.into(),
            ..Default::default()
        };
        let out = dir.0.join(direction);
        let inputs = [source.clone()];
        let req = RunRequest {
            sources: &inputs,
            output: &out,
            options: &opts,
            scan_rows: None,
            concurrency: Some(1),
        };
        let first = batch::run(&NoNative, &req, &StopToken::new(), None, |_| {}).unwrap();
        assert_eq!(first.counts["converted"], 1, "{first:?}");
        let target = out.join("hdr.jpg.static-hdr");
        let m = output::verify(&target).unwrap();
        assert!(m.get("identifier").is_none());
        assert_eq!(m["timing"], json!({"applicable":false}));
        assert_eq!(m["output_files"].as_object().unwrap().len(), 1);
        assert_eq!(std::fs::read(target.join("hdr.jpg")).unwrap(), bytes);
        assert_eq!(
            batch::run(&NoNative, &req, &StopToken::new(), None, |_| {})
                .unwrap()
                .counts["resumed"],
            1
        );
        std::fs::write(target.join("hdr.jpg"), b"bad").unwrap();
        assert_eq!(
            batch::run(&NoNative, &req, &StopToken::new(), None, |_| {})
                .unwrap()
                .counts["failed"],
            1
        );
        assert_eq!(
            batch::run(
                &NoNative,
                &req,
                &StopToken::new(),
                Some(&|_, _| ExistingOutput::Overwrite),
                |_| {}
            )
            .unwrap()
            .counts["converted"],
            1
        );
    }
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
