//! Cover time <-> frame tests (ROADMAP A4): B-frame presentation order,
//! half-duration tolerance, tie refusal, out-of-range refusal, round-trips.

use momentstays_core::cover::{frame_to_time_us, time_us_to_frame, video_track};
use momentstays_core::mp4::sample_tables;

fn bx(t: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(t.as_bytes());
    v.extend(payload);
    v
}

fn full(t: &str, version: u8, body: &[u8]) -> Vec<u8> {
    let mut p = vec![version, 0, 0, 0];
    p.extend(body);
    bx(t, &p)
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

/// One video track: decode times from stts, presentation from ctts, samples
/// laid out back to back in a single chunk. Returns (track_bytes, mdat_bytes).
fn track(timescale: u32, stts: &[(u32, u32)], ctts: &[(u32, i32)]) -> (Vec<u8>, Vec<u8>) {
    let mut mdhd_body = u32s(&[0, 0, timescale, 0]);
    mdhd_body.extend([0u8; 4]);
    let mut mdia = full("mdhd", 0, &mdhd_body);
    let mut hdlr_body = vec![0u8; 4];
    hdlr_body.extend(b"vide");
    hdlr_body.extend([0u8; 12]);
    mdia.extend(full("hdlr", 0, &hdlr_body));
    let total: u32 = stts.iter().map(|(c, _)| c).sum();
    let mut stbl = full("stsd", 0, &[u32s(&[1]), bx("avc1", &[])].concat());
    let mut body = u32s(&[stts.len() as u32]);
    for (c, d) in stts {
        body.extend(u32s(&[*c, *d]));
    }
    stbl.extend(full("stts", 0, &body));
    let mut body = u32s(&[ctts.len() as u32]);
    for (c, o) in ctts {
        body.extend(u32s(&[*c]));
        body.extend(o.to_be_bytes());
    }
    stbl.extend(full("ctts", 0, &body));
    let body = u32s(&[1, 1, total, 1]);
    stbl.extend(full("stsc", 0, &body));
    let mut body = u32s(&[0, total]);
    body.extend(u32s(&vec![10u32; total as usize]));
    stbl.extend(full("stsz", 0, &body));
    // Chunk offset patched by the file builder (moov size is stable: the
    // stco box always holds exactly one offset).
    stbl.extend(full("stco", 0, &u32s(&[1, 0])));
    mdia.extend(bx("minf", &bx("stbl", &stbl)));
    let mut trak = bx("tkhd", &[]);
    trak.extend(bx("mdia", &mdia));
    let mdat = vec![0xabu8; total as usize * 10];
    (bx("trak", &trak), mdat)
}

fn file(tracks: Vec<(Vec<u8>, Vec<u8>)>) -> Vec<u8> {
    // Build once with zero offsets to measure moov, then patch chunk offsets.
    let build = |offsets: &[usize]| {
        let mut traks = Vec::new();
        for (i, (t, _)) in tracks.iter().enumerate() {
            let mut t = t.clone();
            // stco value is the last 4 bytes of the track (single-offset box).
            let at = t.len() - 4;
            t[at..at + 4].copy_from_slice(&(offsets[i] as u32).to_be_bytes());
            traks.push(t);
        }
        let mut moov_body = Vec::new();
        for t in traks {
            moov_body.extend(t);
        }
        moov_body.extend(bx("mvhd", &[]));
        let mut out = bx("ftyp", b"isom\0\0\0\0isom");
        out.extend(bx("moov", &moov_body));
        out
    };
    let head = build(&vec![0; tracks.len()]);
    let mut offsets = Vec::new();
    let mut at = head.len() + 8;
    for (_, m) in &tracks {
        offsets.push(at);
        at += m.len();
    }
    let mut out = build(&offsets);
    let mut mdat_body = Vec::new();
    for (_, m) in &tracks {
        mdat_body.extend(m);
    }
    out.extend(bx("mdat", &mdat_body));
    out
}

fn bframe_track() -> Vec<u8> {
    // Decode [0,100,200], presentation [300,100,200] (timescale 1000).
    let (t, m) = track(1000, &[(3, 100)], &[(1, 300), (1, 0), (1, 0)]);
    file(vec![(t, m)])
}

#[test]
fn frame_to_time_uses_presentation_order() {
    let data = bframe_track();
    let tracks = sample_tables(&data).unwrap();
    let v = video_track(&tracks).unwrap();
    assert_eq!(frame_to_time_us(v, 0).unwrap(), 100_000);
    assert_eq!(frame_to_time_us(v, 1).unwrap(), 200_000);
    assert_eq!(frame_to_time_us(v, 2).unwrap(), 300_000);
    assert!(frame_to_time_us(v, 3).is_err());
}

#[test]
fn time_to_frame_tolerance_and_refusals() {
    let data = bframe_track();
    let tracks = sample_tables(&data).unwrap();
    let v = video_track(&tracks).unwrap();
    assert_eq!(time_us_to_frame(v, 100_000).unwrap(), 0);
    assert_eq!(time_us_to_frame(v, 190_000).unwrap(), 1);
    // Exact tie between two samples: refused, not guessed.
    assert!(time_us_to_frame(v, 150_000).is_err());
    // Far outside: refused.
    assert!(time_us_to_frame(v, 1_000_000).is_err());
    assert!(time_us_to_frame(v, -5).is_err());
}

#[test]
fn roundtrip_is_exact() {
    let data = bframe_track();
    let tracks = sample_tables(&data).unwrap();
    let v = video_track(&tracks).unwrap();
    for frame in 0..3 {
        let t = frame_to_time_us(v, frame).unwrap();
        assert_eq!(time_us_to_frame(v, t).unwrap(), frame);
    }
}

#[test]
fn video_track_selection_is_strict() {
    // Two video tracks: ambiguous.
    let (t1, m1) = track(1000, &[(1, 100)], &[(1, 0)]);
    let (t2, m2) = track(1000, &[(1, 100)], &[(1, 0)]);
    let data = file(vec![(t1, m1), (t2, m2)]);
    let tracks = sample_tables(&data).unwrap();
    assert!(video_track(&tracks).is_err());
    // No video track at all: build a file with only an audio track by
    // relabeling the handler (same layout, different fourcc).
    let (t, m) = track(1000, &[(1, 100)], &[(1, 0)]);
    let mut t = t;
    let pos = t.windows(4).position(|w| w == b"vide").unwrap();
    t[pos..pos + 4].copy_from_slice(b"soun");
    let data = file(vec![(t, m)]);
    let tracks = sample_tables(&data).unwrap();
    assert!(video_track(&tracks).is_err());
}

#[test]
fn real_file_frame_time() {
    use base64::Engine;
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    let data = base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap();
    let tracks = sample_tables(&data).unwrap();
    let v = video_track(&tracks).unwrap();
    // Presentation [4096,6144,8192,10240,12288] @10240 -> us.
    assert_eq!(frame_to_time_us(v, 2).unwrap(), 800_000);
    assert_eq!(time_us_to_frame(v, 800_000).unwrap(), 2);
    assert_eq!(time_us_to_frame(v, 400_000).unwrap(), 0);
}
