//! `output::write_atomic` / `ensure_distinct`: outputs are replaced only by
//! complete, verified files, and an input is never overwritten in place.

use momentstays_core::output::{ensure_distinct, same_bytes, write_atomic};
use momentstays_core::Error;
use std::fs;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ms-output-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn leftovers(d: &PathBuf) -> Vec<String> {
    fs::read_dir(d)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

#[test]
fn parts_are_written_in_order_and_checked() {
    let d = dir("ok");
    let target = d.join("out.bin");
    let parts: [&[u8]; 3] = [b"head", b"-video-", b"tail"];
    let n = write_atomic(&target, &parts, same_bytes(&parts)).unwrap();
    assert_eq!(n, 15);
    assert_eq!(fs::read(&target).unwrap(), b"head-video-tail");
    assert!(leftovers(&d).is_empty());
}

#[test]
fn failed_check_keeps_the_existing_file_and_no_temp() {
    let d = dir("fail");
    let target = d.join("out.bin");
    fs::write(&target, b"original").unwrap();
    let parts: [&[u8]; 1] = [b"new contents"];
    let err = write_atomic(&target, &parts, |_| Err(Error::new("boom"))).unwrap_err();
    assert_eq!(err.0, "boom");
    assert_eq!(fs::read(&target).unwrap(), b"original");
    assert!(leftovers(&d).is_empty());
}

#[test]
fn check_sees_the_bytes_on_disk() {
    let d = dir("seen");
    let target = d.join("out.bin");
    let parts: [&[u8]; 2] = [b"ab", b"cd"];
    let mut seen = Vec::new();
    write_atomic(&target, &parts, |w| {
        seen = w.to_vec();
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, b"abcd");
}

#[test]
fn missing_directory_fails_cleanly() {
    let d = dir("missing");
    let target = d.join("no-such-dir").join("out.bin");
    let parts: [&[u8]; 1] = [b"x"];
    assert!(write_atomic(&target, &parts, same_bytes(&parts)).is_err());
    assert!(!target.exists());
}

#[test]
fn output_naming_an_input_is_refused() {
    let d = dir("distinct");
    let input = d.join("in.jpg");
    fs::write(&input, b"x").unwrap();
    // Same file through a different spelling of the path.
    let same = d.join(".").join("in.jpg");
    assert!(ensure_distinct(&[&input], &same).is_err());
    assert!(ensure_distinct(&[&input], &d.join("out.jpg")).is_ok());
}
