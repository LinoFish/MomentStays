//! Batch engine (Swift `NativeBatch` replacement): all three directions,
//! resume and existing-output policies, option JSON compatibility, the
//! movie writer's Swift-side refusals, still writers and the C ABI.

use base64::Engine;
use momentstays_core::engine::batch::{self, ExistingOutput, RunRequest};
use momentstays_core::engine::movie::{self, MovieRequest};
use momentstays_core::engine::options::ConversionOptions;
use momentstays_core::engine::{still, NoNative, StopToken};
use momentstays_core::live2motion::mett_cover_time;
use momentstays_core::model::{self, Cover, MotionModel, Vendor};
use momentstays_core::{format, jpeg, live, remux};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn b64(path: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    base64::engine::general_purpose::STANDARD
        .decode(std::fs::read_to_string(p).unwrap().trim_ascii())
        .unwrap()
}

/// `real.mp4` without its edit list: 5 frames at 0.4, 0.6, ... 1.2 s.
fn video() -> Vec<u8> {
    let mut d = b64("tests/fixtures-samples/real.mp4.b64");
    let i = d.windows(4).position(|w| w == b"edts").unwrap();
    d[i..i + 4].copy_from_slice(b"free");
    d
}

fn motion(cover: Cover, vendor: Vendor) -> Vec<u8> {
    let m = MotionModel {
        still: b64("tests/fixtures-pycli/base.jpg.b64"),
        video: video(),
        cover,
        source_format: "test".into(),
    };
    model::write(&m, vendor).unwrap()
}

struct Dir(PathBuf);
impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!(
            "ms-engine-{name}-{}-{}",
            std::process::id(),
            momentstays_core::engine::uuid()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn inputs(dir: &Path) {
    std::fs::write(
        dir.join("google.jpg"),
        motion(Cover::Time(800_000), Vendor::Google),
    )
    .unwrap();
    std::fs::write(
        dir.join("samsung.jpg"),
        motion(Cover::Time(600_000), Vendor::Samsung),
    )
    .unwrap();
    std::fs::write(
        dir.join("huawei.jpg"),
        motion(Cover::Frame(2), Vendor::Huawei),
    )
    .unwrap();
    std::fs::write(
        dir.join("static.jpg"),
        b64("tests/fixtures-pycli/base.jpg.b64"),
    )
    .unwrap();
}

fn run(
    sources: &[PathBuf],
    output: &Path,
    options: &ConversionOptions,
    policy: Option<ExistingOutput>,
) -> batch::Outcome {
    let decide = policy.map(|p| move |_: &Path, _: &Path| p);
    batch::run(
        &NoNative,
        &RunRequest {
            sources,
            output,
            options,
            scan_rows: None,
            concurrency: Some(2),
        },
        &StopToken::new(),
        decide
            .as_ref()
            .map(|d| d as &(dyn Fn(&Path, &Path) -> ExistingOutput + Sync)),
        |_| {},
    )
    .unwrap()
}

fn counts(o: &batch::Outcome) -> [usize; 4] {
    ["converted", "resumed", "skipped", "failed"].map(|k| o.counts[k])
}

fn files(o: &batch::Outcome) -> Vec<Value> {
    o.report_data["files"].as_array().unwrap().clone()
}

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
        assert_eq!(manifest["implementation"], "gui-native-1.1.5");
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
fn still_writers_keep_coding_bytes() {
    let ultra = b64("tests/fixtures-pycli/ultra.jpg.b64");
    let p = format::parse(&ultra).unwrap();
    let still_bytes = p.still.to_vec();
    let aux = jpeg::auxiliary(&still_bytes).unwrap();
    assert_eq!(aux.len(), 2);
    let packed = still::pack_gain_map(&still_bytes, &still_bytes[aux[1].clone()]).unwrap();
    assert_eq!(
        jpeg::coding_hash(&packed).unwrap(),
        jpeg::coding_hash(&still_bytes).unwrap()
    );
    let after = jpeg::auxiliary(&packed).unwrap();
    assert_eq!(&packed[after[1].clone()], &still_bytes[aux[1].clone()]);
    // Live still without identifier: motion gone, gain map unchanged.
    let clean = still::live_still(&still_bytes, "", false).unwrap();
    assert!(format::parse(&clean).is_err());
    assert_eq!(
        format::auxiliary_hashes(&clean).unwrap(),
        format::auxiliary_hashes(&still_bytes).unwrap()
    );
    assert_eq!(live::still_content_identifier(&clean).unwrap(), None);
}

#[test]
fn swift_number_text() {
    for (v, s) in [
        (2.0, "2.0"),
        (0.5, "0.5"),
        (1e-5, "1e-05"),
        (1.5e-7, "1.5e-07"),
        (1e16, "1e+16"),
        (2.3, "2.3"),
    ] {
        assert_eq!(still::swift_double(v), s);
    }
}

#[test]
fn gain_lookup_matches_swift_math() {
    let t = still::gain_lookup(2.0);
    assert_eq!((t[0], t[255]), (0, 255));
    assert!(t.windows(2).all(|w| w[0] <= w[1]));
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
fn hdr_writers_match_the_reference_bytes() {
    let base = b64("tests/fixtures-pycli/base.jpg.b64");
    assert_eq!(
        still::ultra(&base, &base, 2.0).unwrap(),
        b64("tests/fixtures-pycli/expected/ultra-2.0.bin.b64")
    );
    assert_eq!(
        still::ultra(&base, &base, 1.8).unwrap(),
        b64("tests/fixtures-pycli/expected/ultra-1.8.bin.b64")
    );
    assert_eq!(
        still::color_profile(&b64("tests/fixtures-pycli/srgb.jpg.b64"), b"FAKEICC").unwrap(),
        b64("tests/fixtures-pycli/expected/srgb-icc.bin.b64")
    );
}

/// A host that "decodes" any gain map to 2x2 gray and "encodes" to base.jpg.
struct FakeHost;
impl momentstays_core::engine::Native for FakeHost {
    fn srgb_icc(&self) -> momentstays_core::Result<Vec<u8>> {
        Ok(b"FAKEICC".to_vec())
    }
    fn decode_gray_jpeg(
        &self,
        _: &[u8],
    ) -> momentstays_core::Result<Option<momentstays_core::engine::Gray>> {
        Ok(Some(momentstays_core::engine::Gray {
            width: 2,
            height: 2,
            stride: 2,
            pixels: vec![0, 64, 128, 255],
        }))
    }
    fn encode_gray_jpeg(
        &self,
        g: &momentstays_core::engine::Gray,
        quality: f64,
    ) -> momentstays_core::Result<Vec<u8>> {
        assert_eq!(quality, 1.0);
        assert_eq!(g.pixels.len(), 4);
        Ok(b64("tests/fixtures-pycli/base.jpg.b64"))
    }
}

#[test]
fn legacy_apple_hdr_is_converted_through_host_codecs() {
    let (src, out) = (Dir::new("src"), Dir::new("out"));
    let legacy = b64("tests/fixtures-pycli/legacy-mpf.jpg.b64");
    let still_bytes = momentstays_core::jpeg::static_only(&legacy).unwrap();
    // Swift refuses to guess the colour space: give the still an ICC profile.
    let icc = momentstays_core::pyjpeg::app(0xE2, b"ICC_PROFILE\0\x01\x01FAKEICC").unwrap();
    let still_bytes =
        momentstays_core::pyjpeg::edit_header(&still_bytes, &Default::default(), &icc).unwrap();
    assert!(momentstays_core::apple_hdr::legacy(&still_bytes)
        .unwrap()
        .0
        .is_some());
    let m = MotionModel {
        still: still_bytes,
        video: video(),
        cover: Cover::Time(800_000),
        source_format: "test".into(),
    };
    // The writer declares any second MPF image an Ultra HDR GainMap (like
    // Swift's assembler). Rename that attribute in place (same length, so
    // no offsets move) to keep a legacy-only Apple HDR Motion Photo.
    let mut file = model::write(&m, Vendor::Google).unwrap();
    let at = file
        .windows(14)
        .position(|w| w == b"hdrgm:Version=")
        .unwrap();
    file[at + 12] = b'x';
    let p = format::parse(&file).unwrap();
    assert!(!p.ultra);
    std::fs::write(src.0.join("hdr.jpg"), &file).unwrap();
    let outcome = batch::run(
        &FakeHost,
        &RunRequest {
            sources: std::slice::from_ref(&src.0),
            output: &out.0,
            options: &ConversionOptions::default(),
            scan_rows: None,
            concurrency: Some(1),
        },
        &StopToken::new(),
        None,
        |_| {},
    )
    .unwrap();
    let f = &files(&outcome)[0];
    assert_eq!(f["status"], "converted", "{f:#}");
    assert_eq!(f["hdr"]["status"], "converted", "{f:#}");
    assert_eq!(f["hdr"]["output"], "ultra-hdr");
    assert_eq!(
        (f["hdr"]["width"].as_u64(), f["hdr"]["height"].as_u64()),
        (Some(2), Some(2))
    );
    let live_still = std::fs::read(out.0.join("hdr.jpg.livephoto/hdr.jpg")).unwrap();
    assert_eq!(
        jpeg::auxiliary(&live_still).unwrap().len(),
        2,
        "gain map packed"
    );
    // Without host codecs the same file still converts, keeping the original HDR.
    let out2 = Dir::new("out2");
    let plain = run(
        std::slice::from_ref(&src.0),
        &out2.0,
        &ConversionOptions::default(),
        None,
    );
    let g = &files(&plain)[0];
    assert_eq!(g["status"], "converted");
    assert_eq!(g["hdr"]["reason"], "旧 HDR 尝试转换未完成，原格式已保留");
    assert!(g["warnings"][0]
        .as_str()
        .unwrap()
        .starts_with("需要原生处理"));
}
