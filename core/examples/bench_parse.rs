//! In-process parse benchmark; file I/O and correctness hashes are not timed.
use std::{env, fs, hint::black_box, time::Instant};
use momentstays_core::{parse, sha256_hex};

fn main() {
    let args: Vec<_> = env::args().collect();
    let data = fs::read(&args[1]).unwrap();
    let owned = args.get(2).map(String::as_str) == Some("owned");
    let photo = match parse(&data) {
        Ok(p) => p,
        Err(e) => { println!("{}", serde_json::json!({"error":e.0})); return; }
    };
    let summary = serde_json::json!({"format":photo.format,"cover_frame":photo.cover_frame,
        "timestamp_us":photo.timestamp,"timestamp_tag":photo.timestamp_tag,"ultra":photo.ultra,
        "still_sha256":sha256_hex(&photo.still),"video_sha256":sha256_hex(photo.video(&data))});
    drop(photo);
    let work = || {
        let p = parse(black_box(&data)).unwrap();
        if owned { black_box(p.video(&data).to_vec()); }
        black_box(p);
    };
    for _ in 0..2 { work(); }
    let start = Instant::now(); work();
    let iterations = (0.01 / start.elapsed().as_secs_f64()).ceil().clamp(1.0, 100.0) as usize;
    let mut samples = Vec::new();
    for _ in 0..9 {
        let start = Instant::now();
        for _ in 0..iterations { work(); }
        samples.push(start.elapsed().as_secs_f64() * 1000.0 / iterations as f64);
    }
    println!("{}", serde_json::json!({"summary":summary,"ms":samples,"iterations":iterations}));
}
