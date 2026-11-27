//! Graceful degradation when the system lacks a decoder (a Flatpak without
//! org.freedesktop.Platform.codecs-extra has no HEVC/AAC decoders). Its own
//! test binary: decoder ranks/plugin paths are read once per process.
#![cfg(all(feature = "linux-native", target_os = "linux"))]
use momentstays_core::engine::Native;
use momentstays_core::platform::SystemNative;
use momentstays_core::ErrorKind;
use std::process::Command;

const HEVC_AAC_DECODERS: &[&str] = &[
    "avdec_h265",
    "libde265dec",
    "vah265dec",
    "vaapih265dec",
    "v4l2slh265dec",
    "v4l2h265dec",
    "nvh265dec",
    "nvh265sldec",
    "msdkh265dec",
    "qsvh265dec",
    "vulkanh265dec",
    "avdec_aac",
    "avdec_aac_fixed",
    "faad",
    "fdkaacdec",
];

fn ffmpeg(args: &[&str]) -> bool {
    match Command::new("ffmpeg").args(args).output() {
        Ok(o) if o.status.success() => true,
        Ok(o) => panic!("ffmpeg failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(_) if std::env::var_os("MOMENTSTAYS_REQUIRE_MEDIA_TOOLS").is_some() => {
            panic!("ffmpeg is required in CI")
        }
        Err(_) => false,
    }
}

#[test]
fn missing_decoders_are_unsupported_not_damaged() {
    let ranks: Vec<String> = HEVC_AAC_DECODERS.iter().map(|d| format!("{d}:0")).collect();
    // Before the first GStreamer/libheif call in this process.
    std::env::set_var("GST_PLUGIN_FEATURE_RANK", ranks.join(","));
    std::env::set_var("LIBHEIF_PLUGIN_PATH", "/nonexistent-momentstays");
    let dir = std::env::temp_dir().join(format!("momentstays-missing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let movie = dir.join("hevc.mp4");
    if !ffmpeg(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "testsrc=s=64x48:r=10:d=1",
        "-f",
        "lavfi",
        "-i",
        "sine=d=1",
        "-pix_fmt",
        "yuv420p",
        "-c:v",
        "libx265",
        "-tag:v",
        "hvc1",
        "-x265-params",
        "log-level=error",
        "-c:a",
        "aac",
        "-shortest",
        movie.to_str().unwrap(),
    ]) {
        eprintln!("skip: ffmpeg not installed");
        return;
    }
    let bytes = std::fs::read(&movie).unwrap();
    let parsed = momentstays_core::mp4::Movie::parse(&bytes).unwrap();
    let track = parsed.video(0).unwrap();
    let first = track
        .samples
        .samples
        .iter()
        .map(|s| s.presentation_time)
        .min()
        .unwrap();
    let t = first as f64 / track.samples.timescale as f64;
    let err = SystemNative.frame_jpeg(&movie, 0, t).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unsupported, "{err}");
    // Unknown (None), not "fails to decode" (Some(false)): the engine then
    // records the native audio check as unavailable instead of refusing.
    assert_eq!(SystemNative.audio_decodes(&movie, 1).unwrap(), None);
    // HEIC: libheif may have a built-in HEVC decoder; either it decodes,
    // or the answer is "unknown" — never "damaged".
    let jpeg = dir.join("still.jpg");
    let heic = dir.join("still.heic");
    assert!(ffmpeg(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "testsrc=s=64x48:d=1",
        "-frames:v",
        "1",
        jpeg.to_str().unwrap(),
    ]));
    let made = Command::new("heif-enc")
        .args([jpeg.to_str().unwrap(), "-o", heic.to_str().unwrap()])
        .env_remove("LIBHEIF_PLUGIN_PATH")
        .output();
    match made {
        Ok(o) if o.status.success() => {}
        _ if std::env::var_os("MOMENTSTAYS_REQUIRE_MEDIA_TOOLS").is_some() => {
            panic!("heif-enc is required in CI")
        }
        _ => return,
    }
    let data = std::fs::read(&heic).unwrap();
    match SystemNative.image_decodes(&data) {
        Some(true) => {}
        None => {
            let e = SystemNative.reencode_jpeg(&heic, 0.9).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::Unsupported, "{e}");
        }
        Some(false) => panic!("missing HEVC decoder reported as damaged HEIC"),
    }
}
