//! Still-image-time track tests (ROADMAP item 9b2).
//!
//! Writer: `remux_to_mov_with_identifier_and_mett` on the real MP4
//! fixture, verified by parse-back (`mett_cover_time`, content
//! identifier, stsd/sample bytes, video samples).
//!
//! Reader: synthetic MOVs with a video track plus meta tracks. The
//! 1-key `mebx` entry, the 3-key entry and the 105-byte sample below
//! are copied verbatim from verified public samples (see
//! `tests/SOURCES.md`): the 1-key entry from the logseq demo (MIT) and
//! the 3-key entry + sample from the osxphotos iPhone file (MIT).

use base64::Engine;
use momentstays_core::live2motion::mett_cover_time;
use momentstays_core::mp4::{boxes, sample_tables};
use momentstays_core::remux::{
    read_content_identifier, remux_to_mov_with_identifier_and_mett,
};

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

fn url_dinf() -> Vec<u8> {
    let url = full("url ", 0, 1, &[]);
    let mut body = u32s(&[1]);
    body.extend(url);
    bx("dinf", &full("dref", 0, 0, &body))
}

/// 1-key `mebx` entry (96 bytes) for `com.apple.quicktime.still-image-time`.
const METT_1KEY: &[u8] = &[
    0x00, 0x00, 0x00, 0x60, 0x6d, 0x65, 0x62, 0x78, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x50, 0x6b, 0x65, 0x79, 0x73,
    0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30,
    0x6b, 0x65, 0x79, 0x64, 0x6d, 0x64, 0x74, 0x61, 0x63, 0x6f, 0x6d, 0x2e,
    0x61, 0x70, 0x70, 0x6c, 0x65, 0x2e, 0x71, 0x75, 0x69, 0x63, 0x6b, 0x74,
    0x69, 0x6d, 0x65, 0x2e, 0x73, 0x74, 0x69, 0x6c, 0x6c, 0x2d, 0x69, 0x6d,
    0x61, 0x67, 0x65, 0x2d, 0x74, 0x69, 0x6d, 0x65, 0x00, 0x00, 0x00, 0x10,
    0x64, 0x74, 0x79, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x41,
];

/// 3-key `mebx` entry (293 bytes) from the osxphotos iPhone file.
const STILL_3KEY: &[u8] = &[
    0x00, 0x00, 0x01, 0x25, 0x6d, 0x65, 0x62, 0x78, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x15, 0x6b, 0x65, 0x79, 0x73,
    0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30,
    0x6b, 0x65, 0x79, 0x64, 0x6d, 0x64, 0x74, 0x61, 0x63, 0x6f, 0x6d, 0x2e,
    0x61, 0x70, 0x70, 0x6c, 0x65, 0x2e, 0x71, 0x75, 0x69, 0x63, 0x6b, 0x74,
    0x69, 0x6d, 0x65, 0x2e, 0x73, 0x74, 0x69, 0x6c, 0x6c, 0x2d, 0x69, 0x6d,
    0x61, 0x67, 0x65, 0x2d, 0x74, 0x69, 0x6d, 0x65, 0x00, 0x00, 0x00, 0x10,
    0x64, 0x74, 0x79, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x41,
    0x00, 0x00, 0x00, 0x58, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x40,
    0x6b, 0x65, 0x79, 0x64, 0x6d, 0x64, 0x74, 0x61, 0x63, 0x6f, 0x6d, 0x2e,
    0x61, 0x70, 0x70, 0x6c, 0x65, 0x2e, 0x71, 0x75, 0x69, 0x63, 0x6b, 0x74,
    0x69, 0x6d, 0x65, 0x2e, 0x6c, 0x69, 0x76, 0x65, 0x2d, 0x70, 0x68, 0x6f,
    0x74, 0x6f, 0x2d, 0x73, 0x74, 0x69, 0x6c, 0x6c, 0x2d, 0x69, 0x6d, 0x61,
    0x67, 0x65, 0x2d, 0x74, 0x72, 0x61, 0x6e, 0x73, 0x66, 0x6f, 0x72, 0x6d,
    0x00, 0x00, 0x00, 0x10, 0x64, 0x74, 0x79, 0x70, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x53, 0x00, 0x00, 0x00, 0x6d, 0x00, 0x00, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x55, 0x6b, 0x65, 0x79, 0x64, 0x6d, 0x64, 0x74, 0x61,
    0x63, 0x6f, 0x6d, 0x2e, 0x61, 0x70, 0x70, 0x6c, 0x65, 0x2e, 0x71, 0x75,
    0x69, 0x63, 0x6b, 0x74, 0x69, 0x6d, 0x65, 0x2e, 0x6c, 0x69, 0x76, 0x65,
    0x2d, 0x70, 0x68, 0x6f, 0x74, 0x6f, 0x2d, 0x73, 0x74, 0x69, 0x6c, 0x6c,
    0x2d, 0x69, 0x6d, 0x61, 0x67, 0x65, 0x2d, 0x74, 0x72, 0x61, 0x6e, 0x73,
    0x66, 0x6f, 0x72, 0x6d, 0x2d, 0x72, 0x65, 0x66, 0x65, 0x72, 0x65, 0x6e,
    0x63, 0x65, 0x2d, 0x64, 0x69, 0x6d, 0x65, 0x6e, 0x73, 0x69, 0x6f, 0x6e,
    0x73, 0x00, 0x00, 0x00, 0x10, 0x64, 0x74, 0x79, 0x70, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x47,
];

/// 105-byte sample of the osxphotos still track (3 items; key 1 is the
/// 9-byte still-image-time item).
const STILL_3KEY_SAMPLE: &[u8] = &[
    0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x01, 0xff, 0x00, 0x00, 0x00,
    0x50, 0x00, 0x00, 0x00, 0x02, 0x3f, 0xef, 0xfd, 0x58, 0x00, 0x00, 0x00,
    0x00, 0x3f, 0x42, 0x89, 0xf4, 0x00, 0x00, 0x00, 0x00, 0x3f, 0xe5, 0x29,
    0x15, 0x40, 0x00, 0x00, 0x00, 0xbf, 0x49, 0xac, 0x0b, 0x60, 0x00, 0x00,
    0x00, 0x3f, 0xf0, 0x00, 0x61, 0xe0, 0x00, 0x00, 0x00, 0x3f, 0xcb, 0xbe,
    0xf6, 0x00, 0x00, 0x00, 0x00, 0xbe, 0x9d, 0x3c, 0x90, 0x60, 0x00, 0x00,
    0x00, 0x3e, 0x81, 0x64, 0x68, 0x40, 0x00, 0x00, 0x00, 0x3f, 0xf0, 0x00,
    0xf1, 0xa0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00,
    0x03, 0x44, 0xf0, 0x00, 0x00, 0x44, 0xb4, 0x00, 0x00,
];

/// The single still-image-time sample: `[size 9][key 1][value 0xff]`.
const STILL_SAMPLE: &[u8] = &[0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x01, 0xff];

/// Build a 1-key `mebx` entry for `key` with an 8-byte `dtyp` payload.
fn mebx_1key(key: &str, dtyp_payload: &[u8; 8]) -> Vec<u8> {
    let mut keyd_body = b"mdta".to_vec();
    keyd_body.extend(key.as_bytes());
    let keyd = bx("keyd", &keyd_body);
    let dtyp = bx("dtyp", dtyp_payload);
    let mut entry_body = u32s(&[(keyd.len() + dtyp.len() + 8) as u32, 1]);
    entry_body.extend(keyd);
    entry_body.extend(dtyp);
    let keys = bx("keys", &entry_body);
    let mut content = vec![0u8; 6];
    content.extend(1u16.to_be_bytes());
    content.extend(keys);
    bx("mebx", &content)
}

fn elst(segments: &[(u32, i32)]) -> Vec<u8> {
    let mut body = u32s(&[segments.len() as u32]);
    for (dur, media) in segments {
        body.extend(dur.to_be_bytes());
        body.extend(media.to_be_bytes());
        body.extend(1u16.to_be_bytes());
        body.extend(0u16.to_be_bytes());
    }
    bx("edts", &full("elst", 0, 0, &body))
}

struct MetaSpec {
    track_id: u32,
    timescale: u32,
    stsd_entry: Vec<u8>,
    sample: Vec<u8>,
    /// Edit-list segments as (duration in movie timescale, media time).
    edts: Option<Vec<(u32, i32)>>,
}

fn meta_track(spec: &MetaSpec) -> (Vec<u8>, Vec<u8>) {
    let mut tkhd_body = u32s(&[0, 0, spec.track_id, 0, 0]);
    tkhd_body.extend([0u8; 8]);
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(u32s(&[0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000]));
    tkhd_body.extend(u32s(&[0, 0]));
    let tkhd = full("tkhd", 0, 7, &tkhd_body);
    let mut mdhd_body = u32s(&[0, 0, spec.timescale, 1]);
    mdhd_body.extend(0x55c4u16.to_be_bytes());
    mdhd_body.extend(0u16.to_be_bytes());
    let mut mdia = full("mdhd", 0, 0, &mdhd_body);
    let mut hdlr_body = vec![0u8; 4];
    hdlr_body.extend(b"meta");
    hdlr_body.extend([0u8; 12]);
    mdia.extend(full("hdlr", 0, 0, &hdlr_body));
    let mut stbl = full("stsd", 0, 0, &[u32s(&[1]), spec.stsd_entry.clone()].concat());
    stbl.extend(full("stts", 0, 0, &u32s(&[1, 1, 1])));
    stbl.extend(full("stsc", 0, 0, &u32s(&[1, 1, 1, 1])));
    stbl.extend(full("stsz", 0, 0, &[u32s(&[0, 1]), u32s(&[spec.sample.len() as u32])].concat()));
    stbl.extend(full("stco", 0, 0, &u32s(&[1, 0])));
    let mut minf = full("nmhd", 0, 0, &[]);
    minf.extend(url_dinf());
    minf.extend(bx("stbl", &stbl));
    mdia.extend(bx("minf", &minf));
    let mut trak = tkhd;
    if let Some(segments) = &spec.edts {
        trak.extend(elst(segments));
    }
    trak.extend(bx("mdia", &mdia));
    (bx("trak", &trak), spec.sample.clone())
}

fn video_track(track_id: u32) -> (Vec<u8>, Vec<u8>) {
    let mut tkhd_body = u32s(&[0, 0, track_id, 0, 0]);
    tkhd_body.extend([0u8; 8]);
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(u32s(&[0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000]));
    tkhd_body.extend(u32s(&[16 << 16, 16 << 16]));
    let tkhd = full("tkhd", 0, 7, &tkhd_body);
    let mut mdhd_body = u32s(&[0, 0, 1000, 0]);
    mdhd_body.extend(0x55c4u16.to_be_bytes());
    mdhd_body.extend(0u16.to_be_bytes());
    let mut mdia = full("mdhd", 0, 0, &mdhd_body);
    let mut hdlr_body = vec![0u8; 4];
    hdlr_body.extend(b"vide");
    hdlr_body.extend([0u8; 12]);
    mdia.extend(full("hdlr", 0, 0, &hdlr_body));
    let mut stbl = full("stsd", 0, 0, &[u32s(&[1]), bx("avc1", &[0; 6])].concat());
    stbl.extend(full("stts", 0, 0, &u32s(&[1, 1, 100])));
    stbl.extend(full("ctts", 0, 0, &u32s(&[1, 1, 0])));
    stbl.extend(full("stsc", 0, 0, &u32s(&[1, 1, 1, 1])));
    stbl.extend(full("stsz", 0, 0, &u32s(&[0, 1, 10])));
    stbl.extend(full("stco", 0, 0, &u32s(&[1, 0])));
    let mut minf = full("vmhd", 0, 1, &[0; 8]);
    minf.extend(url_dinf());
    minf.extend(bx("stbl", &stbl));
    mdia.extend(bx("minf", &minf));
    let mut trak = tkhd;
    trak.extend(bx("mdia", &mdia));
    (bx("trak", &trak), vec![7u8; 10])
}

/// Assemble a file: video track first, then the given meta tracks.
/// The stco of every track (last 4 bytes of its trak) is patched to the
/// real mdat chunk offset in a second pass, like `tests/remux.rs`.
fn file(movie_ts: u32, metas: &[MetaSpec]) -> Vec<u8> {
    let mut built: Vec<(Vec<u8>, Vec<u8>)> = vec![video_track(1)];
    for (i, m) in metas.iter().enumerate() {
        let mut spec = MetaSpec {
            track_id: (i + 2) as u32,
            timescale: m.timescale,
            stsd_entry: m.stsd_entry.clone(),
            sample: m.sample.clone(),
            edts: m.edts.clone(),
        };
        spec.track_id = (i + 2) as u32;
        built.push(meta_track(&spec));
    }
    let build = |offsets: &[usize]| {
        let mut traks = Vec::new();
        for (i, (t, _)) in built.iter().enumerate() {
            let mut t = t.clone();
            let at = t.len() - 4;
            t[at..at + 4].copy_from_slice(&(offsets[i] as u32).to_be_bytes());
            traks.push(t);
        }
        let mut moov_body = full("mvhd", 0, 0, &u32s(&[0, 0, movie_ts, 0, 0x00010000, 0, 0]));
        for t in traks {
            moov_body.extend(t);
        }
        let mut out = bx("ftyp", b"isom\0\0\0\0isom");
        out.extend(bx("moov", &moov_body));
        out
    };
    let head = build(&vec![0; built.len()]);
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

fn still_spec(edts: Vec<(u32, i32)>) -> MetaSpec {
    MetaSpec {
        track_id: 0,
        timescale: 600,
        stsd_entry: METT_1KEY.to_vec(),
        sample: STILL_SAMPLE.to_vec(),
        edts: Some(edts),
    }
}

fn real_mp4() -> Vec<u8> {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

fn sha256_hex(data: &[u8]) -> String {
    momentstays_core::sha256_hex(data)
}

fn meta_stsd_entry(out: &[u8]) -> Vec<u8> {
    let top = boxes(out, 0, out.len()).unwrap();
    let moov = top.iter().find(|b| b.is(b"moov")).unwrap().clone();
    let traks: Vec<_> = boxes(out, moov.payload, moov.end)
        .unwrap()
        .into_iter()
        .filter(|b| b.is(b"trak"))
        .collect();
    for trak in traks {
        let mdia = boxes(out, trak.payload, trak.end).unwrap().into_iter().find(|b| b.is(b"mdia")).unwrap();
        let hdlr = boxes(out, mdia.payload, mdia.end).unwrap().into_iter().find(|b| b.is(b"hdlr")).unwrap();
        if &out[hdlr.payload + 8..hdlr.payload + 12] != b"meta" {
            continue;
        }
        let minf = boxes(out, mdia.payload, mdia.end).unwrap().into_iter().find(|b| b.is(b"minf")).unwrap();
        let stbl = boxes(out, minf.payload, minf.end).unwrap().into_iter().find(|b| b.is(b"stbl")).unwrap();
        let stsd = boxes(out, stbl.payload, stbl.end).unwrap().into_iter().find(|b| b.is(b"stsd")).unwrap();
        let entries = boxes(out, stsd.payload + 8, stsd.end).unwrap();
        assert_eq!(entries.len(), 1);
        return out[entries[0].start..entries[0].end].to_vec();
    }
    panic!("no meta track in output");
}

#[test]
fn writer_roundtrip_cover_time() {
    let src = real_mp4();
    let uuid = "123E4567-E89B-12D3-A456-426614174000";
    let out = remux_to_mov_with_identifier_and_mett(&src, 0, uuid, 1_420_000).expect("mett remux");
    assert_eq!(mett_cover_time(&out).unwrap(), Some(1_420_000));
    assert_eq!(read_content_identifier(&out).unwrap(), Some(uuid.to_string()));
    assert_eq!(meta_stsd_entry(&out), METT_1KEY);
    let tables = sample_tables(&out).unwrap();
    let meta = tables.iter().find(|t| t.handler == "meta").expect("meta table");
    assert_eq!(meta.samples.len(), 1);
    let s = &meta.samples[0];
    assert_eq!(&out[s.offset..s.offset + s.size], STILL_SAMPLE);
    assert_eq!(meta.timescale, 600);
    let src_tables = sample_tables(&src).unwrap();
    let src_video = src_tables.iter().find(|t| t.handler == "vide").unwrap();
    let out_video = tables.iter().find(|t| t.handler == "vide").unwrap();
    assert_eq!(out_video.samples.len(), src_video.samples.len());
    let hash = |t: &momentstays_core::mp4::TrackSamples, d: &[u8]| {
        let mut all = Vec::new();
        for s in &t.samples {
            all.extend_from_slice(&d[s.offset..s.offset + s.size]);
        }
        sha256_hex(&all)
    };
    assert_eq!(hash(out_video, &out), hash(src_video, &src));
}

#[test]
fn writer_cover_zero_uses_single_segment_elst() {
    let src = real_mp4();
    let out =
        remux_to_mov_with_identifier_and_mett(&src, 0, "123E4567-E89B-12D3-A456-426614174000", 0)
            .expect("mett remux");
    assert_eq!(mett_cover_time(&out).unwrap(), Some(0));
    let top = boxes(&out, 0, out.len()).unwrap();
    let moov = top.iter().find(|b| b.is(b"moov")).unwrap().clone();
    let mut elst_count = 0;
    for trak in boxes(&out, moov.payload, moov.end).unwrap().into_iter().filter(|b| b.is(b"trak")) {
        for edts in boxes(&out, trak.payload, trak.end).unwrap().into_iter().filter(|b| b.is(b"edts")) {
            let elst = boxes(&out, edts.payload, edts.end).unwrap().into_iter().find(|b| b.is(b"elst")).unwrap();
            let count = u32::from_be_bytes(out[elst.payload + 4..elst.payload + 8].try_into().unwrap());
            let mdia = boxes(&out, trak.payload, trak.end).unwrap().into_iter().find(|b| b.is(b"mdia")).unwrap();
            let hdlr = boxes(&out, mdia.payload, mdia.end).unwrap().into_iter().find(|b| b.is(b"hdlr")).unwrap();
            if &out[hdlr.payload + 8..hdlr.payload + 12] == b"meta" {
                elst_count = count;
            }
        }
    }
    assert_eq!(elst_count, 1);
}

#[test]
fn writer_refuses_negative_cover() {
    let src = real_mp4();
    assert!(remux_to_mov_with_identifier_and_mett(&src, 0, "x", -1).is_err());
}

#[test]
fn reader_native_1key_and_3key_forms() {
    // Native movie timescale 600, empty edit 740/600 s.
    let native = file(600, &[still_spec(vec![(740, -1), (1, 0)])]);
    assert_eq!(mett_cover_time(&native).unwrap(), Some(1_233_333));
    // Our output form: movie timescale 1_000_000, cover in microseconds.
    let ours = file(1_000_000, &[still_spec(vec![(1_420_000, -1), (1667, 0)])]);
    assert_eq!(mett_cover_time(&ours).unwrap(), Some(1_420_000));
    // Real 3-key entry + 105-byte sample from the osxphotos iPhone file.
    let three = file(
        600,
        &[MetaSpec {
            track_id: 0,
            timescale: 600,
            stsd_entry: STILL_3KEY.to_vec(),
            sample: STILL_3KEY_SAMPLE.to_vec(),
            edts: Some(vec![(740, -1), (1, 0)]),
        }],
    );
    assert_eq!(mett_cover_time(&three).unwrap(), Some(1_233_333));
}

#[test]
fn reader_ignores_other_meta_tracks() {
    let orientation = MetaSpec {
        track_id: 0,
        timescale: 600,
        stsd_entry: mebx_1key(
            "com.apple.quicktime.video-orientation",
            &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x42],
        ),
        sample: vec![0u8; 10],
        edts: None,
    };
    let info = MetaSpec {
        track_id: 0,
        timescale: 600,
        stsd_entry: mebx_1key(
            "com.apple.quicktime.live-photo-info",
            &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        ),
        sample: vec![0u8; 9],
        edts: None,
    };
    // Orientation alone: no marker.
    let only_orientation = file(600, &[MetaSpec {
        track_id: 0,
        timescale: orientation.timescale,
        stsd_entry: orientation.stsd_entry.clone(),
        sample: orientation.sample.clone(),
        edts: None,
    }]);
    assert_eq!(mett_cover_time(&only_orientation).unwrap(), None);
    // All three native meta tracks: only the still track matches.
    let mut still = still_spec(vec![(756, -1), (1, 0)]);
    still.timescale = 600;
    let all = file(
        600,
        &[
            MetaSpec {
                track_id: 0,
                timescale: 600,
                stsd_entry: orientation.stsd_entry.clone(),
                sample: orientation.sample.clone(),
                edts: None,
            },
            info,
            still,
        ],
    );
    assert_eq!(mett_cover_time(&all).unwrap(), Some(1_260_000));
    // Two still tracks: ambiguous, must fail rather than guess.
    let two = file(600, &[still_spec(vec![(740, -1), (1, 0)]), still_spec(vec![(756, -1), (1, 0)])]);
    assert!(mett_cover_time(&two).is_err());
}
