//! MP4 -> MOV sample-copy remux tests (ROADMAP item 9a).

use momentstays_core::mp4::{boxes, sample_tables, TrackSamples};
use momentstays_core::remux::{check_remux, remux_to_mov, remux_to_mp4};

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

struct TrackSpec {
    handler: &'static str,
    timescale: u32,
    track_id: u32,
    width: u32,
    height: u32,
    volume: u16,
    stts: Vec<(u32, u32)>,
    ctts: Option<Vec<(u32, i32)>>,
    ctts_version: u8,
    sizes: Vec<u32>,
    stsd_entry: &'static str,
    minf_header: &'static str,
}

fn build_track(s: &TrackSpec) -> (Vec<u8>, Vec<u8>) {
    let mut tkhd_body = u32s(&[0, 0, s.track_id, 0, 0]);
    tkhd_body.extend([0u8; 8]);
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(s.volume.to_be_bytes());
    tkhd_body.extend(0u16.to_be_bytes());
    tkhd_body.extend(u32s(&[0x00010000, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000]));
    tkhd_body.extend(u32s(&[s.width, s.height]));
    let tkhd = full("tkhd", 0, 7, &tkhd_body);
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
    if let Some(entries) = &s.ctts {
        let mut body = u32s(&[entries.len() as u32]);
        for (c, o) in entries {
            body.extend(u32s(&[*c]));
            body.extend(o.to_be_bytes());
        }
        stbl.extend(full("ctts", s.ctts_version, 0, &body));
    }
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
    minf.extend(url_dinf());
    minf.extend(bx("stbl", &stbl));
    mdia.extend(bx("minf", &minf));
    let mut trak = tkhd;
    trak.extend(bx("mdia", &mdia));
    let mdat: Vec<u8> = s
        .sizes
        .iter()
        .enumerate()
        .flat_map(|(i, n)| vec![(i + 1) as u8; *n as usize])
        .collect();
    (bx("trak", &trak), mdat)
}

fn file(specs: &[TrackSpec]) -> Vec<u8> {
    let built: Vec<(Vec<u8>, Vec<u8>)> = specs.iter().map(build_track).collect();
    let build = |offsets: &[usize]| {
        let mut traks = Vec::new();
        for (i, (t, _)) in built.iter().enumerate() {
            let mut t = t.clone();
            let at = t.len() - 4;
            t[at..at + 4].copy_from_slice(&(offsets[i] as u32).to_be_bytes());
            traks.push(t);
        }
        let mut moov_body = full("mvhd", 0, 0, &u32s(&[0, 0, 1000, 0, 0x00010000, 0, 0]));
        for t in traks {
            moov_body.extend(t);
        }
        let mut out = bx("ftyp", b"isom\0\0\0\0isom");
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

fn video_spec() -> TrackSpec {
    TrackSpec {
        handler: "vide",
        timescale: 1000,
        track_id: 1,
        width: 16 << 16,
        height: 16 << 16,
        volume: 0,
        stts: vec![(3, 100)],
        ctts: Some(vec![(3, 0)]),
    ctts_version: 0,
        sizes: vec![10, 20, 30],
        stsd_entry: "avc1",
        minf_header: "vmhd",
    }
}

fn audio_spec() -> TrackSpec {
    TrackSpec {
        handler: "soun",
        timescale: 48000,
        track_id: 2,
        width: 0,
        height: 0,
        volume: 0x0100,
        stts: vec![(2, 1024)],
        ctts: None,
    ctts_version: 0,
        sizes: vec![5, 7],
        stsd_entry: "mp4a",
        minf_header: "smhd",
    }
}

#[test]
fn video_audio_remux_is_lossless() {
    let src = file(&[video_spec(), audio_spec()]);
    let out = remux_to_mov(&src, 0).expect("remux");
    assert_eq!(&out[4..8], b"ftyp");
    assert_eq!(&out[8..12], b"qt  ");
    check_remux(&src, &out, 0).expect("remux verifies");
    let tracks = sample_tables(&out).expect("output parses");
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].handler, "vide");
    assert_eq!(tracks[1].handler, "soun");
    assert_eq!(tracks[0].samples.len(), 3);
    assert_eq!(tracks[1].samples.len(), 2);
    // Movie timescale is 1_000_000.
    let top = boxes(&out, 0, out.len()).unwrap();
    let moov = top.iter().find(|b| b.is(b"moov")).unwrap();
    let mvhd = boxes(&out, moov.payload, moov.end).unwrap().into_iter().find(|b| b.is(b"mvhd")).unwrap();
    assert_eq!(&out[mvhd.payload + 12..mvhd.payload + 16], &1000000u32.to_be_bytes());
}

#[test]
fn meta_track_is_dropped() {
    let meta = TrackSpec {
        handler: "meta",
        timescale: 1000,
        track_id: 3,
        width: 0,
        height: 0,
        volume: 0,
        stts: vec![(1, 100)],
        ctts: None,
    ctts_version: 0,
        sizes: vec![4],
        stsd_entry: "mett",
        minf_header: "nmhd",
    };
    let src = file(&[video_spec(), meta, audio_spec()]);
    let out = remux_to_mov(&src, 0).expect("remux");
    check_remux(&src, &out, 0).expect("remux verifies");
    let tracks = sample_tables(&out).expect("output parses");
    assert_eq!(tracks.len(), 2);
    assert!(tracks.iter().all(|t| t.handler == "vide" || t.handler == "soun"));
}

#[test]
fn negative_presentation_is_refused() {
    let mut v = video_spec();
    v.ctts = Some(vec![(1, -500), (2, 0)]);
    v.ctts_version = 1;
    let src = file(&[v]);
    assert!(remux_to_mov(&src, 0).is_err());
}

#[test]
fn second_video_selection() {
    let v2 = TrackSpec { track_id: 4, ..video_spec() };
    let src = file(&[video_spec(), v2]);
    let out = remux_to_mov(&src, 1).expect("second video");
    check_remux(&src, &out, 1).expect("remux verifies");
    assert!(remux_to_mov(&src, 5).is_err());
}

fn sample_bytes_hash(t: &TrackSamples, d: &[u8]) -> String {
    let mut all = Vec::new();
    for s in &t.samples {
        all.extend_from_slice(&d[s.offset..s.offset + s.size]);
    }
    momentstays_core::sha256_hex(&all)
}

/// MP4 -> MOV -> MP4 round-trip (ROADMAP item A5): per-track sample
/// count, sizes, bytes and presentation times must survive both hops.
fn assert_roundtrip(src: &[u8]) {
    let mov = remux_to_mov(src, 0).expect("mp4->mov");
    let back = remux_to_mp4(&mov).expect("mov->mp4");
    assert_eq!(&back[4..8], b"ftyp");
    assert_eq!(&back[8..12], b"mp42");
    let src_tables = sample_tables(src).expect("src parses");
    let back_tables = sample_tables(&back).expect("round-trip parses");
    assert_eq!(src_tables.len(), back_tables.len());
    for (s, o) in src_tables.iter().zip(back_tables.iter()) {
        assert_eq!(s.handler, o.handler);
        assert_eq!(s.timescale, o.timescale);
        assert_eq!(s.samples.len(), o.samples.len());
        assert_eq!(
            s.samples.iter().map(|x| x.size).collect::<Vec<_>>(),
            o.samples.iter().map(|x| x.size).collect::<Vec<_>>()
        );
        assert_eq!(sample_bytes_hash(s, src), sample_bytes_hash(o, &back));
        assert_eq!(
            s.samples.iter().map(|x| x.presentation_time).collect::<Vec<_>>(),
            o.samples.iter().map(|x| x.presentation_time).collect::<Vec<_>>()
        );
    }
}

#[test]
fn mov_mp4_roundtrip_synthetic() {
    let src = file(&[video_spec(), audio_spec()]);
    assert_roundtrip(&src);
}

#[test]
fn mov_mp4_roundtrip_real_file() {
    use base64::Engine;
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    let src = base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap();
    assert_roundtrip(&src);
}

#[test]
fn real_file_remux() {
    use base64::Engine;
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    let src = base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap();
    let out = remux_to_mov(&src, 0).expect("real remux");
    check_remux(&src, &out, 0).expect("real remux verifies");
    std::fs::write("/tmp/real-remux.mov", &out).unwrap();
}
