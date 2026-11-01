//! Rust stand-in for `apple/shared/tests/NativeDriver.swift`: one JSON
//! request on stdin, one key-sorted JSON line on stdout (`{"error": …}`
//! and exit 1 on failure). Same modes, keys and defaults, so the
//! `apple/macos/scripts/check_*.py` harnesses can run against either
//! engine and their outputs can be diffed.
//!
//! Steps that need the host's codecs (decoding a gain map or a video
//! frame, re-encoding a still, generating fixtures) report
//! "需要原生处理：…" here; in the app they run through `engine::Native`.

use momentstays_core::engine::request::{handle, NoEvents};
use momentstays_core::engine::{sorted, NoNative, StopToken};
use momentstays_core::Error;
use serde_json::{json, Value};
use std::io::Read;

fn main() {
    let mut text = String::new();
    let parsed = std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| Error::new(e.to_string()))
        .and_then(|_| {
            serde_json::from_str::<Value>(&text).map_err(|_| Error::new("Missing request"))
        });
    match parsed.and_then(|req| handle(&NoNative, &NoEvents, &StopToken::new(), &req)) {
        Ok(v) => println!("{}", sorted(&v)),
        Err(e) => {
            println!("{}", sorted(&json!({"error": e.0})));
            std::process::exit(1);
        }
    }
}
