//! Cover time <-> frame conversion (ROADMAP A4) on top of `mp4::sample_tables`.
//!
//! A "frame" here is an index into the presentation-sorted sample list, the
//! same convention Swift `AndroidExport.pair()` uses when it returns the cover
//! frame index (and the same number the Huawei LIVE_ tail carries). Times are
//! microseconds, rounded like Swift (`(seconds * 1_000_000).rounded()`).
//!
//! Rules, mirroring Swift `pair()` and the product rule of never guessing:
//! - frame -> time uses the sample's presentation time; a negative result is refused.
//! - time -> frame picks the sample with the nearest presentation time and
//!   requires the error to fit within half that sample's duration plus a 2 us
//!   tolerance; ties and out-of-range targets are refused, not approximated.
//! - Only a single video track is accepted; zero or several video tracks are
//!   refused as ambiguous.
//! - Times are raw sample-table times: the edit list (elst) is NOT applied.
//!   Both directions use the same tables, so round-trips within one file are
//!   exact; do not compare these times against edit-list-adjusted players.

use crate::mp4::TrackSamples;
use crate::{fail, Result};

/// The single video track of a file, or an error when there is none or more than one.
pub fn video_track<'a>(tracks: &'a [TrackSamples]) -> Result<&'a TrackSamples> {
    let videos: Vec<&TrackSamples> = tracks.iter().filter(|t| t.handler == "vide").collect();
    match videos.len() {
        0 => fail("视频没有视频轨，无法换算封面"),
        1 => Ok(videos[0]),
        _ => fail("视频含多个视频轨，无法确定封面所在轨"),
    }
}

/// Presentation times in ascending order, each paired with its decode-order index.
fn sorted_presentation(track: &TrackSamples) -> Vec<(i64, usize)> {
    let mut v: Vec<(i64, usize)> = track
        .samples
        .iter()
        .enumerate()
        .map(|(i, s)| (s.presentation_time, i))
        .collect();
    v.sort_by_key(|(t, _)| *t);
    v
}

/// Duration of the sorted sample at `i`, in track timescale units: the gap to
/// the next sorted sample, or the last decode-order sample's stts delta for
/// the final sorted sample (mirrors Swift's `durations.last` fallback).
fn sorted_duration(track: &TrackSamples, sorted: &[(i64, usize)], i: usize) -> Option<i64> {
    if i + 1 < sorted.len() {
        return Some(sorted[i + 1].0 - sorted[i].0);
    }
    track.samples.last().map(|s| s.duration as i64)
}

/// Frame (presentation-sorted index) -> cover time in microseconds.
pub fn frame_to_time_us(track: &TrackSamples, frame: usize) -> Result<i64> {
    let sorted = sorted_presentation(track);
    let (t, _) = match sorted.get(frame) {
        Some(v) => v,
        None => return fail("封面帧号超出视频样本范围"),
    };
    if *t < 0 {
        return fail("封面帧的显示时间为负，无法换算");
    }
    let ts = track.timescale as i64;
    if ts <= 0 {
        return fail("视频时间尺度无效");
    }
    // Round (t * 1_000_000 / ts), Swift's `.rounded()` on non-negative values.
    Ok((*t * 1_000_000 + ts / 2) / ts)
}

/// Cover time in microseconds -> frame (presentation-sorted index).
pub fn time_us_to_frame(track: &TrackSamples, time_us: i64) -> Result<usize> {
    if time_us < 0 {
        return fail("封面时间为负，无法换算");
    }
    let ts = track.timescale as i64;
    if ts <= 0 {
        return fail("视频时间尺度无效");
    }
    let sorted = sorted_presentation(track);
    if sorted.is_empty() {
        return fail("视频没有样本，无法换算封面");
    }
    // Compare in units of us * timescale to stay in integers:
    // |time_us * ts - pts * 1_000_000| <= duration * 1_000_000 / 2 + 2 * ts
    // becomes, multiplied by 2: 2 * diff <= duration * 1_000_000 + 4 * ts.
    let mut best: Option<(i128, usize)> = None;
    let mut best_count = 0usize;
    for (i, (pts, _)) in sorted.iter().enumerate() {
        let diff = (time_us as i128 * ts as i128 - *pts as i128 * 1_000_000).abs();
        match best {
            Some((b, _)) if diff > b => {}
            Some((b, _)) if diff == b => {
                best_count += 1;
            }
            _ => {
                best = Some((diff, i));
                best_count = 1;
            }
        }
    }
    let (diff, i) = best.unwrap();
    if best_count > 1 {
        return fail("封面时间对应多个样本，存在歧义");
    }
    let duration = match sorted_duration(track, &sorted, i) {
        Some(d) if d > 0 => d as i128,
        _ => return fail("封面样本时长无效，无法换算"),
    };
    if 2 * diff > duration * 1_000_000 + 4 * ts as i128 {
        return fail("封面时间越界或超出半个样本时长");
    }
    Ok(i)
}
