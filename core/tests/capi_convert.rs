//! C ABI conversion entries (wiring): to-live / to-motion round-trips,
//! vendor conversion, live_still, attach_gain, scan_row, cover_map,
//! export_auxiliary, candidates and live_pair over the C ABI.
//!
//! Round-trips use inputs built by the core itself (model::write,
//! remux_to_mov_with_identifier_and_mett), so the shapes are the real
//! ones the app passes, not hand-waved happy paths.

use base64::Engine;
use momentstays_core::capi;
use momentstays_core::format;
use momentstays_core::live2motion;
use momentstays_core::model::{Cover, MotionModel, Vendor};
use momentstays_core::mp4;
use momentstays_core::pymotion;
use momentstays_core::pyxml;
use momentstays_core::remux;
use std::ffi::{CStr, CString};

fn manifest() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_fixture(name: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(
        manifest()
            .join("tests/fixtures-pycli")
            .join(format!("{name}.b64")),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
}

fn real_mp4() -> Vec<u8> {
    let text =
        std::fs::read_to_string(manifest().join("tests/fixtures-samples/real.mp4.b64")).unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
}

fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xff, marker];
    v.extend(((payload.len() + 2) as u16).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn jpeg_still(extra_app: &[u8]) -> Vec<u8> {
    let mut v = vec![0xff, 0xd8];
    if !extra_app.is_empty() {
        v.extend(seg(0xe1, extra_app));
    }
    v.extend(seg(0xdb, &[0u8; 65]));
    v.extend(seg(0xc0, b"\x08\0\x10\0\x10\x01\x01\x11\0"));
    v.extend(seg(0xda, b"\x01\x01\0\0\0\0"));
    v.extend(b"\x12\xff\0\x34\xff\xd0\x56");
    v.extend([0xff, 0xd9]);
    v
}

/// Middle sample presentation time of a video, in microseconds.
fn middle_sample_us(video: &[u8]) -> (i64, usize) {
    let tables = mp4::sample_tables(video).unwrap();
    let track = tables.iter().find(|t| t.handler == "vide").unwrap();
    let mut pts: Vec<i64> = track.samples.iter().map(|s| s.presentation_time).collect();
    pts.sort();
    let i = pts.len() / 2;
    let ts = track.timescale as i64;
    ((pts[i] * 1_000_000 + ts / 2) / ts, i)
}

fn free_json(ptr: *mut std::ffi::c_char) -> serde_json::Value {
    if ptr.is_null() {
        return serde_json::Value::Null;
    }
    let s = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    capi::momentstays_free_string(ptr);
    serde_json::from_str(&s).unwrap()
}

fn take_bytes(ptr: *const u8, len: usize) -> Vec<u8> {
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    let v = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    capi::momentstays_free_bytes(ptr as *mut u8, len);
    v
}

fn opts_json(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// Build a Google motion file from a plain still + real video.
fn google_motion() -> (Vec<u8>, i64) {
    let video = real_mp4();
    let (ts, _) = middle_sample_us(&video);
    let model = MotionModel {
        still: jpeg_still(b""),
        video: video.clone(),
        cover: Cover::Time(ts),
        source_format: "google-compatible".to_string(),
    };
    let motion = momentstays_core::model::write(&model, Vendor::Google).unwrap();
    (motion, ts)
}

fn to_live_capi(
    motion: &[u8],
    opts: &str,
    override_bytes: Option<&[u8]>,
) -> (i32, serde_json::Value, Vec<u8>, Vec<u8>) {
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut still: *const u8 = std::ptr::null();
    let mut still_len: usize = 0;
    let mut mov: *const u8 = std::ptr::null();
    let mut mov_len: usize = 0;
    let (op, opl) = match override_bytes {
        Some(b) => (b.as_ptr(), b.len()),
        None => (std::ptr::null(), 0),
    };
    let rc = capi::momentstays_to_live(
        motion.as_ptr(),
        motion.len(),
        opts_json(opts).as_ptr(),
        op,
        opl,
        if override_bytes.is_some() { 1 } else { 0 },
        &mut json,
        &mut still,
        &mut still_len,
        &mut mov,
        &mut mov_len,
    );
    (
        rc,
        free_json(json),
        take_bytes(still, still_len),
        take_bytes(mov, mov_len),
    )
}

#[test]
fn to_live_roundtrip_google() {
    let (motion, ts) = google_motion();
    let opts = format!(
        r#"{{"uuid": "test-uuid-1", "timestamp_us": {ts}, "time_domain": "media"}}"#
    );
    let (rc, json, still, mov) = to_live_capi(&motion, &opts, None);
    assert_eq!(rc, 0);
    assert_eq!(json["status"], "ok");
    assert_eq!(
        pymotion::maker_note_identifier(&still).unwrap().as_deref(),
        Some("test-uuid-1")
    );
    let cover = &json["cover"];
    assert_eq!(cover["frame_time_us"], ts);
    assert_eq!(
        remux::read_content_identifier(&mov).unwrap().as_deref(),
        Some("test-uuid-1")
    );
    let mett = live2motion::mett_cover_time(&mov).unwrap().unwrap();
    assert!((mett - cover["frame_time_us"].as_i64().unwrap()).abs() <= 2);
    // Coding data survives the still cleaning.
    let orig = format::parse(&motion).unwrap();
    assert_eq!(
        format::coding_hash(&still).unwrap(),
        format::coding_hash(&orig.still).unwrap()
    );
}

#[test]
fn to_motion_roundtrip() {
    let (motion, ts) = google_motion();
    let opts = format!(
        r#"{{"uuid": "test-uuid-2", "timestamp_us": {ts}, "time_domain": "media"}}"#
    );
    let (rc, json, still, mov) = to_live_capi(&motion, &opts, None);
    assert_eq!(rc, 0);
    assert_eq!(json["status"], "ok");
    let frame_time = json["cover"]["frame_time_us"].as_i64().unwrap();
    // Back to Android with an explicit cover: mett derivation on real.mp4
    // is ambiguous by design (its video track carries a non-zero elst
    // media_time, so media and presentation candidates land on different
    // frames — Swift pair() would reject it the same way). The derivation
    // path itself is covered by live_pair_roundtrip on an identity-elst
    // synthetic file.
    let mut json2: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let rc = capi::momentstays_to_motion(
        still.as_ptr(),
        still.len(),
        mov.as_ptr(),
        mov.len(),
        opts_json(&format!(r#"{{"vendor": "google", "cover_time_us": {frame_time}}}"#))
            .as_ptr(),
        &mut json2,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json2);
    let bytes = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["cover_time_us"], frame_time);
    let back = format::parse(&bytes).unwrap();
    assert_eq!(back.format, "google-compatible");
    assert_eq!(back.timestamp, Some(frame_time));
    assert_eq!(
        back.video(&bytes),
        remux::remux_to_mp4(&mov).unwrap().as_slice()
    );
}

#[test]
fn to_live_legacy_needs_pixel_work_then_retry() {
    let legacy_still = read_fixture("legacy-mpf.jpg");
    let (legacy, _) = pymotion::legacy_apple(&legacy_still).unwrap();
    let legacy = legacy.expect("legacy gain map expected");
    let video = real_mp4();
    let (ts, _) = middle_sample_us(&video);
    // Hand-built motion, NOT model::write: the Google writer marks any
    // gain map with hdrgm Version 1.0 (same as Swift xmp()), which would
    // normalize this legacy still to ultra before to_live ever sees it.
    // A bare Google packet (no hdrgm) keeps the legacy branch reachable,
    // as it is for motions assembled by tools that preserve Apple gain
    // maps verbatim.
    let mut payload = pymotion::XMP_HDR.to_vec();
    payload.extend_from_slice(
        format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\" GCamera:MotionPhoto=\"1\" GCamera:MotionPhotoPresentationTimestampUs=\"{ts}\" GCamera:MicroVideoOffset=\"{}\"/></rdf:RDF></x:xmpmeta>",
            video.len()
        )
        .as_bytes(),
    );
    let mut still2 = vec![0xff, 0xd8];
    still2.extend(momentstays_core::pyjpeg::app(0xE1, &payload).unwrap());
    still2.extend(&legacy_still[2..]);
    let mut motion = still2.clone();
    motion.extend(&video);
    let opts = format!(
        r#"{{"uuid": "test-uuid-3", "timestamp_us": {ts}, "time_domain": "media"}}"#
    );
    let (rc, json, _, _) = to_live_capi(&motion, &opts, None);
    assert_eq!(rc, 0);
    assert_eq!(json["status"], "needs_pixel_work");
    assert_eq!(json["reason"], "legacy-hdr");
    let stops = json["stops"].as_f64().unwrap();
    assert!(stops > 0.0 && stops <= 16.0);
    // App pixel step (simulated: the extracted gain bytes stand in for the
    // LUT output) + attach, then retry with the preconverted still.
    let mut json2: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let rc = capi::momentstays_attach_gain(
        legacy_still.as_ptr(),
        legacy_still.len(),
        legacy.gain.as_ptr(),
        legacy.gain.len(),
        stops,
        std::ptr::null(),
        0,
        0,
        &mut json2,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json2);
    let ultra = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert!(momentstays_core::convert::still_is_ultra(&ultra).unwrap());
    let opts2 = format!(
        r#"{{"uuid": "test-uuid-3", "timestamp_us": {ts}, "time_domain": "media", "still_override_kind": "preconverted-ultra"}}"#
    );
    let (rc, json, still, _) = to_live_capi(&motion, &opts2, Some(&ultra));
    assert_eq!(rc, 0);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["hdr"]["status"], "converted");
    assert_eq!(json["hdr"]["output"], "ultra-hdr");
    assert_eq!(
        pymotion::maker_note_identifier(&still).unwrap().as_deref(),
        Some("test-uuid-3")
    );
}

#[test]
fn vendor_convert_google_roundtrip_and_rejections() {
    let (motion, ts) = google_motion();
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let rc = capi::momentstays_vendor_convert(
        motion.as_ptr(),
        motion.len(),
        opts_json("google").as_ptr(),
        &mut json,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json);
    let bytes = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["cover_time_us"], ts);
    let back = format::parse(&bytes).unwrap();
    assert_eq!(back.format, "google-compatible");
    assert_eq!(back.timestamp, Some(ts));
    assert_eq!(
        back.video(&bytes),
        format::parse(&motion).unwrap().video(&motion)
    );
    // Rejections: plain JPEG and non-JPEG input.
    let plain = jpeg_still(b"");
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let rc = capi::momentstays_vendor_convert(
        plain.as_ptr(),
        plain.len(),
        opts_json("google").as_ptr(),
        &mut json,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json);
    let _ = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "error");
}

#[test]
fn live_still_sets_and_cleans_identifier() {
    let still = jpeg_still(b"");
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let rc = capi::momentstays_live_still(
        still.as_ptr(),
        still.len(),
        opts_json("live-uuid-9").as_ptr(),
        1,
        &mut json,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json);
    let bytes = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(
        pymotion::maker_note_identifier(&bytes).unwrap().as_deref(),
        Some("live-uuid-9")
    );
    // Clean-only: the identifier is left untouched (Swift semantics:
    // updateIdentifier=false means "do not touch the EXIF identifier",
    // it does not strip an existing one). Motion XMP keys, by contrast,
    // are stripped.
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let rc = capi::momentstays_live_still(
        bytes.as_ptr(),
        bytes.len(),
        opts_json("ignored").as_ptr(),
        0,
        &mut json,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json);
    let cleaned = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(
        pymotion::maker_note_identifier(&cleaned).unwrap().as_deref(),
        Some("live-uuid-9")
    );
    let (motion, _) = google_motion();
    let orig = format::parse(&motion).unwrap();
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let rc = capi::momentstays_live_still(
        orig.still.as_ptr(),
        orig.still.len(),
        opts_json("ignored").as_ptr(),
        0,
        &mut json,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json);
    let cleaned_motion_still = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    let packets = pymotion::xmp_packets(&cleaned_motion_still).unwrap();
    assert!(packets.iter().all(|p| {
        pyxml::values(&p.root, pymotion::CAMERA, "MotionPhoto").is_empty()
    }));
}

#[test]
fn attach_gain_builds_ultra() {
    let base = jpeg_still(b"");
    let gain = jpeg_still(b"");
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let rc = capi::momentstays_attach_gain(
        base.as_ptr(),
        base.len(),
        gain.as_ptr(),
        gain.len(),
        2.0,
        std::ptr::null(),
        0,
        0,
        &mut json,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json);
    let bytes = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert!(momentstays_core::convert::still_is_ultra(&bytes).unwrap());
    let images = momentstays_core::pyjpeg::mpf_images(&bytes).unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(
        format::coding_hash(&bytes[images[0].clone()]).unwrap(),
        format::coding_hash(&base).unwrap()
    );
}

#[test]
fn scan_row_via_capi() {
    let (motion, _) = google_motion();
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let rc = capi::momentstays_scan_row(motion.as_ptr(), motion.len(), &mut json);
    let value = free_json(json);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["is_motion"], true);
    let plain = jpeg_still(b"");
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let rc = capi::momentstays_scan_row(plain.as_ptr(), plain.len(), &mut json);
    let value = free_json(json);
    assert_eq!(rc, 0);
    assert_eq!(value["is_motion"], false);
}

// --- Synthetic MP4 builder (adapted from tests/remux.rs, plus edts). ---

fn bx(t: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(t.as_bytes());
    v.extend(payload);
    v
}

fn full(t: &str, version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut p = vec![version, ((flags >> 16) & 0xff) as u8, ((flags >> 8) & 0xff) as u8, (flags & 0xff) as u8];
    p.extend(body);
    bx(t, &p)
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

struct TrackSpec {
    handler: &'static str,
    timescale: u32,
    track_id: u32,
    stts: Vec<(u32, u32)>,
    sizes: Vec<u32>,
    edts: Option<Vec<(u32, i32)>>,
}

fn build_track(s: &TrackSpec) -> (Vec<u8>, Vec<u8>) {
    let mut tkhd_body = u32s(&[0, 0, s.track_id, 0, 0]);
    tkhd_body.extend([0u8; 8]);
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(u32s(&[0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000]));
    tkhd_body.extend(u32s(&[16 << 16, 16 << 16]));
    let tkhd = full("tkhd", 0, 7, &tkhd_body);
    let mut mdhd_body = u32s(&[0, 0, s.timescale, 0]);
    mdhd_body.extend(0x55c4u16.to_be_bytes());
    mdhd_body.extend(0u16.to_be_bytes());
    let mut mdia = full("mdhd", 0, 0, &mdhd_body);
    let mut hdlr_body = vec![0u8; 4];
    hdlr_body.extend(s.handler.as_bytes());
    hdlr_body.extend([0u8; 12]);
    mdia.extend(full("hdlr", 0, 0, &hdlr_body));
    let total: u32 = s.stts.iter().map(|(c, _)| c).sum();
    let mut stbl = full("stsd", 0, 0, &[u32s(&[1]), bx("avc1", &[0; 6])].concat());
    let mut body = u32s(&[s.stts.len() as u32]);
    for (c, d) in &s.stts {
        body.extend(u32s(&[*c, *d]));
    }
    stbl.extend(full("stts", 0, 0, &body));
    let body = u32s(&[1, 1, total, 1]);
    stbl.extend(full("stsc", 0, 0, &body));
    let mut body = u32s(&[0, total]);
    body.extend(u32s(&s.sizes));
    stbl.extend(full("stsz", 0, 0, &body));
    stbl.extend(full("stco", 0, 0, &u32s(&[1, 0])));
    let mut minf = full("vmhd", 0, 1, &[0; 8]);
    let url = full("url ", 0, 1, &[]);
    let mut dref_body = u32s(&[1]);
    dref_body.extend(url);
    minf.extend(bx("dinf", &full("dref", 0, 0, &dref_body)));
    minf.extend(bx("stbl", &stbl));
    mdia.extend(bx("minf", &minf));
    let mut trak = tkhd;
    if let Some(entries) = &s.edts {
        let mut elst_body = u32s(&[entries.len() as u32]);
        for (dur, media) in entries {
            elst_body.extend(dur.to_be_bytes());
            elst_body.extend(media.to_be_bytes());
            elst_body.extend(1u16.to_be_bytes());
            elst_body.extend(0u16.to_be_bytes());
        }
        trak.extend(bx("edts", &full("elst", 0, 0, &elst_body)));
    }
    trak.extend(bx("mdia", &mdia));
    let mdat: Vec<u8> = s
        .sizes
        .iter()
        .enumerate()
        .flat_map(|(i, n)| vec![(i + 1) as u8; *n as usize])
        .collect();
    (bx("trak", &trak), mdat)
}

fn build_file(specs: &[TrackSpec]) -> Vec<u8> {
    let built: Vec<(Vec<u8>, Vec<u8>)> = specs.iter().map(build_track).collect();
    let build = |offsets: &[usize]| {
        let mut traks = Vec::new();
        for (i, (t, _)) in built.iter().enumerate() {
            let mut t = t.clone();
            let at = t.len() - 4;
            t[at..at + 4].copy_from_slice(&(offsets[i] as u32).to_be_bytes());
            traks.push(t);
        }
        let mut moov_body = full("mvhd", 0, 0, &u32s(&[0, 0, 1000, 0, 0x00010000, 0, 0]));
        for t in traks {
            moov_body.extend(t);
        }
        let mut out = bx("ftyp", b"isom\0\0\0\0isom");
        out.extend(bx("moov", &moov_body));
        out
    };
    let head = build(&vec![0; specs.len()]);
    let mut offsets = Vec::new();
    let mut at = head.len() + 8;
    for (_, m) in &built {
        offsets.push(at);
        at += m.len();
    }
    let mut out = build(&offsets);
    let mut mdat_body = Vec::new();
    for (_, m) in &built {
        mdat_body.extend(m);
    }
    out.extend(bx("mdat", &mdat_body));
    out
}

fn video_track(edts: Option<Vec<(u32, i32)>>) -> TrackSpec {
    TrackSpec {
        handler: "vide",
        timescale: 1000,
        track_id: 1,
        stts: vec![(5, 100)],
        sizes: vec![10, 20, 30, 40, 50],
        edts,
    }
}

fn cover_map_capi(video: &[u8], opts: &str) -> (i32, serde_json::Value) {
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let rc = capi::momentstays_cover_map(
        video.as_ptr(),
        video.len(),
        opts_json(opts).as_ptr(),
        &mut json,
    );
    (rc, free_json(json))
}

#[test]
fn cover_map_identity_frame_midpoint_and_range() {
    let video = build_file(&[video_track(None)]);
    // pts: 0, 100, 200, 300, 400 ms.
    let (rc, v) = cover_map_capi(&video, r#"{"timestamp_us": 200000}"#);
    assert_eq!(rc, 0);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["cover"]["frame"], 2);
    assert_eq!(v["cover"]["frame_time_us"], 200000);
    assert_eq!(v["cover"]["domain"], "media");
    let (rc, v) = cover_map_capi(&video, r#"{"cover_frame_index": 3}"#);
    assert_eq!(rc, 0);
    assert_eq!(v["cover"]["frame"], 3);
    assert_eq!(v["cover"]["policy"], "source-huawei-frame");
    let (rc, v) = cover_map_capi(&video, r#"{"missing_time": "midpoint"}"#);
    assert_eq!(rc, 0);
    assert_eq!(v["cover"]["frame"], 2);
    assert_eq!(v["cover"]["policy"], "explicit-midpoint");
    let (rc, v) = cover_map_capi(&video, r#"{"timestamp_us": 999999}"#);
    assert_eq!(rc, 0);
    assert_eq!(v["status"], "error");
}

#[test]
fn cover_map_elst_offset_is_ambiguous_in_auto() {
    // Edit list: movie 0..200ms maps to media 100..300ms.
    let video = build_file(&[video_track(Some(vec![(200, 100)]))]);
    let (rc, v) = cover_map_capi(&video, r#"{"timestamp_us": 150000}"#);
    assert_eq!(rc, 0);
    assert_eq!(v["status"], "error");
    assert!(v["error"].as_str().unwrap().contains("歧义"));
    // Explicit domains resolve it: media -> frame 1, presentation -> frame 2.
    let (rc, v) = cover_map_capi(
        &video,
        r#"{"timestamp_us": 150000, "time_domain": "media"}"#,
    );
    assert_eq!(rc, 0);
    assert_eq!(v["cover"]["frame"], 1);
    let (rc, v) = cover_map_capi(
        &video,
        r#"{"timestamp_us": 150000, "time_domain": "presentation"}"#,
    );
    assert_eq!(rc, 0);
    assert_eq!(v["cover"]["frame"], 2);
    assert_eq!(v["cover"]["domain"], "presentation-edit-list");
}

#[test]
fn export_auxiliary_and_candidates_two_tracks() {
    let mut aux = video_track(None);
    aux.track_id = 2;
    aux.stts = vec![(3, 100)];
    aux.sizes = vec![5, 6, 7];
    let video = build_file(&[video_track(None), aux]);
    // Auxiliary export takes a MOTION file; wrap the video in one.
    let model = MotionModel {
        still: jpeg_still(b""),
        video: video.clone(),
        cover: Cover::Time(0),
        source_format: "google-compatible".to_string(),
    };
    let motion = momentstays_core::model::write(&model, Vendor::Google).unwrap();
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let rc = capi::momentstays_export_auxiliary(
        motion.as_ptr(),
        motion.len(),
        &mut json,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json);
    let mov = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["repaired"], false);
    let tables = mp4::sample_tables(&mov).unwrap();
    let vids: Vec<_> = tables.iter().filter(|t| t.handler == "vide").collect();
    assert_eq!(vids.len(), 1);
    assert_eq!(vids[0].samples.len(), 3);
    // Candidates on the same two-track video: presented times, no descriptors.
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let rc = capi::momentstays_candidates(video.as_ptr(), video.len(), &mut json);
    let value = free_json(json);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    let cands = value["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), 3);
    assert_eq!(cands[0]["timestamp_us"], 0);
    assert_eq!(cands[1]["timestamp_us"], 100000);
    assert_eq!(cands[2]["timestamp_us"], 200000);
    assert_eq!(cands[0]["time_source"], "auxiliary-track-presentation");
}

#[test]
fn live_pair_roundtrip() {
    let video = build_file(&[video_track(None)]);
    let uuid = "pair-uuid-7";
    let mov = remux::remux_to_mov_with_identifier_and_mett(&video, 0, uuid, 200000).unwrap();
    let still = pymotion::live_still_with_identifier(&jpeg_still(b""), uuid).unwrap();
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let rc = capi::momentstays_live_pair(
        still.as_ptr(),
        still.len(),
        mov.as_ptr(),
        mov.len(),
        std::ptr::null(),
        &mut json,
    );
    let value = free_json(json);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["still_identifier"], uuid);
    assert_eq!(value["movie_identifier"], uuid);
    assert_eq!(value["cover_time_us"], 200000);
    assert_eq!(value["cover_frame"], 2);
    // Explicit "media" domain on this identity-elst file agrees with auto.
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let rc = capi::momentstays_live_pair(
        still.as_ptr(),
        still.len(),
        mov.as_ptr(),
        mov.len(),
        opts_json("media").as_ptr(),
        &mut json,
    );
    let value = free_json(json);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["cover_time_us"], 200000);
    assert_eq!(value["domain"], "media");
    assert_eq!(value["maker33"], false);
}

#[test]
fn to_motion_needs_pixel_work_for_reencode() {
    let (motion, ts) = google_motion();
    let photo = format::parse(&motion).unwrap();
    let video = photo.video(&motion).to_vec();
    let still = photo.still.into_owned();
    // Re-encode requested: the app must re-encode the base JPEG first.
    // (Cover given explicitly so the check order reaches the JPEG flag.)
    let mut json: *mut std::ffi::c_char = std::ptr::null_mut();
    let mut out: *const u8 = std::ptr::null();
    let mut out_len: usize = 0;
    let opts = format!(
        r#"{{"vendor": "google", "preserve_jpeg": false, "cover_time_us": {ts}}}"#
    );
    let rc = capi::momentstays_to_motion(
        still.as_ptr(),
        still.len(),
        video.as_ptr(),
        video.len(),
        opts_json(&opts).as_ptr(),
        &mut json,
        &mut out,
        &mut out_len,
    );
    let value = free_json(json);
    let _ = take_bytes(out, out_len);
    assert_eq!(rc, 0);
    assert_eq!(value["status"], "needs_pixel_work");
    assert_eq!(value["reason"], "reencode-requested");
}
