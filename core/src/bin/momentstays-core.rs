//! momentstays-core CLI: inspect / extract Motion Photos on any platform.
use std::{env, fs, process::ExitCode};

fn usage() -> ExitCode {
    eprintln!("usage:\n  momentstays-core inspect <file>...\n  momentstays-core extract <file> --still <out> --video <out>");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("inspect") if args.len() >= 2 => {
            let mut code = ExitCode::SUCCESS;
            for path in &args[1..] {
                let mut v = match fs::read(path) {
                    Ok(d) => momentstays_core::inspect(&d),
                    Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
                };
                if v["ok"] != true { code = ExitCode::from(1); }
                v["path"] = serde_json::Value::String(path.clone());
                println!("{v}");
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
            let data = match fs::read(&args[1]) { Ok(d) => d, Err(e) => { eprintln!("{e}"); return ExitCode::from(1) } };
            match momentstays_core::parse(&data) {
                Ok(p) => {
                    if let Err(e) = fs::write(still, &p.still).and_then(|_| fs::write(video, p.video(&data))) { eprintln!("{e}"); return ExitCode::from(1); }
                    ExitCode::SUCCESS
                }
                Err(e) => { eprintln!("{e}"); ExitCode::from(1) }
            }
        }
        _ => usage(),
    }
}
