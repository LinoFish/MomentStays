mod engine_support;
use engine_support::{b64, files, motion, run, Dir};
use momentstays_core::{
    engine::{
        self,
        options::ConversionOptions,
        request::{self, Events, NoEvents},
        scanrow, Native, NoNative, StopToken,
    },
    model::{Cover, Vendor},
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
fn call(req: Value) -> Value {
    request::handle(&NoNative, &NoEvents, &StopToken::new(), &req).unwrap()
}
fn original(p: &Path) -> Value {
    json!({"source":p,"source_kind":"local","scan_row":request::row_json(&scanrow::scan_file(&NoNative,p,"",None))})
}
fn prepare(items: Vec<Value>) -> Value {
    call(json!({"mode":"export-plan","kind":"originals","items":items}))
}
fn execute(plan: &Value, out: &Path, policy: &str) -> Value {
    call(
        json!({"mode":"export","kind":plan["kind"],"plans":plan["plans"],"output":out,"existing_output":policy}),
    )
}
fn photo(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, b64("tests/fixtures-pycli/base.jpg.b64")).unwrap();
    p
}
fn result(dir: &Path, sidecars: bool) -> Value {
    let p = dir.join("in/source.jpg");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, motion(Cover::Time(800_000), Vendor::Google)).unwrap();
    let options = ConversionOptions {
        export_data_files: sidecars,
        ..Default::default()
    };
    let r = run(&[p], &dir.join("converted"), &options, None);
    assert_eq!(r.counts["converted"], 1, "{}", r.report_data);
    files(&r).remove(0)
}
fn result_plan(entry: &Value) -> Value {
    call(
        json!({"mode":"export-plan","kind":"results","items":[{"source":entry["output"],"verification":entry["verification"]}]}),
    )
}
fn output(r: &Value) -> PathBuf {
    PathBuf::from(r["report_data"]["files"][0]["output"].as_str().unwrap())
}
#[test]
fn originals_selected_scope_copy_resume_skip_overwrite_and_source_readonly() {
    let d = Dir::new("export-original");
    let a = photo(&d.0.join("in"), "a.jpg");
    let _b = photo(&d.0.join("in"), "unselected.jpg");
    let before = std::fs::read(&a).unwrap();
    let p = prepare(vec![original(&a)]);
    assert_eq!(p["plans"].as_array().unwrap().len(), 1);
    let out = d.0.join("out");
    let r = execute(&p, &out, "cancel");
    assert_eq!(r["counts"]["copied"], 1);
    assert!(r["counts"].get("converted").is_none());
    let target = output(&r);
    assert_eq!(std::fs::read(target.join("a.jpg")).unwrap(), before);
    assert!(!target.join("unselected.jpg").exists());
    let old = std::fs::metadata(target.join("a.jpg"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(execute(&p, &out, "resume")["counts"]["resumed"], 1);
    assert_eq!(
        std::fs::metadata(target.join("a.jpg"))
            .unwrap()
            .modified()
            .unwrap(),
        old
    );
    assert_eq!(execute(&p, &out, "skip")["counts"]["skipped"], 1);
    std::fs::write(target.join("a.jpg"), b"bad").unwrap();
    assert_eq!(execute(&p, &out, "resume")["counts"]["failed"], 1);
    assert_eq!(execute(&p, &out, "overwrite")["counts"]["copied"], 1);
    assert_eq!(std::fs::read(&a).unwrap(), before);
}
#[test]
fn gapped_burst_originals_stay_together_without_added_markers_or_unselected_members() {
    let d = Dir::new("export-gaps");
    let a = photo(&d.0.join("in"), "IMG_20230108_205201_BURST003_COVER.JPG");
    let b = photo(&d.0.join("in"), "IMG_20230108_205201_BURST007.JPG");
    photo(&d.0.join("in"), "IMG_20230108_205201_BURST009.JPG");
    let p = prepare(vec![original(&a), original(&b)]);
    assert_eq!(p["plans"].as_array().unwrap().len(), 1);
    assert_eq!(p["plans"][0]["sources"].as_array().unwrap().len(), 2);
    let r = execute(&p, &d.0.join("out"), "cancel");
    assert_eq!(r["counts"]["copied"], 2);
    let target = output(&r);
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 3);
    for src in [a, b] {
        assert_eq!(
            std::fs::read(target.join(src.file_name().unwrap())).unwrap(),
            std::fs::read(src).unwrap()
        );
    }
}
#[test]
fn rejects_stale_version_changed_plan_source_overlap_and_corrupt_unreadable_image() {
    let d = Dir::new("export-stale");
    let a = photo(&d.0.join("in"), "a.jpg");
    let frozen = original(&a);
    let p = prepare(vec![frozen.clone()]);
    assert_eq!(
        execute(&p, &d.0.join("in/nested"), "overwrite")["counts"]["failed"],
        1
    );
    assert!(!d.0.join("in/nested").exists());
    let mut tampered = p.clone();
    tampered["plans"][0]["target_name"] = json!("../bad");
    assert_eq!(
        execute(&tampered, &d.0.join("out"), "overwrite")["counts"]["failed"],
        1
    );
    std::fs::write(&a, b"\xff\xd8\xff\xd9").unwrap();
    assert_eq!(
        execute(&p, &d.0.join("out"), "overwrite")["counts"]["failed"],
        1
    );
    assert_eq!(prepare(vec![frozen])["files"][0]["status"], "failed");
    assert!(prepare(vec![original(&a)])["plans"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn photos_static_and_native_decode_failure_are_not_copied() {
    let d = Dir::new("export-static");
    let a = photo(&d.0.join("in"), "a.jpg");
    let mut item = original(&a);
    item["source_kind"] = json!("photos");
    assert_eq!(prepare(vec![item])["files"][0]["status"], "failed");
    struct Reject;
    impl Native for Reject {
        fn image_decodes(&self, _: &[u8]) -> Option<bool> {
            Some(false)
        }
    }
    let r = request::handle(
        &Reject,
        &NoEvents,
        &StopToken::new(),
        &json!({"mode":"export-plan","kind":"originals","items":[original(&a)]}),
    )
    .unwrap();
    assert!(r["plans"].as_array().unwrap().is_empty());
    struct Accept;
    impl Native for Accept {
        fn image_decodes(&self, _: &[u8]) -> Option<bool> {
            Some(true)
        }
    }
    std::fs::write(&a, b"\xff\xd8\xff\xd9unknown tail").unwrap();
    let r = request::handle(
        &Accept,
        &NoEvents,
        &StopToken::new(),
        &json!({"mode":"export-plan","kind":"originals","items":[original(&a)]}),
    )
    .unwrap();
    assert!(
        r["plans"].as_array().unwrap().is_empty(),
        "codec success cannot prove an unknown resource container is complete"
    );
}
#[test]
fn complete_apple_pair_copied_but_missing_or_ambiguous_movie_rejected() {
    let d = Dir::new("export-pair");
    let entry = result(&d.0, true);
    let folder = PathBuf::from(entry["output"].as_str().unwrap());
    let m = engine::output::verify(&folder).unwrap();
    let still = folder.join(m["output_files"]["still"].as_str().unwrap());
    let movie = folder.join(m["output_files"]["video"].as_str().unwrap());
    let item = || {
        let row = engine::live::pairing::scan_file_with_native(&NoNative, &still, "");
        let mut row = row;
        row.version = engine::identity::SourceVersion::read(&still);
        json!({"source":still,"source_kind":"local","scan_row":request::row_json(&row)})
    };
    let p = call(
        json!({"mode":"export-plan","kind":"originals","direction":"to-android","items":[item()]}),
    );
    assert_eq!(p["plans"][0]["resources"].as_array().unwrap().len(), 2);
    let r = execute(&p, &d.0.join("export"), "cancel");
    assert_eq!(r["counts"]["copied"], 1);
    std::fs::copy(&movie, folder.join("duplicate.mov")).unwrap();
    assert!(prepare(vec![item()])["plans"]
        .as_array()
        .unwrap()
        .is_empty());
    std::fs::remove_file(folder.join("duplicate.mov")).unwrap();
    std::fs::remove_file(movie).unwrap();
    assert_eq!(
        execute(&p, &d.0.join("export"), "overwrite")["counts"]["failed"],
        1
    );
    assert!(prepare(vec![item()])["plans"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn results_without_sidecars_use_persisted_verification_and_preserve_every_file() {
    let d = Dir::new("export-results");
    let entry = result(&d.0, false);
    let folder = PathBuf::from(entry["output"].as_str().unwrap());
    assert!(!folder.join("manifest.json").exists());
    assert!(entry["verification"].is_object());
    let v = call(json!({"mode":"verify","input":folder,"verification":entry["verification"]}));
    assert_eq!(v, entry["verification"]);
    std::fs::write(folder.join("unknown-private.bin"), b"opaque data").unwrap();
    let p = result_plan(&entry);
    assert_eq!(p["plans"].as_array().unwrap().len(), 1);
    let r = execute(&p, &d.0.join("export"), "cancel");
    assert_eq!(r["counts"]["copied"], 1);
    let out = output(&r);
    assert_eq!(
        std::fs::read(out.join("unknown-private.bin")).unwrap(),
        b"opaque data"
    );
    assert!(!out.join("manifest.json").exists());
    assert_eq!(
        execute(&p, &d.0.join("export"), "resume")["counts"]["resumed"],
        1
    );
    let no = call(json!({"mode":"export-plan","kind":"results","items":[{"source":folder}]}));
    assert_eq!(no["files"][0]["status"], "failed");
}
#[test]
fn missing_result_attachment_or_changed_inventory_invalidates_plan_and_preserves_old_export() {
    let d = Dir::new("export-attachment");
    let entry = result(&d.0, true);
    let folder = PathBuf::from(entry["output"].as_str().unwrap());
    let p = result_plan(&entry);
    let r = execute(&p, &d.0.join("export"), "cancel");
    let out = output(&r);
    let before = std::fs::read(out.join("manifest.json")).unwrap();
    std::fs::write(folder.join("added.bin"), b"added").unwrap();
    assert_eq!(
        execute(&p, &d.0.join("export"), "overwrite")["counts"]["failed"],
        1
    );
    assert_eq!(std::fs::read(out.join("manifest.json")).unwrap(), before);
    std::fs::remove_file(folder.join("added.bin")).unwrap();
    let m = &entry["verification"];
    std::fs::remove_file(folder.join(m["output_files"]["video"].as_str().unwrap())).unwrap();
    assert_eq!(result_plan(&entry)["files"][0]["status"], "failed");
}
#[test]
fn cancellation_before_work_and_between_items_does_not_continue_or_report_success() {
    let d = Dir::new("export-cancel");
    let a = photo(&d.0.join("in"), "a.jpg");
    let b = photo(&d.0.join("in"), "b.jpg");
    let p = prepare(vec![original(&a), original(&b)]);
    let token = StopToken::new();
    token.cancel();
    let req =
        json!({"mode":"export","kind":"originals","plans":p["plans"],"output":d.0.join("out")});
    let r = request::handle(&NoNative, &NoEvents, &token, &req).unwrap();
    assert_eq!(r["counts"]["copied"], 0);
    assert!(!d.0.join("out").exists());
    struct Cancel<'a>(&'a StopToken);
    impl Events for Cancel<'_> {
        fn progress(&self, _: &engine::batch::Progress) {
            self.0.cancel();
        }
    }
    let token = StopToken::new();
    let r = request::handle(&NoNative, &Cancel(&token), &token, &req).unwrap();
    assert_eq!(r["counts"]["copied"], 1);
    assert_eq!(r["cancelled"], true);
}
#[cfg(unix)]
#[test]
fn symlinks_and_case_collisions_are_refused() {
    use std::os::unix::fs::symlink;
    let d = Dir::new("export-links");
    let a = photo(&d.0.join("in"), "a.jpg");
    let link = d.0.join("in/link.jpg");
    symlink(&a, &link).unwrap();
    assert_eq!(
        prepare(vec![original(&link)])["files"][0]["status"],
        "failed"
    );
    let entry = result(&d.0.join("results"), true);
    let folder = PathBuf::from(entry["output"].as_str().unwrap());
    symlink(&a, folder.join("alias.jpg")).unwrap();
    assert_eq!(result_plan(&entry)["files"][0]["status"], "failed");
}
#[test]
fn cancellation_during_copy_removes_stage_and_keeps_existing_result() {
    let d = Dir::new("export-cancel-stream");
    let entry = result(&d.0, true);
    let folder = PathBuf::from(entry["output"].as_str().unwrap());
    let large = folder.join("large.bin");
    let f = std::fs::File::create(&large).unwrap();
    f.set_len(16 << 20).unwrap();
    let p = result_plan(&entry);
    let out = d.0.join("export");
    let first = execute(&p, &out, "cancel");
    let target = output(&first);
    let before = std::fs::read(target.join("manifest.json")).unwrap();
    let token = Arc::new(StopToken::new());
    let seen = Arc::new(AtomicBool::new(false));
    let token2 = token.clone();
    let seen2 = seen.clone();
    let watch = out.clone();
    let thread = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < 15 {
            if std::fs::read_dir(&watch)
                .unwrap()
                .filter_map(Result::ok)
                .any(|e| {
                    e.file_name().to_string_lossy().starts_with(".motionlive-")
                        && e.path().join("large.bin").exists()
                })
            {
                seen2.store(true, Ordering::SeqCst);
                token2.cancel();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });
    let req = json!({"mode":"export","kind":"results","plans":p["plans"],"output":out,"existing_output":"overwrite"});
    let r = request::handle(&NoNative, &NoEvents, &token, &req).unwrap();
    thread.join().unwrap();
    assert!(seen.load(Ordering::SeqCst));
    assert_eq!(r["cancelled"], true);
    assert_eq!(r["counts"]["copied"], 0);
    assert_eq!(std::fs::read(target.join("manifest.json")).unwrap(), before);
    assert_eq!(std::fs::read_dir(out).unwrap().count(), 1);
}

#[test]
fn c_abi_supports_export_plan_export_and_inline_verification() {
    use engine::cabi::{ms_core_engine, MsNative};
    use momentstays_core::ffi::{ms_core_bytes, ms_core_free};
    unsafe extern "C" fn unknown_decode(_: *mut std::ffi::c_void, _: *const u8, _: usize) -> i32 {
        -1
    }
    let invoke = |req: Value| unsafe {
        let data = req.to_string();
        let host = MsNative {
            context: std::ptr::null_mut(),
            image_decodes: Some(unknown_decode),
            srgb_icc: None,
            decode_gray_jpeg: None,
            encode_gray_jpeg: None,
            reencode_jpeg: None,
            heic_gain_map: None,
            frame_jpeg: None,
            progress: None,
            existing_output: None,
            rows_ready: None,
            prepare_output_times: None,
        };
        let r = ms_core_engine(&host, data.as_ptr(), data.len(), std::ptr::null());
        let mut n = 0;
        let bytes = ms_core_bytes(r, 0, &mut n);
        let v: Value = serde_json::from_slice(std::slice::from_raw_parts(bytes, n)).unwrap();
        ms_core_free(r);
        v
    };
    let d = Dir::new("export-cabi");
    let a = photo(&d.0.join("in"), "a.jpg");
    let p = invoke(json!({"mode":"export-plan","kind":"originals","items":[original(&a)]}));
    assert_eq!(p["plans"].as_array().unwrap().len(), 1, "{p}");
    let r = invoke(
        json!({"mode":"export","kind":"originals","plans":p["plans"],"output":d.0.join("out")}),
    );
    assert_eq!(r["counts"]["copied"], 1, "{r}");
    let e = result(&d.0.join("result"), false);
    let v = invoke(json!({"mode":"verify","input":e["output"],"verification":e["verification"]}));
    assert_eq!(v, e["verification"]);
}
#[test]
fn burst_results_export_all_original_archives_and_count_source_photos() {
    let d = Dir::new("export-burst-result");
    let mut inputs = Vec::new();
    for name in [
        "IMG_20230108_205201_BURST003_COVER.JPG",
        "IMG_20230108_205201_BURST009.JPG",
    ] {
        inputs.push(photo(&d.0.join("in"), name));
    }
    let r = run(
        &inputs,
        &d.0.join("converted"),
        &ConversionOptions::default(),
        None,
    );
    assert_eq!(r.counts["converted"], 2);
    let e = files(&r).remove(0);
    let p = result_plan(&e);
    let r = execute(&p, &d.0.join("export"), "cancel");
    assert_eq!(r["counts"]["copied"], 2);
    let m = engine::output::verify(&output(&r)).unwrap();
    assert_eq!(m["burst"]["members"].as_array().unwrap().len(), 2);
    for src in inputs {
        assert!(m["output_sha256"]
            .as_object()
            .unwrap()
            .values()
            .any(|h| h == &momentstays_core::sha256_hex(&std::fs::read(&src).unwrap())));
    }
}
#[test]
fn adding_pair_candidate_after_plan_invalidates_it() {
    let d = Dir::new("export-pair-scope");
    let e = result(&d.0, true);
    let folder = PathBuf::from(e["output"].as_str().unwrap());
    let m = &e["verification"];
    let still = folder.join(m["output_files"]["still"].as_str().unwrap());
    let movie = folder.join(m["output_files"]["video"].as_str().unwrap());
    let p = prepare(vec![original(&still)]);
    assert_eq!(p["plans"].as_array().unwrap().len(), 1);
    std::fs::copy(&movie, folder.join("duplicate.mov")).unwrap();
    assert_eq!(
        execute(&p, &d.0.join("out"), "cancel")["counts"]["failed"],
        1
    );
}
