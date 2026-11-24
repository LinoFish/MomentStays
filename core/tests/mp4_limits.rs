//! Resource limits of the sample-table reader: hostile counts are refused
//! before they size any allocation.

mod common;

use common::{bx, full};
use momentstays_core::mp4;

fn be(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

/// One video track of `n` samples of `size` bytes each, one chunk, stsz in
/// the fixed-size form (so a small file can claim many samples).
fn mp4_with(n: u32, size: u32) -> Vec<u8> {
    mp4_custom(n, size, be(&[1, 1, n, 1]), 1, None)
}

/// [`mp4_with`] with explicit stsc rows (`entry_count` + triples, as
/// bytes), `chunks` stco entries (all pointing at the first sample) and an
/// optional `edts` box inside the trak.
fn mp4_custom(
    n: u32,
    size: u32,
    stsc_body: Vec<u8>,
    chunks: u32,
    edts: Option<Vec<u8>>,
) -> Vec<u8> {
    let ftyp = bx("ftyp", b"isom\0\0\0\0isom");
    let trak = |chunk_offset: u32| {
        let tkhd = full("tkhd", 0, &[vec![0u8; 80]].concat());
        let mut mdhd = vec![0u8; 20];
        mdhd[8..12].copy_from_slice(&1000u32.to_be_bytes());
        let hdlr = full("hdlr", 0, &[&[0u8; 4][..], b"vide", &[0u8; 13]].concat());
        let stsd = full("stsd", 0, &[be(&[1]), bx("avc1", &[0u8; 78])].concat());
        let stts = full("stts", 0, &be(&[1, n, 1]));
        let stsc = full("stsc", 0, &stsc_body);
        let stsz = full("stsz", 0, &be(&[size, n]));
        let mut stco_body = be(&[chunks]);
        for _ in 0..chunks {
            stco_body.extend(be(&[chunk_offset]));
        }
        let stco = full("stco", 0, &stco_body);
        let stbl = bx("stbl", &[stsd, stts, stsc, stsz, stco].concat());
        let dinf = bx(
            "dinf",
            &full("dref", 0, &[be(&[1]), full("url ", 0, &[])].concat()),
        );
        let minf = bx("minf", &[full("vmhd", 0, &[0u8; 8]), dinf, stbl].concat());
        bx(
            "trak",
            &[
                tkhd,
                edts.clone().unwrap_or_default(),
                bx("mdia", &[full("mdhd", 0, &mdhd), hdlr, minf].concat()),
            ]
            .concat(),
        )
    };
    let mut mvhd = vec![0u8; 96];
    mvhd[8..12].copy_from_slice(&1000u32.to_be_bytes());
    let moov = |off: u32| bx("moov", &[full("mvhd", 0, &mvhd), trak(off)].concat());
    let off = (ftyp.len() + moov(0).len() + 8) as u32;
    let mut out = ftyp;
    out.extend(moov(off));
    out.extend(bx("mdat", &vec![0u8; (n as usize) * (size as usize)]));
    out
}

#[test]
fn the_sample_budget_is_exact() {
    let ok = mp4::sample_tables(&mp4_with(1_000_000, 1)).expect("1,000,000 samples fit");
    assert_eq!(ok[0].samples.len(), 1_000_000);
    let err = mp4::sample_tables(&mp4_with(1_000_001, 1)).unwrap_err();
    assert!(err.0.contains("安全资源限制"), "{}", err.0);
    assert_eq!(err.kind(), momentstays_core::ErrorKind::Unsupported);
}

#[test]
fn a_tiny_file_cannot_claim_a_huge_fixed_size_table() {
    // 20 MB of mdat claiming 20 million one-byte samples used to allocate
    // ~1.8 GB before failing.
    let started = std::time::Instant::now();
    let err = mp4::sample_tables(&mp4_with(20_000_000, 1)).unwrap_err();
    assert!(err.0.contains("安全资源限制"), "{}", err.0);
    assert!(started.elapsed().as_secs() < 5);
}

#[test]
fn ordinary_clips_are_unaffected() {
    let t = mp4::sample_tables(&mp4_with(300, 1000)).unwrap();
    assert_eq!(t[0].samples.len(), 300);
}

#[test]
fn a_huge_later_stsc_row_is_refused_without_expanding_the_earlier_one() {
    // Two chunks; the second stsc row claims first_chunk 0xFFFFFFF0. The
    // first row used to expand to ~4 billion chunk entries (tens of GB)
    // before the second row's bounds check ran.
    let stsc = be(&[2, 1, 1, 1, 0xFFFF_FFF0, 1, 1]);
    let started = std::time::Instant::now();
    let err = mp4::sample_tables(&mp4_custom(2, 1, stsc, 2, None)).unwrap_err();
    assert!(err.0.contains("块映射越界"), "{}", err.0);
    assert!(started.elapsed().as_secs() < 2);
    // The well-formed two-chunk form still reads.
    let ok = mp4::sample_tables(&mp4_custom(2, 1, be(&[1, 1, 1, 1]), 2, None)).unwrap();
    assert_eq!(ok[0].chunks.len(), 2);
}

fn elst(version: u8, rows: &[(u64, i64)]) -> Vec<u8> {
    let mut body = be(&[rows.len() as u32]);
    for (dur, media) in rows {
        if version == 1 {
            body.extend(dur.to_be_bytes());
            body.extend(media.to_be_bytes());
        } else {
            body.extend((*dur as u32).to_be_bytes());
            body.extend((*media as i32).to_be_bytes());
        }
        body.extend([0, 1, 0, 0]);
    }
    bx("edts", &full("elst", version, &body))
}

#[test]
fn version_1_edit_lists_accept_empty_edits() {
    // An empty edit (media_time -1) in a 64-bit elst used to be refused as
    // an out-of-range offset, so such movies could not be converted.
    for version in [0u8, 1] {
        let edts = elst(version, &[(500, -1), (300, 0)]);
        let d = mp4_custom(300, 1000, be(&[1, 1, 300, 1]), 1, Some(edts));
        let movie = mp4::Movie::parse(&d).unwrap();
        let edits = mp4::edit_list(&d, &movie.tracks[0].trak).unwrap();
        assert_eq!(edits, Some(vec![(500, -1), (300, 0)]), "elst v{version}");
        momentstays_core::remux::remux_to_mov(&d, 0).unwrap();
    }
}

#[test]
fn unknown_edit_list_versions_are_refused() {
    let mut edts = elst(0, &[(300, 0)]);
    edts[16] = 2; // elst version byte (edts header 8 + elst header 8)
    let d = mp4_custom(300, 1000, be(&[1, 1, 300, 1]), 1, Some(edts));
    let movie = mp4::Movie::parse(&d).unwrap();
    let err = mp4::edit_list(&d, &movie.tracks[0].trak).unwrap_err();
    assert_eq!(err.kind(), momentstays_core::ErrorKind::Unsupported);
    assert!(momentstays_core::remux::remux_to_mov(&d, 0).is_err());
}
