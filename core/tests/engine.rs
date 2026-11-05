//! Batch/route integration: all three directions, resume, output decisions,
//! request JSON, movie refusals and the C ABI. HDR/output checks have separate files.

use base64::Engine;
use momentstays_core::engine::batch::{self, ExistingOutput};
use momentstays_core::engine::movie::{self, MovieRequest};
use momentstays_core::engine::options::ConversionOptions;
use momentstays_core::engine::{still, NoNative, StopToken};
use momentstays_core::live2motion::mett_cover_time;
use momentstays_core::model::{Cover, Vendor};
use momentstays_core::{format, jpeg, live, remux};
use serde_json::{json, Value};
use std::path::PathBuf;

mod engine_support;
use engine_support::*;

#[test]
fn swift_option_json_is_kept() {
    let o = ConversionOptions::default();
    assert_eq!(
        o.json(),
        json!({"legacy_hdr":"auto","missing_time":"error","time_domain":"auto","video_track":"primary",
               "output_mode":"livephoto","repair_auxiliary_duration":false,"export_data_files":true})
    );
    let mut a = o.clone();
    a.direction = "to-android".into();
    assert_eq!(
        a.json(),
        json!({"direction":"to-android","android":{"vendor":"google","container":"jpeg","hdr":"ultra",
               "image_quality":0.95,"gain_quality":0.95,"preserve_jpeg":true},"time_domain":"auto","export_data_files":true})
    );
    a.direction = "android-to-android".into();
    assert_eq!(a.json()["layout"], "google-to-honor-a-modern-1");
}

#[test]
fn motion_to_live_batch_converts_resumes_and_respects_existing_output() {
    let (src, out) = (Dir::new("src"), Dir::new("out"));
    inputs(&src.0);
    let sources = vec![src.0.clone()];
    let mut options = ConversionOptions::default();
    let first = run(&sources, &out.0, &options, None);
    assert_eq!(counts(&first), [3, 0, 1, 0], "{:#}", first.report_data);
    assert!(first.report.as_ref().is_some_and(|r| r.exists()));
    for f in files(&first).iter().filter(|f| f["status"] == "converted") {
        let target = PathBuf::from(f["output"].as_str().unwrap());
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(target.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["implementation"], batch::REVISION);
        let names = &manifest["output_files"];
        let source = PathBuf::from(f["source"].as_str().unwrap());
        assert_eq!(
            names["still"],
            source.file_name().unwrap().to_str().unwrap()
        );
        let still_bytes = std::fs::read(target.join(names["still"].as_str().unwrap())).unwrap();
        let mov = std::fs::read(target.join(names["video"].as_str().unwrap())).unwrap();
        let id = manifest["identifier"].as_str().unwrap();
        assert_eq!(
            live::still_content_identifier(&still_bytes)
                .unwrap()
                .as_deref(),
            Some(id)
        );
        assert_eq!(
            remux::read_content_identifier(&mov).unwrap().as_deref(),
            Some(id)
        );
        assert!(
            format::parse(&still_bytes).is_err(),
            "still keeps no motion"
        );
        // Covers: Google and Huawei frame 2 (0.8 s), Samsung frame 1 (0.6 s).
        let want = if f["source"].as_str().unwrap().ends_with("samsung.jpg") {
            600_000
        } else {
            800_000
        };
        assert_eq!(mett_cover_time(&mov).unwrap(), Some(want));
        assert_eq!(manifest["timing"]["cover_seconds"], want as f64 / 1e6);
    }
    let again = run(&sources, &out.0, &options, None);
    assert_eq!(counts(&again), [0, 3, 1, 0]);
    options.time_domain = "media".into();
    let changed = run(&sources, &out.0, &options, None);
    assert_eq!(counts(&changed), [0, 0, 1, 3]);
    assert!(files(&changed)
        .iter()
        .any(|f| f["error"] == "同名输出已存在且内容或设置不同；未覆盖，请选择新的输出目录"));
    assert_eq!(
        counts(&run(&sources, &out.0, &options, Some(ExistingOutput::Skip))),
        [0, 0, 4, 0]
    );
    assert_eq!(
        counts(&run(
            &sources,
            &out.0,
            &options,
            Some(ExistingOutput::Overwrite)
        )),
        [3, 0, 1, 0]
    );
    let cancelled = run(&sources, &out.0, &options, Some(ExistingOutput::Cancel));
    assert!(cancelled.cancelled);
    assert_eq!(counts(&cancelled)[0], 0);
}

#[test]
fn live_to_motion_for_every_vendor_and_resume() {
    let (src, live_out) = (Dir::new("src"), Dir::new("live"));
    std::fs::write(
        src.0.join("g.jpg"),
        motion(Cover::Time(800_000), Vendor::Google),
    )
    .unwrap();
    let o = run(
        std::slice::from_ref(&src.0),
        &live_out.0,
        &ConversionOptions::default(),
        None,
    );
    assert_eq!(counts(&o)[0], 1);
    let pair = live_out.0.join("g.jpg.livephoto");
    for vendor in [
        "google", "xiaomi", "samsung", "huawei", "honor", "oppo", "vivo",
    ] {
        let back = Dir::new("back");
        let mut options = ConversionOptions {
            direction: "to-android".into(),
            ..Default::default()
        };
        options.android.vendor = vendor.into();
        let r = run(std::slice::from_ref(&pair), &back.0, &options, None);
        assert_eq!(counts(&r), [1, 0, 0, 0], "{vendor}: {:#}", r.report_data);
        let file = back.0.join("g.jpg.motionphoto/g.MP.jpg");
        let bytes = std::fs::read(&file).unwrap();
        let p = format::parse(&bytes).unwrap();
        if vendor == "huawei" {
            assert_eq!(p.cover_frame, Some(2));
        } else {
            assert_eq!(p.timestamp, Some(800_000), "{vendor}");
        }
        assert_eq!(
            p.video(&bytes).len(),
            remux::remux_to_mp4(&std::fs::read(pair.join("g.mov")).unwrap())
                .unwrap()
                .len()
        );
        assert_eq!(
            counts(&run(std::slice::from_ref(&pair), &back.0, &options, None)),
            [0, 1, 0, 0]
        );
        if ["google", "honor"].contains(&vendor) {
            let manifest_path = back.0.join("g.jpg.motionphoto/manifest.json");
            let mut manifest: Value =
                serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
            manifest.as_object_mut().unwrap().remove("xmp_profile");
            std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            let refused = run(std::slice::from_ref(&pair), &back.0, &options, None);
            assert_eq!(counts(&refused), [0, 0, 0, 1]);
            assert!(refused.report_data.to_string().contains("旧版XMP"));
            assert_eq!(std::fs::read(&file).unwrap(), bytes);
            assert_eq!(
                counts(&run(
                    std::slice::from_ref(&pair),
                    &back.0,
                    &options,
                    Some(ExistingOutput::Overwrite)
                )),
                [1, 0, 0, 0]
            );
            assert_eq!(
                counts(&run(std::slice::from_ref(&pair), &back.0, &options, None)),
                [0, 1, 0, 0]
            );
        }
    }
}

#[test]
fn google_to_honor_accepts_only_google_standard_files() {
    let (src, out) = (Dir::new("src"), Dir::new("out"));
    inputs(&src.0);
    let options = ConversionOptions {
        direction: "android-to-android".into(),
        ..Default::default()
    };
    let r = run(std::slice::from_ref(&src.0), &out.0, &options, None);
    assert_eq!(counts(&r), [1, 0, 1, 2], "{:#}", r.report_data);
    for f in files(&r).iter().filter(|f| f["status"] == "failed") {
        assert!(
            f["error"].as_str().unwrap().starts_with("来源不支持"),
            "{f}"
        );
    }
    let bytes = std::fs::read(out.0.join("google.jpg.motionphoto/google.MP.jpg")).unwrap();
    assert_eq!(format::parse(&bytes).unwrap().timestamp, Some(800_000));
}

#[test]
fn scan_rows_match_swift_texts() {
    let src = Dir::new("scan");
    inputs(&src.0);
    let rows = batch::scan(
        &NoNative,
        std::slice::from_ref(&src.0),
        "to-apple",
        &StopToken::new(),
        None,
        |_| {},
        |_, _| {},
    )
    .unwrap();
    let by: Vec<(&str, bool, bool, &str)> = rows
        .iter()
        .map(|r| {
            (
                r.relative.as_str(),
                r.is_motion,
                r.confirmed_static,
                r.cover.as_str(),
            )
        })
        .collect();
    assert_eq!(
        by,
        [
            ("google.jpg", true, false, "0.800000 秒"),
            ("huawei.jpg", true, false, "原始第 2 帧（从0计）"),
            ("samsung.jpg", true, false, "0.600000 秒"),
            ("static.jpg", false, true, "—"),
        ]
    );
    assert!(rows.iter().all(|r| r.version.is_some()));
}

#[test]
fn movie_writer_keeps_swift_refusals_and_report_keys() {
    let source = video();
    let still_bytes =
        still::live_still(&b64("tests/fixtures-pycli/base.jpg.b64"), "ID-1", true).unwrap();
    let req = MovieRequest {
        still: Some(&still_bytes),
        identifier: "ID-1",
        video_track: "primary",
        timestamp_us: Some(600_000),
        missing_time: "error",
        time_domain: "auto",
        ..Default::default()
    };
    let (mov, timing) = movie::movie(&source, &req).unwrap();
    assert_eq!(mett_cover_time(&mov).unwrap(), Some(600_000));
    for key in [
        "requested_seconds",
        "mapped_source_seconds",
        "timestamp_domain",
        "cover_seconds",
        "source_frame_duration_seconds",
        "cover_duration_seconds",
        "snap_error_seconds",
        "timing_policy",
        "tracks",
        "identifier",
        "source_video_track_count",
        "primary_source_track_id",
        "secondary_source_track_ids",
        "video_selection_policy",
        "selected_source_track_id",
        "selected_video_role",
        "selected_frame_count",
        "selected_width",
        "selected_height",
    ] {
        assert!(timing.get(key).is_some(), "missing {key}");
    }
    assert_eq!(timing["timing_policy"], "source-xmp");
    let err = |r: MovieRequest| movie::movie(&source, &r).unwrap_err().0;
    assert_eq!(
        err(MovieRequest {
            video_track: "secondary",
            ..req.clone()
        }),
        "原片没有辅助视频轨，请选择主轨"
    );
    assert_eq!(
        err(MovieRequest {
            identifier: "OTHER",
            ..req.clone()
        }),
        "ImageIO could not read back the still ContentIdentifier"
    );
    assert_eq!(
        err(MovieRequest {
            timestamp_us: None,
            ..req.clone()
        }),
        "No valid source cover timestamp; use --missing-time midpoint only if intended"
    );
    let (_, mid) = movie::movie(
        &source,
        &MovieRequest {
            timestamp_us: None,
            missing_time: "midpoint",
            ..req.clone()
        },
    )
    .unwrap();
    assert_eq!(mid["timing_policy"], "explicit-midpoint");
    assert_eq!(mid["requested_seconds"], 0.8);
    assert_eq!(
        err(MovieRequest {
            timestamp_us: Some(5_000_000),
            ..req
        }),
        "Cover PTS outside the video media/edit-list timelines; no automatic midpoint fallback"
    );
}

#[test]
fn c_abi_runs_requests_and_reports_progress() {
    use momentstays_core::engine::cabi::{ms_core_engine, ms_stop_free, ms_stop_new, MsNative};
    use momentstays_core::ffi::{ms_core_bytes, ms_core_free};
    use std::sync::atomic::{AtomicUsize, Ordering};
    unsafe extern "C" fn progress(ctx: *mut std::ffi::c_void, _: *const u8, _: usize) {
        (*(ctx as *const AtomicUsize)).fetch_add(1, Ordering::SeqCst);
    }
    let (src, out) = (Dir::new("src"), Dir::new("out"));
    inputs(&src.0);
    let seen = AtomicUsize::new(0);
    let table = MsNative {
        context: &seen as *const AtomicUsize as *mut _,
        image_decodes: None,
        srgb_icc: None,
        decode_gray_jpeg: None,
        encode_gray_jpeg: None,
        reencode_jpeg: None,
        heic_gain_map: None,
        frame_jpeg: None,
        progress: Some(progress),
        existing_output: None,
        rows_ready: None,
    };
    let req =
        json!({"mode": "batch", "inputs": [src.0], "output": out.0, "export_data_files": false})
            .to_string();
    unsafe {
        let token = ms_stop_new();
        let r = ms_core_engine(&table, req.as_ptr(), req.len(), token);
        let mut len = 0;
        let p = ms_core_bytes(r, 0, &mut len);
        let v: Value = serde_json::from_slice(std::slice::from_raw_parts(p, len)).unwrap();
        ms_core_free(r);
        ms_stop_free(token);
        assert_eq!(v["counts"]["converted"], 3, "{v}");
        assert_eq!(v["report_data"]["files"].as_array().unwrap().len(), 4);
    }
    assert_eq!(
        seen.load(Ordering::SeqCst),
        4,
        "one progress event per file"
    );
    // Null table, bad request.
    unsafe {
        let bad = b"{}";
        let r = ms_core_engine(std::ptr::null(), bad.as_ptr(), bad.len(), std::ptr::null());
        let mut len = 0;
        let p = ms_core_bytes(r, 0, &mut len);
        let v: Value = serde_json::from_slice(std::slice::from_raw_parts(p, len)).unwrap();
        ms_core_free(r);
        assert_eq!(v["error"], "Missing request");
    }
}

#[test]
fn ui_helper_modes_cover_preview_pairing_and_import_guard() {
    use momentstays_core::engine::request::{handle, NoEvents};
    let (src, out) = (Dir::new("src"), Dir::new("out"));
    inputs(&src.0);
    let call = |req: Value| handle(&NoNative, &NoEvents, &StopToken::new(), &req);
    let v = call(json!({"mode": "preview-cover", "input": src.0.join("samsung.jpg")})).unwrap();
    assert_eq!(v["cover"]["frame"], 1);
    assert_eq!(v["frames"], 5);
    assert!(call(json!({"mode": "still-check", "input": src.0.join("google.jpg")})).is_err());
    run(
        std::slice::from_ref(&src.0),
        &out.0,
        &ConversionOptions::default(),
        None,
    );
    let still_path = out.0.join("google.jpg.livephoto/google.jpg");
    assert_eq!(
        call(json!({"mode": "still-check", "input": still_path})).unwrap()["ok"],
        true
    );
    let pair = call(json!({"mode": "android-pair", "input": still_path})).unwrap();
    assert_eq!(
        (pair["frame"].as_i64(), pair["cover_us"].as_i64()),
        (Some(2), Some(800_000))
    );
    assert!(pair["movie"].as_str().unwrap().ends_with("google.mov"));
}

#[test]
fn byte_helpers_use_real_writers_and_reject_invalid_base64() {
    use momentstays_core::engine::request::{handle, NoEvents};
    let call = |req: Value| handle(&NoNative, &NoEvents, &StopToken::new(), &req);
    let base = b64("tests/fixtures-pycli/base.jpg.b64");
    let movie = video();
    let enc = |v: &[u8]| base64::engine::general_purpose::STANDARD.encode(v);
    let result = call(json!({"mode":"android-assemble", "still_base64":enc(&base),
        "video_base64":enc(&movie), "vendor":"google", "time_us":800_000}))
    .unwrap();
    let motion = base64::engine::general_purpose::STANDARD
        .decode(result["data_base64"].as_str().unwrap())
        .unwrap();
    let photo = format::parse(&motion).unwrap();
    assert_eq!(photo.video(&motion), movie);
    assert_eq!(photo.timestamp, Some(800_000));
    let result = call(json!({"mode":"live-still", "data_base64":enc(&motion),
        "identifier":"REAL-HELPER-ID"}))
    .unwrap();
    let image = base64::engine::general_purpose::STANDARD
        .decode(result["data_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        live::still_content_identifier(&image).unwrap().as_deref(),
        Some("REAL-HELPER-ID")
    );
    assert_eq!(
        format::coding_hash(&image).unwrap(),
        format::coding_hash(&base).unwrap()
    );
    assert_eq!(jpeg::static_only(&image).unwrap(), image);
    assert!(call(json!({"mode":"live-still", "data_base64":"!bad"})).is_err());
    assert!(call(json!({"mode":"android-assemble", "still_base64":enc(&base)})).is_err());
}
