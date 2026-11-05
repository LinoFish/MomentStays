//! System backend gates: real JPEG pixels, ownership and invalid dimensions.
#[cfg(target_vendor = "apple")]
#[test]
fn apple_gray_roundtrip_and_rejection() {
    use momentstays_core::engine::{Gray, Native};
    let backend = momentstays_core::platform::SystemNative;
    let gray = Gray {
        width: 16,
        height: 16,
        stride: 16,
        pixels: vec![128; 256],
    };
    let encoded = backend.encode_gray_jpeg(&gray, 1.0).unwrap();
    assert_eq!(backend.image_decodes(&encoded), Some(true));
    let decoded = backend.decode_gray_jpeg(&encoded).unwrap().unwrap();
    assert_eq!((decoded.width, decoded.height), (16, 16));
    for row in decoded.pixels.chunks(decoded.stride).take(16) {
        assert!(row[..16].iter().all(|&v| v.abs_diff(128) <= 1));
    }
    let rgba = backend.decode_srgb_jpeg(&encoded).unwrap().unwrap();
    assert_eq!((rgba.width, rgba.height), (16, 16));
    assert!(rgba.stride >= rgba.width * 4);
    for row in rgba.pixels.chunks(rgba.stride).take(rgba.height) {
        for p in row[..rgba.width * 4].as_chunks::<4>().0 {
            assert_eq!(p[3], 255);
            assert!(p[0].abs_diff(p[1]) <= 1 && p[1].abs_diff(p[2]) <= 1);
        }
    }
    // Own the decoded buffer after ImageIO objects and input are gone.
    drop(encoded);
    assert!(!decoded.pixels.is_empty());
    assert!(!rgba.pixels.is_empty());
    let truncated = Gray {
        pixels: vec![0],
        ..gray.clone()
    };
    assert!(backend.encode_gray_jpeg(&truncated, 1.0).is_err());
    assert!(backend.encode_gray_jpeg(&gray, f64::NAN).is_err());
    assert_eq!(backend.image_decodes(b"not an image"), Some(false));
    let icc = backend.srgb_icc().unwrap();
    assert_eq!(&icc[36..40], b"acsp");
}

#[cfg(target_vendor = "apple")]
#[test]
fn apple_srgb_decode_retains_coded_axes() {
    use momentstays_core::engine::{Gray, Native};
    let backend = momentstays_core::platform::SystemNative;
    let (width, height) = (32, 48);
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            pixels.push(20 + 90 * (x / 16) as u8 + 60 * (y / 16) as u8);
        }
    }
    let encoded = backend
        .encode_gray_jpeg(
            &Gray {
                width,
                height,
                stride: width,
                pixels,
            },
            1.0,
        )
        .unwrap();
    let rgba = backend.decode_srgb_jpeg(&encoded).unwrap().unwrap();
    assert_eq!((rgba.width, rgba.height), (width, height));
    // Probe centers of the six blocks, away from JPEG boundary ringing.
    // Relative order detects flips/transposes without assuming gray's ICC curve.
    let value = |x: usize, y: usize| rgba.pixels[y * rgba.stride + 4 * x];
    for y in [8, 24, 40] {
        assert!(value(8, y) < value(24, y));
    }
    for x in [8, 24] {
        assert!(value(x, 8) < value(x, 24));
        assert!(value(x, 24) < value(x, 40));
    }
}
