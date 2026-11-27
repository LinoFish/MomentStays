//! Per-track sample facts from MP4/MOV bytes: the platform-independent part
//! of Swift `MotionLive.trackInfo` (AVAssetReader) and of the timeline math
//! in `MotionPreviewSession.prepare`.
//!
//! Swift reads compressed packets through AVAssetReader; here the sample
//! tables are read directly. Timestamps are given both in the media
//! timeline and mapped through the track's edit list (AVAssetTrack
//! `segments`), so a host can use either. Checked against AVFoundation by
//! `apple/macos/scripts/check_core_decisions.py`: sizes, durations, hashes
//! and media timestamps match on every track; the mapped timeline matches
//! on video tracks but not on AAC tracks (AVFoundation applies a priming
//! segment this mapping does not see) or after an empty edit that is not a
//! whole number of track ticks (truncated here). The Apple app therefore
//! does not use this request yet (core/ENGINE.md).
use crate::mp4::{self, Movie};
use crate::{fail, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq)]
pub struct TrackInfo {
    pub handler: String,
    pub timescale: u32,
    /// Decode order, like AVAssetReader output.
    pub media_pts: Vec<i64>,
    /// Edit-list mapped presentation time in track units; None when the
    /// media time falls in no (or more than one) non-empty edit.
    pub presented_pts: Vec<Option<i64>>,
    pub durations: Vec<u64>,
    pub sizes: Vec<usize>,
    /// SHA-256 of the compressed sample bytes in decode order (Swift `hash`).
    pub sha256: String,
}

/// Map media time `t` (track units) through `edits` (movie-timescale
/// durations, track-timescale media times), like the preview's `presented`.
pub fn present(t: i64, edits: Option<&[(i64, i64)]>, track_ts: u32, movie_ts: u32) -> Option<i64> {
    let Some(edits) = edits else {
        return Some(t);
    };
    let (ts, mts) = (track_ts as i128, movie_ts as i128);
    let mut start_movie = 0i128;
    let mut hits = Vec::new();
    for &(dur, media) in edits {
        let dur = dur as i128;
        if media >= 0 {
            let from = media as i128;
            let to = from * mts + dur * ts; // in track·movie units
            if from * mts <= t as i128 * mts && (t as i128) * mts < to {
                // presented = start + (t - media), in track units.
                hits.push(start_movie * ts / mts + (t as i128 - from));
            }
        }
        start_movie += dur;
    }
    (hits.len() == 1).then(|| hits[0] as i64)
}

pub fn track_info(d: &[u8]) -> Result<Vec<TrackInfo>> {
    let movie = Movie::parse(d)?;
    let mut out = Vec::new();
    for t in &movie.tracks {
        let edits = mp4::edit_list(d, &t.trak)?;
        let mut hash = Sha256::new();
        for s in &t.samples.samples {
            let Some(bytes) = d.get(s.offset..s.offset.saturating_add(s.size)) else {
                return fail("MP4 采样超出文件范围");
            };
            hash.update(bytes);
        }
        let samples = &t.samples.samples;
        out.push(TrackInfo {
            handler: t.samples.handler.clone(),
            timescale: t.samples.timescale,
            media_pts: samples.iter().map(|s| s.presentation_time).collect(),
            presented_pts: samples
                .iter()
                .map(|s| {
                    present(
                        s.presentation_time,
                        edits.as_deref(),
                        t.samples.timescale,
                        movie.timescale,
                    )
                })
                .collect(),
            durations: samples.iter().map(|s| s.duration).collect(),
            sizes: samples.iter().map(|s| s.size).collect(),
            sha256: crate::hex(&hash.finalize()),
        });
    }
    Ok(out)
}

pub fn json(tracks: &[TrackInfo]) -> Value {
    Value::Array(
        tracks
            .iter()
            .map(|t| {
                json!({"handler": t.handler, "timescale": t.timescale, "media_pts": t.media_pts,
                    "presented_pts": t.presented_pts, "durations": t.durations, "sizes": t.sizes,
                    "sha256": t.sha256})
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_mapping_matches_avfoundation_segments() {
        // No edits: identity.
        assert_eq!(present(7, None, 600, 600), Some(7));
        // Empty edit of 740/600 s, then media from 0: shifted by 740 ticks.
        let edits = [(740, -1), (3000, 0)];
        assert_eq!(present(0, Some(&edits), 600, 600), Some(740));
        assert_eq!(present(2999, Some(&edits), 600, 600), Some(3739));
        assert_eq!(present(3000, Some(&edits), 600, 600), None);
        // B-frame shift: media 2048 maps to 0 (track 10240, movie 1000).
        let shift = [(3000, 2048)];
        assert_eq!(present(2048, Some(&shift), 10240, 1000), Some(0));
        assert_eq!(present(1024, Some(&shift), 10240, 1000), None);
    }
}
