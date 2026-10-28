//! MP4/MOV sample-table reader tests (ROADMAP item 7): synthetic tracks
//! with exact expected sample offsets, sizes and timestamps.

use momentstays_core::mp4::{sample_tables, Sample};

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

fn mdhd(timescale: u32) -> Vec<u8> {
    let mut body = u32s(&[0, 0, timescale, 0]);
    body.extend([0u8; 4]);
    full("mdhd", 0, &body)
}

fn hdlr(kind: &str) -> Vec<u8> {
    let mut body = vec![0u8; 4];
    body.extend(kind.as_bytes());
    body.extend([0u8; 12]);
    full("hdlr", 0, &body)
}

fn stsd() -> Vec<u8> {
    let mut body = u32s(&[1]);
    body.extend(bx("avc1", &[]));
    full("stsd", 0, &body)
}

struct TrackSpec {
    timescale: u32,
    stts: Vec<(u32, u32)>,
    ctts: Option<(u8, Vec<(u32, i32)>)>,
    stsc: Vec<(u32, u32)>,
    sizes: Vec<u32>,
    sample_count: usize,
    fixed_size: u32,
    stco: Vec<u32>,
    co64: Option<Vec<u64>>,
    stss: Option<Vec<u32>>,
}

fn stbl(spec: &TrackSpec) -> Vec<u8> {
    let mut s = stsd();
    let mut body = u32s(&[spec.stts.len() as u32]);
    for (c, d) in &spec.stts {
        body.extend(u32s(&[*c, *d]));
    }
    s.extend(full("stts", 0, &body));
    if let Some((version, entries)) = &spec.ctts {
        let mut body = u32s(&[entries.len() as u32]);
        for (c, o) in entries {
            body.extend(u32s(&[*c]));
            body.extend(o.to_be_bytes());
        }
        s.extend(full("ctts", *version, &body));
    }
    let mut body = u32s(&[spec.stsc.len() as u32]);
    for (f, n) in &spec.stsc {
        body.extend(u32s(&[*f, *n, 1]));
    }
    s.extend(full("stsc", 0, &body));
    let mut body = u32s(&[spec.fixed_size, spec.sample_count as u32]);
    if spec.fixed_size == 0 {
        body.extend(u32s(&spec.sizes));
    }
    s.extend(full("stsz", 0, &body));
    if let Some(offsets) = &spec.co64 {
        let mut body = u32s(&[offsets.len() as u32]);
        for o in offsets {
            body.extend(o.to_be_bytes());
        }
        s.extend(full("co64", 0, &body));
    } else {
        let body = {
            let mut b = u32s(&[spec.stco.len() as u32]);
            b.extend(u32s(&spec.stco));
            b
        };
        s.extend(full("stco", 0, &body));
    }
    if let Some(numbers) = &spec.stss {
        let mut body = u32s(&[numbers.len() as u32]);
        body.extend(u32s(numbers));
        s.extend(full("stss", 0, &body));
    }
    bx("stbl", &s)
}

fn track(spec: &TrackSpec) -> Vec<u8> {
    let mut mdia = mdhd(spec.timescale);
    mdia.extend(hdlr("vide"));
    mdia.extend(bx("minf", &stbl(spec)));
    let mut trak = bx("tkhd", &[]);
    trak.extend(bx("mdia", &mdia));
    bx("trak", &trak)
}

/// Build a file whose mdat holds the samples back to back, chunked per
/// `stsc`; sample i is filled with byte (i+1) so offsets are checkable.
fn file_with_chunks(spec: &TrackSpec, chunks: &[Vec<usize>]) -> Vec<u8> {
    // First pass with zero chunk offsets to measure the moov size.
    let mut spec0 = TrackSpec {
        timescale: spec.timescale,
        stts: spec.stts.clone(),
        ctts: spec.ctts.clone(),
        stsc: spec.stsc.clone(),
        sizes: spec.sizes.clone(),
        sample_count: spec.sample_count,
        fixed_size: spec.fixed_size,
        stco: vec![0; spec.stco.len()],
        co64: spec.co64.as_ref().map(|v| vec![0; v.len()]),
        stss: spec.stss.clone(),
    };
    let ftyp = bx("ftyp", b"isom\0\0\0\0isom");
    let moov = bx("moov", &[track(&spec0), bx("mvhd", &[])].concat());
    let mdat_header = 8usize;
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    let mut at = ftyp.len() + moov.len() + mdat_header;
    for chunk in chunks {
        offsets.push(at as u32);
        for &sample in chunk {
            let size = if spec.fixed_size != 0 {
                spec.fixed_size as usize
            } else {
                spec.sizes[sample] as usize
            };
            data.extend(vec![(sample + 1) as u8; size]);
            at += size;
        }
    }
    spec0.stco = offsets.clone();
    spec0.co64 = spec.co64.as_ref().map(|_| offsets.iter().map(|o| *o as u64).collect());
    let moov = bx("moov", &[track(&spec0), bx("mvhd", &[])].concat());
    let mut out = ftyp;
    out.extend(moov);
    out.extend(bx("mdat", &data));
    out
}

fn expect(sample: &Sample, offset: usize, size: usize, decode: u64, presentation: i64, sync: bool) {
    assert_eq!(sample.offset, offset, "offset");
    assert_eq!(sample.size, size, "size");
    assert_eq!(sample.decode_time, decode, "decode time");
    assert_eq!(sample.presentation_time, presentation, "presentation time");
    assert_eq!(sample.sync, sync, "sync");
}

#[test]
fn video_track_samples() {
    let spec = TrackSpec {
        timescale: 1000,
        stts: vec![(2, 100), (3, 40)],
        ctts: Some((0, vec![(1, 50), (2, 0), (2, 10)])),
        stsc: vec![(1, 2), (2, 1), (3, 2)],
        sizes: vec![10, 20, 30, 40, 50],
        sample_count: 5,
        fixed_size: 0,
        stco: vec![0, 0, 0],
        co64: None,
        stss: Some(vec![1, 3, 5]),
    };
    let data = file_with_chunks(&spec, &[vec![0, 1], vec![2], vec![3, 4]]);
    let tracks = sample_tables(&data).expect("sample tables");
    assert_eq!(tracks.len(), 1);
    let t = &tracks[0];
    assert_eq!(t.handler, "vide");
    assert_eq!(t.timescale, 1000);
    assert_eq!(t.samples.len(), 5);
    // Chunk offsets: recompute from the file layout.
    let ftyp_len = 8 + 16;
    let moov_len = data.len() - ftyp_len - 8 - (10 + 20 + 30 + 40 + 50);
    let base = ftyp_len + moov_len + 8;
    expect(&t.samples[0], base, 10, 0, 50, true);
    expect(&t.samples[1], base + 10, 20, 100, 100, false);
    expect(&t.samples[2], base + 30, 30, 200, 200, true);
    expect(&t.samples[3], base + 60, 40, 240, 250, false);
    expect(&t.samples[4], base + 100, 50, 280, 290, true);
    for (i, s) in t.samples.iter().enumerate() {
        assert!(data[s.offset..s.offset + s.size].iter().all(|b| *b == (i + 1) as u8));
    }
}

#[test]
fn co64_constant_size_and_defaults() {
    let spec = TrackSpec {
        timescale: 90000,
        stts: vec![(3, 3000)],
        ctts: None,
        stsc: vec![(1, 3)],
        sizes: vec![],
        sample_count: 3,
        fixed_size: 7,
        stco: vec![],
        co64: Some(vec![0]),
        stss: None,
    };
    let data = file_with_chunks(&spec, &[vec![0, 1, 2]]);
    let tracks = sample_tables(&data).expect("sample tables");
    let t = &tracks[0];
    assert_eq!(t.samples.len(), 3);
    assert!(t.samples.iter().all(|s| s.size == 7));
    assert_eq!(
        t.samples.iter().map(|s| s.decode_time).collect::<Vec<_>>(),
        vec![0, 3000, 6000]
    );
    assert!(t.samples.iter().all(|s| s.presentation_time == s.decode_time as i64));
    assert!(t.samples.iter().all(|s| s.sync));
}

#[test]
fn ctts_v1_signed_offsets() {
    let spec = TrackSpec {
        timescale: 1000,
        stts: vec![(2, 100)],
        ctts: Some((1, vec![(1, -10), (1, 25)])),
        stsc: vec![(1, 2)],
        sizes: vec![5, 5],
        sample_count: 2,
        fixed_size: 0,
        stco: vec![0],
        co64: None,
        stss: None,
    };
    let data = file_with_chunks(&spec, &[vec![0, 1]]);
    let t = &sample_tables(&data).expect("sample tables")[0];
    assert_eq!(t.samples[0].presentation_time, -10);
    assert_eq!(t.samples[1].presentation_time, 125);
}

#[test]
fn empty_track_is_ok() {
    let spec = TrackSpec {
        timescale: 1000,
        stts: vec![],
        ctts: None,
        stsc: vec![],
        sizes: vec![],
        sample_count: 0,
        fixed_size: 0,
        stco: vec![],
        co64: None,
        stss: None,
    };
    let data = file_with_chunks(&spec, &[]);
    let t = &sample_tables(&data).expect("sample tables")[0];
    assert!(t.samples.is_empty());
}

/// Real H.264 file (ffmpeg testsrc, 16x16, 5 frames, 1984 bytes), cross-checked
/// against ffprobe: same packet count, positions, sizes and sync flags.
/// ffprobe's dts/pts are uniformly shifted by -4096 (edit list); the raw
/// sample-table times below are unshifted, with identical deltas and ctts
/// offsets, which is what frame<->time conversion and remux need.
#[test]
fn real_file_matches_ffprobe() {
    use base64::Engine;
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64"),
    )
    .unwrap();
    let data = base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap();
    let tracks = sample_tables(&data).expect("sample tables");
    assert_eq!(tracks.len(), 1);
    let t = &tracks[0];
    assert_eq!(t.handler, "vide");
    assert_eq!(t.timescale, 10240);
    assert_eq!(t.samples.len(), 5);
    let want: [(usize, usize, u64, i64, bool); 5] = [
        (48, 898, 0, 4096, true),
        (946, 39, 2048, 6144, false),
        (985, 51, 4096, 8192, false),
        (1036, 50, 6144, 10240, false),
        (1086, 45, 8192, 12288, false),
    ];
    for (s, (offset, size, decode, presentation, sync)) in t.samples.iter().zip(want) {
        assert_eq!(s.offset, offset);
        assert_eq!(s.size, size);
        assert_eq!(s.decode_time, decode);
        assert_eq!(s.presentation_time, presentation);
        assert_eq!(s.sync, sync);
    }
}

#[test]
fn inconsistent_counts_are_rejected() {
    // stts covers 4 samples but stsz declares 5.
    let spec = TrackSpec {
        timescale: 1000,
        stts: vec![(4, 10)],
        ctts: None,
        stsc: vec![(1, 5)],
        sizes: vec![1, 1, 1, 1, 1],
        sample_count: 5,
        fixed_size: 0,
        stco: vec![0],
        co64: None,
        stss: None,
    };
    let data = file_with_chunks(&spec, &[vec![0, 1, 2, 3, 4]]);
    assert!(sample_tables(&data).is_err());
    // stsc starts at chunk 2, not 1.
    let spec = TrackSpec {
        timescale: 1000,
        stts: vec![(1, 10)],
        ctts: None,
        stsc: vec![(2, 1)],
        sizes: vec![1],
        sample_count: 1,
        fixed_size: 0,
        stco: vec![0],
        co64: None,
        stss: None,
    };
    let data = file_with_chunks(&spec, &[vec![0]]);
    assert!(sample_tables(&data).is_err());
}
