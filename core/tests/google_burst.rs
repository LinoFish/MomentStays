mod engine_support;
use engine_support::{b64, counts, files, run, Dir};
use momentstays_core::{
    engine::{batch, burst, options::ConversionOptions, output, scanrow, NoNative},
    jpeg, tiff, xmp,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const NAMES: [&str; 3] = [
    "IMG_20230108_205201_BURST000_COVER.jpg",
    "IMG_20230108_205201_BURST001.jpg",
    "IMG_20230108_205201_BURST002.jpg",
];

fn photo(make: &str, properties: &str) -> Vec<u8> {
    let mut t = b"II\x2a\0\x08\0\0\0".to_vec();
    let make = format!("{make}\0");
    let model = b"Pixel test\0";
    t.extend(tiff::serialize(
        true,
        &[
            tiff::entry(true, 0x010f, 2, make.len(), 38),
            tiff::entry(true, 0x0110, 2, model.len(), 38 + make.len()),
        ],
        0,
    ));
    t.extend(make.as_bytes());
    t.extend(model);
    let mut exif = jpeg::EXIF.to_vec();
    exif.extend(t);
    let mut out = b64("tests/fixtures-pycli/base.jpg.b64");
    out.splice(2..2, jpeg::app(0xe1, &exif).unwrap());
    if !properties.is_empty() {
        let xml = format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:RDF><rdf:Description xmlns:g="http://ns.google.com/photos/1.0/camera/" xmlns:c="http://ns.google.com/photos/1.0/creations/" {properties}</rdf:Description></rdf:RDF></x:xmpmeta>"#
        );
        let mut payload = xmp::HEADER.to_vec();
        payload.extend(xml.as_bytes());
        out.splice(2..2, jpeg::app(0xe1, &payload).unwrap());
    }
    out
}
fn fixture(dir: &Path, properties: &[&str]) -> Vec<PathBuf> {
    std::fs::create_dir_all(dir).unwrap();
    NAMES
        .iter()
        .zip(properties)
        .map(|(name, props)| {
            let p = dir.join(name);
            std::fs::write(&p, photo("Google", props)).unwrap();
            p
        })
        .collect()
}
fn plan(inputs: &[PathBuf]) -> Value {
    burst::plans(
        &inputs
            .iter()
            .map(|p| scanrow::scan_file(&NoNative, p, "", None))
            .collect::<Vec<_>>(),
    )
    .remove(0)
}
fn folder(result: &batch::Outcome) -> PathBuf {
    PathBuf::from(files(result)[0]["output"].as_str().unwrap())
}

#[test]
fn google_zero_based_protocol_requires_make_and_explicit_frame_names() {
    assert!(burst::filename(NAMES[0], "Google").is_some());
    assert!(burst::filename(NAMES[0], "").is_none());
    for make in ["Apple", "Samsung", "Xiaomi"] {
        assert!(burst::filename(NAMES[1], make).is_none());
    }
    for name in [
        "renamed.jpg",
        "IMG_20230230_205201_BURST000.jpg",
        "PXL_20230108.jpg",
        "IMG_20230108_205201_BURST000.heic",
    ] {
        assert!(burst::filename(name, "Google").is_none());
    }
    let metadata = photo(
        "Google",
        r#"g:BurstID="portrait-or-burst" g:BurstPrimary="1">"#,
    );
    assert!(burst::inspect(Path::new("portrait.jpg"), &metadata).is_none());
    assert!(burst::inspect(Path::new(NAMES[0]), &photo("Samsung", "")).is_none());
}

#[test]
fn google_group_converts_archives_resumes_and_retains_xmp() {
    let d = Dir::new("google-burst");
    let inputs = fixture(
        &d.0.join("in"),
        &[
            r#"g:BurstID="group-a" c:CameraBurstID="group-a" g:BurstPrimary="1">"#,
            r#"><g:BurstID>group-a</g:BurstID><g:BurstPrimary>0</g:BurstPrimary>"#,
            r#"g:BurstID="group-a" g:BurstPrimary="false">"#,
        ],
    );
    let p = plan(&inputs);
    assert_eq!(p["source_format"], "google-burst");
    assert_eq!(p["members"][0]["index"], 0);
    assert_eq!(p["has_gaps"], false);
    assert_eq!(p["representative"], inputs[0].to_str().unwrap());
    let result = run(
        &inputs,
        &d.0.join("out"),
        &ConversionOptions::default(),
        None,
    );
    assert_eq!(counts(&result), [3, 0, 0, 0], "{result:?}");
    let m = output::verify(&folder(&result)).unwrap();
    assert_eq!(m["source_format"], "google-burst");
    assert_eq!(m["implementation"], "google-burst-1");
    for (i, input) in inputs.iter().enumerate() {
        let original = std::fs::read(input).unwrap();
        assert_eq!(
            std::fs::read(folder(&result).join(format!("source-burst-{i:03}.bin"))).unwrap(),
            original
        );
        let out = std::fs::read(folder(&result).join(NAMES[i])).unwrap();
        assert_eq!(
            jpeg::coding_hash(&out).unwrap(),
            jpeg::coding_hash(&original).unwrap()
        );
        assert_eq!(
            xmp::packets(&out).unwrap()[0].xml,
            xmp::packets(&original).unwrap()[0].xml
        );
    }
    assert_eq!(
        counts(&run(
            &inputs,
            &d.0.join("out"),
            &ConversionOptions::default(),
            Some(batch::ExistingOutput::Resume)
        )),
        [0, 3, 0, 0]
    );
    let mut bad = m;
    bad["source_format"] = json!("honor-burst");
    assert!(burst::verify(&folder(&result), &bad).is_err());
}

#[test]
fn google_filename_only_gaps_and_partial_selection_keep_existing_safety() {
    let d = Dir::new("google-burst-partial");
    let inputs = fixture(&d.0.join("in"), &["", "", ""]);
    assert_eq!(plan(&inputs)["supported"], true);
    assert_eq!(
        counts(&run(
            &inputs[..2],
            &d.0.join("reject"),
            &ConversionOptions::default(),
            None
        )),
        [0, 0, 0, 2]
    );
    std::fs::remove_file(&inputs[1]).unwrap();
    let selected = vec![inputs[0].clone(), inputs[2].clone()];
    let p = plan(&selected);
    assert_eq!(p["has_gaps"], true);
    assert_eq!(
        counts(&run(
            &selected,
            &d.0.join("gapped"),
            &ConversionOptions::default(),
            None
        )),
        [2, 0, 0, 0]
    );
    let mut options = ConversionOptions {
        burst_plans: vec![p.clone()],
        ..Default::default()
    };
    options.burst_choices.insert(
        p["id"].as_str().unwrap().into(),
        json!({"signature":p["signature"],"independent":true}),
    );
    let copied = run(&selected[..1], &d.0.join("copy"), &options, None);
    assert_eq!(copied.counts["copied"], 1, "{copied:?}");
    assert!(output::verify(&folder(&copied)).unwrap()["burst"]["uuid"].is_null());
}

#[test]
fn google_metadata_conflicts_and_portraits_are_not_silently_grouped() {
    let d = Dir::new("google-burst-conflicts");
    for props in [
        r#"g:BurstID="a" c:CameraBurstID="b">"#,
        r#"g:BurstPrimary="0">"#,
        r#"g:BurstPrimary="unexpected">"#,
        r#"g:BurstID="a"><g:BurstID>b</g:BurstID>"#,
        r#"g:PortraitVersion="1">"#,
        r#"><g:SpecialTypeID><rdf:Bag><rdf:li>com.google.SpecialType-PORTRAIT</rdf:li></rdf:Bag></g:SpecialTypeID>"#,
    ] {
        let evidence = burst::inspect(Path::new(NAMES[0]), &photo("Google", props)).unwrap();
        assert_eq!(evidence["supported"], false, "{props}: {evidence}");
    }
    for (case, second) in [r#"g:BurstID="b">"#, ">"].iter().enumerate() {
        let inputs = fixture(
            &d.0.join(format!("in-{case}")),
            &[r#"g:BurstID="a">"#, second],
        );
        assert_eq!(plan(&inputs)["supported"], false);
        assert_eq!(
            counts(&run(
                &inputs,
                &d.0.join(format!("out-{case}")),
                &ConversionOptions::default(),
                None
            )),
            [0, 0, 0, 2]
        );
    }
}

#[test]
fn google_namespace_and_primary_evidence_are_respected() {
    let d = Dir::new("google-burst-namespaces");
    let inputs = fixture(
        &d.0.join("a"),
        &[
            r#"xmlns:fake="urn:fake" fake:BurstID="wrong" fake:BurstPrimary="0">"#,
            r#"g:BurstPrimary="1">"#,
        ],
    );
    let p = plan(&inputs);
    assert_eq!(p["supported"], true);
    assert_eq!(p["source_cover_count"], 2);
    assert!(p["representative"].is_null());
    let other = fixture(&d.0.join("b"), &["", ""]);
    assert_ne!(p["id"], plan(&other)["id"]);
    let mut options = ConversionOptions::default();
    options.burst_choices.insert(
        p["id"].as_str().unwrap().into(),
        json!({"signature":p["signature"],"representative":inputs[1].to_str().unwrap()}),
    );
    assert_eq!(
        counts(&run(&inputs, &d.0.join("out"), &options, None)),
        [2, 0, 0, 0]
    );
}

#[test]
fn google_extended_xmp_and_unsupported_containers_remain_explicit() {
    let data = photo("Google", "");
    let xml = br#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:camera="http://ns.google.com/photos/1.0/camera/" camera:BurstID="extended-group" camera:BurstPrimary="1"/>"#;
    let mut payload = xmp::EXTENDED.to_vec();
    payload.extend(xmp::md5_upper(xml).as_bytes());
    payload.extend((xml.len() as u32).to_be_bytes());
    payload.extend(0u32.to_be_bytes());
    payload.extend(xml);
    let mut extended = data.clone();
    extended.splice(2..2, jpeg::app(0xe1, &payload).unwrap());
    let frame = burst::inspect(Path::new(NAMES[0]), &extended).unwrap();
    assert_eq!(frame["supported"], true);
    assert_eq!(frame["google_group"], "extended-group");
    // Broken Extended XMP must remain a visible unsupported burst candidate.
    let at = extended.windows(xml.len()).position(|w| w == xml).unwrap();
    extended[at] = b'!';
    assert_eq!(
        burst::inspect(Path::new(NAMES[0]), &extended).unwrap()["supported"],
        false
    );
    for properties in [r#"g:MotionPhoto="1">"#, r#"g:BurstID="unclosed""#] {
        let frame = burst::inspect(Path::new(NAMES[0]), &photo("Google", properties)).unwrap();
        assert_eq!(frame["supported"], false, "{frame}");
    }
}
