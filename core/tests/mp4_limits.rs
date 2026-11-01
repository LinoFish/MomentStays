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
    let ftyp = bx("ftyp", b"isom\0\0\0\0isom");
    let trak = |chunk_offset: u32| {
        let tkhd = full("tkhd", 0, &[vec![0u8; 80]].concat());
        let mut mdhd = vec![0u8; 20];
        mdhd[8..12].copy_from_slice(&1000u32.to_be_bytes());
        let hdlr = full("hdlr", 0, &[&[0u8; 4][..], b"vide", &[0u8; 13]].concat());
        let stsd = full("stsd", 0, &[be(&[1]), bx("avc1", &[0u8; 78])].concat());
        let stts = full("stts", 0, &be(&[1, n, 1]));
        let stsc = full("stsc", 0, &be(&[1, 1, n, 1]));
        let stsz = full("stsz", 0, &be(&[size, n]));
        let stco = full("stco", 0, &be(&[1, chunk_offset]));
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
