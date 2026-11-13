mod engine_support;
use engine_support::{b64, counts, files, run, Dir};
use momentstays_core::{
    engine::{batch, burst, options::ConversionOptions, output, scanrow, NoNative},
    jpeg, tiff,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn photo(make: &str) -> Vec<u8> {
    let mut t = b"II\x2a\0\x08\0\0\0".to_vec();
    let make = format!("{make}\0");
    let model = b"Test model\0";
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
    out
}
fn sef(mut image: Vec<u8>, version: u32, records: &[(u16, &str, &[u8])]) -> Vec<u8> {
    let mut ranges = Vec::new();
    for (tag, name, data) in records {
        let start = image.len();
        image.extend(0u16.to_le_bytes());
        image.extend(tag.to_le_bytes());
        image.extend((name.len() as u32).to_le_bytes());
        image.extend(name.as_bytes());
        image.extend(*data);
        ranges.push((*tag, start, image.len() - start));
    }
    let head = image.len();
    image.extend(b"SEFH");
    image.extend(version.to_le_bytes());
    image.extend((records.len() as u32).to_le_bytes());
    for (tag, start, length) in ranges {
        image.extend(0u16.to_le_bytes());
        image.extend(tag.to_le_bytes());
        image.extend(((head - start) as u32).to_le_bytes());
        image.extend((length as u32).to_le_bytes());
    }
    image.extend((12 + 12 * records.len() as u32).to_le_bytes());
    image.extend(b"SEFT");
    image
}
fn samsung(group: &[u8]) -> Vec<u8> {
    sef(
        photo("samsung"),
        107,
        &[
            (0x09e0, "Burst_Shot_Info", group),
            (0x09e1, "BurstShot_Best_Photo_Info", b"4"),
            (0x0a01, "Image_UTC_Data", b"1700000000000"),
        ],
    )
}
fn save(dir: &Path, names: &[&str], data: &[u8]) -> Vec<PathBuf> {
    std::fs::create_dir_all(dir).unwrap();
    names
        .iter()
        .map(|name| {
            let path = dir.join(name);
            std::fs::write(&path, data).unwrap();
            path
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
const HUAWEI: [&str; 3] = [
    "IMG_20230108_205201_BURST001_COVER.jpg",
    "IMG_20230108_205201_BURST002.jpg",
    "IMG_20230108_205201_BURST004.jpg",
];
const SAMSUNG: [&str; 3] = [
    "20230108_205201_001.jpg",
    "20230108_205201_002.jpg",
    "20230108_205201_004.jpg",
];

#[test]
fn huawei_known_honor_protocol_uses_separate_brand_contract_and_archives() {
    let d = Dir::new("huawei-burst");
    let inputs = save(&d.0.join("in"), &HUAWEI, &photo("HUAWEI"));
    let p = plan(&inputs);
    assert_eq!(p["source_format"], "huawei-burst");
    assert_eq!(p["has_gaps"], true);
    let honor = burst::inspect(&inputs[0], &photo("HONOR")).unwrap();
    assert_ne!(honor["group_id"], p["id"]);
    assert!(burst::filename("IMG_20230108_205201_BURST000.jpg", "HUAWEI").is_none());
    assert!(burst::filename("IMG_20230108_205201_BURST001.heic", "HUAWEI").is_none());
    assert!(burst::filename("IMG_20230108_205201_NEWBURST001.jpg", "HUAWEI").is_none());
    let result = run(
        &inputs,
        &d.0.join("out"),
        &ConversionOptions::default(),
        None,
    );
    assert_eq!(counts(&result), [3, 0, 0, 0], "{result:?}");
    let m = output::verify(&folder(&result)).unwrap();
    assert_eq!(m["source_format"], "huawei-burst");
    assert_eq!(m["implementation"], "huawei-burst-1");
    for (i, path) in inputs.iter().enumerate() {
        assert_eq!(
            std::fs::read(path).unwrap(),
            std::fs::read(folder(&result).join(format!("source-burst-{i:03}.bin"))).unwrap()
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
    let bad = burst::inspect(
        &inputs[0],
        &[photo("HUAWEI"), b"unknown-new-tail".to_vec()].concat(),
    )
    .unwrap();
    assert_eq!(bad["supported"], false);
}

#[test]
fn samsung_requires_sef_evidence_and_explicit_representative_then_preserves_tail() {
    let d = Dir::new("samsung-burst");
    let inputs = save(&d.0.join("in"), &SAMSUNG, &samsung(b"489489125"));
    let p = plan(&inputs);
    assert_eq!(p["source_format"], "samsung-burst");
    assert_eq!(p["supported"], true, "{p}");
    assert_eq!(p["has_gaps"], true);
    assert!(
        p["representative"].is_null(),
        "unknown best-frame payload is never guessed"
    );
    assert_eq!(
        counts(&run(
            &inputs,
            &d.0.join("no-choice"),
            &ConversionOptions::default(),
            None
        )),
        [0, 0, 0, 3]
    );
    let mut options = ConversionOptions::default();
    options.burst_choices.insert(
        p["id"].as_str().unwrap().into(),
        json!({"signature":p["signature"],"representative":inputs[1].to_str().unwrap()}),
    );
    let result = run(&inputs, &d.0.join("out"), &options, None);
    assert_eq!(counts(&result), [3, 0, 0, 0], "{result:?}");
    let m = output::verify(&folder(&result)).unwrap();
    assert_eq!(m["source_format"], "samsung-burst");
    assert_eq!(m["implementation"], "samsung-burst-1");
    for (i, path) in inputs.iter().enumerate() {
        let original = std::fs::read(path).unwrap();
        let out = std::fs::read(folder(&result).join(SAMSUNG[i])).unwrap();
        assert_eq!(
            &original[jpeg::image_end(&original).unwrap()..],
            &out[jpeg::image_end(&out).unwrap()..]
        );
        assert_eq!(
            jpeg::coding_hash(&out).unwrap(),
            jpeg::coding_hash(&original).unwrap()
        );
        assert_eq!(
            burst::inspect(Path::new(SAMSUNG[i]), &out).unwrap()["supported"],
            true
        );
        assert_eq!(
            original,
            std::fs::read(folder(&result).join(format!("source-burst-{i:03}.bin"))).unwrap()
        );
    }
    assert_eq!(
        counts(&run(&inputs, &d.0.join("out"), &options, None)),
        [0, 3, 0, 0]
    );
}

#[test]
fn samsung_unknown_versions_records_and_ambiguous_copies_never_convert() {
    let file = Path::new(SAMSUNG[0]);
    let older = sef(
        photo("Samsung"),
        106,
        &[(0x09e0, "Burst_Shot_Info", b"123")],
    );
    assert_eq!(burst::inspect(file, &older).unwrap()["supported"], true);
    let bare = burst::inspect(file, &photo("Samsung")).unwrap();
    assert_eq!(bare["supported"], false);
    assert!(burst::inspect(file, &photo("Google")).is_none());
    assert!(burst::filename("20230230_205201_001.jpg", "Samsung").is_none());
    assert!(burst::filename("20230108_205201_000.jpg", "Samsung").is_none());
    assert!(burst::filename("20230108_205201_001.heic", "Samsung").is_none());
    let cases: Vec<Vec<u8>> = vec![
        sef(
            photo("Samsung"),
            108,
            &[(0x09e0, "Burst_Shot_Info", b"123")],
        ),
        sef(
            photo("Samsung"),
            107,
            &[(0x09e0, "Burst_Shot_Info", b"\x01\x00\x00\x00")],
        ),
        sef(photo("Samsung"), 107, &[(0x09e0, "Wrong_Name", b"123")]),
        sef(
            photo("Samsung"),
            107,
            &[(0x09e1, "BurstShot_Best_Photo_Info", b"1")],
        ),
        sef(
            photo("Samsung"),
            107,
            &[
                (0x09e0, "Burst_Shot_Info", b"123"),
                (0x09e0, "Burst_Shot_Info", b"123"),
            ],
        ),
        sef(
            photo("Samsung"),
            107,
            &[
                (0x09e0, "Burst_Shot_Info", b"123"),
                (0x0a30, "MotionPhoto_Data", b"video"),
            ],
        ),
        sef(
            photo("Samsung"),
            107,
            &[
                (0x09e0, "Burst_Shot_Info", b"123"),
                (0xffff, "Future_Data", b"123"),
            ],
        ),
        sef(
            [photo("Samsung"), b"unindexed-video".to_vec()].concat(),
            107,
            &[(0x09e0, "Burst_Shot_Info", b"123")],
        ),
    ];
    for data in cases {
        assert_eq!(burst::inspect(file, &data).unwrap()["supported"], false);
    }
    let mut corrupt = samsung(b"123");
    let end = corrupt.len();
    corrupt[end - 8..end - 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(burst::inspect(file, &corrupt).unwrap()["supported"], false);
}

#[test]
fn samsung_partial_inventory_conflicting_evidence_and_independent_export() {
    let d = Dir::new("samsung-burst-safety");
    let inputs = save(&d.0.join("in"), &SAMSUNG, &samsung(b"123"));
    assert_eq!(
        counts(&run(
            &inputs[..2],
            &d.0.join("partial"),
            &ConversionOptions::default(),
            None
        )),
        [0, 0, 0, 2]
    );
    std::fs::write(&inputs[1], samsung(b"456")).unwrap();
    assert_eq!(plan(&inputs)["supported"], false);
    std::fs::write(&inputs[1], samsung(b"123")).unwrap();
    let p = plan(&inputs);
    let mut options = ConversionOptions {
        burst_plans: vec![p.clone()],
        ..Default::default()
    };
    options.burst_choices.insert(
        p["id"].as_str().unwrap().into(),
        json!({"signature":p["signature"],"independent":true}),
    );
    let r = run(&inputs[..1], &d.0.join("independent"), &options, None);
    assert_eq!(r.counts["copied"], 1, "{r:?}");
    assert_eq!(
        std::fs::read(folder(&r).join(SAMSUNG[0])).unwrap(),
        std::fs::read(&inputs[0]).unwrap()
    );
    assert_eq!(
        output::verify(&folder(&r)).unwrap()["source_format"],
        "samsung-burst"
    );
    std::fs::write(&inputs[1], samsung(b"789")).unwrap();
    assert_eq!(
        counts(&run(&inputs, &d.0.join("stale"), &options, None)),
        [0, 0, 0, 3]
    );
}
