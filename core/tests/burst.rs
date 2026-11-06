mod engine_support;
use engine_support::{b64, counts, files, run, Dir};
use momentstays_core::engine::{
    batch, burst, hdr, options::ConversionOptions, output, request, scanrow, Native, NoNative,
    StopToken,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn fixture(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
    std::fs::create_dir_all(dir).unwrap();
    names
        .iter()
        .map(|n| {
            let p = dir.join(n);
            std::fs::write(&p, b64("tests/fixtures-pycli/base.jpg.b64")).unwrap();
            p
        })
        .collect()
}
const NAMES: [&str; 3] = [
    "IMG_20230108_205201_BURST001_COVER.JPG",
    "IMG_20230108_205201_BURST002.JPG",
    "IMG_20230108_205201_BURST003.JPG",
];
fn plan(inputs: &[PathBuf]) -> Value {
    burst::plans(
        &inputs
            .iter()
            .map(|p| scanrow::scan_file(&NoNative, p, "", None))
            .collect::<Vec<_>>(),
    )
    .remove(0)
}
fn target(result: &batch::Outcome) -> PathBuf {
    PathBuf::from(files(result)[0]["output"].as_str().unwrap())
}

#[test]
fn strict_protocol_never_guesses_time_or_other_manufacturers() {
    assert!(burst::filename(NAMES[0], "HONOR").is_some());
    assert!(burst::filename(NAMES[0], "Apple").is_none());
    for n in [
        "IMG_20230230_205201_BURST001.JPG",
        "IMG_20230108_255201_BURST001.JPG",
        "IMG_20230108_205201_BURST000.JPG",
        "IMG_20230108_205201_BURST01.JPG",
        "IMG_20230108_205201_BURST001.HEIC",
        "IMG_20230108_205201.JPG",
        "IMG_中文_205201_BURST001.JPG",
    ] {
        assert!(burst::filename(n, "").is_none(), "{n}");
    }
}
#[test]
fn stable_group_and_uuid_complete_atomic_output_resume_and_archives() {
    let d = Dir::new("burst-success");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let p = plan(&inputs);
    assert_eq!(p, plan(&inputs));
    let result = run(
        &inputs,
        &d.0.join("out"),
        &ConversionOptions::default(),
        None,
    );
    assert_eq!(counts(&result), [3, 0, 0, 0]);
    assert!(files(&result)
        .iter()
        .all(|e| e["processing_kind"] == "burst-marker-conversion"));
    let folder = target(&result);
    let m = output::verify(&folder).unwrap();
    assert_eq!(m["burst"]["members"].as_array().unwrap().len(), 3);
    assert_eq!(m["burst"]["original_total_known"], false);
    let ids: std::collections::HashSet<_> = m["burst"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["photo_identifier"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    for (i, input) in inputs.iter().enumerate() {
        assert_eq!(
            std::fs::read(input).unwrap(),
            std::fs::read(folder.join(format!("source-burst-{i:03}.bin"))).unwrap()
        );
    }
    assert_eq!(
        counts(&run(
            &inputs,
            &d.0.join("out"),
            &ConversionOptions::default(),
            None
        )),
        [0, 3, 0, 0]
    );
    assert_eq!(
        counts(&run(
            &inputs,
            &d.0.join("out"),
            &ConversionOptions::default(),
            Some(batch::ExistingOutput::Resume)
        )),
        [0, 3, 0, 0]
    );
    let second = run(
        &inputs,
        &d.0.join("other"),
        &ConversionOptions::default(),
        None,
    );
    assert_eq!(
        output::verify(&target(&second)).unwrap()["burst"]["uuid"],
        m["burst"]["uuid"]
    );
}
#[test]
fn source_namespaces_do_not_merge_and_sdr_stays_skipped() {
    let d = Dir::new("burst-namespace");
    let a = fixture(&d.0.join("a"), &NAMES);
    let b = fixture(&d.0.join("b"), &NAMES);
    assert_ne!(plan(&a)["id"], plan(&b)["id"]);
    let mut all = a;
    all.extend(b);
    all.extend(fixture(&d.0.join("ordinary"), &["nearby.jpg"]));
    assert_eq!(
        counts(&run(
            &all,
            &d.0.join("out"),
            &ConversionOptions::default(),
            None
        )),
        [6, 0, 1, 0]
    );
}
#[test]
fn partial_selection_requires_explicit_independent_export() {
    let d = Dir::new("burst-partial");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let p = plan(&inputs);
    let mut options = ConversionOptions {
        burst_plans: vec![p.clone()],
        ..Default::default()
    };
    assert_eq!(
        counts(&run(&inputs[..2], &d.0.join("reject"), &options, None)),
        [0, 0, 0, 2]
    );
    options.burst_choices.insert(
        p["id"].as_str().unwrap().into(),
        json!({"signature":p["signature"],"independent":true}),
    );
    let r = run(&inputs[..2], &d.0.join("separate"), &options, None);
    assert_eq!(counts(&r), [0, 0, 0, 0], "{r:?}");
    assert_eq!(r.counts["copied"], 2);
    assert!(files(&r)
        .iter()
        .all(|e| e["processing_kind"] == "original-copy"));
    let resumed = run(&inputs[..2], &d.0.join("separate"), &options, None);
    assert_eq!(resumed.counts["copied"], 2);
    assert_eq!(resumed.counts["resumed"], 0);
    assert!(files(&resumed)
        .iter()
        .all(|e| e["status"] == "resumed" && e["processing_kind"] == "original-copy"));
    let m = output::verify(&target(&r)).unwrap();
    assert!(m["burst"]["uuid"].is_null());
    for i in 0..2 {
        assert_eq!(
            std::fs::read(&inputs[i]).unwrap(),
            std::fs::read(target(&r).join(NAMES[i])).unwrap()
        );
    }
}
#[test]
fn gaps_are_allowed_while_duplicate_and_cover_rules_are_explicit() {
    let d = Dir::new("burst-sequence");
    let missing = fixture(&d.0.join("missing"), &[NAMES[0], NAMES[2]]);
    assert_eq!(plan(&missing)["ordered"], true);
    assert_eq!(plan(&missing)["has_gaps"], true);
    assert_eq!(
        counts(&run(
            &missing,
            &d.0.join("out-missing"),
            &ConversionOptions::default(),
            None
        )),
        [2, 0, 0, 0]
    );
    let duplicate = fixture(
        &d.0.join("duplicate"),
        &[NAMES[0], "IMG_20230108_205201_BURST001.JPG"],
    );
    assert_eq!(plan(&duplicate)["ordered"], false);
    let covers = fixture(
        &d.0.join("covers"),
        &[NAMES[0], "IMG_20230108_205201_BURST002_COVER.JPG"],
    );
    let p = plan(&covers);
    assert_eq!(
        counts(&run(
            &covers,
            &d.0.join("out-covers"),
            &ConversionOptions::default(),
            None
        )),
        [0, 0, 0, 2]
    );
    let mut options = ConversionOptions::default();
    options.burst_choices.insert(
        p["id"].as_str().unwrap().into(),
        json!({"signature":p["signature"],"representative":covers[1]}),
    );
    let r = run(&covers, &d.0.join("chosen"), &options, None);
    assert_eq!(counts(&r), [2, 0, 0, 0], "{r:?}");
    assert_eq!(
        output::verify(&target(&r)).unwrap()["burst"]["members"][1]["representative"],
        true
    );
}
#[test]
fn invalid_or_stale_representative_and_changed_inventory_never_publish() {
    let d = Dir::new("burst-stale");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let p = plan(&inputs);
    assert!(burst::convert(
        &NoNative,
        &p,
        &inputs,
        burst::Choice {
            representative: Some("/not-a-member"),
            independent: false
        },
        &d.0.join("out"),
        &StopToken::new(),
        false
    )
    .is_err());
    fixture(&d.0.join("in"), &["IMG_20230108_205201_BURST004.JPG"]);
    assert!(burst::convert(
        &NoNative,
        &p,
        &inputs,
        burst::Choice {
            representative: None,
            independent: false
        },
        &d.0.join("out"),
        &StopToken::new(),
        false
    )
    .is_err());
    assert!(!d.0.join("out").exists());
}
#[test]
fn single_frame_has_no_burst_marker_and_corrupt_jpeg_is_rejected() {
    let d = Dir::new("burst-single");
    let inputs = fixture(&d.0.join("in"), &NAMES[..1]);
    let p = plan(&inputs);
    let r = burst::convert(
        &NoNative,
        &p,
        &inputs,
        burst::Choice {
            representative: None,
            independent: false,
        },
        &d.0.join("out"),
        &StopToken::new(),
        false,
    )
    .unwrap();
    assert!(r["burst"]["uuid"].is_null());
    assert_eq!(r["processing_kind"], "original-copy");
    let batched = run(
        &inputs,
        &d.0.join("batch"),
        &ConversionOptions::default(),
        None,
    );
    assert_eq!(batched.counts["copied"], 1);
    assert_eq!(batched.counts["converted"], 0);
    std::fs::write(&inputs[0], [0xff, 0xd8]).unwrap();
    let p = plan(&inputs);
    assert_eq!(p["supported"], false);
}
#[test]
fn hdr_and_motion_combinations_are_blocked_without_silent_loss() {
    let d = Dir::new("burst-combinations");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let base = b64("tests/fixtures-pycli/base.jpg.b64");
    let paired = momentstays_core::engine::still::live_still(&base,"CCA01975-5655-4A4C-88C6-D620975EF319",true).unwrap();
    std::fs::write(&inputs[1], &paired).unwrap();
    let combined = plan(&inputs);
    assert_eq!(combined["supported"],false);
    assert!(combined["reason"].as_str().unwrap().contains("配对"));
    std::fs::write(&inputs[1], hdr::ultra(&base, &base, 2.0).unwrap()).unwrap();
    assert_eq!(plan(&inputs)["supported"], false);
    assert_eq!(
        counts(&run(
            &inputs,
            &d.0.join("out"),
            &ConversionOptions::default(),
            None
        )),
        [0, 0, 0, 3]
    );
    std::fs::write(
        &inputs[1],
        engine_support::motion(
            momentstays_core::model::Cover::Time(800_000),
            momentstays_core::model::Vendor::Google,
        ),
    )
    .unwrap();
    assert_eq!(plan(&inputs)["supported"], false);
}
#[test]
fn cancel_and_native_decode_failure_leave_no_partial_group() {
    struct Bad;
    impl Native for Bad {
        fn image_decodes(&self, _: &[u8]) -> Option<bool> {
            Some(false)
        }
    }
    let d = Dir::new("burst-cancel");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let p = plan(&inputs);
    let token = StopToken::new();
    token.cancel();
    assert!(burst::convert(
        &NoNative,
        &p,
        &inputs,
        burst::Choice {
            representative: None,
            independent: false
        },
        &d.0.join("cancelled"),
        &token,
        false
    )
    .is_err());
    assert!(burst::convert(
        &Bad,
        &p,
        &inputs,
        burst::Choice {
            representative: None,
            independent: false
        },
        &d.0.join("bad"),
        &StopToken::new(),
        false
    )
    .is_err());
    assert!(!d.0.join("bad").exists());
    assert!(!std::fs::read_dir(&d.0).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".motionlive-")));
}
#[test]
fn overwrite_is_explicit_and_lost_archive_blocks_resume() {
    let d = Dir::new("burst-overwrite");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let p = plan(&inputs);
    let out = d.0.join("out");
    burst::convert(
        &NoNative,
        &p,
        &inputs,
        burst::Choice {
            representative: None,
            independent: false,
        },
        &out,
        &StopToken::new(),
        false,
    )
    .unwrap();
    let before = std::fs::read(out.join("manifest.json")).unwrap();
    assert!(burst::convert(
        &NoNative,
        &p,
        &inputs,
        burst::Choice {
            representative: inputs[2].to_str(),
            independent: false
        },
        &out,
        &StopToken::new(),
        false
    )
    .is_err());
    assert_eq!(before, std::fs::read(out.join("manifest.json")).unwrap());
    burst::convert(
        &NoNative,
        &p,
        &inputs,
        burst::Choice {
            representative: inputs[2].to_str(),
            independent: false,
        },
        &out,
        &StopToken::new(),
        true,
    )
    .unwrap();
    std::fs::remove_file(out.join("source-burst-001.bin")).unwrap();
    assert!(output::verify(&out).is_err());
    assert!(burst::convert(
        &NoNative,
        &p,
        &inputs,
        burst::Choice {
            representative: inputs[2].to_str(),
            independent: false
        },
        &out,
        &StopToken::new(),
        false
    )
    .is_err());
}
#[test]
fn cli_and_cabi_request_share_plan_and_group_route() {
    let d = Dir::new("burst-request");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let scan = request::handle(
        &NoNative,
        &request::NoEvents,
        &StopToken::new(),
        &json!({"mode":"scan","inputs":inputs,"full_rows":true}),
    )
    .unwrap();
    assert!(scan["rows"][0]["burst"].is_object());
    assert_eq!(scan["rows"][0]["motion"], false);
    let plan = request::handle(
        &NoNative,
        &request::NoEvents,
        &StopToken::new(),
        &json!({"mode":"burst-plan","scan_rows":scan["rows"]}),
    )
    .unwrap();
    assert_eq!(plan["groups"].as_array().unwrap().len(), 1);
    let result=request::handle(&NoNative,&request::NoEvents,&StopToken::new(),&json!({"mode":"batch","inputs":inputs,"output":d.0.join("out"),"scan_rows":scan["rows"],"burst_plans":plan["groups"]})).unwrap();
    assert_eq!(result["counts"]["converted"], 3);
}

#[test]
fn source_mutation_during_conversion_preserves_existing_output() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Mutate {
        source: PathBuf,
        calls: AtomicUsize,
    }
    impl Native for Mutate {
        fn image_decodes(&self, _: &[u8]) -> Option<bool> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                let mut bytes = std::fs::read(&self.source).unwrap();
                bytes.push(0);
                std::fs::write(&self.source, bytes).unwrap();
            }
            Some(true)
        }
    }
    let d = Dir::new("burst-mid-change");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let p = plan(&inputs);
    let out = d.0.join("out");
    burst::convert(
        &NoNative,
        &p,
        &inputs,
        burst::Choice {
            representative: None,
            independent: false,
        },
        &out,
        &StopToken::new(),
        false,
    )
    .unwrap();
    let before = std::fs::read(out.join("manifest.json")).unwrap();
    let native = Mutate {
        source: inputs[1].clone(),
        calls: AtomicUsize::new(0),
    };
    assert!(burst::convert(
        &native,
        &p,
        &inputs,
        burst::Choice {
            representative: None,
            independent: false
        },
        &out,
        &StopToken::new(),
        true
    )
    .is_err());
    assert_eq!(std::fs::read(out.join("manifest.json")).unwrap(), before);
    output::verify(&out).unwrap();
}
#[test]
fn oversized_named_input_is_rejected_without_loading_or_decoding() {
    let d = Dir::new("burst-limit");
    let inputs = fixture(&d.0.join("in"), &NAMES[..1]);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&inputs[0])
        .unwrap()
        .set_len(burst::MAX_IMAGE + 1)
        .unwrap();
    let row = scanrow::scan_file(&NoNative, &inputs[0], "", None);
    assert_eq!(row.burst.unwrap()["supported"], false);
}
#[test]
fn actual_cabi_response_owns_scan_burst_facts() {
    let d = Dir::new("burst-cabi");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let request = json!({"mode":"scan","inputs":inputs,"full_rows":true}).to_string();
    unsafe {
        let response = momentstays_core::engine::cabi::ms_core_engine(
            std::ptr::null(),
            request.as_ptr(),
            request.len(),
            std::ptr::null(),
        );
        assert!(!response.is_null());
        let mut length = 0;
        let ptr = momentstays_core::ffi::ms_core_bytes(response, 0, &mut length);
        assert!(!ptr.is_null());
        let result: Value =
            serde_json::from_slice(std::slice::from_raw_parts(ptr, length)).unwrap();
        momentstays_core::ffi::ms_core_free(response);
        assert!(result["rows"][0]["burst"].is_object(), "{result}");
        assert_eq!(result["rows"][0]["motion"], false);
    }
}

#[test]
fn cancellation_during_group_staging_never_exposes_members() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Cancel<'a> {
        token: &'a StopToken,
        calls: AtomicUsize,
    }
    impl Native for Cancel<'_> {
        fn image_decodes(&self, _: &[u8]) -> Option<bool> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                self.token.cancel();
            }
            Some(true)
        }
    }
    let d = Dir::new("burst-mid-cancel");
    let inputs = fixture(&d.0.join("in"), &NAMES);
    let p = plan(&inputs);
    let target = d.0.join("out");
    let token = StopToken::new();
    let native = Cancel {
        token: &token,
        calls: AtomicUsize::new(0),
    };
    assert!(burst::convert(
        &native,
        &p,
        &inputs,
        burst::Choice::default(),
        &target,
        &token,
        false
    )
    .is_err());
    assert!(!target.exists());
    assert!(!std::fs::read_dir(&d.0).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".motionlive-")));
}
