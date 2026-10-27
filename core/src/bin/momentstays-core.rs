//! momentstays-core CLI: inspect / extract / bench Motion Photos on any platform.
//! Inputs are memory-mapped (no whole-file copy); `inspect` runs files in parallel
//! and prints JSON Lines in input order.
use std::{env, fs, io::Write, path::PathBuf, process::ExitCode, sync::atomic::{AtomicUsize, Ordering}, time::Instant};

fn usage() -> ExitCode {
    eprintln!(concat!("usage:\n  momentstays-core inspect [-j N] <file|dir>...\n",
        "  momentstays-core extract <file> --still <out> --video <out>\n",
        "  momentstays-core bench [<file|dir>...]   (no args: synthetic 12/50/200 MB)"));
    ExitCode::from(2)
}

/// Read-only map of a file. Empty files map to an empty slice.
struct Input { map: Option<memmap2::Mmap> }
impl Input {
    fn open(path: &PathBuf) -> std::io::Result<Self> {
        let f = fs::File::open(path)?;
        if f.metadata()?.len() == 0 { return Ok(Input { map: None }); }
        // SAFETY: read-only map; a file truncated underneath us is an accepted CLI risk.
        Ok(Input { map: Some(unsafe { memmap2::Mmap::map(&f)? }) })
    }
    fn bytes(&self) -> &[u8] { self.map.as_deref().unwrap_or(&[]) }
}

fn expand(args: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for a in args {
        let p = PathBuf::from(a);
        if p.is_dir() {
            let mut files: Vec<PathBuf> = fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
            files.sort();
            out.extend(files);
        } else { out.push(p); }
    }
    out
}

fn inspect_one(path: &PathBuf) -> serde_json::Value {
    let mut v = match Input::open(path) {
        Ok(i) => momentstays_core::inspect(i.bytes()),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    };
    v["path"] = serde_json::Value::String(path.display().to_string());
    v
}

fn parallel<T: Send>(n: usize, jobs: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let mut slots: Vec<Option<T>> = (0..n).map(|_| None).collect();
    let chunks = std::sync::Mutex::new(&mut slots);
    std::thread::scope(|s| {
        for _ in 0..jobs.max(1).min(n.max(1)) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= n { break; }
                let r = f(i);
                chunks.lock().unwrap()[i] = Some(r);
            });
        }
    });
    slots.into_iter().map(|x| x.unwrap()).collect()
}

fn jobs_default() -> usize { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) }

fn bench_data(label: &str, d: &[u8], out: &mut Vec<serde_json::Value>) {
    let mb = d.len() as f64 / 1048576.0;
    let runs = if mb > 100.0 { 3 } else { 10 };
    let time = |f: &dyn Fn()| { f(); let t = Instant::now(); for _ in 0..runs { f(); } t.elapsed().as_secs_f64() / runs as f64 };
    let parse = time(&|| { momentstays_core::parse(d).unwrap(); });
    let inspect = time(&|| { std::hint::black_box(momentstays_core::inspect(d)); });
    out.push(serde_json::json!({"input": label, "mb": (mb * 10.0).round() / 10.0,
        "parse_ms": (parse * 1e4).round() / 10.0, "parse_mb_s": (mb / parse).round(),
        "inspect_ms": (inspect * 1e4).round() / 10.0, "inspect_mb_s": (mb / inspect).round()}));
}

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("inspect") if args.len() >= 2 => {
            let mut jobs = jobs_default();
            if args.get(1).map(String::as_str) == Some("-j") {
                jobs = match args.get(2).and_then(|n| n.parse().ok()) { Some(n) => n, None => return usage() };
                args.drain(1..3);
            }
            let files = expand(&args[1..]);
            let results = parallel(files.len(), jobs, |i| inspect_one(&files[i]));
            let mut out = std::io::BufWriter::new(std::io::stdout().lock());
            let mut code = ExitCode::SUCCESS;
            for v in results {
                if v["ok"] != true { code = ExitCode::from(1); }
                let _ = writeln!(out, "{v}");
            }
            code
        }
        Some("extract") if args.len() == 6 => {
            let (mut still, mut video) = (None, None);
            let mut i = 2;
            while i + 1 < args.len() {
                match args[i].as_str() { "--still" => still = Some(&args[i + 1]), "--video" => video = Some(&args[i + 1]), _ => return usage() }
                i += 2;
            }
            let (Some(still), Some(video)) = (still, video) else { return usage() };
            let input = match Input::open(&PathBuf::from(&args[1])) { Ok(d) => d, Err(e) => { eprintln!("{e}"); return ExitCode::from(1) } };
            let data = input.bytes();
            match momentstays_core::parse(data) {
                Ok(p) => {
                    if let Err(e) = fs::write(still, &p.still).and_then(|_| fs::write(video, p.video(data))) { eprintln!("{e}"); return ExitCode::from(1); }
                    ExitCode::SUCCESS
                }
                Err(e) => { eprintln!("{e}"); ExitCode::from(1) }
            }
        }
        Some("bench") => {
            let mut rows = Vec::new();
            if args.len() == 1 {
                for (s, v) in [(4, 8), (10, 40), (12, 188)] {
                    let d = momentstays_core::bench::google_motion_photo(s, v);
                    bench_data(&format!("synthetic still {s} MB + video {v} MB"), &d, &mut rows);
                }
            } else {
                for f in expand(&args[1..]) {
                    if let Ok(i) = Input::open(&f) {
                        if momentstays_core::parse(i.bytes()).is_ok() { bench_data(&f.display().to_string(), i.bytes(), &mut rows); }
                    }
                }
            }
            for r in rows { println!("{r}"); }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}
