//! No-panic sweep over the public byte-level entry points that
//! `fuzz_light.rs` does not cover (Apple HDR, OPPO UHDR, EXIF time, XMP,
//! live/still writers, cover and remux helpers, scan, recovery...). Same
//! mutation strategy (xorshift, flips/truncation/length pokes, stacked up to
//! three per input) over every fixture in `tests/fixtures-*` plus engine-made
//! Live pairs; every unique panic site is collected and reported at the end
//! instead of stopping at the first one. `FUZZ_ITERS=5000` runs deeper.

mod common;

use base64::Engine;
use momentstays_core::*;
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Mutex, OnceLock};

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

fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut v = seed.to_vec();
    let rounds = 1 + rng.below(3);
    for _ in 0..rounds {
        match rng.below(6) {
            0 => {
                for _ in 0..1 + rng.below(8) {
                    if v.is_empty() {
                        break;
                    }
                    let i = rng.below(v.len());
                    v[i] = rng.next() as u8;
                }
            }
            1 => {
                let n = rng.below(v.len() + 1);
                v.truncate(n);
            }
            2 => {
                for _ in 0..rng.below(65) {
                    v.push(rng.next() as u8);
                }
            }
            3 => {
                if v.len() >= 4 {
                    let i = rng.below(v.len() - 3);
                    let w = match rng.below(4) {
                        0 => 0,
                        1 => 0xFFFF_FFFF,
                        2 => 1,
                        _ => rng.next() as u32,
                    };
                    if rng.below(2) == 0 {
                        v[i..i + 4].copy_from_slice(&w.to_be_bytes());
                    } else {
                        v[i..i + 2].copy_from_slice(&(w as u16).to_be_bytes());
                    }
                }
            }
            4 => {
                if v.len() >= 4 {
                    let a = rng.below(v.len() - 3);
                    let len = 1 + rng.below((v.len() - a).min(64));
                    let chunk = v[a..a + len].to_vec();
                    let at = rng.below(v.len() + 1);
                    v.splice(at..at, chunk);
                }
            }
            _ => {
                if !v.is_empty() {
                    let a = rng.below(v.len());
                    let len = rng.below((v.len() - a).min(128) + 1);
                    v[a..a + len].fill(0);
                }
            }
        }
    }
    v
}

fn b64(path: &str) -> Vec<u8> {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    let text = std::fs::read_to_string(p).unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(text.trim_ascii())
        .unwrap()
}

fn seeds() -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    for name in [
        "base",
        "extended-motion",
        "extended",
        "legacy-mpf",
        "motion-element",
        "motion-legacy",
        "motion-notime",
        "motion",
        "srgb",
        "static",
        "ultra",
    ] {
        out.push((
            name.to_string(),
            b64(&format!("tests/fixtures-pycli/{name}.jpg.b64")),
        ));
    }
    out.push((
        "heic-regress".into(),
        b64("tests/fixtures-regress/heic-no-exif-scan-recursion.heic.b64"),
    ));
    let mp4 = b64("tests/fixtures-samples/real.mp4.b64");
    out.push(("real.mp4".into(), mp4.clone()));
    out.push(("bench".into(), bench::google_motion_photo(0, 0)));
    // A Live pair produced by the engine: MOV with identifier + marker track.
    let motion = bench::google_motion_photo(0, 0);
    if let Ok(p) = format::parse(&motion) {
        if let Ok(live) = live::from_motion(
            &p,
            &motion,
            "ABCDEF01-2345-6789-ABCD-EF0123456789",
            live::TimeDomain::Auto,
            0,
        ) {
            out.push(("live.mov".into(), live.movie));
            out.push(("live.jpg".into(), live.still));
        }
    }
    // HEIC seeds from the shared builders.
    out.push(("heic-still".into(), common::build_still(true, true)));
    out
}

type Target = (&'static str, Box<dyn Fn(&[u8])>);

fn run_targets(input: &[u8], hits: &Mutex<BTreeMap<String, String>>) {
    let targets: Vec<Target> = vec![
        ("inspect", Box::new(|d| drop(inspect(d)))),
        ("inspect_light", Box::new(|d| drop(inspect_light(d)))),
        ("coding_hash", Box::new(|d| drop(format::coding_hash(d)))),
        (
            "auxiliary_hashes",
            Box::new(|d| drop(format::auxiliary_hashes(d))),
        ),
        ("apple_hdr::read", Box::new(|d| drop(apple_hdr::read(d)))),
        (
            "apple_hdr::legacy",
            Box::new(|d| drop(apple_hdr::legacy(d))),
        ),
        ("heic_gain::read", Box::new(|d| drop(heic_gain::read(d)))),
        ("oppo_uhdr::read", Box::new(|d| drop(oppo_uhdr::read(d)))),
        (
            "exif_time",
            Box::new(|d| {
                let _ = exif_time::capture_time_from_bytes(d);
            }),
        ),
        ("xmp::packets", Box::new(|d| drop(xmp::packets(d)))),
        (
            "pymotion::xmp_packets",
            Box::new(|d| drop(pymotion::xmp_packets(d))),
        ),
        (
            "live_still",
            Box::new(|d| {
                drop(pymotion::live_still(
                    d,
                    "ABCDEF01-2345-6789-ABCD-EF0123456789",
                ))
            }),
        ),
        (
            "live_still_clean",
            Box::new(|d| drop(pymotion::live_still_clean(d))),
        ),
        ("jpeg::auxiliary", Box::new(|d| drop(jpeg::auxiliary(d)))),
        (
            "jpeg::image_ranges",
            Box::new(|d| drop(jpeg::image_ranges(d))),
        ),
        (
            "jpeg::static_only",
            Box::new(|d| drop(jpeg::static_only(d))),
        ),
        (
            "jpeg::coding_hash",
            Box::new(|d| drop(jpeg::coding_hash(d))),
        ),
        ("mpf_images", Box::new(|d| drop(pyjpeg::mpf_images(d)))),
        ("heif::exif", Box::new(|d| drop(heif::exif(d)))),
        (
            "heif::coding_hash",
            Box::new(|d| drop(heif::coding_hash(d))),
        ),
        (
            "heif::layout_lenient",
            Box::new(|d| drop(heif::layout_lenient(d))),
        ),
        (
            "still_content_identifier",
            Box::new(|d| drop(live::still_content_identifier(d))),
        ),
        (
            "recovery",
            Box::new(|d| drop(recovery::prepare_conversion(d))),
        ),
        (
            "scan::identify",
            Box::new(|d| {
                drop(scan::identify(d, "jpeg"));
                drop(scan::identify(d, "heic"));
            }),
        ),
        (
            "scan::classify_hdr",
            Box::new(|d| {
                drop(scan::classify_hdr(d, true));
                drop(scan::classify_hdr(d, false));
            }),
        ),
        (
            "scan::confirmed_static",
            Box::new(|d| {
                let _ = scan::confirmed_static(d);
            }),
        ),
        ("scan::has_iso", Box::new(|d| drop(scan::has_iso(d)))),
        ("make_model", Box::new(|d| drop(vendor::make_model(d)))),
        ("image_times", Box::new(|d| drop(mp4::image_times(d)))),
        (
            "movie",
            Box::new(|d| {
                if let Ok(m) = mp4::Movie::parse(d) {
                    drop(remux::audio_codecs(d, &m));
                    drop(remux::remux_mp4(d, &m));
                    drop(live2motion::mett_cover_time_in(d, &m));
                    drop(live::resolve_cover_in(
                        d,
                        &m,
                        model::Cover::Time(500_000),
                        live::TimeDomain::Auto,
                        0,
                    ));
                }
            }),
        ),
        (
            "cover",
            Box::new(|d| {
                if let Ok(t) = mp4::sample_tables(d) {
                    if let Ok(v) = cover::video_track(&t) {
                        drop(cover::frame_to_time_us(v, 3));
                        drop(cover::time_us_to_frame(v, 500_000));
                    }
                }
            }),
        ),
        (
            "read_content_identifier",
            Box::new(|d| drop(remux::read_content_identifier(d))),
        ),
        (
            "kept_handlers",
            Box::new(|d| drop(remux::kept_handlers(d, 0))),
        ),
        (
            "top_level_types",
            Box::new(|d| drop(mp4::top_level_types(d, ""))),
        ),
        (
            "from_motion",
            Box::new(|d| {
                if let Ok(p) = format::parse(d) {
                    drop(live::from_motion(
                        &p,
                        d,
                        "ABCDEF01-2345-6789-ABCD-EF0123456789",
                        live::TimeDomain::Auto,
                        0,
                    ));
                }
            }),
        ),
        (
            "live2motion::prepare",
            Box::new(|d| {
                static MOV: OnceLock<Vec<u8>> = OnceLock::new();
                static STILL: OnceLock<Vec<u8>> = OnceLock::new();
                let mov = MOV.get_or_init(|| b64("tests/fixtures-samples/real.mp4.b64"));
                let still = STILL.get_or_init(|| b64("tests/fixtures-pycli/static.jpg.b64"));
                drop(live2motion::prepare(d, mov));
                drop(live2motion::prepare(still, d));
            }),
        ),
        (
            "heifwrite",
            Box::new(|d| {
                drop(heifwrite::write_xmp(d, b"<x/>"));
                drop(heifwrite::plan_heic(
                    d,
                    b"\0\0\0\x08free",
                    model::Vendor::Google,
                    0,
                ));
            }),
        ),
        (
            "apple_note",
            Box::new(|d| drop(pyjpeg::apple_note(d, &[(17, "ABC")]))),
        ),
        (
            "hdr",
            Box::new(|d| {
                let end = jpeg::image_end(d).unwrap_or(d.len() / 2);
                drop(hdr::honor_ehdr(d, end));
                drop(hdr::honor_private(d, end));
                drop(hdr::known_ultra_version(d));
                drop(vendor::locate(d, end));
            }),
        ),
        ("scan_row", Box::new(|d| drop(scan::scan_row(d)))),
    ];
    for (name, f) in &targets {
        if let Err(e) = catch_unwind(AssertUnwindSafe(|| f(input))) {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            let loc = LAST.lock().unwrap().clone();
            let mut h = hits.lock().unwrap();
            h.entry(format!("{name} @ {loc}")).or_insert_with(|| {
                format!(
                    "{msg} | len={} head={:02x?}",
                    input.len(),
                    &input[..input.len().min(16)]
                )
            });
        }
    }
}

static LAST: Mutex<String> = Mutex::new(String::new());

#[test]
fn no_panic_on_entry_points() {
    let iters: usize = std::env::var("FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        *LAST.lock().unwrap() = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
    }));
    let hits = Mutex::new(BTreeMap::new());
    let seeds = seeds();
    let mut rng = Rng(0x0dd_ba11_cafe_f00d);
    for (_, seed) in &seeds {
        run_targets(seed, &hits);
        for _ in 0..iters {
            let input = mutate(&mut rng, seed);
            run_targets(&input, &hits);
        }
    }
    std::panic::set_hook(prev);
    let hits = hits.into_inner().unwrap();
    for (k, v) in &hits {
        eprintln!("PANIC {k}: {v}");
    }
    assert!(hits.is_empty(), "{} panic sites", hits.len());
}
