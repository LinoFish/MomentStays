//! Structural fixtures for vendor layouts the Python reference does not parse.
use momentstays_core::{format, jpeg, parse};

fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xff, marker];
    v.extend(((payload.len() + 2) as u16).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn jpeg_still(extra_app: &[u8]) -> Vec<u8> {
    let mut v = vec![0xff, 0xd8];
    v.extend(seg(0xe1, extra_app));
    v.extend(seg(0xdb, &[0u8; 65]));
    v.extend(seg(0xc0, b"\x08\0\x10\0\x10\x01\x01\x11\0"));
    v.extend(seg(0xda, b"\x01\x01\0\0\0\0"));
    v.extend(b"\x12\xff\0\x34\xff\xd0\x56");
    v.extend([0xff, 0xd9]);
    v
}

fn mp4() -> Vec<u8> {
    let b = |t: &[u8], p: &[u8]| { let mut v = ((p.len() + 8) as u32).to_be_bytes().to_vec(); v.extend(t); v.extend(p); v };
    let mut v = b(b"ftyp", b"isom\0\0\0\0isom");
    v.extend(b(b"moov", &[0; 16]));
    v.extend(b(b"mdat", b"\xff\xd9ftyp-decoy"));
    v
}

#[test]
fn huawei_live_tail() {
    let video = mp4();
    let mut d = jpeg_still(b"Exif\0\0");
    d.extend(&video);
    let pad = |s: &str| format!("{s:<20}").into_bytes();
    d.extend(pad("v2_f12"));
    d.extend(pad("anything"));
    d.extend(pad(&format!("LIVE_{}", video.len() + 20)));
    let p = parse(&d).expect("huawei parses");
    assert_eq!(p.format, "huawei-honor");
    assert_eq!(p.cover_frame, Some(12));
    assert_eq!(p.timestamp_tag.as_deref(), Some("Huawei:frame-index"));
    assert_eq!(p.video(&d), &video[..]);
    assert_eq!(p.still, jpeg_still(b"Exif\0\0"));
}

#[test]
fn huawei_bad_tail_is_rejected_not_guessed() {
    let mut d = jpeg_still(b"x");
    d.extend(mp4());
    d.extend(format!("{:<20}{:<20}{:<20}", "v9_f1", "", "LIVE_40").as_bytes());
    assert_eq!(parse(&d).unwrap_err().0, "华为/荣耀 LIVE_ 尾标无效");
}

fn samsung(video: &[u8]) -> Vec<u8> {
    let mut d = jpeg_still(b"Exif\0\0");
    let at = d.len();
    let mut record = vec![0, 0, 0x30, 0x0a];
    record.extend(16u32.to_le_bytes());
    record.extend(b"MotionPhoto_Data");
    record.extend(video);
    d.extend(&record);
    let head = d.len();
    let mut dir = b"SEFH".to_vec();
    dir.extend(106u32.to_le_bytes());
    dir.extend(1u32.to_le_bytes());
    dir.extend([0, 0, 0x30, 0x0a]);
    dir.extend(((head - at) as u32).to_le_bytes());
    dir.extend((record.len() as u32).to_le_bytes());
    let size = dir.len() as u32;
    d.extend(dir);
    d.extend(size.to_le_bytes());
    d.extend(b"SEFT");
    d
}

#[test]
fn samsung_sef_jpeg() {
    let video = mp4();
    let d = samsung(&video);
    let p = parse(&d).expect("samsung parses");
    assert_eq!(p.format, "samsung-sef");
    assert_eq!(p.video(&d), &video[..]);
    assert_eq!(p.timestamp, None);
}

#[test]
fn samsung_directory_out_of_range() {
    let mut d = samsung(&mp4());
    let n = d.len();
    d[n - 8..n - 4].copy_from_slice(&9999u32.to_le_bytes());
    assert_eq!(parse(&d).unwrap_err().0, "三星 SEF 目录长度无效");
}

#[test]
fn coding_hash_ignores_metadata_segments() {
    let a = jpeg_still(b"Exif\0\0one");
    let b = jpeg_still(b"Exif\0\0two-different-metadata");
    assert_eq!(jpeg::coding_hash(&a).unwrap(), jpeg::coding_hash(&b).unwrap());
    assert_eq!(format::coding_hash(&a).unwrap(), format::coding_hash(&b).unwrap());
}

#[test]
fn plain_jpeg_is_not_motion() {
    assert_eq!(parse(&jpeg_still(b"x")).unwrap_err().0, "不是受支持的动态照片");
}

#[test]
fn entropy_ff00_is_not_a_marker() {
    let s = jpeg::segments(&jpeg_still(b"x")).unwrap();
    assert_eq!(s.last().unwrap().marker, 0xd9);
}
