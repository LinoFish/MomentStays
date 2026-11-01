use base64::Engine;
use momentstays_core::{
    mp4::{self, BoxParseMode},
    recovery, remux,
};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
fn source() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(include_str!("fixtures-samples/real.mp4.b64").trim())
        .unwrap()
}
fn damaged() -> Vec<u8> {
    let mut d = source();
    let b = mp4::boxes(&d, 0, d.len()).unwrap().pop().unwrap();
    d[b.start..b.start + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    d
}
#[test]
fn recover_terminal_length_preserves_all_bytes_and_requires_consent() {
    let input = damaged();
    let (walk, warnings) = recovery::inspect_boxes(&input, BoxParseMode::Tolerant).unwrap();
    assert!(!walk.is_empty());
    assert!(!warnings.is_empty());
    assert!(recovery::inspect_boxes(&input, BoxParseMode::Strict).is_err());
    assert!(recovery::prepare_conversion(&input)
        .unwrap()
        .approve(false)
        .is_err());
    let prepared = recovery::prepare_conversion(&input).unwrap();
    assert!(!prepared.warnings.is_empty());
    let fixed = prepared.approve(true).unwrap();
    assert_eq!(fixed, source());
    let mov =
        remux::remux_to_mov_with_identifier(&fixed, 0, "123E4567-E89B-12D3-A456-426614174000")
            .unwrap();
    remux::check_remux(&fixed, &mov, 0).unwrap();
}
#[test]
fn recovery_rejects_missing_bytes_and_unrecognized_tail() {
    let mut input = damaged();
    input.truncate(1600);
    assert!(recovery::prepare_conversion(&input).is_err());
    let mut input = source();
    input.extend(b"unknown");
    assert!(recovery::prepare_conversion(&input).is_err());
}
#[test]
fn cli_recovers_only_after_confirmation_and_never_overwrites() {
    let dir = std::env::temp_dir().join(format!("momentstays-recovery-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.mp4");
    let output = dir.join("output.mov");
    fs::write(&input, damaged()).unwrap();
    let _ = fs::remove_file(&output);
    for answer in ["no\n", "", "yes\n"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_momentstays-convert"))
            .arg("to-live")
            .arg(&input)
            .args([
                "--uuid",
                "123E4567-E89B-12D3-A456-426614174000",
                "--recover-boxes",
                "-o",
            ])
            .arg(&output)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(answer.as_bytes())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert_eq!(
            result.status.success(),
            answer == "yes\n",
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(output.exists(), answer == "yes\n");
        assert_eq!(fs::read(&input).unwrap(), damaged());
    }
    let original = fs::read(&output).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_momentstays-convert"))
        .arg("to-live")
        .arg(&input)
        .args([
            "--uuid",
            "123E4567-E89B-12D3-A456-426614174000",
            "--recover-boxes",
            "-o",
        ])
        .arg(&output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"yes\n").unwrap();
    assert!(!child.wait().unwrap().success());
    assert_eq!(fs::read(&output).unwrap(), original);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hostile_run_length_counts_fail_before_expansion() {
    let mut input = source();
    let type_at = input.windows(4).position(|w| w == b"stts").unwrap();
    // First stts run count. The file still has only five actual samples.
    input[type_at + 12..type_at + 16].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(mp4::sample_tables(&input).is_err());
    let mut input = source();
    let type_at = input.windows(4).position(|w| w == b"stsz").unwrap();
    input[type_at + 12..type_at + 16].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(mp4::sample_tables(&input).is_err());
}

#[test]
fn conversion_recovery_rejects_samples_pointing_into_headers() {
    let mut input = source();
    let type_at = input.windows(4).position(|w| w == b"stco").unwrap();
    input[type_at + 12..type_at + 16].copy_from_slice(&0u32.to_be_bytes());
    assert!(
        mp4::sample_tables(&input).is_ok(),
        "structurally readable table"
    );
    assert!(
        recovery::prepare_conversion(&input).is_err(),
        "headers are not media payload"
    );
}
