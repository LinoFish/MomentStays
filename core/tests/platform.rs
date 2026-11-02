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
    // Own the decoded buffer after ImageIO objects and input are gone.
    drop(encoded);
    assert!(!decoded.pixels.is_empty());
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
