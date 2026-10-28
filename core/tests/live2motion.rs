//! Live -> Motion assembly tests (ROADMAP item 8).

use base64::Engine;
use momentstays_core::format;
use momentstays_core::live2motion::{assemble, mett_cover_time, LivePhotoInput};
use momentstays_core::model::{Cover, Vendor};
use momentstays_core::mp4::sample_tables;
use momentstays_core::pymotion::{live_still_clean, live_still_with_identifier, XMP_HDR};
use momentstays_core::tiff::Tiff;
use momentstays_core::pyjpeg;
use momentstays_core::remux::{check_remux, remux_to_mp4};

fn bx(t: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(t.as_bytes());
    v.extend(payload);
    v
}

fn full(t: &str, version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut p = vec![version, ((flags >> 16) & 0xff) as u8, ((flags >> 8) & 0xff) as u8, (flags & 0xff) as u8];
    p.extend(body);
    bx(t, &p)
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

fn dinf() -> Vec<u8> {
    let url = full("url ", 0, 1, &[]);
    let mut body = u32s(&[1]);
    body.extend(url);
    bx("dinf", &full("dref", 0, 0, &body))
}

struct TSpec {
    handler: &'static str,
    timescale: u32,
    track_id: u32,
    stts: Vec<(u32, u32)>,
    sizes: Vec<u32>,
    stsd_entry: &'static str,
    minf_header: &'static str,
    edts: Option<Vec<(u32, i32)>>,
}

fn track(s: &TSpec) -> (Vec<u8>, Vec<u8>) {
    let mut tkhd_body = u32s(&[0, 0, s.track_id, 0, 0]);
    tkhd_body.extend([0u8; 8]);
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(u32s(&[0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000]));
    tkhd_body.extend(u32s(&[0, 0]));
    let mut trak = full("tkhd", 0, 7, &tkhd_body);
    if let Some(entries) = &s.edts {
        let mut body = u32s(&[entries.len() as u32]);
        for (dur, media) in entries {
            body.extend(u32s(&[*dur]));
            body.extend(media.to_be_bytes());
            // rate_integer (1) + rate_fraction (0): normal playback speed.
            body.extend(1u16.to_be_bytes());
            body.extend(0u16.to_be_bytes());
        }
        trak.extend(bx("edts", &full("elst", 0, 0, &body)));
    }
    let mut mdhd_body = u32s(&[0, 0, s.timescale, 0]);
    mdhd_body.extend(0x55c4u16.to_be_bytes());
    mdhd_body.extend(0u16.to_be_bytes());
    let mut mdia = full("mdhd", 0, 0, &mdhd_body);
    let mut hdlr_body = vec![0u8; 4];
    hdlr_body.extend(s.handler.as_bytes());
    hdlr_body.extend([0u8; 12]);
    mdia.extend(full("hdlr", 0, 0, &hdlr_body));
    let total: u32 = s.stts.iter().map(|(c, _)| c).sum();
    let mut stbl = full("stsd", 0, 0, &[u32s(&[1]), bx(s.stsd_entry, &[0; 6])].concat());
    let mut body = u32s(&[s.stts.len() as u32]);
    for (c, d) in &s.stts {
        body.extend(u32s(&[*c, *d]));
    }
    stbl.extend(full("stts", 0, 0, &body));
    let body = u32s(&[1, 1, total, 1]);
    stbl.extend(full("stsc", 0, 0, &body));
    let mut body = u32s(&[0, total]);
    body.extend(u32s(&s.sizes));
    stbl.extend(full("stsz", 0, 0, &body));
    stbl.extend(full("stco", 0, 0, &u32s(&[1, 0])));
    let header = match s.minf_header {
        "vmhd" => full("vmhd", 0, 1, &[0; 8]),
        "smhd" => full("smhd", 0, 0, &[0; 4]),
        _ => full("nmhd", 0, 0, &[]),
    };
    let mut minf = header;
    minf.extend(dinf());
    minf.extend(bx("stbl", &stbl));
    mdia.extend(bx("minf", &minf));
    trak.extend(bx("mdia", &mdia));
    let mdat: Vec<u8> = s
        .sizes
        .iter()
        .enumerate()
        .flat_map(|(i, n)| vec![(i + 1) as u8; *n as usize])
        .collect();
    (bx("trak", &trak), mdat)
}

/// Build a MOV (ftyp `qt  `, movie timescale 1_000_000) from track specs.
fn mov(specs: &[TSpec]) -> Vec<u8> {
    let built: Vec<(Vec<u8>, Vec<u8>)> = specs.iter().map(track).collect();
    let build = |offsets: &[usize]| {
        let mut traks = Vec::new();
        for (i, (t, _)) in built.iter().enumerate() {
            let mut t = t.clone();
            let at = t.len() - 4;
            t[at..at + 4].copy_from_slice(&(offsets[i] as u32).to_be_bytes());
            traks.push(t);
        }
        let mut moov_body = full("mvhd", 0, 0, &u32s(&[0, 0, 1000000, 0, 0x00010000, 0, 0]));
        for t in traks {
            moov_body.extend(t);
        }
        let mut out = bx("ftyp", b"qt  \0\0\0\0qt  ");
        out.extend(bx("moov", &moov_body));
        out
    };
    let head = build(&vec![0; specs.len()]);
    let mut offsets = Vec::new();
    let mut at = head.len() + 8;
    for (_, m) in &built {
        offsets.push(at);
        at += m.len();
    }
    let mut out = build(&offsets);
    let mut mdat_body = Vec::new();
    for (_, m) in &built {
        mdat_body.extend(m);
    }
    out.extend(bx("mdat", &mdat_body));
    out
}

fn video_spec() -> TSpec {
    TSpec {
        handler: "vide",
        timescale: 1000,
        track_id: 1,
        stts: vec![(3, 100)],
        sizes: vec![10, 20, 30],
        stsd_entry: "avc1",
        minf_header: "vmhd",
        edts: None,
    }
}

fn audio_spec() -> TSpec {
    TSpec {
        handler: "soun",
        timescale: 48000,
        track_id: 2,
        stts: vec![(2, 1024)],
        sizes: vec![5, 7],
        stsd_entry: "mp4a",
        minf_header: "smhd",
        edts: None,
    }
}

fn mett_spec() -> TSpec {
    TSpec {
        handler: "meta",
        timescale: 1000000,
        track_id: 3,
        stts: vec![(1, 100000)],
        sizes: vec![1],
        stsd_entry: "mett",
        minf_header: "nmhd",
        edts: Some(vec![(500000, -1), (100000, 0)]),
    }
}

#[test]
fn mov_to_mp4_drops_metadata_track() {
    let src = mov(&[video_spec(), audio_spec(), mett_spec()]);
    let out = remux_to_mp4(&src).expect("mov->mp4");
    assert_eq!(&out[8..12], b"mp42");
    check_remux(&src, &out, 0).expect("mp4 remux verifies");
    let tracks = sample_tables(&out).expect("output parses");
    assert_eq!(tracks.len(), 2);
    assert!(tracks.iter().all(|t| t.handler == "vide" || t.handler == "soun"));
}

#[test]
fn mov_to_mp4_refuses_unexpected_layout() {
    let two_videos = mov(&[video_spec(), TSpec { track_id: 4, ..video_spec() }]);
    assert!(remux_to_mp4(&two_videos).is_err());
    let two_audios = mov(&[video_spec(), audio_spec(), TSpec { track_id: 5, ..audio_spec() }]);
    assert!(remux_to_mp4(&two_audios).is_err());
}

#[test]
fn mett_cover_time_through_edit_list() {
    let src = mov(&[video_spec(), mett_spec()]);
    assert_eq!(mett_cover_time(&src).expect("cover time"), Some(500000));
    let no_mett = mov(&[video_spec(), audio_spec()]);
    assert_eq!(mett_cover_time(&no_mett).expect("no mett"), None);
    let no_elst = mov(&[video_spec(), TSpec { edts: None, ..mett_spec() }]);
    assert_eq!(mett_cover_time(&no_elst).expect("no elst"), Some(0));
}

// --- live_still_clean ---

fn app1(payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, 0xE1];
    let len = (payload.len() + 2) as u16;
    v.extend(len.to_be_bytes());
    v.extend(payload);
    v
}

fn minimal_jpeg_with_motion_xmp() -> Vec<u8> {
    let xml = concat!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">",
        "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
        "<rdf:Description ",
        "xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\" ",
        "xmlns:Container=\"http://ns.google.com/photos/1.0/container/\" ",
        "xmlns:Item=\"http://ns.google.com/photos/1.0/container/item/\" ",
        "xmlns:OpCamera=\"http://ns.oplus.com/photos/1.0/camera/\" ",
        "xmlns:Vivo=\"http://ns.vivo.com/photos/1.0/camera/\" ",
        "GCamera:MotionPhoto=\"1\" ",
        "GCamera:MotionPhotoPresentationTimestampUs=\"500000\" ",
        "GCamera:MicroVideoOffset=\"1234\" ",
        "OpCamera:MotionPhotoPrimaryPresentationTimestampUs=\"500000\" ",
        "OpCamera:VideoLength=\"999\" ",
        "Vivo:VMotionPhotoVersion=\"2\">",
        "<Container:Directory><rdf:Seq>",
        "<rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"Primary\" Item:Mime=\"image/jpeg\" Item:Length=\"0\"/></rdf:li>",
        "<rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"GainMap\" Item:Mime=\"image/jpeg\" Item:Length=\"7\"/></rdf:li>",
        "<rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"MotionPhoto\" Item:Mime=\"video/mp4\" Item:Length=\"999\"/></rdf:li>",
        "</rdf:Seq></Container:Directory>",
        "</rdf:Description></rdf:RDF></x:xmpmeta>"
    );
    let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    xmp.extend(xml.as_bytes());
    let mut out = vec![0xFF, 0xD8];
    out.extend(app1(&xmp));
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend([0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);
    out.extend(app1(&exif));
    // Minimal frame/scan headers; segment contents are opaque to the parser.
    out.extend([0xFF, 0xDB, 0x00, 0x04, 0x00, 0x11]);
    out.extend([0xFF, 0xC0, 0x00, 0x08, 0x08, 0x00, 0x10, 0x00, 0x10, 0x01]);
    out.extend([0xFF, 0xC4, 0x00, 0x04, 0x00, 0x11]);
    out.extend([0xFF, 0xDA, 0x00, 0x04, 0x00, 0x11]);
    out.push(0x42);
    out.extend([0xFF, 0xD9]);
    out
}

fn exif_payload(jpeg: &[u8]) -> Vec<u8> {
    pyjpeg::segments(jpeg)
        .unwrap()
        .into_iter()
        .find(|s| s.marker == 0xE1 && s.payload(jpeg).starts_with(b"Exif\0\0"))
        .map(|s| s.payload(jpeg).to_vec())
        .unwrap()
}

#[test]
fn clean_strips_motion_xmp_keeps_exif_and_gainmap_li() {
    let src = minimal_jpeg_with_motion_xmp();
    let cleaned = live_still_clean(&src).expect("clean");
    let text = String::from_utf8_lossy(&cleaned);
    assert!(!text.contains("MotionPhotoPresentationTimestampUs"));
    assert!(!text.contains("MicroVideoOffset"));
    assert!(!text.contains("MotionPhotoPrimaryPresentationTimestampUs"));
    assert!(!text.contains("VideoLength"));
    assert!(!text.contains("VMotionPhotoVersion"));
    assert!(!text.contains("MotionPhoto\" Item:Mime=\"video/mp4\""));
    assert!(text.contains("GainMap"));
    assert_eq!(exif_payload(&cleaned), exif_payload(&src));
    pyjpeg::segments(&cleaned).expect("cleaned still parses");
}

#[test]
fn clean_drops_trailing_junk() {
    let mut src = minimal_jpeg_with_motion_xmp();
    src.extend(b"JUNKJUNK");
    let cleaned = live_still_clean(&src).expect("clean trailing");
    let plain = live_still_clean(&minimal_jpeg_with_motion_xmp()).expect("clean plain");
    assert_eq!(cleaned, plain);
}

#[test]
fn clean_refuses_heic() {
    let mut fake = Vec::new();
    fake.extend(24u32.to_be_bytes());
    fake.extend(b"ftyp");
    fake.extend(b"heic");
    fake.extend(0u32.to_be_bytes());
    fake.extend(b"mif1");
    fake.extend([0u8; 64]);
    assert!(live_still_clean(&fake).is_err());
}

// --- end-to-end assembly ---

fn fixture(name: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-pycli").join(name),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

#[test]
fn assemble_google_from_live_pair() {
    let still = fixture("static.jpg.b64");
    let movie = mov(&[video_spec(), audio_spec()]);
    let input = LivePhotoInput { still_jpeg: still, movie_mov: movie };
    let out = assemble(&input, Vendor::Google, Cover::Time(500000)).expect("assemble google");
    let parsed = format::parse(&out).expect("output parses");
    assert_eq!(parsed.timestamp, Some(500000));
    let video = remux_to_mp4(&input.movie_mov).expect("remux for comparison");
    assert_eq!(parsed.video(&out), video.as_slice());
}

// --- live_still_with_identifier (item 6) ---

/// JPEG like `minimal_jpeg_with_motion_xmp` but with a valid minimal
/// TIFF EXIF (II, 0 entries) when `with_exif`, or no EXIF at all.
fn jpeg_with_motion_xmp(with_exif: bool) -> Vec<u8> {
    let xml = concat!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">",
        "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
        "<rdf:Description ",
        "xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\" ",
        "xmlns:Container=\"http://ns.google.com/photos/1.0/container/\" ",
        "xmlns:Item=\"http://ns.google.com/photos/1.0/container/item/\" ",
        "GCamera:MotionPhoto=\"1\" ",
        "GCamera:MotionPhotoPresentationTimestampUs=\"500000\" ",
        "GCamera:MicroVideoOffset=\"1234\">",
        "<Container:Directory><rdf:Seq>",
        "<rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"Primary\" Item:Mime=\"image/jpeg\" Item:Length=\"0\"/></rdf:li>",
        "<rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"MotionPhoto\" Item:Mime=\"video/mp4\" Item:Length=\"999\"/></rdf:li>",
        "</rdf:Seq></Container:Directory>",
        "</rdf:Description></rdf:RDF></x:xmpmeta>"
    );
    let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    xmp.extend(xml.as_bytes());
    let mut out = vec![0xFF, 0xD8];
    out.extend(app1(&xmp));
    if with_exif {
        let mut exif = b"Exif\0\0".to_vec();
        exif.extend([0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        out.extend(app1(&exif));
    }
    out.extend([0xFF, 0xDB, 0x00, 0x04, 0x00, 0x11]);
    out.extend([0xFF, 0xC0, 0x00, 0x08, 0x08, 0x00, 0x10, 0x00, 0x10, 0x01]);
    out.extend([0xFF, 0xC4, 0x00, 0x04, 0x00, 0x11]);
    out.extend([0xFF, 0xDA, 0x00, 0x04, 0x00, 0x11]);
    out.push(0x42);
    out.extend([0xFF, 0xD9]);
    out
}

/// Coded image bytes: every segment except EXIF/XMP header segments,
/// plus trailing scan data. Header-only edits must not move these.
fn coding_bytes(jpeg: &[u8]) -> Vec<u8> {
    let segs = pyjpeg::segments(jpeg).expect("segments");
    let mut out = Vec::new();
    for s in &segs {
        let pl = s.payload(jpeg);
        if s.marker == 0xE1 && (pl.starts_with(pyjpeg::EXIF_HDR) || pl.starts_with(XMP_HDR)) {
            continue;
        }
        out.extend_from_slice(&jpeg[s.start..s.end]);
    }
    if let Some(last) = segs.last() {
        out.extend_from_slice(&jpeg[last.end..]);
    }
    out
}

/// MakerNote tag 17 (ContentIdentifier) read back from a JPEG's EXIF,
/// mirroring Swift `movie()`'s still check (`maker["17"] == uuid`).
fn makernote_uuid(jpeg: &[u8]) -> Option<String> {
    let segs = pyjpeg::segments(jpeg).ok()?;
    let exif = segs
        .iter()
        .find(|s| s.marker == 0xE1 && s.payload(jpeg).starts_with(pyjpeg::EXIF_HDR))?;
    let tiff_bytes = &exif.payload(jpeg)[6..];
    let t = Tiff::new(tiff_bytes).ok()?;
    let (root, _) = t.root().ok()?;
    let ptr = root.iter().find(|f| f.tag == 0x8769)?;
    let (exif_fields, _) = t.directory(ptr.offset(t.little).ok()?).ok()?;
    let note_field = exif_fields.iter().find(|f| f.tag == 0x927C)?;
    let note_off = note_field.offset(t.little).ok()?;
    let note = &tiff_bytes[note_off..note_off + note_field.count];
    if !note.starts_with(b"Apple iOS\0\x00\x01") || note.len() < 16 {
        return None;
    }
    let little = note[12] == b'I';
    let rd16 = |p: usize| -> u16 {
        if little {
            u16::from_le_bytes([note[p], note[p + 1]])
        } else {
            u16::from_be_bytes([note[p], note[p + 1]])
        }
    };
    let rd32 = |p: usize| -> u32 {
        if little {
            u32::from_le_bytes([note[p], note[p + 1], note[p + 2], note[p + 3]])
        } else {
            u32::from_be_bytes([note[p], note[p + 1], note[p + 2], note[p + 3]])
        }
    };
    let n = rd16(14) as usize;
    for i in 0..n {
        let p = 16 + i * 12;
        if p + 12 > note.len() || rd16(p) != 17 {
            continue;
        }
        let count = rd32(p + 4) as usize;
        let raw = if count <= 4 {
            note[p + 8..p + 8 + count].to_vec()
        } else {
            let off = rd32(p + 8) as usize;
            note[off..off + count].to_vec()
        };
        return Some(String::from_utf8_lossy(&raw).trim_end_matches('\0').to_string());
    }
    None
}

#[test]
fn still_with_identifier_inserts_and_reads_back() {
    let src = jpeg_with_motion_xmp(false);
    let uuid = "123E4567-E89B-12D3-A456-426614174000";
    let out = live_still_with_identifier(&src, uuid).expect("identifier");
    assert_eq!(makernote_uuid(&out).as_deref(), Some(uuid));
    assert_eq!(coding_bytes(&src), coding_bytes(&out), "coding data unchanged");
    let text = String::from_utf8_lossy(&out);
    assert!(!text.contains("MotionPhotoPresentationTimestampUs"));
    assert!(!text.contains("MicroVideoOffset"));
    assert!(!text.contains("MotionPhoto\" Item:Mime=\"video/mp4\""));
    assert!(format::parse(&out).is_err(), "no longer a motion photo");
}

#[test]
fn still_with_identifier_replaces_and_keeps_coding() {
    let first =
        live_still_with_identifier(&jpeg_with_motion_xmp(false), "AAAAAAAA-0000-0000-0000-000000000001").unwrap();
    let second =
        live_still_with_identifier(&first, "BBBBBBBB-0000-0000-0000-000000000002").unwrap();
    assert_eq!(
        makernote_uuid(&second).as_deref(),
        Some("BBBBBBBB-0000-0000-0000-000000000002")
    );
    assert_eq!(coding_bytes(&first), coding_bytes(&second));
}

/// LONG8 (type 16) MakerNote field value read back from a JPEG's EXIF.
/// Real iPhone MakerNotes use LONG8 for LivePhotoVideoIndex (tag 0x17);
/// the writer used to reject it as an unknown field type.
fn makernote_long8(jpeg: &[u8], want: u16) -> Option<u64> {
    let segs = pyjpeg::segments(jpeg).ok()?;
    let exif = segs
        .iter()
        .find(|s| s.marker == 0xE1 && s.payload(jpeg).starts_with(pyjpeg::EXIF_HDR))?;
    let tiff_bytes = &exif.payload(jpeg)[6..];
    let t = Tiff::new(tiff_bytes).ok()?;
    let (root, _) = t.root().ok()?;
    let ptr = root.iter().find(|f| f.tag == 0x8769)?;
    let (exif_fields, _) = t.directory(ptr.offset(t.little).ok()?).ok()?;
    let note_field = exif_fields.iter().find(|f| f.tag == 0x927C)?;
    let note_off = note_field.offset(t.little).ok()?;
    let note = &tiff_bytes[note_off..note_off + note_field.count];
    if !note.starts_with(b"Apple iOS\0\x00\x01") || note.len() < 16 {
        return None;
    }
    let little = note[12] == b'I';
    let rd16 = |p: usize| -> u16 {
        if little {
            u16::from_le_bytes([note[p], note[p + 1]])
        } else {
            u16::from_be_bytes([note[p], note[p + 1]])
        }
    };
    let rd32 = |p: usize| -> u32 {
        if little {
            u32::from_le_bytes([note[p], note[p + 1], note[p + 2], note[p + 3]])
        } else {
            u32::from_be_bytes([note[p], note[p + 1], note[p + 2], note[p + 3]])
        }
    };
    let n = rd16(14) as usize;
    for i in 0..n {
        let p = 16 + i * 12;
        if p + 12 > note.len() || rd16(p) != want || rd16(p + 2) != 16 {
            continue;
        }
        let count = rd32(p + 4) as usize;
        if count != 1 {
            return None;
        }
        let off = rd32(p + 8) as usize;
        let raw = &note[off..off + 8];
        return Some(if little {
            u64::from_le_bytes(raw.try_into().ok()?)
        } else {
            u64::from_be_bytes(raw.try_into().ok()?)
        });
    }
    None
}

#[test]
fn still_with_identifier_keeps_long8_makernote_fields() {
    // Regression: a real iPhone 15 Pro Max MakerNote (CC0 sample, see
    // SOURCES.md) carries LONG8 fields; replacing the identifier must
    // keep their values verbatim, not fail on the field type.
    let long8: u64 = 0x1122334455667788;
    let old_uuid = b"AAAAAAAA-1111-2222-3333-444444444444\0";
    let ent = |tag: u16, ty: u16, count: u32, val: [u8; 4]| {
        let mut e = Vec::new();
        e.extend(tag.to_le_bytes());
        e.extend(ty.to_le_bytes());
        e.extend(count.to_le_bytes());
        e.extend(val);
        e
    };
    // MakerNote, little-endian: dir at 16 (3 entries), next at 52,
    // LONG8 value at 56, old uuid at 64.
    let mut note = b"Apple iOS\0\x00\x01".to_vec();
    note.extend(b"II");
    note.extend(3u16.to_le_bytes());
    note.extend(ent(0x0001, 9, 1, 42u32.to_le_bytes()));
    note.extend(ent(0x0011, 2, 37, 64u32.to_le_bytes()));
    note.extend(ent(0x0017, 16, 1, 56u32.to_le_bytes()));
    note.extend([0u8; 4]);
    note.extend(long8.to_le_bytes());
    note.extend(old_uuid);
    assert_eq!(note.len(), 101);
    // TIFF, little-endian: root IFD at 8 (Exif ptr 26), Exif IFD at 26
    // (MakerNote ptr 44), note at 44.
    let mut tiff = b"II".to_vec();
    tiff.extend(42u16.to_le_bytes());
    tiff.extend(8u32.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(ent(0x8769, 4, 1, 26u32.to_le_bytes()));
    tiff.extend([0u8; 4]);
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(ent(0x927C, 7, note.len() as u32, 44u32.to_le_bytes()));
    tiff.extend([0u8; 4]);
    tiff.extend(&note);
    assert_eq!(tiff.len(), 44 + note.len());
    let mut jpeg = vec![0xFF, 0xD8];
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend(&tiff);
    jpeg.extend(app1(&exif));
    jpeg.extend([0xFF, 0xDB, 0x00, 0x04, 0x00, 0x11]);
    jpeg.extend([0xFF, 0xC0, 0x00, 0x08, 0x08, 0x00, 0x10, 0x00, 0x10, 0x01]);
    jpeg.extend([0xFF, 0xC4, 0x00, 0x04, 0x00, 0x11]);
    jpeg.extend([0xFF, 0xDA, 0x00, 0x04, 0x00, 0x11]);
    jpeg.push(0x42);
    jpeg.extend([0xFF, 0xD9]);
    let new_uuid = "123E4567-E89B-12D3-A456-426614174000";
    let out = live_still_with_identifier(&jpeg, new_uuid).expect("live_still");
    assert_eq!(makernote_uuid(&out).as_deref(), Some(new_uuid));
    assert_eq!(makernote_long8(&out, 0x17), Some(long8), "LONG8 value kept");
    assert_eq!(coding_bytes(&out), coding_bytes(&jpeg), "coding bytes unchanged");
}

#[test]
fn still_with_identifier_refuses_heic() {
    let mut fake = Vec::new();
    fake.extend(24u32.to_be_bytes());
    fake.extend(b"ftyp");
    fake.extend(b"heic");
    fake.extend(0u32.to_be_bytes());
    fake.extend(b"mif1");
    fake.extend([0u8; 64]);
    assert!(live_still_with_identifier(&fake, "123E4567-E89B-12D3-A456-426614174000").is_err());
}

#[test]
fn assemble_huawei_from_live_pair_uses_frame() {
    let still = fixture("static.jpg.b64");
    let movie = mov(&[video_spec()]);
    let input = LivePhotoInput { still_jpeg: still, movie_mov: movie };
    let out = assemble(&input, Vendor::Huawei, Cover::Frame(2)).expect("assemble huawei");
    let parsed = format::parse(&out).expect("output parses");
    assert_eq!(parsed.cover_frame, Some(2));
}
