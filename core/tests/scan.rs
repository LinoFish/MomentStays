//! Scan row tests (ROADMAP item 4): Swift `BatchEngine.scanFile` /
//! `NativeDriver` scan alignment — identify labels, HDR order, cover
//! text, detail text, confirmed-static.

mod common;

use base64::Engine;
use common::{build_still, bx, full, u16b, u32b};
use momentstays_core::format;
use momentstays_core::heifwrite::assemble_heic;
use momentstays_core::model::{Cover, MotionModel, Vendor};
use momentstays_core::scan::{classify_hdr, cover_text, identify, scan_row};
use std::fs;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-pycli")
}

fn read(name: &str) -> Vec<u8> {
    let text = fs::read_to_string(dir().join(format!("{name}.b64"))).unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
}

fn real_mp4() -> Vec<u8> {
    let text = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
}

fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, marker];
    v.extend(((payload.len() + 2) as u16).to_be_bytes());
    v.extend(payload);
    v
}

fn app1(payload: &[u8]) -> Vec<u8> {
    seg(0xE1, payload)
}

/// Minimal still: SOI + DQT + SOF0 (16x16) + DHT + SOS + 1 entropy
/// byte + EOI.
fn skeleton() -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    out.extend(seg(0xDB, &[0x00, 0x11]));
    out.extend(seg(0xC0, &[0x08, 0x00, 0x10, 0x00, 0x10, 0x01]));
    out.extend(seg(0xC4, &[0x00, 0x11]));
    out.extend(seg(0xDA, &[0x00, 0x11]));
    out.push(0x42);
    out.extend([0xFF, 0xD9]);
    out
}

fn tiff_with_make_model(make: &str, model: &str) -> Vec<u8> {
    tiff_with_make_model_endian(make, model, true)
}

/// TIFF with Make/Model root entries. Header (8) + root IFD
/// (2 + 24 + 4 = 30) = 38; values follow.
fn tiff_with_make_model_endian(make: &str, model: &str, little: bool) -> Vec<u8> {
    let u16 = |v: u16| -> [u8; 2] {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let u32 = |v: u32| -> [u8; 4] {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let mut m = make.as_bytes().to_vec();
    m.push(0);
    let mut o = model.as_bytes().to_vec();
    o.push(0);
    let ent = |tag: u16, count: u32, off: u32| {
        let mut e = Vec::new();
        e.extend(u16(tag));
        e.extend(u16(2));
        e.extend(u32(count));
        e.extend(u32(off));
        e
    };
    let mut t = if little { b"II".to_vec() } else { b"MM".to_vec() };
    t.extend(u16(42));
    t.extend(u32(8));
    t.extend(u16(2));
    t.extend(ent(0x010F, m.len() as u32, 38));
    t.extend(ent(0x0110, o.len() as u32, 38 + m.len() as u32));
    t.extend([0u8; 4]);
    assert_eq!(t.len(), 38);
    t.extend(&m);
    t.extend(&o);
    t
}

fn jpeg_with_exif(tiff: &[u8]) -> Vec<u8> {
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend(tiff);
    let mut out = vec![0xFF, 0xD8];
    out.extend(app1(&exif));
    out.extend(skeleton()[2..].to_vec());
    out
}

/// HEIC still with a custom Exif item (same two-pass iloc layout as
/// `common::build_still`, minus mime/iref).
fn heic_with_exif(exif: &[u8]) -> Vec<u8> {
    let hvc1 = b"fake-hvc1-coding-data-0123456789".to_vec();
    let items: Vec<(u16, &str, Vec<u8>)> =
        vec![(1, "hvc1", hvc1), (2, "Exif", exif.to_vec())];
    let mut hdlr_body = vec![0u8; 4];
    hdlr_body.extend(b"pict");
    hdlr_body.extend([0u8; 4]);
    let hdlr = full("hdlr", 0, &hdlr_body);
    let pitm = full("pitm", 0, &u16b(1).to_vec());
    let mut infe_boxes = Vec::new();
    for (id, ty, _) in &items {
        let mut body = u16b(*id).to_vec();
        body.extend([0u8, 0]);
        body.extend(ty.as_bytes());
        body.push(0);
        infe_boxes.push(full("infe", 2, &body));
    }
    let mut iinf_body = u16b(items.len() as u16).to_vec();
    for b in &infe_boxes {
        iinf_body.extend(b);
    }
    let iinf = full("iinf", 0, &iinf_body);
    let build_meta = |offsets: &[(usize, usize)]| -> Vec<u8> {
        let mut iloc_body = vec![0x44u8, 0x00];
        iloc_body.extend(u16b(items.len() as u16));
        for (i, (id, _, _)) in items.iter().enumerate() {
            let (off, len) = offsets[i];
            iloc_body.extend(u16b(*id));
            iloc_body.extend(u16b(0));
            iloc_body.extend(u16b(1));
            iloc_body.extend(u32b(off as u32));
            iloc_body.extend(u32b(len as u32));
        }
        let iloc = full("iloc", 0, &iloc_body);
        let mut children = hdlr.clone();
        children.extend(&pitm);
        children.extend(&iinf);
        children.extend(&iloc);
        full("meta", 0, &children)
    };
    let ftyp = bx("ftyp", b"heic\0\0\0\0heic");
    let zero: Vec<(usize, usize)> = items.iter().map(|_| (0, 0)).collect();
    let meta_len = build_meta(&zero).len();
    let mdat_start = ftyp.len() + meta_len + 8;
    let mut offsets = Vec::new();
    let mut at = mdat_start;
    for (_, _, bytes) in &items {
        offsets.push((at, bytes.len()));
        at += bytes.len();
    }
    let meta = build_meta(&offsets);
    let mut mdat_body = Vec::new();
    for (_, _, bytes) in &items {
        mdat_body.extend(bytes);
    }
    let mut out = ftyp;
    out.extend(meta);
    out.extend(bx("mdat", &mdat_body));
    out
}

/// Valid Honor EHDR tail (300 bytes), same layout as the fixtures
/// suite's `honor_tail`.
fn honor_tail() -> Vec<u8> {
    let mut t = vec![0u8; 300];
    t[107..111].copy_from_slice(&107u32.to_le_bytes());
    t[111..119].copy_from_slice(b"CDRDATA\0");
    t[247..251].copy_from_slice(&128u32.to_le_bytes());
    t[251..255].copy_from_slice(&132u32.to_le_bytes());
    t[255..269].copy_from_slice(b"HiHonor_EhdrEn");
    t[276..280].copy_from_slice(&1u32.to_le_bytes());
    t[280..295].copy_from_slice(b"HiHonor_ExtInfo");
    t
}

#[test]
fn identify_brand_prefix() {
    let jpeg = jpeg_with_exif(&tiff_with_make_model("HONOR", "Magic"));
    let id = identify(&jpeg, "google-compatible");
    assert_eq!(id.name, "荣耀");
    assert_eq!(
        id.detail,
        "厂商依据：原片 EXIF Make；HONOR / Magic\n动态格式：Google 兼容"
    );
}

#[test]
fn identify_case_insensitive_make() {
    let jpeg = jpeg_with_exif(&tiff_with_make_model("samsung", "SM-S9080"));
    let id = identify(&jpeg, "google-compatible");
    assert_eq!(id.name, "三星");
}

#[test]
fn identify_format_fallback() {
    let id = identify(&skeleton(), "samsung-sef");
    assert_eq!(id.name, "三星");
    assert_eq!(
        id.detail,
        "厂商依据：动态照片格式（非机身识别）\nEXIF：未记录\n动态格式：三星 SEF"
    );
}

#[test]
fn identify_unknown() {
    let id = identify(&skeleton(), "");
    assert_eq!(id.name, "未知");
    assert_eq!(
        id.detail,
        "未找到可确认的厂商字段\nEXIF：未记录\n动态格式：未知。Google 兼容格式不代表 Google 厂商。"
    );
    let jpeg = jpeg_with_exif(&tiff_with_make_model("Canon", "EOS"));
    let id = identify(&jpeg, "google-compatible");
    assert_eq!(id.name, "未知");
    assert!(id.detail.contains("动态格式：Google 兼容。"));
}

#[test]
fn identify_heic_exif_recursion() {
    // The Samsung S22 path: HEIC Exif carries the brand, read through
    // the fake-JPEG recursion; HEIC format names have no table entry,
    // so the raw string is shown. The Exif item is a 4-byte big-endian
    // TIFF offset + a big-endian TIFF, like `common::exif_item`.
    let mut exif_item = vec![0u8, 0, 0, 0];
    exif_item.extend(tiff_with_make_model_endian("samsung", "SM-S9080", false));
    let heic = heic_with_exif(&exif_item);
    let id = identify(&heic, "samsung-heic-sef");
    assert_eq!(id.name, "三星");
    assert_eq!(
        id.detail,
        "厂商依据：原片 EXIF Make；samsung / SM-S9080\n动态格式：samsung-heic-sef"
    );
}

#[test]
fn classify_hdr_order() {
    assert_eq!(
        classify_hdr(&build_still(true, true), false).unwrap(),
        "HEIC（原样保留）"
    );
    assert_eq!(classify_hdr(&skeleton(), true).unwrap(), "Ultra HDR");
    let mut honor = skeleton();
    honor.extend(honor_tail());
    assert_eq!(classify_hdr(&honor, false).unwrap(), "荣耀私有 HDR");
    let mut iso = vec![0xFF, 0xD8];
    iso.extend(seg(
        0xE2,
        b"urn:iso:std:iso:ts:21496:-1\0\x00\x00\x00\x00\x00",
    ));
    iso.extend(skeleton()[2..].to_vec());
    assert_eq!(classify_hdr(&iso, false).unwrap(), "ISO HDR");
    assert_eq!(
        classify_hdr(&read("legacy-mpf.jpg"), false).unwrap(),
        "旧 Apple HDR"
    );
    assert_eq!(classify_hdr(&skeleton(), false).unwrap(), "SDR / 其他");
}

#[test]
fn cover_text_forms() {
    assert_eq!(cover_text(Some(5), Some(1_420_000)), "原始第 5 帧（从0计）");
    assert_eq!(cover_text(None, Some(1_420_000)), "1.420000 秒");
    assert_eq!(cover_text(None, None), "未记录");
}

fn google_model() -> MotionModel {
    let data = read("motion.jpg");
    let p = format::parse(&data).expect("motion.jpg parses");
    assert_eq!(p.format, "google-compatible");
    assert_eq!(p.timestamp, Some(400000));
    MotionModel::from_parsed(&p, &data).unwrap()
}

#[test]
fn scan_row_google_motion() {
    let m = google_model();
    let out = momentstays_core::model::write(&m, Vendor::Google).unwrap();
    let row = scan_row(&out);
    assert!(row.is_motion);
    assert_eq!(row.hdr, "SDR / 其他");
    assert_eq!(row.cover, "0.400000 秒");
    assert_eq!(row.detail, "google-compatible：等待转换");
    assert_eq!(row.manufacturer, "未知");
    assert!(row.manufacturer_detail.contains("动态格式：Google 兼容。"));
    assert!(!row.confirmed_static);
}

#[test]
fn scan_row_samsung_motion_fallback() {
    let m = google_model();
    let out = momentstays_core::model::write(&m, Vendor::Samsung).unwrap();
    let row = scan_row(&out);
    assert!(row.is_motion);
    assert_eq!(row.hdr, "SDR / 其他");
    assert_eq!(row.cover, "0.400000 秒");
    assert_eq!(row.detail, "samsung-sef：等待转换");
    assert_eq!(row.manufacturer, "三星");
    assert_eq!(
        row.manufacturer_detail,
        "厂商依据：动态照片格式（非机身识别）\nEXIF：未记录\n动态格式：三星 SEF"
    );
}

#[test]
fn scan_row_huawei_frame_cover() {
    let mut m = google_model();
    m.cover = Cover::Frame(3);
    let out = momentstays_core::model::write(&m, Vendor::Huawei).unwrap();
    let back = format::parse(&out).expect("huawei parses");
    assert_eq!(back.cover_frame, Some(3));
    let row = scan_row(&out);
    assert!(row.is_motion);
    assert_eq!(row.cover, "原始第 3 帧（从0计）");
    assert_eq!(row.detail, "huawei-honor：等待转换");
}

#[test]
fn scan_row_heic_motion() {
    let still = build_still(true, true);
    let out = assemble_heic(&still, &real_mp4(), Vendor::Google, 1_420_000).unwrap();
    let row = scan_row(&out);
    assert!(row.is_motion);
    assert_eq!(row.hdr, "HEIC（原样保留）");
    assert_eq!(row.cover, "1.420000 秒");
    assert_eq!(row.detail, "google-heic：等待转换");
    assert_eq!(row.manufacturer, "未知");
}

#[test]
fn scan_row_static_jpeg() {
    let row = scan_row(&skeleton());
    assert!(!row.is_motion);
    assert_eq!(row.hdr, "—");
    assert_eq!(row.cover, "—");
    assert_eq!(row.detail, "静态照片，不含动态内容，不参与转换");
    assert!(row.confirmed_static);
}

#[test]
fn scan_row_motion_claim_without_video_is_error_not_static() {
    let xml = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\" GCamera:MotionPhoto=\"1\"/></rdf:RDF></x:xmpmeta>";
    let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    xmp.extend(xml);
    let mut jpeg = vec![0xFF, 0xD8];
    jpeg.extend(app1(&xmp));
    jpeg.extend(skeleton()[2..].to_vec());
    let row = scan_row(&jpeg);
    assert!(!row.is_motion);
    assert!(!row.confirmed_static);
    assert!(!row.detail.is_empty());
    assert_ne!(row.detail, "静态照片，不含动态内容，不参与转换");
}
