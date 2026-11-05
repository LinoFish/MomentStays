//! Synthetic framing and codec-boundary checks; no private sample data.
use momentstays_core::engine::{
    hdr as processing, hdr::honor, still, Gray, Native, NoNative, Rgba,
};
use momentstays_core::Result;
use momentstays_core::{hdr, jpeg, pyjpeg, tiff};

fn jpeg_frame(w: u16, h: u16, channels: u8, metadata: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![255, 216];
    for m in metadata {
        out.extend(m);
    }
    let mut sof = vec![8];
    sof.extend(h.to_be_bytes());
    sof.extend(w.to_be_bytes());
    sof.push(channels);
    for i in 0..channels {
        sof.extend([i + 1, 0x11, 0]);
    }
    out.extend(pyjpeg::app(0xc0, &sof).unwrap());
    out.extend([255, 217]);
    out
}

fn camera_exif() -> Vec<u8> {
    let mut t = b"II\x2a\0\x08\0\0\0".to_vec();
    t.extend(tiff::serialize(
        true,
        &[
            tiff::entry(true, 0x10f, 2, 6, 68),
            tiff::entry(true, 0x110, 2, 9, 74),
            tiff::entry(true, 0x8769, 4, 1, 50),
        ],
        0,
    ));
    assert_eq!(t.len(), 50);
    t.extend(tiff::serialize(
        true,
        &[tiff::entry(true, 0xa001, 3, 1, 1)],
        0,
    ));
    assert_eq!(t.len(), 68);
    t.extend(b"HONOR\0ELP-AN00\0");
    let mut e = jpeg::EXIF.to_vec();
    e.extend(t);
    pyjpeg::app(0xe1, &e).unwrap()
}

fn extension(luma: &[u8]) -> Vec<u8> {
    let mut t = vec![0; 16];
    t.extend(16u32.to_le_bytes());
    t.extend(b"CDRDATA\0");
    t.extend(luma);
    let mut header = [0; 128];
    header[..24].copy_from_slice(&[
        2, 1, 0, 0, 1, 1, 0, 0, 1, 2, 57, 1, 214, 0, 0, 0, 208, 7, 0, 0, 214, 0, 0, 0,
    ]);
    t.extend(header);
    t.extend(128u32.to_le_bytes());
    t.extend(((luma.len() + 132) as u32).to_le_bytes());
    t.extend(b"HiHonor_EhdrEn\0\0\0\0\0\0");
    t.push(0);
    t.extend(1u32.to_le_bytes());
    t.extend(b"HiHonor_ExtInfo\0\0\0\0\0");
    t
}

fn source() -> Vec<u8> {
    let base = jpeg_frame(
        3072,
        4096,
        3,
        &[
            camera_exif(),
            pyjpeg::app(0xe9, b"ELP-AN00 11.0.0.110(SP5C00E110R103P1)\0").unwrap(),
        ],
    );
    let mut d = base;
    d.extend(extension(&jpeg_frame(3072, 4096, 1, &[])));
    d
}

fn frame_size(data: &[u8]) -> (usize, usize) {
    let segments = jpeg::segments(data).unwrap();
    let p = segments
        .iter()
        .find(|s| s.marker == 0xc0)
        .unwrap()
        .payload(data);
    (
        u16::from_be_bytes([p[3], p[4]]) as usize,
        u16::from_be_bytes([p[1], p[2]]) as usize,
    )
}

fn rgba(width: usize, height: usize, stride: usize, color: [u8; 4]) -> Rgba {
    let mut pixels = vec![0; stride * height];
    for y in 0..height {
        for x in 0..width {
            pixels[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&color);
        }
    }
    Rgba {
        width,
        height,
        stride,
        pixels,
    }
}

struct Codec;
impl Native for Codec {
    fn decode_gray_jpeg(&self, jpeg: &[u8]) -> Result<Option<Gray>> {
        let (width, height) = frame_size(jpeg);
        Ok(Some(Gray {
            width,
            height,
            stride: width,
            pixels: vec![42; width * height],
        }))
    }
    fn decode_srgb_jpeg(&self, jpeg: &[u8]) -> Result<Option<Rgba>> {
        let (w, h) = frame_size(jpeg);
        Ok(Some(rgba(w, h, 4 * w, [0, 255, 0, 255])))
    }
    fn encode_gray_jpeg(&self, im: &Gray, quality: f64) -> Result<Vec<u8>> {
        assert_eq!(quality, 1.0);
        assert!(matches!((im.width, im.height), (768, 1024) | (750, 1000)));
        Ok(jpeg_frame(im.width as u16, im.height as u16, 1, &[]))
    }
    fn srgb_icc(&self) -> Result<Vec<u8>> {
        Ok(b"synthetic ICC for byte tests".to_vec())
    }
}

#[test]
fn variable_luma_resource_is_preserved_and_length_checked() {
    let image = source();
    let end = jpeg::image_end(&image).unwrap();
    let parsed = hdr::honor_ehdr(&image, end).unwrap().unwrap();
    assert_eq!(parsed.luma, jpeg_frame(3072, 4096, 1, &[]));
    assert_eq!(jpeg::static_only(&image).unwrap(), image);
    let empty = extension(&[]);
    assert!(hdr::honor_ehdr(&empty, 0).unwrap().unwrap().luma.is_empty());
    let mut truncated = image.clone();
    let at = truncated.len() - 49;
    truncated[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(hdr::honor_ehdr(&truncated, end).is_err());
    let mut corrupt = image.clone();
    corrupt[end + 28] = 0;
    assert!(hdr::honor_ehdr(&corrupt, end).is_err());
}

#[test]
fn range_and_stride_guards_exclude_padding_and_require_opaque_srgb() {
    let mut gray = Gray {
        width: 8,
        height: 4,
        stride: 12,
        pixels: vec![255; 48],
    };
    for y in 0..4 {
        gray.pixels[y * 12..y * 12 + 4].fill(0);
        gray.pixels[y * 12 + 4..y * 12 + 8].fill(141);
    }
    let mut base = rgba(8, 4, 40, [128, 128, 128, 255]);
    assert_eq!(honor::map_luma(&gray, &base).unwrap().pixels, vec![41, 174]);
    gray.pixels[1] = 142;
    assert!(honor::map_luma(&gray, &base).is_err());
    gray.pixels[1] = 0;
    base.pixels[3] = 0;
    assert!(honor::map_luma(&gray, &base).is_err());
    base.pixels[3] = 255;
    base.stride = 31;
    assert!(honor::map_luma(&gray, &base).is_err());
    base = rgba(8, 4, 32, [0, 0, 0, 255]);
    base.pixels.pop();
    assert!(honor::map_luma(&gray, &base).is_err());
    gray.pixels.clear();
    assert!(honor::map_luma(&gray, &rgba(8, 4, 32, [0, 0, 0, 255])).is_err());
}

#[test]
fn frozen_reference_vectors_use_srgb_channel_weights_and_center_sampling() {
    let gray = Gray {
        width: 4,
        height: 4,
        stride: 4,
        pixels: vec![42; 16],
    };
    let mut base = rgba(4, 4, 16, [0, 255, 0, 255]);
    assert_eq!(honor::map_luma(&gray, &base).unwrap().pixels, vec![87]);
    // The four center pixels are blue; the outer green border contributes
    // to neither the bilinear center nor the original source coding.
    for y in 1..=2 {
        for x in 1..=2 {
            base.pixels[y * 16 + x * 4..y * 16 + x * 4 + 4].copy_from_slice(&[0, 0, 255, 255]);
        }
    }
    assert_eq!(honor::map_luma(&gray, &base).unwrap().pixels, vec![86]);
}

#[test]
fn conversion_requires_no_reference_and_keeps_coding_and_private_resource() {
    let input = source();
    let tail = still::honor_private(&input).unwrap();
    let converted = honor::convert(&Codec, &input).unwrap();
    assert_eq!(
        jpeg::coding_hash(&converted).unwrap(),
        jpeg::coding_hash(&input).unwrap()
    );
    assert_eq!(still::honor_private(&converted).unwrap(), tail);
    assert_eq!(jpeg::image_ranges(&converted).unwrap().len(), 2);
    assert_eq!(jpeg::static_only(&converted).unwrap(), converted);
    assert_eq!(
        still::honor_private(&still::live_still(&converted, "test-id", true).unwrap()).unwrap(),
        tail
    );
    assert!(converted.windows(b"0.0001".len()).any(|w| w == b"0.0001"));
    assert!(honor::convert(&NoNative, &input).is_err());
}

#[test]
fn unknown_firmware_and_missing_luma_are_not_synthesized() {
    let mut unknown = source();
    let i = unknown
        .windows(b"11.0.0.110".len())
        .position(|w| w == b"11.0.0.110")
        .unwrap();
    unknown[i] = b'9';
    assert!(honor::convert(&Codec, &unknown).is_err());
    let mut no_luma = jpeg_frame(3072, 4096, 3, &[]);
    no_luma.extend(extension(&[]));
    assert!(honor::convert(&Codec, &no_luma).is_err());
    assert!(
        still::ultra_with_offset(&source(), &jpeg_frame(768, 1024, 1, &[]), f64::NAN, 0.0).is_err()
    );
}

#[test]
fn existing_hdr_toggle_controls_batch_conversion_and_preservation() {
    use base64::Engine;
    use momentstays_core::engine::{batch, options::ConversionOptions, StopToken, Timings};
    use momentstays_core::model::{self, Cover, MotionModel, Vendor};
    use std::path::PathBuf;
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let temp = Temp(std::env::temp_dir().join(format!(
        "honor-hdr-toggle-{}",
        momentstays_core::engine::uuid()
    )));
    std::fs::create_dir_all(&temp.0).unwrap();
    let video_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64");
    let mut video = base64::engine::general_purpose::STANDARD
        .decode(std::fs::read_to_string(video_path).unwrap().trim_ascii())
        .unwrap();
    let at = video.windows(4).position(|w| w == b"edts").unwrap();
    video[at..at + 4].copy_from_slice(b"free");
    let input = source();
    let expected_tail = still::honor_private(&input).unwrap();
    let motion = model::write(
        &MotionModel {
            still: input,
            video,
            cover: Cover::Time(800_000),
            source_format: "test".into(),
        },
        Vendor::Google,
    )
    .unwrap();
    let path = temp.0.join("input.jpg");
    std::fs::write(&path, &motion).unwrap();
    for enabled in [false, true] {
        let options = ConversionOptions {
            legacy_hdr: enabled,
            ..Default::default()
        };
        let target = temp.0.join(if enabled {
            "on.livephoto"
        } else {
            "off.livephoto"
        });
        let native: &dyn Native = if enabled { &Codec } else { &NoNative };
        batch::convert(
            native,
            &path,
            &target,
            &options,
            &StopToken::new(),
            false,
            &Timings::default(),
        )
        .unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(target.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(
            manifest["hdr"]["status"],
            if enabled {
                "converted-experimental"
            } else {
                "unsupported-preserved"
            }
        );
        let output =
            std::fs::read(target.join(manifest["output_files"]["still"].as_str().unwrap()))
                .unwrap();
        assert_eq!(still::honor_private(&output).unwrap(), expected_tail);
        assert_eq!(
            jpeg::image_ranges(&output).unwrap().len(),
            if enabled { 2 } else { 1 }
        );
        assert_eq!(std::fs::read(&path).unwrap(), motion);
    }
}

#[test]
fn large_luma_extension_is_preserved_instead_of_rejected_at_one_megabyte() {
    let metadata = vec![pyjpeg::app(0xe8, &vec![0; 60_000]).unwrap(); 18];
    let luma = jpeg_frame(3072, 4096, 1, &metadata);
    let tail = extension(&luma);
    assert!(tail.len() > 1_048_576);
    let mut image = jpeg_frame(3072, 4096, 3, &[]);
    image.extend(&tail);
    let parsed = hdr::honor_ehdr(&image, jpeg::image_end(&image).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(parsed.luma, luma);
    assert_eq!(parsed.extension, tail);
    assert_eq!(jpeg::static_only(&image).unwrap(), image);
}

#[test]
fn four_evaluated_headers_and_both_dimensions_are_supported_conservatively() {
    let mode2 = [1, 2, 57, 1];
    let mode4 = [1, 4, 100, 0];
    for (mode, iso, exposure, other) in [
        (mode2, 214u32, 2000u32, 214u32),
        (mode4, 502, 6000, 502),
        (mode4, 2765, 4000, 2764),
        (mode4, 1157, 8000, 1157),
    ] {
        let mut header = [0u8; 128];
        header[..8].copy_from_slice(&[2, 1, 0, 0, 1, 1, 0, 0]);
        header[8..12].copy_from_slice(&mode);
        header[12..16].copy_from_slice(&iso.to_le_bytes());
        header[16..20].copy_from_slice(&exposure.to_le_bytes());
        header[20..24].copy_from_slice(&other.to_le_bytes());
        assert!(honor::profile::supports_header(&header));
        header[12] ^= 128;
        assert!(!honor::profile::supports_header(&header));
    }
    assert!(honor::profile::supports_dimensions(3072, 4096));
    assert!(honor::profile::supports_dimensions(3000, 4000));
    assert!(!honor::profile::supports_dimensions(4096, 3072));
    let mut input = jpeg_frame(
        3000,
        4000,
        3,
        &[
            camera_exif(),
            pyjpeg::app(0xe9, b"ELP-AN00 11.0.0.110(SP5C00E110R103P1)\0").unwrap(),
        ],
    );
    let mut tail = extension(&jpeg_frame(3000, 4000, 1, &[]));
    let at = tail.len() - 49 - 132;
    tail[at + 8..at + 12].copy_from_slice(&mode4);
    tail[at + 12..at + 16].copy_from_slice(&502u32.to_le_bytes());
    tail[at + 16..at + 20].copy_from_slice(&6000u32.to_le_bytes());
    tail[at + 20..at + 24].copy_from_slice(&502u32.to_le_bytes());
    input.extend(tail);
    let result = honor::convert(&Codec, &input).unwrap();
    let ranges = jpeg::image_ranges(&result).unwrap();
    assert_eq!(frame_size(&result[ranges[1].clone()]), (750, 1000));
}

#[test]
fn hdr_processing_is_independent_of_live_photo_assembly() {
    let input = source();
    let on = processing::Options {
        try_private_hdr: true,
        already_ultra_hdr: false,
        allow_legacy_conversion: true,
    };
    let off = processing::Options {
        try_private_hdr: false,
        ..on
    };
    let preserved = processing::process(&NoNative, input.clone(), off).unwrap();
    assert_eq!(preserved.image, input);
    assert_eq!(preserved.report["status"], "unsupported-preserved");
    let converted = processing::process(&Codec, input.clone(), on).unwrap();
    assert_eq!(converted.report["profile"], honor::PROFILE);
    assert_eq!(converted.report["status"], "converted-experimental");
    assert_eq!(
        jpeg::coding_hash(&converted.image).unwrap(),
        jpeg::coding_hash(&input).unwrap()
    );
    let bytes = converted.image.clone();
    let standard = processing::process(
        &NoNative,
        converted.image,
        processing::Options {
            already_ultra_hdr: true,
            ..on
        },
    )
    .unwrap();
    assert_eq!(standard.image, bytes);
    assert_eq!(standard.report["status"], "preserved");
    let refused = processing::process(&NoNative, input.clone(), on).unwrap();
    assert_eq!(refused.image, input);
    assert_eq!(refused.report["status"], "unsupported-preserved");
}

#[test]
fn standalone_private_hdr_obeys_switch_and_keeps_extension() {
    use momentstays_core::engine::{batch, options::ConversionOptions, output, scanrow, StopToken};
    let root = std::env::temp_dir().join(format!(
        "ms-private-static-{}",
        momentstays_core::engine::uuid()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(root.join("input")).unwrap();
    let path = root.join("input/private.jpg");
    let original = source();
    std::fs::write(&path, &original).unwrap();
    assert!(scanrow::scan_file(&Codec, &path, "private.jpg", None).is_static_hdr());
    for enabled in [false, true] {
        let out = root.join(enabled.to_string());
        let opts = ConversionOptions {
            legacy_hdr: enabled,
            ..Default::default()
        };
        let sources = [path.clone()];
        let result = batch::run(
            &Codec,
            &batch::RunRequest {
                sources: &sources,
                output: &out,
                options: &opts,
                scan_rows: None,
                concurrency: Some(1),
            },
            &StopToken::new(),
            None,
            |_| {},
        )
        .unwrap();
        assert_eq!(result.counts["converted"], 1, "{result:?}");
        let pair = out.join("private.jpg.static-hdr");
        let m = output::verify(&pair).unwrap();
        assert_eq!(
            m["hdr"]["status"],
            if enabled {
                "converted-experimental"
            } else {
                "unsupported-preserved"
            }
        );
        let image = std::fs::read(pair.join("private.jpg")).unwrap();
        assert_eq!(
            processing::honor_private(&image).unwrap(),
            processing::honor_private(&original).unwrap()
        );
        assert_eq!(m["output_files"].as_object().unwrap().len(), 1);
        assert!(m.get("identifier").is_none());
    }
    std::fs::remove_dir_all(root).unwrap();
}
