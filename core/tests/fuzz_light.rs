//! Lightweight deterministic fuzzer (ROADMAP item 5, stable-toolchain
//! edition): seeded mutations over small valid seeds, run against
//! `format::parse`, `pyjpeg::segments` and `heif::layout`. The only
//! property asserted here is no-panic: every input must return `Ok`
//! or `Err`, never unwind. A failure prints target + seed + iteration
//! so the input regenerates deterministically from the fixed seed.
//!
//! A second test differentially checks mutated MP4s against ffprobe,
//! but only on the sound subset: inputs ffprobe reads completely
//! cleanly (exit 0 AND empty stderr with `-v error`) AND discovers
//! all of the seed's tracks. On those, our per-track sample counts
//! must equal ffprobe's per-stream packet counts in order — same
//! tables, same packets. Everything else is skipped, not asserted:
//! exit-code comparison in either direction proved unsound during
//! development (ffprobe rejects table-valid files for corrupt
//! bitstreams — seed iter 10, a byte flip inside mdat — and silently
//! drops broken tracks while still reading the rest cleanly, whereas
//! our exact-enumeration parser correctly fails the whole file
//! rather than silently dropping a track). The test skips itself when
//! ffprobe is unavailable.

mod common;

use base64::Engine;
use common::{build_still_spec, bx, exif_item, motion_xmp, HeicItem, HeicStillSpec};
use momentstays_core::format;
use momentstays_core::heif;
use momentstays_core::mp4;
use momentstays_core::pyjpeg;
use momentstays_core::pymotion::XMP_HDR;
use momentstays_core::remux::{remux_to_mov, remux_to_mp4};
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// xorshift64: small, deterministic, no new dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn real_mp4() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64");
    let text = fs::read_to_string(path).unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, marker];
    v.extend(((payload.len() + 2) as u16).to_be_bytes());
    v.extend(payload);
    v
}

/// A plausible Google Motion Photo JPEG: SOI, Exif, motion XMP with a
/// Container directory, SOS, one scan byte, EOI, then the video bytes.
fn jpeg_seed() -> Vec<u8> {
    let video = real_mp4();
    let xml = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
        <rdf:Description rdf:about=\"\" xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\" \
        xmlns:Container=\"http://ns.google.com/photos/1.0/container/\" \
        xmlns:Item=\"http://ns.google.com/photos/1.0/container/item/\" \
        GCamera:MotionPhoto=\"1\" GCamera:MotionPhotoVersion=\"1\" \
        GCamera:MotionPhotoPresentationTimestampUs=\"1420000\">\
        <Container:Directory><rdf:Seq>\
        <rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"Primary\" Item:Mime=\"image/jpeg\" Item:Length=\"0\"/></rdf:li>\
        <rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"MotionPhoto\" Item:Mime=\"video/mp4\" Item:Length=\"{}\"/></rdf:li>\
        </rdf:Seq></Container:Directory></rdf:Description></rdf:RDF></x:xmpmeta>",
        video.len()
    );
    let mut xmp = XMP_HDR.to_vec();
    xmp.extend(xml.as_bytes());
    let mut tiff = b"MM".to_vec();
    tiff.extend([0u8, 42]);
    tiff.extend(8u32.to_be_bytes());
    tiff.extend(0u16.to_be_bytes());
    tiff.extend(0u32.to_be_bytes());
    let mut exif = pyjpeg::EXIF_HDR.to_vec();
    exif.extend(tiff);
    let mut v = vec![0xFF, 0xD8];
    v.extend(seg(0xE1, &exif));
    v.extend(seg(0xE1, &xmp));
    v.extend(seg(0xDA, &[0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]));
    v.push(0x42);
    v.extend([0xFF, 0xD9]);
    v.extend(&video);
    v
}

fn heic_still_seed() -> Vec<u8> {
    let spec = HeicStillSpec { iinf_version: 0, infe_version: 2, iloc_version: 0, offset_width: 4, length_width: 4, base_width: 0, base: 0, index_width: 0, with_iref: true };
    let items = vec![
        HeicItem { id: 1, ty: "hvc1", data: b"fake-hvc1-coding-data-0123456789abcdef".to_vec(), method: 0, extents: 1 },
        HeicItem { id: 2, ty: "Exif", data: exif_item(), method: 0, extents: 1 },
        HeicItem { id: 3, ty: "mime", data: motion_xmp(1_420_000), method: 0, extents: 1 },
    ];
    build_still_spec(&spec, &items)
}

fn heic_motion_seed() -> Vec<u8> {
    let mut v = heic_still_seed();
    v.extend(bx("mpvd", &real_mp4()));
    v
}

fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut v = seed.to_vec();
    match rng.below(6) {
        // Random byte flips.
        0 => {
            let n = 1 + rng.below(8);
            for _ in 0..n {
                if v.is_empty() {
                    break;
                }
                let i = rng.below(v.len());
                v[i] = rng.next() as u8;
            }
        }
        // Truncation.
        1 => {
            let n = rng.below(v.len() + 1);
            v.truncate(n);
        }
        // Extension with random bytes.
        2 => {
            let n = rng.below(65);
            for _ in 0..n {
                v.push(rng.next() as u8);
            }
        }
        // Length-field poke: random u16/u32, big or little endian.
        3 => {
            if !v.is_empty() {
                let i = rng.below(v.len());
                if rng.below(2) == 0 && i + 2 <= v.len() {
                    v[i..i + 2].copy_from_slice(&(rng.next() as u16).to_be_bytes());
                } else if i + 4 <= v.len() {
                    let w = rng.next() as u32;
                    if rng.below(2) == 0 {
                        v[i..i + 4].copy_from_slice(&w.to_be_bytes());
                    } else {
                        v[i..i + 4].copy_from_slice(&w.to_le_bytes());
                    }
                }
            }
        }
        // Slice duplication.
        4 => {
            if v.len() >= 4 {
                let a = rng.below(v.len() - 3);
                let len = 1 + rng.below((v.len() - a).min(64));
                let chunk = v[a..a + len].to_vec();
                let at = rng.below(v.len() + 1);
                v.splice(at..at, chunk);
            }
        }
        // Zero a random range.
        _ => {
            if !v.is_empty() {
                let a = rng.below(v.len());
                let len = rng.below((v.len() - a).min(128) + 1);
                for b in &mut v[a..a + len] {
                    *b = 0;
                }
            }
        }
    }
    v
}

fn hex_head(v: &[u8]) -> String {
    v.iter().take(64).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join("")
}

fn run_no_panic(target: &str, seed_name: &str, seed: &[u8], iters: usize) {
    let mut rng = Rng(0x5eed_f00d_1234_5678);
    for i in 0..iters {
        let input = mutate(&mut rng, seed);
        let r = catch_unwind(AssertUnwindSafe(|| match target {
            "parse" => {
                let _ = format::parse(&input);
            }
            "segments" => {
                let _ = pyjpeg::segments(&input);
            }
            "layout" => {
                let _ = heif::layout(&input);
            }
            "tables" => {
                let _ = mp4::sample_tables(&input);
            }
            "remux_mp4" => {
                let _ = remux_to_mp4(&input);
            }
            "remux_mov" => {
                let _ = remux_to_mov(&input, 0);
            }
            _ => unreachable!(),
        }));
        if r.is_err() {
            panic!("fuzz panic: target={target} seed={seed_name} iter={i} len={} head={}", input.len(), hex_head(&input));
        }
    }
}

#[test]
fn no_panic_on_mutations() {
    let jpeg = jpeg_seed();
    let heic_still = heic_still_seed();
    let heic_motion = heic_motion_seed();
    let mp4 = real_mp4();
    // Seeds must exercise the success paths, not just early errors.
    assert!(format::parse(&jpeg).is_ok(), "jpeg seed parses");
    assert!(format::parse(&heic_motion).is_ok(), "heic motion seed parses");
    assert!(heif::layout(&heic_still).is_ok(), "heic still seed layouts");
    assert!(pyjpeg::segments(&jpeg).is_ok(), "jpeg seed segments");
    assert!(mp4::sample_tables(&mp4).is_ok(), "mp4 seed sample tables");
    run_no_panic("parse", "jpeg", &jpeg, 1500);
    run_no_panic("parse", "heic-motion", &heic_motion, 1500);
    run_no_panic("parse", "mp4", &mp4, 1500);
    run_no_panic("segments", "jpeg", &jpeg, 1500);
    run_no_panic("layout", "heic-still", &heic_still, 1500);
    run_no_panic("tables", "mp4", &mp4, 1500);
    run_no_panic("remux_mp4", "mp4", &mp4, 1500);
    run_no_panic("remux_mov", "mp4", &mp4, 1500);
}

/// ffprobe packet counts per stream, but only when ffprobe reads the
/// file completely cleanly (exit 0 and empty stderr). Returns None
/// for anything else (rejected, errored, timed out, unavailable):
/// those inputs are skipped, never asserted on.
fn ffprobe_clean_packet_counts(path: &std::path::Path, err_path: &std::path::Path) -> Option<Vec<usize>> {
    let err_file = fs::File::create(err_path).ok()?;
    let mut child: Child = Command::new("ffprobe")
        .args(["-v", "error", "-count_packets", "-show_entries", "stream=nb_read_packets", "-of", "csv=p=0"])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(err_file)
        .spawn()
        .ok()?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(10) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    };
    if !status.success() {
        return None;
    }
    if fs::read_to_string(err_path).map(|s| !s.trim().is_empty()).unwrap_or(true) {
        return None;
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read;
        stdout.read_to_string(&mut out).ok()?;
    }
    let _ = child.wait();
    let mut counts = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        counts.push(line.parse::<usize>().ok()?);
    }
    if counts.is_empty() {
        return None;
    }
    Some(counts)
}

/// Regression for fuzzer finds: the box walk tolerates damage after
/// the needed content (trailing dust, a bogus trailing header, a cut
/// udta tail, a corrupt size on a complete final box), while damage
/// to needed content still fails at content validation.
#[test]
fn trailing_damage_ignored_but_damaged_tables_still_fail() {
    let mp4 = real_mp4();
    let base: Vec<usize> = mp4::sample_tables(&mp4).unwrap().iter().map(|t| t.samples.len()).collect();
    for dust in 1..8 {
        let mut v = mp4.clone();
        v.extend(std::iter::repeat(0xAB).take(dust));
        let got: Vec<usize> = mp4::sample_tables(&v).unwrap().iter().map(|t| t.samples.len()).collect();
        assert_eq!(got, base, "{dust} trailing bytes ignored");
    }
    // 20 trailing bytes with a bogus huge size: ignored, not an error.
    let mut v = mp4.clone();
    v.extend(std::iter::repeat(0xAB).take(20));
    let got: Vec<usize> = mp4::sample_tables(&v).unwrap().iter().map(|t| t.samples.len()).collect();
    assert_eq!(got, base, "trailing bogus box ignored");
    // Cutting 31 bytes off the file (udta tail only, mirroring fuzz
    // iter 18): sample tables are intact, so parsing still succeeds.
    let mut cut = mp4.clone();
    cut.truncate(mp4.len() - 31);
    let got: Vec<usize> = mp4::sample_tables(&cut).unwrap().iter().map(|t| t.samples.len()).collect();
    assert_eq!(got, base, "cut udta tail still parses");
    // Cutting 100 bytes eats 2 bytes off stbl's last child through the
    // chain of clamps: a needed table is truncated, must fail.
    let mut cut2 = mp4.clone();
    cut2.truncate(mp4.len() - 100);
    assert!(mp4::sample_tables(&cut2).is_err(), "cut into stbl tail still fails");
    // A corrupt size on the complete final box: clamped to file end,
    // content is complete, so parsing still succeeds.
    let moov_at = mp4::boxes(&mp4, 0, mp4.len()).unwrap().into_iter().find(|b| b.is(b"moov")).unwrap().start;
    let mut bad = mp4.clone();
    bad[moov_at..moov_at + 4].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
    let got: Vec<usize> = mp4::sample_tables(&bad).unwrap().iter().map(|t| t.samples.len()).collect();
    assert_eq!(got, base, "overrunning final box size clamped");
    // Cutting deep into stbl: needed tables are gone, must fail.
    let mut deep = mp4.clone();
    deep.truncate(1600);
    assert!(mp4::sample_tables(&deep).is_err(), "cut into stbl still fails");
}

#[test]
fn mp4_mutations_agree_with_ffprobe() {
    // ffprobe absent -> skip, not fail: it is a differential oracle,
    // not part of the core.
    if Command::new("ffprobe").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_err() {
        eprintln!("ffprobe unavailable, skipping differential check");
        return;
    }
    let mp4 = real_mp4();
    // Sanity: the unmutated seed must agree exactly first.
    let seed_counts: Vec<usize> = mp4::sample_tables(&mp4).unwrap().iter().map(|t| t.samples.len()).collect();
    let seed_path = std::env::temp_dir().join(format!("momentstays-fuzz-seed-{}.mp4", std::process::id()));
    let seed_err = std::env::temp_dir().join(format!("momentstays-fuzz-seed-{}.err", std::process::id()));
    fs::write(&seed_path, &mp4).unwrap();
    let probe_seed = ffprobe_clean_packet_counts(&seed_path, &seed_err);
    let _ = fs::remove_file(&seed_path);
    let _ = fs::remove_file(&seed_err);
    assert_eq!(probe_seed.as_deref(), Some(seed_counts.as_slice()), "seed packet counts agree with ffprobe");
    let mut rng = Rng(0xd1ff_5eed_9876_5432);
    let mut checked = 0usize;
    let mut disagreements = Vec::new();
    for i in 0..25 {
        let input = mutate(&mut rng, &mp4);
        let path = std::env::temp_dir().join(format!("momentstays-fuzz-{}-{i}.mp4", std::process::id()));
        let err_path = std::env::temp_dir().join(format!("momentstays-fuzz-{}-{i}.err", std::process::id()));
        if fs::write(&path, &input).is_err() {
            continue;
        }
        let probe = ffprobe_clean_packet_counts(&path, &err_path);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&err_path);
        let Some(want) = probe else { continue };
        // Partial discovery (ffprobe dropped a broken track) is a
        // contract difference, not a disagreement: skip it.
        if want.len() != seed_counts.len() {
            continue;
        }
        checked += 1;
        let ours = catch_unwind(AssertUnwindSafe(|| mp4::sample_tables(&input)));
        match ours {
            Ok(Ok(tracks)) => {
                let got: Vec<usize> = tracks.iter().map(|t| t.samples.len()).collect();
                if got != want {
                    disagreements.push((i, got, want));
                }
            }
            Ok(Err(_)) => disagreements.push((i, vec![usize::MAX], want)),
            Err(_) => panic!("fuzz panic: sample_tables on mp4 iter={i} len={} head={}", input.len(), hex_head(&input)),
        }
    }
    assert!(disagreements.is_empty(), "packet-count disagreements on clean ffprobe reads: {disagreements:?} (checked {checked} of 25)");
}
