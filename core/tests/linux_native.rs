//! Linux system codec backend (`--features linux-native`) on synthetic media
//! made with ffmpeg / heif-enc, plus the public osxphotos iPhone sample when
//! it is provided (`MOMENTSTAYS_PUBLIC_SAMPLES`, see tests/SOURCES.md).
//! Without the tools these tests skip, unless MOMENTSTAYS_REQUIRE_MEDIA_TOOLS
//! is set (CI), where a missing tool is a failure.
#![cfg(all(feature = "linux-native", target_os = "linux"))]
use momentstays_core::engine::{Gray, Native};
use momentstays_core::platform::{portable, SystemNative};
use momentstays_core::{jpeg, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn work(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "momentstays-linux-native-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run a tool; None (skip) when it is not installed and not required.
fn tool(program: &str, args: &[&str]) -> Option<()> {
    match Command::new(program).args(args).output() {
        Ok(out) if out.status.success() => Some(()),
        Ok(out) => panic!(
            "{program} {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(_) if std::env::var_os("MOMENTSTAYS_REQUIRE_MEDIA_TOOLS").is_some() => {
            panic!("{program} is required in CI")
        }
        Err(_) => {
            eprintln!("skip: {program} not installed");
            None
        }
    }
}

fn gray_frame_jpeg(width: usize, height: usize, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            pixels.push(f(x, y));
        }
    }
    SystemNative
        .encode_gray_jpeg(
            &Gray {
                width,
                height,
                stride: width,
                pixels,
            },
            1.0,
        )
        .unwrap()
}

fn mean_luma(jpeg: &[u8]) -> f64 {
    let rgba = SystemNative.decode_srgb_jpeg(jpeg).unwrap().unwrap();
    let mut sum = 0u64;
    for row in rgba.pixels.chunks(rgba.stride).take(rgba.height) {
        for p in row[..rgba.width * 4].chunks(4) {
            sum += p[1] as u64;
        }
    }
    sum as f64 / (rgba.width * rgba.height) as f64
}

#[test]
fn jpeg_gray_encode_decode_and_refusals() {
    let encoded = gray_frame_jpeg(16, 16, |_, _| 128);
    assert_eq!(SystemNative.image_decodes(&encoded), Some(true));
    let decoded = SystemNative.decode_gray_jpeg(&encoded).unwrap().unwrap();
    assert_eq!(
        (decoded.width, decoded.height, decoded.stride),
        (16, 16, 16)
    );
    assert!(decoded.pixels.iter().all(|v| v.abs_diff(128) <= 1));
    // One component, JFIF, the size the HDR writers check.
    let segs = jpeg::segments(&encoded).unwrap();
    let sof = segs.iter().find(|s| s.marker == 0xC0).unwrap();
    assert_eq!(sof.payload(&encoded)[5], 1);
    // Colour JPEG is not a gray map: Ok(None), not an error.
    let Some(colour) = rgb_jpeg() else {
        return;
    };
    assert_eq!(SystemNative.decode_gray_jpeg(&colour).unwrap(), None);
    // Truncation is "incomplete" (ImageIO status) everywhere.
    let cut = &colour[..colour.len() * 2 / 3];
    assert_eq!(SystemNative.image_decodes(cut), Some(false));
    assert!(SystemNative.decode_srgb_jpeg(cut).is_err());
    assert_eq!(SystemNative.image_decodes(b"not an image"), Some(false));
    let bad = Gray {
        width: 16,
        height: 16,
        stride: 16,
        pixels: vec![0],
    };
    assert!(SystemNative.encode_gray_jpeg(&bad, 1.0).is_err());
    let ok = Gray {
        pixels: vec![0; 256],
        ..bad
    };
    assert!(SystemNative.encode_gray_jpeg(&ok, f64::NAN).is_err());
    assert!(SystemNative.encode_gray_jpeg(&ok, 1.01).is_err());
    let padded = Gray {
        width: 3,
        height: 2,
        stride: 8,
        pixels: vec![200; 16],
    };
    let out = SystemNative.encode_gray_jpeg(&padded, 1.0).unwrap();
    let back = SystemNative.decode_gray_jpeg(&out).unwrap().unwrap();
    assert_eq!((back.width, back.height), (3, 2));
}

#[test]
fn srgb_decode_keeps_coded_axes() {
    // Same probe as the Apple test: block order detects flips/transposes.
    let (w, h) = (32, 48);
    let encoded = gray_frame_jpeg(w, h, |x, y| 20 + 90 * (x / 16) as u8 + 60 * (y / 16) as u8);
    let rgba = SystemNative.decode_srgb_jpeg(&encoded).unwrap().unwrap();
    assert_eq!((rgba.width, rgba.height, rgba.stride), (w, h, w * 4));
    let v = |x: usize, y: usize| rgba.pixels[y * rgba.stride + 4 * x];
    for y in [8, 24, 40] {
        assert!(v(8, y) < v(24, y));
    }
    for x in [8, 24] {
        assert!(v(x, 8) < v(x, 24) && v(x, 24) < v(x, 40));
    }
    assert!(rgba.pixels.chunks(4).all(|p| p[3] == 255));
}

fn write_tmp(name: &str, data: &[u8]) -> PathBuf {
    let p = work(name).join(name);
    std::fs::write(&p, data).unwrap();
    p
}

/// 24x16 colour JPEG (red left, blue right, made by ffmpeg) with EXIF
/// orientation 6, an ICC profile, XMP and a vendor segment.
fn rgb_jpeg() -> Option<Vec<u8>> {
    let dir = work("rgb");
    let out = dir.join("rgb.jpg");
    tool(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "nullsrc=s=24x16:d=1,format=rgb24,geq=r='if(lt(X\\,12)\\,220\\,30)':g=40:b='if(lt(X\\,12)\\,30\\,220)'",
            "-frames:v",
            "1",
            "-q:v",
            "2",
            out.to_str()?,
        ],
    )?;
    let plain = std::fs::read(out).unwrap();
    let mut segments = vec![exif(6)];
    segments.extend(portable::icc_segments(&SystemNative.srgb_icc().unwrap()).unwrap());
    let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    xmp.extend(b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>");
    segments.push(jpeg::app(0xE1, &xmp).unwrap());
    segments.push(jpeg::app(0xEA, b"vendor").unwrap());
    // ffmpeg's mjpeg writes JFIF + a COM segment; put ours after SOI/JFIF.
    Some(portable::insert_segments(&plain, &segments).unwrap())
}

fn exif(orientation: u16) -> Vec<u8> {
    portable::exif_orientation_segment(orientation)
}

#[test]
fn reencode_jpeg_keeps_exif_icc_and_coded_pixels() {
    let Some(src) = rgb_jpeg() else {
        return;
    };
    let path = write_tmp("src.jpg", &src);
    let out = SystemNative.reencode_jpeg(&path, 0.95).unwrap();
    assert_eq!(SystemNative.image_decodes(&out), Some(true));
    // EXIF (orientation untouched) and ICC carried; XMP/vendor dropped.
    let carried = portable::carried_segments(&out).unwrap();
    assert_eq!(carried, portable::carried_segments(&src).unwrap());
    assert!(!out.windows(6).any(|w| w == b"vendor"));
    assert!(!out.windows(9).any(|w| w == b"xmpmeta x"));
    // Pixels not rotated (orientation stays metadata).
    let rgba = SystemNative.decode_srgb_jpeg(&out).unwrap().unwrap();
    assert_eq!((rgba.width, rgba.height), (24, 16));
    // Red left, blue right; the ICC (sRGB) maps to itself.
    let px = |x: usize| &rgba.pixels[8 * rgba.stride + 4 * x..][..3];
    assert!(px(4)[0] > 150 && px(4)[2] < 100, "{:?}", px(4));
    assert!(px(20)[2] > 150 && px(20)[0] < 100, "{:?}", px(20));
    assert!(SystemNative.reencode_jpeg(&path, -0.1).is_err());
    assert!(SystemNative
        .reencode_jpeg(&write_tmp("x.bin", b"plain"), 0.9)
        .is_err());
}

fn heic(dir: &Path, name: &str, jpeg_input: &[u8]) -> Option<PathBuf> {
    let src = dir.join(format!("{name}.jpg"));
    std::fs::write(&src, jpeg_input).unwrap();
    let out = dir.join(format!("{name}.heic"));
    tool(
        "heif-enc",
        &["-q", "90", src.to_str()?, "-o", out.to_str()?],
    )?;
    Some(out)
}

#[test]
fn heic_without_gain_map_decodes_and_reencodes() {
    let dir = work("heic");
    let Some(colour) = rgb_jpeg() else {
        return;
    };
    let Some(path) = heic(&dir, "plain", &colour) else {
        return;
    };
    let data = std::fs::read(&path).unwrap();
    assert_eq!(SystemNative.image_decodes(&data), Some(true));
    assert!(SystemNative.heic_gain_map(&path).unwrap().is_none());
    let out = SystemNative.reencode_jpeg(&path, 0.9).unwrap();
    let rgba = SystemNative.decode_srgb_jpeg(&out).unwrap().unwrap();
    // Coded orientation: heif-enc stores pixels upright + irot, or rotated
    // pixels; either way the JPEG keeps the HEIC's coded axes and an
    // orientation tag that shows it upright.
    let o = portable::heif_primary_orientation(&data)
        .unwrap()
        .unwrap_or(portable::Orientation::IDENTITY);
    let coded = o.size(rgba.width, rgba.height);
    assert!(coded == (24, 16) || coded == (16, 24), "{coded:?}");
    // Damaged HEIC: a decoder error, never a panic or a pass.
    let mut broken = data.clone();
    let n = broken.len();
    broken.truncate(n - 64);
    assert_ne!(SystemNative.image_decodes(&broken), Some(true));
}

struct Movie {
    path: PathBuf,
}

/// 64x48, 10 fps, 30 frames; frame N has video-range luma 16 + 7N
/// (full-range RGB ≈ 8.15·N). B-frames + edit list.
fn movie(dir: &Path, codec: &str, rotation: Option<&str>) -> Option<Movie> {
    let plain = dir.join(format!("{codec}-plain.mp4"));
    let mut args = vec![
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "nullsrc=s=64x48:r=10:d=3,geq=lum='16+7*N':cb=128:cr=128",
        "-f",
        "lavfi",
        "-i",
        "sine=f=440:d=3",
        "-pix_fmt",
        "yuv420p",
        "-g",
        "10",
        "-bf",
        "2",
        "-c:a",
        "aac",
        "-shortest",
    ];
    match codec {
        "h264" => args.extend(["-c:v", "libx264"]),
        _ => args.extend([
            "-c:v",
            "libx265",
            "-tag:v",
            "hvc1",
            "-x265-params",
            "log-level=error",
        ]),
    }
    args.push(plain.to_str()?);
    tool("ffmpeg", &args)?;
    let Some(rotation) = rotation else {
        return Some(Movie { path: plain });
    };
    let rotated = dir.join(format!("{codec}-rotated.mov"));
    tool(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-display_rotation",
            rotation,
            "-i",
            plain.to_str()?,
            "-c",
            "copy",
            rotated.to_str()?,
        ],
    )?;
    Some(Movie { path: rotated })
}

fn sample_seconds(path: &Path, index: usize) -> f64 {
    let data = std::fs::read(path).unwrap();
    let m = momentstays_core::mp4::Movie::parse(&data).unwrap();
    let t = m.video(0).unwrap();
    let mut pts: Vec<i64> = t
        .samples
        .samples
        .iter()
        .map(|s| s.presentation_time)
        .collect();
    pts.sort_unstable();
    pts[index] as f64 / t.samples.timescale as f64
}

fn check_frames(codec: &str) {
    let dir = work(codec);
    let Some(m) = movie(&dir, codec, None) else {
        return;
    };
    // Presentation-order frames, including ones behind B-frame reordering.
    for index in [0usize, 1, 2, 7, 10, 13, 29] {
        let t = sample_seconds(&m.path, index);
        let (jpeg, w, h) = SystemNative.frame_jpeg(&m.path, 0, t).unwrap();
        assert_eq!((w, h), (64, 48));
        assert_eq!(SystemNative.image_decodes(&jpeg), Some(true));
        let luma = mean_luma(&jpeg);
        let expected = (index * 7) as f64 * 255.0 / 219.0;
        assert!(
            (luma - expected).abs() < 3.0,
            "{codec} frame {index}: luma {luma}, expected {expected}"
        );
        // Mid-frame times show the same frame (zero-tolerance generator).
        let (mid, _, _) = SystemNative.frame_jpeg(&m.path, 0, t + 0.04).unwrap();
        assert!(
            (mean_luma(&mid) - expected).abs() < 3.0,
            "{codec} mid {index}"
        );
    }
    assert!(SystemNative.frame_jpeg(&m.path, 1, 0.0).is_err());
    assert!(SystemNative.frame_jpeg(&m.path, 0, f64::NAN).is_err());
    // Outside every edit: refused, like AVFoundation's segment lookup.
    assert!(SystemNative.frame_jpeg(&m.path, 0, 60.0).is_err());
    assert_eq!(SystemNative.audio_decodes(&m.path, 1).unwrap(), Some(true));
    assert_eq!(SystemNative.audio_decodes(&m.path, 2).unwrap(), Some(false));
}

#[test]
fn h264_frames_by_presentation_time() {
    check_frames("h264");
}

#[test]
fn hevc_frames_by_presentation_time() {
    check_frames("hevc");
}

#[test]
fn track_transform_is_applied() {
    let dir = work("rotate");
    // Left half dark, right half bright, then 90° counter-clockwise.
    let plain = dir.join("lr.mp4");
    let Some(()) = tool(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "nullsrc=s=64x48:r=10:d=1,geq=lum='if(lt(X\\,32)\\,40\\,220)':cb=128:cr=128",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx264",
            plain.to_str().unwrap(),
        ],
    ) else {
        return;
    };
    let rotated = dir.join("lr-ccw.mov");
    tool(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-display_rotation",
            "90",
            "-i",
            plain.to_str().unwrap(),
            "-c",
            "copy",
            rotated.to_str().unwrap(),
        ],
    )
    .unwrap();
    let data = std::fs::read(&rotated).unwrap();
    assert_eq!(portable::video_orientation(&data, 0).unwrap().exif(), 8);
    let (jpeg, w, h) = SystemNative
        .frame_jpeg(&rotated, 0, sample_seconds(&rotated, 0))
        .unwrap();
    assert_eq!((w, h), (48, 64));
    let rgba = SystemNative.decode_srgb_jpeg(&jpeg).unwrap().unwrap();
    let v = |x: usize, y: usize| rgba.pixels[y * rgba.stride + 4 * x + 1];
    // Counter-clockwise: the bright right half ends up on top.
    assert!(
        v(24, 10) > 150 && v(24, 54) < 100,
        "{} {}",
        v(24, 10),
        v(24, 54)
    );
}

#[test]
fn audio_without_audio_tracks() {
    let dir = work("silent");
    let path = dir.join("v.mp4");
    let Some(()) = tool(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=s=32x32:r=5:d=1",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx264",
            path.to_str().unwrap(),
        ],
    ) else {
        return;
    };
    assert_eq!(SystemNative.audio_decodes(&path, 0).unwrap(), Some(true));
    assert_eq!(SystemNative.audio_decodes(&path, 1).unwrap(), Some(false));
    let lpcm = dir.join("pcm.mov");
    tool(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=stereo:d=1",
            "-c:a",
            "pcm_s16le",
            lpcm.to_str().unwrap(),
        ],
    )
    .unwrap();
    // Digital silence is valid: decodability, not loudness.
    assert_eq!(SystemNative.audio_decodes(&lpcm, 1).unwrap(), Some(true));
}

/// The public iPhone 15 Pro Live Photo pair (osxphotos, MIT), when present.
fn public(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("MOMENTSTAYS_PUBLIC_SAMPLES")?;
    let p = PathBuf::from(dir).join(name);
    p.exists().then_some(p)
}

#[test]
fn public_iphone_heic_gain_map() {
    let Some(path) = public("C3090F66-942C-41D3-BEC7-4F2B4876A109.heic") else {
        eprintln!("skip: MOMENTSTAYS_PUBLIC_SAMPLES not set");
        return;
    };
    let data = std::fs::read(&path).unwrap();
    assert_eq!(
        momentstays_core::sha256_hex(&data),
        "abcc6aeb1031596b62297d4302b532edbd6d6ad4f9fdd76b783d7a83b954b5ac"
    );
    assert_eq!(SystemNative.image_decodes(&data), Some(true));
    let gain = SystemNative.heic_gain_map(&path).unwrap().unwrap();
    assert_eq!(gain.versions, ["131072"]);
    assert_eq!(gain.headrooms.len(), 1);
    assert!((gain.headrooms[0] - 4.946382).abs() < 1e-9);
    let image = gain.image.unwrap();
    // Coded (unrotated) axes, like ImageIO's auxiliary data.
    assert_eq!((image.width, image.height), (2016, 1512));
    assert!(image.stride >= image.width);
    let base = SystemNative.reencode_jpeg(&path, 0.9).unwrap();
    let rgba = SystemNative.decode_srgb_jpeg(&base).unwrap().unwrap();
    assert_eq!((rgba.width, rgba.height), (4032, 3024));
    // EXIF orientation 6 (irot 270) and the Display P3 ICC are carried.
    let exif = portable::carried_segments(&base).unwrap();
    assert!(exif.iter().any(|s| s[4..10] == *jpeg::EXIF));
    assert!(portable::jpeg_icc(&base).unwrap().is_some());
    let stops = momentstays_core::apple_hdr::stops(&data).unwrap().unwrap();
    assert!((gain.headrooms[0].log2() - stops).abs() < 0.002);
}

#[test]
fn public_iphone_live_movie_frames_and_audio() {
    let Some(path) = public("C3090F66-942C-41D3-BEC7-4F2B4876A109_3.mov") else {
        eprintln!("skip: MOMENTSTAYS_PUBLIC_SAMPLES not set");
        return;
    };
    let data = std::fs::read(&path).unwrap();
    let movie = momentstays_core::mp4::Movie::parse(&data).unwrap();
    let audio = movie
        .tracks
        .iter()
        .filter(|t| t.samples.handler == "soun")
        .count();
    let t = sample_seconds(&path, 10);
    match SystemNative.frame_jpeg(&path, 0, t) {
        Ok((jpeg, w, h)) => {
            let o = portable::video_orientation(&data, 0).unwrap();
            let (cw, ch) = o.size(w as usize, h as usize);
            assert!(cw >= ch || o.swaps_axes(), "{w}x{h}");
            assert_eq!(SystemNative.image_decodes(&jpeg), Some(true));
        }
        // HEVC decoder absent (no codecs-extra / gst-libav): Unsupported.
        Err(e) => assert_eq!(e.kind(), ErrorKind::Unsupported, "{e}"),
    }
    assert_eq!(
        SystemNative.audio_decodes(&path, audio).unwrap(),
        Some(true)
    );
}
