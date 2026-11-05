#![allow(dead_code)] // Integration test crates each use a different subset.
//! Shared existing fixtures and engine harness; no alternate conversion logic.
use base64::Engine;
use momentstays_core::engine::batch::{self, ExistingOutput, RunRequest};
use momentstays_core::engine::options::ConversionOptions;
use momentstays_core::engine::{NoNative, StopToken};
use momentstays_core::model::{self, Cover, MotionModel, Vendor};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) fn b64(path: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    base64::engine::general_purpose::STANDARD
        .decode(std::fs::read_to_string(p).unwrap().trim_ascii())
        .unwrap()
}

/// `real.mp4` without its edit list: 5 frames at 0.4, 0.6, ... 1.2 s.
pub(crate) fn video() -> Vec<u8> {
    let mut d = b64("tests/fixtures-samples/real.mp4.b64");
    let i = d.windows(4).position(|w| w == b"edts").unwrap();
    d[i..i + 4].copy_from_slice(b"free");
    d
}

pub(crate) fn motion(cover: Cover, vendor: Vendor) -> Vec<u8> {
    let m = MotionModel {
        still: b64("tests/fixtures-pycli/base.jpg.b64"),
        video: video(),
        cover,
        source_format: "test".into(),
    };
    model::write(&m, vendor).unwrap()
}

pub(crate) struct Dir(pub(crate) PathBuf);
impl Dir {
    pub(crate) fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!(
            "ms-engine-{name}-{}-{}",
            std::process::id(),
            momentstays_core::engine::uuid()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn inputs(dir: &Path) {
    std::fs::write(
        dir.join("google.jpg"),
        motion(Cover::Time(800_000), Vendor::Google),
    )
    .unwrap();
    std::fs::write(
        dir.join("samsung.jpg"),
        motion(Cover::Time(600_000), Vendor::Samsung),
    )
    .unwrap();
    std::fs::write(
        dir.join("huawei.jpg"),
        motion(Cover::Frame(2), Vendor::Huawei),
    )
    .unwrap();
    std::fs::write(
        dir.join("static.jpg"),
        b64("tests/fixtures-pycli/base.jpg.b64"),
    )
    .unwrap();
}

pub(crate) fn run(
    sources: &[PathBuf],
    output: &Path,
    options: &ConversionOptions,
    policy: Option<ExistingOutput>,
) -> batch::Outcome {
    let decide = policy.map(|p| move |_: &Path, _: &Path| p);
    batch::run(
        &NoNative,
        &RunRequest {
            sources,
            output,
            options,
            scan_rows: None,
            concurrency: Some(2),
        },
        &StopToken::new(),
        decide
            .as_ref()
            .map(|d| d as &(dyn Fn(&Path, &Path) -> ExistingOutput + Sync)),
        |_| {},
    )
    .unwrap()
}

pub(crate) fn counts(o: &batch::Outcome) -> [usize; 4] {
    ["converted", "resumed", "skipped", "failed"].map(|k| o.counts[k])
}

pub(crate) fn files(o: &batch::Outcome) -> Vec<Value> {
    o.report_data["files"].as_array().unwrap().clone()
}
