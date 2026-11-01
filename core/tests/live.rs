//! Motion Photo -> Live Photo pair (`live::from_motion`, `motion-to-live`).
//!
//! `real.mp4` (5 frames at 0.2 s, media PTS 0.4..1.2 s) has an edit list
//! starting at media 0.4 s, so frame i is shown at i * 0.2 s. A cover of
//! 0.4 s therefore means frame 0 on the media timeline and frame 2 on the
//! presentation timeline: the case the converter must refuse to guess.

mod common;

use base64::Engine;
use momentstays_core::format;
use momentstays_core::heifwrite::assemble_heic;
use momentstays_core::live::{self, TimeDomain};
use momentstays_core::live2motion::mett_cover_time;
use momentstays_core::model::{self, Cover, MotionModel, Vendor};
use momentstays_core::mp4;
use momentstays_core::remux::read_content_identifier;
use std::path::PathBuf;
use std::process::Command;

const UUID: &str = "0F2A7C3E-1B4D-4E5F-8A9B-0C1D2E3F4A5B";

fn b64(path: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    base64::engine::general_purpose::STANDARD
        .decode(std::fs::read_to_string(p).unwrap().trim_ascii())
        .unwrap()
}

fn real_mp4() -> Vec<u8> {
    b64("tests/fixtures-samples/real.mp4.b64")
}

/// `real.mp4` with its edit list disabled (the `edts` box renamed to
/// `free`, sizes unchanged): media and presentation timelines coincide.
fn real_mp4_without_edit_list() -> Vec<u8> {
    let mut d = real_mp4();
    let i = d.windows(4).position(|w| w == b"edts").unwrap();
    d[i..i + 4].copy_from_slice(b"free");
    d
}

fn motion_jpeg(video: &[u8], cover: Cover, vendor: Vendor) -> Vec<u8> {
    let m = MotionModel {
        still: b64("tests/fixtures-pycli/base.jpg.b64"),
        video: video.to_vec(),
        cover,
        source_format: "test".into(),
    };
    model::write(&m, vendor).unwrap()
}

fn convert(file: &[u8], domain: TimeDomain) -> momentstays_core::Result<live::LivePhoto> {
    let p = format::parse(file).unwrap();
    live::from_motion(&p, file, UUID, domain, 0)
}

/// Checks every Live Photo pair must pass, whatever the cover.
fn assert_valid_pair(source: &[u8], pair: &live::LivePhoto) {
    let p = format::parse(source).unwrap();
    assert_eq!(pair.identifier, UUID);
    assert_eq!(
        live::still_content_identifier(&pair.still)
            .unwrap()
            .as_deref(),
        Some(UUID)
    );
    assert_eq!(
        format::coding_hash(&pair.still).unwrap(),
        format::coding_hash(&p.still).unwrap(),
        "still coding data unchanged"
    );
    assert!(
        format::parse(&pair.still).is_err(),
        "no motion left in the still"
    );
    assert_eq!(
        read_content_identifier(&pair.movie).unwrap().as_deref(),
        Some(UUID)
    );
    assert_eq!(
        mett_cover_time(&pair.movie).unwrap(),
        Some(pair.cover.marker_us)
    );
    // Video samples are the source's, byte for byte.
    let src = mp4::sample_tables(p.video(source)).unwrap();
    let out = mp4::sample_tables(&pair.movie).unwrap();
    let vid = |t: &[mp4::TrackSamples], d: &[u8]| -> Vec<Vec<u8>> {
        let v = t.iter().find(|t| t.handler == "vide").unwrap();
        v.samples
            .iter()
            .map(|s| d[s.offset..s.offset + s.size].to_vec())
            .collect()
    };
    assert_eq!(vid(&src, p.video(source)), vid(&out, &pair.movie));
}

#[test]
fn ambiguous_cover_is_refused_in_auto_mode() {
    let file = motion_jpeg(&real_mp4(), Cover::Time(400_000), Vendor::Google);
    let err = convert(&file, TimeDomain::Auto).unwrap_err();
    assert!(err.0.contains("歧义"), "{}", err.0);
}

#[test]
fn media_and_presentation_domains_pick_their_frame() {
    let file = motion_jpeg(&real_mp4(), Cover::Time(400_000), Vendor::Google);

    let media = convert(&file, TimeDomain::Media).unwrap();
    assert_valid_pair(&file, &media);
    assert_eq!(media.cover.domain, "media");
    assert_eq!(media.cover.frame, 0);
    assert_eq!(media.cover.frame_time_us, 400_000);
    assert_eq!(media.cover.marker_us, 0, "frame 0 is shown at movie time 0");
    assert_eq!(media.cover.error_us, Some(0));

    let pres = convert(&file, TimeDomain::Presentation).unwrap();
    assert_valid_pair(&file, &pres);
    assert_eq!(pres.cover.domain, "presentation-edit-list");
    assert_eq!(pres.cover.frame, 2);
    assert_eq!(pres.cover.frame_time_us, 800_000);
    assert_eq!(pres.cover.marker_us, 400_000);
}

#[test]
fn unambiguous_cover_converts_in_auto_mode() {
    // 0 s: frame 0 on the presentation timeline; on the media timeline it is
    // 0.4 s before the first frame, out of tolerance. One answer only.
    let file = motion_jpeg(&real_mp4(), Cover::Time(0), Vendor::Google);
    let pair = convert(&file, TimeDomain::Auto).unwrap();
    assert_valid_pair(&file, &pair);
    assert_eq!(pair.cover.domain, "presentation-edit-list");
    assert_eq!((pair.cover.frame, pair.cover.marker_us), (0, 0));
}

#[test]
fn without_edit_list_both_timelines_agree() {
    let file = motion_jpeg(
        &real_mp4_without_edit_list(),
        Cover::Time(600_000),
        Vendor::Samsung,
    );
    let pair = convert(&file, TimeDomain::Auto).unwrap();
    assert_valid_pair(&file, &pair);
    assert_eq!(pair.cover.frame, 1);
    assert_eq!(pair.cover.marker_us, 600_000);
}

#[test]
fn huawei_frame_cover_is_used_as_is() {
    let file = motion_jpeg(&real_mp4(), Cover::Frame(3), Vendor::Huawei);
    let pair = convert(&file, TimeDomain::Auto).unwrap();
    assert_valid_pair(&file, &pair);
    assert_eq!(pair.cover.domain, "frame");
    assert_eq!(pair.cover.frame, 3);
    assert_eq!(pair.cover.frame_time_us, 1_000_000);
    assert_eq!(pair.cover.marker_us, 600_000);
    assert_eq!(pair.cover.error_us, None);
}

#[test]
fn cover_outside_the_video_is_refused() {
    let file = motion_jpeg(&real_mp4(), Cover::Time(5_000_000), Vendor::Google);
    assert!(convert(&file, TimeDomain::Auto).is_err());
}

#[test]
fn heic_motion_photo_gives_a_heic_still() {
    let still = common::build_still(true, true);
    let file = assemble_heic(
        &still,
        &real_mp4_without_edit_list(),
        Vendor::Google,
        400_000,
    )
    .unwrap();
    let pair = convert(&file, TimeDomain::Auto).unwrap();
    assert!(pair.heic);
    assert_valid_pair(&file, &pair);
    assert_eq!(pair.cover.frame, 0);
}

#[test]
fn identifiers_are_fresh_uppercase_v4() {
    let a = live::new_content_identifier().unwrap();
    let b = live::new_content_identifier().unwrap();
    assert_ne!(a, b);
    assert_eq!(a.len(), 36);
    assert_eq!(&a[14..15], "4");
    assert!("89AB".contains(&a[19..20]));
    assert_eq!(a, a.to_uppercase());
}

#[test]
fn still_without_identifier_reads_none() {
    let still = b64("tests/fixtures-pycli/base.jpg.b64");
    assert_eq!(live::still_content_identifier(&still).unwrap(), None);
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ms-live-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn cli_writes_a_named_pair() {
    let dir = tmp("cli");
    let input = dir.join("IMG_0001.MP.jpg");
    std::fs::write(
        &input,
        motion_jpeg(&real_mp4(), Cover::Time(400_000), Vendor::Google),
    )
    .unwrap();
    let out_dir = dir.join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let bin = env!("CARGO_BIN_EXE_momentstays-convert");
    let run = |extra: &[&str]| {
        let mut args = vec![
            "motion-to-live",
            input.to_str().unwrap(),
            "-o",
            out_dir.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        Command::new(bin).args(&args).output().unwrap()
    };
    // Auto refuses the ambiguous cover and writes nothing.
    let refused = run(&[]);
    assert!(!refused.status.success());
    assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 0);

    let ok = run(&["--time-domain", "presentation", "--uuid", UUID]);
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(v["content_identifier"], UUID);
    assert_eq!(v["cover"]["frame"], 2);
    let still = std::fs::read(out_dir.join("IMG_0001.MP.jpg")).unwrap();
    let movie = std::fs::read(out_dir.join("IMG_0001.MP.mov")).unwrap();
    assert_eq!(
        live::still_content_identifier(&still).unwrap().as_deref(),
        Some(UUID)
    );
    assert_eq!(
        read_content_identifier(&movie).unwrap().as_deref(),
        Some(UUID)
    );
}

#[test]
fn cli_generates_an_identifier_and_needs_an_existing_dir() {
    let dir = tmp("cli2");
    let input = dir.join("a.jpg");
    std::fs::write(
        &input,
        motion_jpeg(&real_mp4(), Cover::Time(0), Vendor::Google),
    )
    .unwrap();
    let bin = env!("CARGO_BIN_EXE_momentstays-convert");
    let missing = Command::new(bin)
        .args([
            "motion-to-live",
            input.to_str().unwrap(),
            "-o",
            dir.join("nope").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    // Output next to the input would be `a.jpg` again: refused, input kept.
    let original = std::fs::read(&input).unwrap();
    let clash = Command::new(bin)
        .args([
            "motion-to-live",
            input.to_str().unwrap(),
            "-o",
            dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!clash.status.success());
    assert!(String::from_utf8_lossy(&clash.stderr).contains("输出路径不能与输入相同"));
    assert_eq!(std::fs::read(&input).unwrap(), original);
    assert!(!dir.join("a.mov").exists(), "no half pair left behind");

    let out = dir.join("out");
    std::fs::create_dir(&out).unwrap();
    let ok = Command::new(bin)
        .args([
            "motion-to-live",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    let id = v["content_identifier"].as_str().unwrap();
    assert_eq!(id.len(), 36);
    let still = std::fs::read(out.join("a.jpg")).unwrap();
    assert_eq!(
        live::still_content_identifier(&still).unwrap().as_deref(),
        Some(id)
    );
    assert!(out.join("a.mov").exists());
}

/// `to-motion --cover-from-mett` reads the marker as movie time and must
/// name the frame shown there. With real.mp4's edit list the marker at 0 s
/// is frame 0; the old media-only lookup refused it as out of range.
#[test]
fn to_motion_huawei_uses_the_frame_shown_at_the_marker() {
    let dir = tmp("mett-huawei");
    let mov =
        momentstays_core::remux::remux_to_mov_with_identifier_and_mett(&real_mp4(), 0, UUID, 0)
            .unwrap();
    let (still, movie, out) = (dir.join("s.jpg"), dir.join("m.mov"), dir.join("o.jpg"));
    std::fs::write(&still, b64("tests/fixtures-pycli/base.jpg.b64")).unwrap();
    std::fs::write(&movie, &mov).unwrap();
    let bin = env!("CARGO_BIN_EXE_momentstays-convert");
    let r = Command::new(bin)
        .args([
            "to-motion",
            still.to_str().unwrap(),
            movie.to_str().unwrap(),
            "--to",
            "huawei",
            "--cover-from-mett",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(format::parse(&bytes).unwrap().cover_frame, Some(0));
}

/// `vendor --to huawei` converts a time cover to a frame with the same
/// timeline rules as `motion-to-live`: ambiguous refused, overridable.
#[test]
fn vendor_time_to_frame_follows_the_time_domain() {
    let dir = tmp("vendor-domain");
    let input = dir.join("g.jpg");
    std::fs::write(
        &input,
        motion_jpeg(&real_mp4(), Cover::Time(400_000), Vendor::Google),
    )
    .unwrap();
    let bin = env!("CARGO_BIN_EXE_momentstays-convert");
    let run = |extra: &[&str], name: &str| {
        let out = dir.join(name);
        let mut args = vec![
            "vendor",
            input.to_str().unwrap(),
            out.to_str().unwrap(),
            "--to",
            "huawei",
        ];
        args.extend_from_slice(extra);
        let r = Command::new(bin).args(&args).output().unwrap();
        (r, out)
    };
    let (auto, _) = run(&[], "a.jpg");
    assert!(!auto.status.success());
    assert!(String::from_utf8_lossy(&auto.stderr).contains("歧义"));
    for (domain, frame) in [("media", 0), ("presentation", 2)] {
        let (r, out) = run(&["--time-domain", domain], &format!("{domain}.jpg"));
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let bytes = std::fs::read(out).unwrap();
        assert_eq!(
            format::parse(&bytes).unwrap().cover_frame,
            Some(frame),
            "{domain}"
        );
    }
}

#[test]
fn absurd_cover_time_is_refused_before_any_arithmetic() {
    let file = motion_jpeg(&real_mp4(), Cover::Time(i64::MAX), Vendor::Google);
    let err = convert(&file, TimeDomain::Auto).unwrap_err();
    assert!(err.0.contains("超出范围"), "{}", err.0);
}
