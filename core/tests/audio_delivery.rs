mod engine_support;
use engine_support::{motion, Dir};
use momentstays_core::{engine::{batch, Native, NoNative, StopToken}, model::{Cover, Vendor}};
use std::path::Path;

struct RefuseAudio;
impl Native for RefuseAudio {
    fn audio_decodes(&self, _: &Path, _: usize) -> momentstays_core::Result<Option<bool>> {
        Ok(Some(false))
    }
}

#[test]
fn native_audio_failure_does_not_publish_a_live_photo() {
    let d = Dir::new("audio-refusal");
    let source = d.0.join("source.jpg");
    let bytes = motion(Cover::Time(800_000), Vendor::Google);
    std::fs::write(&source, &bytes).unwrap();
    let target = d.0.join("out.livephoto");
    let result = batch::convert(&RefuseAudio, &source, &target, &Default::default(),
        &StopToken::new(), false, &Default::default());
    assert!(result.unwrap_err().0.contains("音频解码"));
    assert!(!target.exists());
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
}

#[test]
fn current_resume_succeeds_but_previous_audio_layout_requires_reconversion() {
    let d = Dir::new("audio-resume");
    let source = d.0.join("source.jpg");
    std::fs::write(&source, motion(Cover::Time(800_000), Vendor::Google)).unwrap();
    let target = d.0.join("out.livephoto");
    let run = || batch::convert(&NoNative, &source, &target, &Default::default(),
        &StopToken::new(), false, &Default::default());
    assert_eq!(run().unwrap()["status"], "converted");
    assert_eq!(run().unwrap()["status"], "resumed");
    let file = target.join("manifest.json");
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    value["implementation"] = serde_json::json!("gui-native-1.1.8");
    std::fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(run().is_err());
}
