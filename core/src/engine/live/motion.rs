//! Lossless sample remux and target Motion Photo assembly; no file publication.
use crate::engine::options::AndroidOptions;
use crate::engine::still;
use crate::model::{self, Vendor};
use crate::mp4::Movie;
use crate::{heif, heifwrite, remux, Result};
use serde_json::{json, Value};

/// Swift `AndroidExport.remux`: MOV -> MP4, samples copied verbatim.
pub fn remux(mov: &[u8]) -> Result<(Vec<u8>, Value)> {
    let parsed = Movie::parse(mov)?;
    let mp4 = remux::remux_mp4(mov, &parsed)?;
    let out = Movie::parse(&mp4)?;
    let tracks: Vec<Value> = out
        .tracks
        .iter()
        .filter(|t| t.samples.handler == "vide")
        .chain(out.tracks.iter().filter(|t| t.samples.handler == "soun"))
        .map(|t| {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            for s in &t.samples.samples {
                h.update(&mp4[s.offset..s.offset + s.size]);
            }
            json!({
                "type": t.samples.handler,
                "sha256": crate::hex(&h.finalize()),
                "samples": t.samples.samples.len(),
                "max_pts_shift_seconds": 0.0,
            })
        })
        .collect();
    let audio_codecs = remux::audio_codecs(&mp4, &out)?;
    Ok((
        mp4,
        json!({"tracks": tracks, "audio_codecs": audio_codecs, "video_reencoded": false, "audio_reencoded": false}),
    ))
}

fn vendor_of(name: &str) -> Vendor {
    match name {
        "samsung" => Vendor::Samsung,
        "huawei" => Vendor::Huawei,
        "oppo" => Vendor::Oppo,
        "vivo" => Vendor::Vivo,
        // google, xiaomi and honor share the Google layout.
        _ => Vendor::Google,
    }
}

/// Swift `AndroidExport.assemble`.
pub fn assemble(
    still: &[u8],
    video: &[u8],
    options: &AndroidOptions,
    time: i64,
    frame: i64,
) -> Result<Vec<u8>> {
    let vendor = vendor_of(&options.vendor);
    let plan = if heif::matches(still) {
        heifwrite::plan_heic(still, video, vendor, time)?
    } else {
        let cleaned = still::live_still(still, "", false)?;
        model::plan_swift(&cleaned, video, time, frame, vendor)?
    };
    let out = plan.concat(video);
    plan.verify(&out, video)?;
    Ok(out)
}
