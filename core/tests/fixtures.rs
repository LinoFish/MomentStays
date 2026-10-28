//! Fixture tests for ROADMAP items 1-2: Ultra HDR, Extended XMP, Honor EHDR,
//! OPPO, vivo, legacy MicroVideo and the Google "recovered" path.
use momentstays_core::{jpeg, mp4, parse, xmp};

const NS: &str = concat!(
    r#"xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" "#,
    r#"xmlns:GCamera="http://ns.google.com/photos/1.0/camera/" "#,
    r#"xmlns:Container="http://ns.google.com/photos/1.0/container/" "#,
    r#"xmlns:Item="http://ns.google.com/photos/1.0/container/item/" "#,
    r#"xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/" "#,
    r#"xmlns:xmpNote="http://ns.adobe.com/xmp/note/" "#,
    r#"xmlns:OpCamera="http://ns.oplus.com/photos/1.0/camera/" "#,
    r#"xmlns:vivo="http://ns.vivo.com/photos/1.0/camera/""#
);

fn xml(attrs: &str, inner: &str) -> String {
    format!(r#"<x:xmpmeta {NS}><rdf:RDF><rdf:Description {attrs}>{inner}</rdf:Description></rdf:RDF></x:xmpmeta>"#)
}

fn container(len: usize) -> String {
    format!(concat!(
        r#"<Container:Directory><rdf:Seq>"#,
        r#"<rdf:li><Container:Item Item:Semantic="Primary" Item:Length="0"/></rdf:li>"#,
        r#"<rdf:li><Container:Item Item:Semantic="MotionPhoto" Item:Length="{}"/></rdf:li>"#,
        r#"</rdf:Seq></Container:Directory>"#), len)
}

fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xff, marker];
    v.extend(((payload.len() + 2) as u16).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn xmp_seg(x: &str) -> Vec<u8> {
    let mut p = xmp::HEADER.to_vec();
    p.extend(x.as_bytes());
    seg(0xe1, &p)
}

/// JPEG whose header holds `segs`, then a tiny scan with stuffed bytes.
fn still(segs: &[Vec<u8>]) -> Vec<u8> {
    let mut v = vec![0xff, 0xd8];
    for s in segs { v.extend(s); }
    v.extend(seg(0xdb, &[0u8; 65]));
    v.extend(seg(0xc0, b"\x08\0\x10\0\x10\x01\x01\x11\0"));
    v.extend(seg(0xda, b"\x01\x01\0\0\0\0"));
    v.extend(b"\x12\xff\0\x34\xff\xd0\x56\xff\xd9");
    v
}

fn bx(t: &[u8], p: &[u8]) -> Vec<u8> {
    let mut v = ((p.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(t);
    v.extend(p);
    v
}

fn mp4_plain() -> Vec<u8> {
    let mut v = bx(b"ftyp", b"isom\0\0\0\0isom");
    v.extend(bx(b"moov", &[0; 16]));
    v.extend(bx(b"mdat", b"\xff\xd9ftyp-decoy"));
    v
}

fn google(still: Vec<u8>, video: &[u8], extra: &str) -> Vec<u8> {
    let x = xml(&format!(r#"GCamera:MotionPhoto="1" GCamera:MotionPhotoPresentationTimestampUs="400000" {extra}"#), &container(video.len()));
    let mut segs = vec![xmp_seg(&x)];
    let mut s = still;
    // re-insert XMP right after SOI
    let body = s.split_off(2);
    for x in segs.drain(..) { s.extend(x); }
    s.extend(body);
    s.extend(video);
    s
}

// ---------- Ultra HDR ----------

/// Primary JPEG with an MPF index pointing at `gain`, which follows the primary.
fn with_mpf(primary_segs: Vec<Vec<u8>>, gain: &[u8]) -> Vec<u8> {
    let build = |rel: u32, size0: u32| {
        let mut t = b"MM\0\x2a\0\0\0\x08".to_vec();
        t.extend(1u16.to_be_bytes());
        t.extend(0xb002u16.to_be_bytes());
        t.extend(7u16.to_be_bytes());
        t.extend(32u32.to_be_bytes());
        t.extend(26u32.to_be_bytes());
        t.extend(0u32.to_be_bytes());
        for (i, (size, off)) in [(size0, 0u32), (gain.len() as u32, rel)].iter().enumerate() {
            t.extend(if i == 0 { 0x030000u32 } else { 0 }.to_be_bytes());
            t.extend(size.to_be_bytes());
            t.extend(off.to_be_bytes());
            t.extend(0u32.to_be_bytes());
        }
        let mut p = jpeg::MPF.to_vec();
        p.extend(t);
        let mut segs = primary_segs.clone();
        segs.push(seg(0xe2, &p));
        still(&segs)
    };
    let draft = build(0, 0);
    let mpf_start = jpeg::segments(&draft).unwrap().iter().find(|s| s.marker == 0xe2).unwrap().start;
    let primary = build((draft.len() - mpf_start - 8) as u32, draft.len() as u32);
    let mut out = primary;
    out.extend(gain);
    out
}

fn gain_map(with_version: bool) -> Vec<u8> {
    let attrs = if with_version { r#"hdrgm:Version="1.0""# } else { "" };
    still(&[xmp_seg(&xml(attrs, ""))])
}

#[test]
fn ultra_hdr_from_gain_map_xmp() {
    let video = mp4_plain();
    let base = with_mpf(vec![xmp_seg(&xml(r#"GCamera:MotionPhoto="1""#, &container(video.len())))], &gain_map(true));
    let mut d = base.clone();
    d.extend(&video);
    let p = parse(&d).unwrap();
    assert!(p.ultra);
    assert_eq!(p.still, base, "MPF offsets already canonical, static-only must be identity");
    assert_eq!(p.video(&d), &video[..]);
    assert_eq!(jpeg::image_ranges(&d).unwrap().len(), 2);
}

#[test]
fn ultra_hdr_declared_in_primary() {
    let video = mp4_plain();
    let mut d = with_mpf(vec![xmp_seg(&xml(r#"GCamera:MotionPhoto="1" hdrgm:Version="1.0""#, &container(video.len())))], &gain_map(false));
    d.extend(&video);
    assert!(parse(&d).unwrap().ultra);
}

#[test]
fn ultra_hdr_without_gain_map_rejected() {
    let video = mp4_plain();
    let mut d = still(&[xmp_seg(&xml(r#"GCamera:MotionPhoto="1" hdrgm:Version="1.0""#, &container(video.len())))]);
    d.extend(&video);
    assert_eq!(parse(&d).unwrap_err().0, "声明为 Ultra HDR，但缺少 MPF 增益图");
}

#[test]
fn mpf_with_non_image_attachment_rejected() {
    let video = mp4_plain();
    let mut d = with_mpf(vec![xmp_seg(&xml(r#"GCamera:MotionPhoto="1""#, &container(video.len())))], b"\xff\xd8garbage-not-jpeg\xff\xd9");
    d.extend(&video);
    assert!(parse(&d).is_err());
}

// ---------- Extended XMP ----------

fn extended(main_attrs: &str, ext_xml: &str, chunk: usize, corrupt_guid: bool) -> Vec<Vec<u8>> {
    let bytes = ext_xml.as_bytes();
    let mut guid = xmp::md5_upper(bytes);
    let main = xmp_seg(&xml(&format!(r#"{main_attrs} xmpNote:HasExtendedXMP="{guid}""#), ""));
    if corrupt_guid { guid = "0".repeat(32); }
    let mut segs = vec![main];
    for (i, part) in bytes.chunks(chunk).enumerate() {
        let mut p = xmp::EXTENDED.to_vec();
        p.extend(guid.as_bytes());
        p.extend((bytes.len() as u32).to_be_bytes());
        p.extend(((i * chunk) as u32).to_be_bytes());
        p.extend(part);
        segs.push(seg(0xe1, &p));
    }
    segs
}

#[test]
fn extended_xmp_split_across_segments() {
    let video = mp4_plain();
    let ext = xml(r#"GCamera:MotionPhotoPresentationTimestampUs="1234""#, &container(video.len()));
    let mut d = still(&extended(r#"GCamera:MotionPhoto="1""#, &ext, 97, false));
    d.extend(&video);
    let p = parse(&d).unwrap();
    assert_eq!(p.timestamp, Some(1234));
    assert_eq!(p.video(&d), &video[..]);
    let pk = xmp::packets(&d).unwrap();
    assert_eq!(pk.iter().find(|p| p.guid.is_some()).unwrap().fragments.len(), ext.len().div_ceil(97));
}

#[test]
fn extended_xmp_missing_reference() {
    let ext = xml("", "");
    let mut segs = extended(r#"GCamera:MotionPhoto="1""#, &ext, 4000, false);
    let guid = xmp::md5_upper(ext.as_bytes());
    segs.truncate(1);
    let d = still(&segs);
    assert_eq!(xmp::packets(&d).unwrap_err().0, format!("XMP 引用的扩展数据缺失：{guid}"));
}

#[test]
fn extended_xmp_bad_md5() {
    let segs = extended("", &xml("", ""), 4000, true);
    assert!(xmp::packets(&still(&segs)).is_err());
    let segs2: Vec<Vec<u8>> = segs.into_iter().skip(1).collect();
    assert_eq!(xmp::packets(&still(&segs2)).unwrap_err().0, "Extended XMP 校验失败");
}

#[test]
fn extended_xmp_missing_fragment() {
    let ext = xml(r#"GCamera:MotionPhoto="1""#, "");
    let mut segs: Vec<Vec<u8>> = extended("", &ext, 50, false).into_iter().skip(1).collect();
    segs.remove(1);
    assert_eq!(xmp::packets(&still(&segs)).unwrap_err().0, "Extended XMP 分片缺失、重叠或长度冲突");
}

// ---------- Honor private EHDR tail ----------

fn honor_tail(supported: bool) -> Vec<u8> {
    let count = 300;
    let mut t = vec![0u8; count];
    t[107..111].copy_from_slice(&107u32.to_le_bytes());
    t[111..119].copy_from_slice(b"CDRDATA\0");
    t[247..251].copy_from_slice(&128u32.to_le_bytes());
    t[251..255].copy_from_slice(&132u32.to_le_bytes());
    t[255..269].copy_from_slice(b"HiHonor_EhdrEn");
    t[276..280].copy_from_slice(&(if supported { 1u32 } else { 2 }).to_le_bytes());
    t[280..295].copy_from_slice(b"HiHonor_ExtInfo");
    t
}

#[test]
fn honor_ehdr_tail_kept_in_still() {
    let video = mp4_plain();
    let primary = still(&[xmp_seg(&xml(r#"GCamera:MotionPhoto="1""#, &container(video.len())))]);
    let mut d = primary.clone();
    d.extend(honor_tail(true));
    d.extend(&video);
    let p = parse(&d).unwrap();
    let mut want = primary;
    want.extend(honor_tail(true));
    assert_eq!(p.still, want);
}

#[test]
fn honor_ehdr_unknown_version_refused() {
    let video = mp4_plain();
    let mut d = still(&[xmp_seg(&xml(r#"GCamera:MotionPhoto="1""#, &container(video.len())))]);
    d.extend(honor_tail(false));
    d.extend(&video);
    assert_eq!(parse(&d).unwrap_err().0, "荣耀私有 HDR 扩展版本尚不支持，未丢弃原数据");
}

// ---------- OPPO / vivo / legacy ----------

#[test]
fn oppo_video_length_excludes_trailer() {
    let video = mp4_plain();
    let trailer = b"OPPO-private-trailer";
    let total = video.len() + trailer.len();
    let x = xml(&format!(concat!(r#"GCamera:MotionPhoto="1" OpCamera:MediaDataOffset="{}" OpCamera:VideoLength="{}" "#,
        r#"OpCamera:MotionPhotoPrimaryPresentationTimestampUs="777""#), total, video.len()), "");
    let mut d = still(&[xmp_seg(&x)]);
    d.extend(&video);
    d.extend(trailer);
    let p = parse(&d).unwrap();
    assert_eq!(p.format, "oppo");
    assert_eq!(p.video(&d), &video[..]);
    assert_eq!(p.timestamp, Some(777));
    assert_eq!(p.timestamp_tag.as_deref(), Some("OpCamera:MotionPhotoPrimaryPresentationTimestampUs"));
}

#[test]
fn oppo_bad_video_length() {
    let video = mp4_plain();
    let x = xml(&format!(r#"GCamera:MotionPhoto="1" OpCamera:MediaDataOffset="{}" OpCamera:VideoLength="99999""#, video.len()), "");
    let mut d = still(&[xmp_seg(&x)]);
    d.extend(&video);
    assert_eq!(parse(&d).unwrap_err().0, "OPPO 视频长度无效");
}

#[test]
fn vivo_v2() {
    let video = mp4_plain();
    let x = xml(&format!(r#"GCamera:MotionPhoto="1" GCamera:MicroVideoOffset="{}" vivo:VMotionPhotoVersion="2""#, video.len()), "");
    let mut d = still(&[xmp_seg(&x)]);
    d.extend(&video);
    let p = parse(&d).unwrap();
    assert_eq!(p.format, "vivo-v2");
    assert_eq!(p.timestamp, None);
}

#[test]
fn legacy_microvideo_negative_new_time_falls_back() {
    let video = mp4_plain();
    let x = xml(&format!(concat!(r#"GCamera:MicroVideo="1" GCamera:MicroVideoOffset="{}" "#,
        r#"GCamera:MotionPhotoPresentationTimestampUs="-1" GCamera:MicroVideoPresentationTimestampUs="55""#), video.len()), "");
    let mut d = still(&[xmp_seg(&x)]);
    d.extend(&video);
    let p = parse(&d).unwrap();
    assert_eq!((p.timestamp, p.timestamp_tag.as_deref()), (Some(55), Some("MicroVideoPresentationTimestampUs")));
}

#[test]
fn conflicting_container_lengths() {
    let video = mp4_plain();
    let inner = format!("{}{}", container(video.len()), "");
    let x = xml(r#"GCamera:MotionPhoto="1""#, &inner);
    let x2 = xml("", &container(video.len() + 1));
    let mut d = still(&[xmp_seg(&x), xmp_seg(&x2)]);
    d.extend(&video);
    assert_eq!(parse(&d).unwrap_err().0, "动态视频长度冲突");
}

#[test]
fn offset_into_jpeg_rejected() {
    let video = mp4_plain();
    let x = xml(&format!(r#"GCamera:MotionPhoto="1" GCamera:MicroVideoOffset="{}""#, video.len() + 30), "");
    let mut d = still(&[xmp_seg(&x)]);
    d.extend(&video);
    assert_eq!(parse(&d).unwrap_err().0, "视频与 JPEG 图像范围重叠");
}

// ---------- Google cover-time descriptors / recovered path ----------

fn desc(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    let n = payload.len();
    // always 4-byte varint, as some writers do
    v.extend([((n >> 21) & 127) as u8 | 128, ((n >> 14) & 127) as u8 | 128, ((n >> 7) & 127) as u8 | 128, (n & 127) as u8]);
    v.extend(payload);
    v
}

fn time_desc(t: i64) -> Vec<u8> {
    let mut p = vec![0; 4];
    p.extend(t.to_be_bytes());
    desc(0xc3, &p)
}

fn sample(primary: i64, high: &[i64]) -> Vec<u8> {
    let mut score = vec![0; 4];
    score.extend(time_desc(primary));
    if !high.is_empty() {
        let mut h = (high.len() as u32).to_be_bytes().to_vec();
        for t in high { h.extend(time_desc(*t)); }
        score.extend(desc(0xc2, &h));
    }
    desc(0xc0, &desc(0xc1, &score))
}

fn full(t: &[u8], p: &[u8]) -> Vec<u8> { let mut v = vec![0; 4]; v.extend(p); bx(t, &v) }

fn mp4_with_cover(sample_bytes: &[u8]) -> Vec<u8> {
    let ftyp = bx(b"ftyp", b"isom\0\0\0\0isom");
    let mdat = bx(b"mdat", sample_bytes);
    let offset = (ftyp.len() + 8) as u32;
    let mut hdlr = vec![0; 8];
    hdlr.extend(b"meta");
    hdlr.extend([0; 12]);
    let mut mett = vec![0; 8];
    mett.extend(b"\0application/motionphoto-image-meta\0");
    let mut stsd = 1u32.to_be_bytes().to_vec();
    stsd.extend(bx(b"mett", &mett));
    let mut stsz = 0u32.to_be_bytes().to_vec();
    stsz.extend(1u32.to_be_bytes());
    stsz.extend((sample_bytes.len() as u32).to_be_bytes());
    let mut stsc = 1u32.to_be_bytes().to_vec();
    for v in [1u32, 1, 1] { stsc.extend(v.to_be_bytes()); }
    let mut stco = 1u32.to_be_bytes().to_vec();
    stco.extend(offset.to_be_bytes());
    let stbl = [full(b"stsd", &stsd), full(b"stsz", &stsz), full(b"stsc", &stsc), full(b"stco", &stco)].concat();
    let minf = bx(b"minf", &bx(b"stbl", &stbl));
    let mdia = bx(b"mdia", &[bx(b"hdlr", &hdlr), minf].concat());
    let moov = bx(b"moov", &bx(b"trak", &mdia));
    [ftyp, mdat, moov].concat()
}

#[test]
fn cover_descriptor_times() {
    let v = mp4_with_cover(&sample(1_420_000, &[100, -1, 900]));
    let t = mp4::image_times(&v).unwrap().unwrap();
    assert_eq!(t.primary, Some(1_420_000));
    assert_eq!(t.high_resolution, vec![Some(100), None, Some(900)]);
}

#[test]
fn cover_descriptor_out_of_order() {
    let v = mp4_with_cover(&sample(1, &[900, 100]));
    assert_eq!(mp4::image_times(&v).unwrap_err().0, "Google 高清候选时间未按顺序排列");
}

#[test]
fn cover_descriptor_truncated() {
    let mut s = sample(1, &[]);
    s.truncate(s.len() - 3);
    assert!(mp4::image_times(&mp4_with_cover(&s)).is_err());
}

#[test]
fn recovered_cover_from_video_when_xmp_stripped() {
    let video = mp4_with_cover(&sample(1_420_000, &[]));
    let mut d = still(&[seg(0xe1, b"Exif\0\0")]);
    d.extend(&video);
    let p = parse(&d).unwrap();
    assert_eq!(p.timestamp, Some(1_420_000));
    assert_eq!(p.timestamp_tag.as_deref(), Some("MP4:motionphoto-image-meta"));
    assert_eq!(p.video(&d), &video[..]);
}

#[test]
fn recovered_without_descriptor_refuses() {
    let mut d = still(&[seg(0xe1, b"Exif\0\0")]);
    d.extend(mp4_plain());
    assert_eq!(parse(&d).unwrap_err().0, "原片缺少主 XMP，内嵌视频也未记录有效封面时间，无法无损恢复封面位置");
}

#[test]
fn xmp_time_wins_over_descriptor_when_identified() {
    let video = mp4_with_cover(&sample(9, &[]));
    let d = google(still(&[]), &video, "");
    assert_eq!(parse(&d).unwrap().timestamp, Some(400000));
}
