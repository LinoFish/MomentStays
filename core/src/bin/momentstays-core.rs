//! momentstays-core CLI: inspect / extract / bench Motion Photos on any platform.
//! Inputs are memory-mapped (no whole-file copy); `inspect` runs files in parallel
//! and prints JSON Lines in input order.
use momentstays_core::files::{self, Found, Mapped};
use momentstays_core::output;
use std::{env, io::Write, path::PathBuf, process::ExitCode, time::Instant};

fn usage() -> ExitCode {
    eprintln!(concat!(
        "usage:\n  momentstays-core inspect [-j N] <file|dir>...\n",
        "  momentstays-core extract <file> --still <out> --video <out>\n",
        "  momentstays-core bench [<file|dir>...]   (no args: synthetic 12/50/200 MB)"
    ));
    ExitCode::from(2)
}

/// Explicit files are taken as given; directories are walked recursively
/// for JPEG/HEIF files, like `momentstays-convert scan`.
fn expand(args: &[String]) -> Vec<Found> {
    let mut out = Vec::new();
    for a in args {
        let p = PathBuf::from(a);
        if p.is_dir() {
            out.extend(files::walk(&p));
        } else {
            out.push(Found::File(p));
        }
    }
    out
}

fn inspect_one(found: &Found) -> serde_json::Value {
    let mut v = match found {
        Found::File(p) => match Mapped::open(p) {
            Ok(i) => momentstays_core::inspect(i.bytes()),
            Err(e) => serde_json::json!({"ok": false, "error": e.to_string(), "error_kind": "io"}),
        },
        Found::Unreadable(_, e) => serde_json::json!({"ok": false, "error": e, "error_kind": "io"}),
    };
    v["path"] = serde_json::Value::String(found.path().display().to_string());
    v
}

fn bench_data(label: &str, d: &[u8], out: &mut Vec<serde_json::Value>) {
    let mb = d.len() as f64 / 1048576.0;
    let runs = if mb > 100.0 { 3 } else { 10 };
    let time = |f: &dyn Fn()| {
        f();
        let t = Instant::now();
        for _ in 0..runs {
            f();
        }
        t.elapsed().as_secs_f64() / runs as f64
    };
    let parse = time(&|| {
        momentstays_core::parse(d).unwrap();
    });
    let inspect = time(&|| {
        std::hint::black_box(momentstays_core::inspect(d));
    });
    out.push(
        serde_json::json!({"input": label, "mb": (mb * 10.0).round() / 10.0,
        "parse_ms": (parse * 1e4).round() / 10.0, "parse_mb_s": (mb / parse).round(),
        "inspect_ms": (inspect * 1e4).round() / 10.0, "inspect_mb_s": (mb / inspect).round()}),
    );
}

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("inspect") if args.len() >= 2 => {
            let mut jobs = files::default_jobs();
            if args.get(1).map(String::as_str) == Some("-j") {
                jobs = match args.get(2).and_then(|n| n.parse().ok()) {
                    Some(n) => n,
                    None => return usage(),
                };
                args.drain(1..3);
            }
            let found = expand(&args[1..]);
            let results = files::parallel_map(found.len(), jobs, |i| inspect_one(&found[i]));
            let mut out = std::io::BufWriter::new(std::io::stdout().lock());
            let mut code = ExitCode::SUCCESS;
            for v in results {
                if v["ok"] != true {
                    code = ExitCode::from(1);
                }
                let _ = writeln!(out, "{v}");
            }
            code
        }
        Some("extract") if args.len() == 6 => {
            let (mut still, mut video) = (None, None);
            let mut i = 2;
            while i + 1 < args.len() {
                match args[i].as_str() {
                    "--still" => still = Some(&args[i + 1]),
                    "--video" => video = Some(&args[i + 1]),
                    _ => return usage(),
                }
                i += 2;
            }
            let (Some(still), Some(video)) = (still, video) else {
                return usage();
            };
            let input = match Mapped::open(&PathBuf::from(&args[1])) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::from(1);
                }
            };
            let data = input.bytes();
            let src = PathBuf::from(&args[1]);
            match momentstays_core::parse(data) {
                Ok(p) => {
                    let write = |path: &String, bytes: &[u8]| {
                        let path = std::path::Path::new(path);
                        output::ensure_distinct(&[&src], path)?;
                        let parts = [bytes];
                        output::write_atomic(path, &parts, output::same_bytes(&parts))
                    };
                    if let Err(e) = write(still, &p.still).and_then(|_| write(video, p.video(data)))
                    {
                        eprintln!("{e}");
                        return ExitCode::from(1);
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::from(1)
                }
            }
        }
        Some("bench") => {
            let mut rows = Vec::new();
            if args.len() == 1 {
                for (s, v) in [(4, 8), (10, 40), (12, 188)] {
                    let d = momentstays_core::bench::google_motion_photo(s, v);
                    bench_data(
                        &format!("synthetic still {s} MB + video {v} MB"),
                        &d,
                        &mut rows,
                    );
                }
            } else {
                for f in expand(&args[1..]) {
                    let Found::File(f) = f else { continue };
                    if let Ok(i) = Mapped::open(&f) {
                        if momentstays_core::parse(i.bytes()).is_ok() {
                            bench_data(&f.display().to_string(), i.bytes(), &mut rows);
                        }
                    }
                }
            }
            for r in rows {
                println!("{r}");
            }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}
