//! Existing HDR byte fixtures and real workflow/codec boundaries, moved by responsibility.
mod engine_support;
use engine_support::{b64, files, run, video, Dir};
use momentstays_core::engine::batch::{self, RunRequest};
use momentstays_core::engine::options::ConversionOptions;
use momentstays_core::engine::{hdr, still, StopToken};
use momentstays_core::model::{self, Cover, MotionModel, Vendor};
use momentstays_core::{format, jpeg, live};

#[test]
fn still_writers_keep_coding_bytes() {
    let ultra = b64("tests/fixtures-pycli/ultra.jpg.b64");
    let p = format::parse(&ultra).unwrap();
    let still_bytes = p.still.to_vec();
    let aux = jpeg::auxiliary(&still_bytes).unwrap();
    assert_eq!(aux.len(), 2);
    let packed = hdr::pack_gain_map(&still_bytes, &still_bytes[aux[1].clone()]).unwrap();
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
        assert_eq!(hdr::swift_double(v), s);
    }
}

#[test]
fn gain_lookup_matches_swift_math() {
    let t = hdr::gain_lookup(2.0);
    assert_eq!((t[0], t[255]), (0, 255));
    assert!(t.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn hdr_writers_preserve_reference_images_and_metadata_with_scoped_xmp() {
    fn normalize_gain_length(e: &mut momentstays_core::pyxml::Element, length: usize) {
        if e.get_attr(momentstays_core::xmp::ITEM, "Semantic") == Some("GainMap") {
            e.set_attr(momentstays_core::xmp::ITEM, "Length", &length.to_string());
        }
        for child in &mut e.children {
            normalize_gain_length(child, length);
        }
    }
    let base = b64("tests/fixtures-pycli/base.jpg.b64");
    for (stops, fixture) in [(2.0, "2.0"), (1.8, "1.8")] {
        let actual = hdr::ultra(&base, &base, stops).unwrap();
        let expected = b64(&format!(
            "tests/fixtures-pycli/expected/ultra-{fixture}.bin.b64"
        ));
        let images = jpeg::auxiliary(&actual).unwrap();
        let reference_images = jpeg::auxiliary(&expected).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(reference_images.len(), 2);
        for (image, reference) in images.iter().zip(&reference_images) {
            let image = &actual[image.clone()];
            let reference = &expected[reference.clone()];
            assert_eq!(
                jpeg::coding_hash(image).unwrap(),
                jpeg::coding_hash(reference).unwrap()
            );
            let packets = momentstays_core::pymotion::xmp_packets(image).unwrap();
            let old = momentstays_core::pymotion::xmp_packets(reference).unwrap();
            assert_eq!(packets.len(), 1);
            assert_eq!(old.len(), 1);
            // The primary's GainMap Length changes with the corrected gain XMP.
            let mut normalized = packets[0].root.clone();
            normalize_gain_length(&mut normalized, reference_images[1].len());
            assert_eq!(
                momentstays_core::pyxml::tostring(&normalized),
                momentstays_core::pyxml::tostring(&old[0].root)
            );
            assert!(image
                .windows(b"<rdf:RDF xmlns:rdf=".len())
                .any(|w| w == b"<rdf:RDF xmlns:rdf="));
        }
    }
    assert_eq!(
        hdr::color_profile(&b64("tests/fixtures-pycli/srgb.jpg.b64"), b"FAKEICC").unwrap(),
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
