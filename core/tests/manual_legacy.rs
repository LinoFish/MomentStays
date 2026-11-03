//! Optional private-media integration: JSON array of local paths in
//! MOMENTSTAYS_LEGACY_FIXTURES. No private bytes or reports are committed.
use momentstays_core::{
    engine::{manual, request, StopToken},
    format, jpeg, live, live2motion, mp4, sha256_hex,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
fn call(req: Value) -> momentstays_core::Result<Value> {
    request::handle(
        &momentstays_core::platform::SystemNative,
        &request::NoEvents,
        &StopToken::new(),
        &req,
    )
}
fn apps(data: &[u8]) -> Vec<Vec<u8>> {
    jpeg::segments(data)
        .unwrap()
        .into_iter()
        .filter(|s| {
            (0xe0..=0xef).contains(&s.marker)
                && !(s.marker == 0xe1 && s.payload(data).starts_with(jpeg::EXIF))
        })
        .map(|s| data[s.start..s.end].to_vec())
        .collect()
}
#[test]
fn private_legacy_end_to_end() {
    let Some(list) = std::env::var_os("MOMENTSTAYS_LEGACY_FIXTURES") else {
        eprintln!("private legacy fixtures not configured");
        return;
    };
    let inputs: Vec<PathBuf> = serde_json::from_slice(&std::fs::read(list).unwrap()).unwrap();
    assert_eq!(inputs.len(), 3);
    let output =
        PathBuf::from(std::env::var_os("MOMENTSTAYS_LEGACY_OUTPUT").expect("isolated output root"));
    std::fs::create_dir_all(&output).unwrap();
    let mut evidence = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        let data = std::fs::read(input).unwrap();
        let source_hash = sha256_hex(&data);
        assert!(
            format::parse(&data).is_err(),
            "global parser stays restricted"
        );
        let photo = manual::parse(&data).unwrap();
        assert_eq!(photo.format, manual::FORMAT);
        let video = photo.video(&data);
        let movie = mp4::Movie::parse(video).unwrap();
        let folder = output.join(format!("sample-{i}"));
        std::fs::create_dir_all(&folder).unwrap();
        let frames = call(
            json!({"mode":"manual-cover-frames","input":input,"output":folder.join("preview.mp4")}),
        )
        .unwrap();
        assert_eq!(std::fs::read(folder.join("preview.mp4")).unwrap(), video);
        assert_eq!(frames["raw_hint_field"].as_str().unwrap(), std::str::from_utf8(&data[data.len()-40..data.len()-20]).unwrap().trim_end());
        for hint in frames["source_hints"].as_array().unwrap() {
            assert_eq!(hint["verified"],false);
            assert!(!hint["sources"].as_array().unwrap().is_empty());
            assert!(frames["frames"].as_array().unwrap().iter().any(|f|f["frame_index"]==hint["frame_index"] && f["presented_us"]==hint["presented_us"]));
        }
        for (pair,expected) in [("0:0",1),("999999999:999999999",0),("0:999999999999999999",1)] {
            let mut hinted = data.clone();
            let n=hinted.len();hinted[n-40..n-20].fill(b' ');hinted[n-40..n-40+pair.len()].copy_from_slice(pair.as_bytes());
            let probe=folder.join("hint-source.jpg");std::fs::write(&probe,&hinted).unwrap();
            let info=call(json!({"mode":"manual-cover-frames","input":probe,"output":folder.join("hint-video.mp4")})).unwrap();
            assert_eq!(info["raw_hint_field"],pair);
            assert_eq!(info["source_hints"].as_array().unwrap().len(),expected);
            if expected>0 { assert_eq!(info["source_hints"][0]["frame_index"],0); }
            if pair=="0:0" { assert_eq!(info["source_hints"][0]["sources"].as_array().unwrap().len(),4); }
            else { assert!(!info["hint_issues"].as_array().unwrap().is_empty()); }
        }
        for direction in ["to-apple", "to-android", "android-to-android"] {
            let row=call(json!({"mode":"scan-file","input":input,"relative":"sample.jpg","direction":direction})).unwrap();
            assert_eq!(row["motion"], direction == "to-apple");
            if direction == "to-apple" {
                assert_eq!(row["cover"], manual::COVER);
            }
        }
        assert!(call(json!({"mode":"batch","inputs":[input],"output":folder.join("unconfirmed"),"missing_time":"midpoint"})).unwrap()["counts"]["failed"].as_u64().unwrap()>0);
        let count = frames["frames"].as_array().unwrap().len();
        let mut choices = Vec::new();
        for position in [0, count / 2, count - 1] {
            let selected = &frames["frames"][position];
            let choice = json!({"source_sha256":source_hash,"source_version":frames["source_version"],"track_index":0,"frame_index":selected["frame_index"]});
            let destination = folder.join(format!("frame-{position}"));
            let req = json!({"mode":"batch","inputs":[input],"output":destination,"manual_cover":choice,"export_data_files":false});
            let mut first_req = req.clone();
            first_req["existing_output"] = json!("overwrite");
            let outcome = call(first_req).unwrap();
            assert_eq!(outcome["counts"]["converted"], 1, "{outcome}");
            let entry = &outcome["report_data"]["files"][0];
            let pair = Path::new(entry["output"].as_str().unwrap());
            let manifest = call(json!({"mode":"verify","input":pair})).unwrap();
            assert_eq!(manifest["timing"]["timing_policy"], "selected-user-frame");
            assert!(manifest["source_timestamp_us"].is_null());
            assert!(manifest["source_cover_frame_index"].is_null());
            let names = manifest["output_files"].as_object().unwrap();
            let still = std::fs::read(pair.join(names["still"].as_str().unwrap())).unwrap();
            let mov = std::fs::read(pair.join(names["video"].as_str().unwrap())).unwrap();
            let back = mp4::Movie::parse(&mov).unwrap();
            assert_eq!(
                format::coding_hash(&still).unwrap(),
                format::coding_hash(&photo.still).unwrap()
            );
            assert_eq!(apps(&photo.still), apps(&still));
            assert_eq!(
                live::still_content_identifier(&still).unwrap().unwrap(),
                manifest["identifier"].as_str().unwrap()
            );
            assert_eq!(
                momentstays_core::remux::read_content_identifier(&mov)
                    .unwrap()
                    .unwrap(),
                manifest["identifier"].as_str().unwrap()
            );
            assert_eq!(
                live2motion::mett_cover_time_in(&mov, &back).unwrap(),
                selected["presented_us"].as_i64()
            );
            let landed = live::resolve_cover_in(
                &mov,
                &back,
                momentstays_core::model::Cover::Time(selected["presented_us"].as_i64().unwrap()),
                live::TimeDomain::Presentation,
                0,
            )
            .unwrap();
            assert_eq!(
                landed.frame,
                selected["frame_index"].as_u64().unwrap() as usize
            );
            for t in movie
                .tracks
                .iter()
                .filter(|t| t.samples.handler == "vide" || t.samples.handler == "soun")
            {
                let b = back
                    .tracks
                    .iter()
                    .find(|b| b.samples.handler == t.samples.handler)
                    .unwrap();
                assert_eq!(t.samples.timescale, b.samples.timescale);
                assert_eq!(t.samples.samples.len(), b.samples.samples.len());
                for (a, b) in t.samples.samples.iter().zip(&b.samples.samples) {
                    assert_eq!(
                        (a.presentation_time, a.decode_time, a.duration),
                        (b.presentation_time, b.decode_time, b.duration)
                    );
                    assert_eq!(
                        &video[a.offset..a.offset + a.size],
                        &mov[b.offset..b.offset + b.size]
                    );
                }
            }
            assert_eq!(
                std::fs::read(pair.join("source-original.bin")).unwrap(),
                data
            );
            assert_eq!(
                std::fs::read(pair.join("source-tail.bin")).unwrap(),
                data[photo.video.end..]
            );
            assert_eq!(
                std::fs::read(pair.join("source-private.bin")).unwrap(),
                data[jpeg::image_end(&data).unwrap()..photo.video.start]
            );
            for (n, seg) in jpeg::segments(&photo.still)
                .unwrap()
                .into_iter()
                .filter(|s| s.marker == 0xe1 && s.payload(&photo.still).starts_with(jpeg::EXIF))
                .enumerate()
            {
                assert_eq!(
                    std::fs::read(pair.join(format!("source-exif-{n}.bin"))).unwrap(),
                    photo.still[seg.start..seg.end]
                );
            }
            assert_eq!(call(req.clone()).unwrap()["counts"]["resumed"], 1);
            let private = std::fs::read(pair.join("source-private.bin")).unwrap();
            std::fs::remove_file(pair.join("source-private.bin")).unwrap();
            assert!(call(json!({"mode":"verify","input":pair})).is_err());
            assert_eq!(call(req.clone()).unwrap()["counts"]["failed"], 1);
            std::fs::write(pair.join("source-private.bin"), private).unwrap();
            let mut overwrite = req.clone();
            overwrite["existing_output"] = json!("overwrite");
            assert_eq!(call(overwrite).unwrap()["counts"]["converted"], 1);
            let mut invalid = req.clone();
            invalid["manual_cover"]["frame_index"] = json!(count as i64 + 1);
            assert_eq!(call(invalid).unwrap()["counts"]["failed"], 1);
            let mut stale = req.clone();
            stale["manual_cover"]["source_sha256"] = json!("0".repeat(64));
            assert_eq!(call(stale).unwrap()["counts"]["failed"], 1);
            let mut bad_track = req.clone();
            bad_track["manual_cover"]["track_index"] = json!(1);
            assert_eq!(call(bad_track).unwrap()["counts"]["failed"], 1);
            choices.push(json!({"frame":selected,"pair":pair,"manifest":manifest}));
        }
        // Footer corruption cannot fall back to a guessed legacy source.
        let mut bad = data.clone();
        let len = bad.len();
        bad[len - 21] = b'x';
        assert!(manual::parse(&bad).is_err());
        let mut versioned = data.clone();
        versioned[len - 60] = b'v';
        assert!(manual::parse(&versioned).is_err());
        let mut over = data.clone();
        over[len - 20..].copy_from_slice(b"LIVE_999999999999999");
        assert!(manual::parse(&over).is_err());
        if photo.video.start > jpeg::image_end(&data).unwrap() {
            let mut gap = data.clone();
            gap[photo.video.start - 12] ^= 1;
            assert!(manual::parse(&gap).is_err());
            let mut signature = data.clone();
            signature[photo.video.start - 1] ^= 1;
            assert!(manual::parse(&signature).is_err());
        }
        let temporary = folder.join("changed.jpg");
        std::fs::write(&temporary, &data).unwrap();
        let mut selection=manual::Selection::from_json(&json!({"source_sha256":source_hash,"source_version":manual::version(&temporary).unwrap(),"track_index":0,"frame_index":0})).unwrap();
        std::fs::write(&temporary, &data).unwrap();
        assert!(manual::resolve(&data, &temporary, &selection).is_err());
        selection.source_version = manual::version(&temporary).unwrap();
        assert!(manual::resolve(&data, &temporary, &selection).is_ok());
        std::fs::remove_file(&temporary).unwrap();
        assert!(manual::resolve(&data, &temporary, &selection).is_err());
        assert_eq!(sha256_hex(&std::fs::read(input).unwrap()), source_hash);
        evidence.push(json!({"input":input,"source_sha256":source_hash,"choices":choices}));
    }
    std::fs::write(
        output.join("production-proof.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
