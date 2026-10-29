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

// ---------------------------------------------------------------------------
// Edit-list-aware cover mapping (wiring: Swift `sourceCoverTiming` / `pair()`).
//
// The A4 functions above deliberately ignore the edit list. The app's cover
// mapping does NOT: it tests the requested time both as a raw media time and
// as a movie-timeline time mapped through the selected track's edit list,
// and refuses when the two point at different frames. This section mirrors
// Swift `MotionLive.sourceCoverTiming` exactly (integer arithmetic, same
// half-duration + 2 us tolerance, same ambiguity refusal), so the C ABI
// `cover_map` entry and the to-live / live-pair entries share one semantics.
// ---------------------------------------------------------------------------

use crate::binary::be;
use crate::mp4::{boxes, BoxRef};

/// One edit-list entry: segment duration in movie-timescale ticks and the
/// media start in track-timescale ticks (-1 for an empty edit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditSegment {
    pub duration: i64,
    pub media_time: i64,
}

/// Movie timescale from moov/mvhd (version 0: offset 12, version 1: offset 20).
pub fn movie_timescale(data: &[u8]) -> Result<u32> {
    let top = boxes(data, 0, data.len())?;
    let moov = match top.iter().find(|b| b.is(b"moov")) {
        Some(m) => m,
        None => return fail("MP4 缺少 moov"),
    };
    let mvhd = match boxes(data, moov.payload, moov.end)?
        .into_iter()
        .find(|b| b.is(b"mvhd"))
    {
        Some(m) => m,
        None => return fail("MP4 缺少 mvhd"),
    };
    if mvhd.end - mvhd.payload < 4 {
        return fail("MP4 mvhd 被截断");
    }
    let at = if data[mvhd.payload] == 1 { 20 } else { 12 };
    if mvhd.end - mvhd.payload < at + 4 {
        return fail("MP4 mvhd 被截断");
    }
    let ts = be(data, mvhd.payload + at, 4)? as u32;
    if ts == 0 {
        return fail("MP4 电影时间尺度无效");
    }
    Ok(ts)
}

/// trak boxes of a file, in file order (same order `sample_tables` uses).
pub fn trak_boxes(data: &[u8]) -> Result<Vec<BoxRef>> {
    let top = boxes(data, 0, data.len())?;
    let moov = match top.iter().find(|b| b.is(b"moov")) {
        Some(m) => m,
        None => return fail("MP4 缺少 moov"),
    };
    Ok(boxes(data, moov.payload, moov.end)?
        .into_iter()
        .filter(|b| b.is(b"trak"))
        .collect())
}

/// Edit list of one trak, or None when the trak has no edts box.
/// (Same box walk as `live2motion`'s private parser, kept here so the cover
/// mapping does not depend on the assembly module.)
pub fn edit_segments(data: &[u8], trak: &BoxRef) -> Result<Option<Vec<EditSegment>>> {
    let edts: Vec<BoxRef> = boxes(data, trak.payload, trak.end)?
        .into_iter()
        .filter(|b| b.is(b"edts"))
        .collect();
    if edts.is_empty() {
        return Ok(None);
    }
    if edts.len() > 1 {
        return fail("视频含多个 edts，无法映射封面");
    }
    let elst: Vec<BoxRef> = boxes(data, edts[0].payload, edts[0].end)?
        .into_iter()
        .filter(|b| b.is(b"elst"))
        .collect();
    if elst.len() != 1 {
        return fail("视频编辑列表缺少唯一的 elst");
    }
    let elst = &elst[0];
    if elst.end - elst.payload < 8 {
        return fail("视频 elst 被截断");
    }
    let version = data[elst.payload];
    let count = be(data, elst.payload + 4, 4)? as usize;
    let (record, at) = if version == 1 {
        (20usize, elst.payload + 8)
    } else {
        (12usize, elst.payload + 8)
    };
    if count > (elst.end - at) / record {
        return fail("视频 elst 被截断");
    }
    let mut out = Vec::new();
    for i in 0..count {
        let p = at + i * record;
        if version == 1 {
            let dur = be(data, p, 8)? as u64;
            if dur > i64::MAX as u64 {
                return fail("视频编辑时长超出范围");
            }
            let media = be(data, p + 8, 8)? as u64 as i64;
            out.push(EditSegment {
                duration: dur as i64,
                media_time: media,
            });
        } else {
            let dur = be(data, p, 4)? as i64;
            let media = be(data, p + 4, 4)? as u32 as i32 as i64;
            out.push(EditSegment {
                duration: dur,
                media_time: media,
            });
        }
    }
    Ok(Some(out))
}

/// Video tracks of a file paired with their trak boxes, in file order.
pub fn video_tracks(data: &[u8]) -> Result<Vec<(TrackSamples, BoxRef)>> {
    let tables = crate::mp4::sample_tables(data)?;
    let traks = trak_boxes(data)?;
    if tables.len() != traks.len() {
        return fail("视频轨道数与采样表不一致");
    }
    Ok(tables
        .into_iter()
        .zip(traks.into_iter())
        .filter(|(t, _)| t.handler == "vide")
        .collect())
}

/// Presentation-sorted (pts, duration) pairs of one track, durations travelling
/// with their sample the way Swift's `zip(pts, durations).sorted` does.
fn sorted_samples(track: &TrackSamples) -> Vec<(i64, i64)> {
    let mut v: Vec<(i64, i64)> = track
        .samples
        .iter()
        .map(|s| (s.presentation_time, s.duration as i64))
        .collect();
    v.sort_by_key(|(t, _)| *t);
    v
}

/// Swift `durationAt`: gap to the next sorted sample, else the sorted-last
/// sample's own duration when positive, else the previous gap.
fn duration_at(sorted: &[(i64, i64)], i: usize) -> Option<i64> {
    if i + 1 < sorted.len() {
        return Some(sorted[i + 1].0 - sorted[i].0);
    }
    if sorted[i].1 > 0 {
        return Some(sorted[i].1);
    }
    if i > 0 {
        return Some(sorted[i].0 - sorted[i - 1].0);
    }
    None
}

/// Round (ticks * 1_000_000 / ts) for non-negative ticks, Swift `.rounded()`.
fn ticks_to_us(ticks: i64, ts: i64) -> i64 {
    (ticks * 1_000_000 + ts / 2) / ts
}

/// Map a movie-timeline time (microseconds) to a media-timeline time
/// (microseconds) through one track's edit list. No edts means identity.
/// Returns None when the movie time falls in an empty edit or past the end.
pub fn movie_to_media_us(
    data: &[u8],
    trak: &BoxRef,
    track_ts: i64,
    movie_us: i64,
) -> Result<Option<i64>> {
    let segments = edit_segments(data, trak)?;
    let segments = match segments {
        None => return Ok(Some(movie_us)),
        Some(s) => s,
    };
    let movie_ts = movie_timescale(data)? as i64;
    if movie_ts <= 0 || track_ts <= 0 {
        return fail("视频时间尺度无效");
    }
    // Requested time in movie ticks, rounded to nearest.
    let req = (movie_us * movie_ts + 500_000) / 1_000_000;
    let mut acc: i64 = 0;
    for seg in &segments {
        if seg.media_time >= 0 && req >= acc && req < acc + seg.duration {
            // Exact rational mapping, segment duration cancels:
            // media = media_start + (req - target_start) * track_ts / movie_ts.
            let num = (req - acc) * track_ts;
            let mapped = seg.media_time + (num + movie_ts / 2) / movie_ts;
            return Ok(Some(ticks_to_us(mapped, track_ts)));
        }
        acc += seg.duration;
    }
    Ok(None)
}

/// Map a media-timeline time (microseconds) to a movie-timeline time
/// (microseconds) through one track's edit list: the inverse of
/// [`movie_to_media_us`], mirroring Swift `CoverCandidateSession.presented`.
/// No edts means identity. Returns None when the media time falls in no
/// segment's source range, and errors when two segments claim it (Swift's
/// "exactly one segment" rule).
pub fn media_to_movie_us(
    data: &[u8],
    trak: &BoxRef,
    track_ts: i64,
    media_us: i64,
) -> Result<Option<i64>> {
    let segments = edit_segments(data, trak)?;
    let segments = match segments {
        None => return Ok(Some(media_us)),
        Some(s) => s,
    };
    let movie_ts = movie_timescale(data)? as i64;
    if movie_ts <= 0 || track_ts <= 0 {
        return fail("视频时间尺度无效");
    }
    // Work in exact integers: a segment's source range in track ticks is
    // [media_time, media_time + duration * track_ts / movie_ts). With
    // media_ticks = media_us * track_ts / 1e6, containment becomes
    // lo * 1e6 <= media_us * track_ts * movie_ts < hi * 1e6 where
    // lo = media_time * movie_ts and hi = lo + duration * track_ts.
    // Rounding stays within 1 us, inside the 2 us cover tolerance.
    let mut acc: i64 = 0;
    let mut found: Option<i64> = None;
    for seg in &segments {
        if seg.media_time >= 0 {
            let lo = seg.media_time * movie_ts;
            let hi = lo + seg.duration * track_ts;
            let v = media_us * track_ts * movie_ts;
            if (lo as i128) * 1_000_000 <= v as i128 && (v as i128) < (hi as i128) * 1_000_000 {
                if found.is_some() {
                    return fail("辅助轨编辑时间线无法唯一定位候选画面");
                }
                let target_start_us = (acc * 1_000_000 + movie_ts / 2) / movie_ts;
                let source_start_us =
                    (seg.media_time * 1_000_000 + track_ts / 2) / track_ts;
                found = Some(target_start_us + (media_us - source_start_us));
            }
        }
        acc += seg.duration;
    }
    Ok(found)
}

/// Request for [`map_cover`], mirroring Swift `movie()`'s request dictionary.
#[derive(Clone, Debug, Default)]
pub struct CoverRequest {
    /// Huawei/Honor frame index (presentation-sorted index on the PRIMARY track).
    pub cover_frame_index: Option<usize>,
    /// Cover time in microseconds (movie timeline for the presentation path).
    pub timestamp_us: Option<i64>,
    /// "high-res-candidate" | "video-metadata" | anything else -> source-xmp.
    pub timestamp_source: Option<String>,
    /// "auto" | "media" | "presentation".
    pub time_domain: String,
    /// "primary" | "secondary".
    pub video_track: String,
    /// "error" | "midpoint".
    pub missing_time: String,
}

/// Result of [`map_cover`], mirroring Swift `SourceCoverTiming`.
#[derive(Clone, Debug)]
pub struct CoverMapping {
    /// Presentation-sorted frame index on the selected track.
    pub frame: usize,
    /// That frame's presentation time, microseconds.
    pub frame_time_us: i64,
    /// That frame's duration, microseconds.
    pub frame_duration_us: i64,
    /// The candidate time that matched (media or edit-list-mapped), microseconds.
    pub mapped_us: i64,
    /// "media" | "presentation-edit-list" | "presentation-identity".
    pub domain: String,
    /// "source-huawei-frame" | "selected-high-res-candidate" |
    /// "source-video-metadata" | "source-xmp" | "explicit-midpoint".
    pub policy: String,
    /// |frame time - candidate| in microseconds, rounded.
    pub error_us: i64,
}

/// Edit-list-aware cover mapping, mirroring Swift `sourceCoverTiming`:
/// media candidate + presentation-edit-list candidate (or identity when the
/// track has no segments), nearest sample within half duration + 2 us,
/// duplicate frames collapsed, exactly one match required.
pub fn map_cover(data: &[u8], req: &CoverRequest) -> Result<CoverMapping> {
    let videos = video_tracks(data)?;
    if videos.is_empty() {
        return fail("视频没有视频轨，无法映射封面");
    }
    let primary = &videos[0].0;
    let (selected, selected_trak) = if req.video_track == "secondary" {
        match videos.get(1) {
            Some(v) => (&v.0, &v.1),
            None => return fail("需要两个视频轨才能选择辅助轨"),
        }
    } else {
        (&videos[0].0, &videos[0].1)
    };
    let ts = selected.timescale as i64;
    if ts <= 0 {
        return fail("视频时间尺度无效");
    }
    let sorted = sorted_samples(selected);
    if sorted.is_empty() {
        return fail("视频没有样本，无法映射封面");
    }
    // Requested time + policy, mirroring Swift's three branches. All
    // candidates are normalised to microseconds: Swift compares CMTimes
    // across timescales exactly, and microsecond rounding (<=1 us) sits
    // inside the 2 us tolerance, so this matches within tolerance even when
    // the primary and selected tracks use different timescales.
    let (requested_us, policy): (i64, String) = if let Some(index) = req.cover_frame_index {
        let primary_sorted = sorted_samples(primary);
        let (t, _) = match primary_sorted.get(index) {
            Some(v) => v,
            None => return fail("华为/荣耀原始封面帧号超出视频范围，未推测封面位置"),
        };
        (
            ticks_to_us(*t, primary.timescale as i64),
            "source-huawei-frame".to_string(),
        )
    } else if let Some(us) = req.timestamp_us {
        if us < 0 {
            return fail("封面时间为负，无法映射");
        }
        let policy = match req.timestamp_source.as_deref() {
            Some("high-res-candidate") => "selected-high-res-candidate",
            Some("video-metadata") => "source-video-metadata",
            _ => "source-xmp",
        };
        (us, policy.to_string())
    } else if req.missing_time == "midpoint" {
        let first = sorted.first().unwrap().0;
        let last = sorted.last().unwrap().0;
        (
            ticks_to_us(first + (last - first) / 2, ts),
            "explicit-midpoint".to_string(),
        )
    } else {
        return fail("没有有效的封面时间来源；仅在明确指定时可用中点");
    };
    let domain = if req.time_domain.is_empty() {
        "auto"
    } else {
        req.time_domain.as_str()
    };
    let is_midpoint = policy == "explicit-midpoint";
    let is_huawei = policy == "source-huawei-frame";
    // Candidate list, mirroring Swift's domain/policy gating.
    let mut candidates: Vec<(i64, &'static str)> = Vec::new();
    if domain != "presentation" || is_midpoint || is_huawei {
        candidates.push((requested_us, "media"));
    }
    if domain != "media" && !is_midpoint && !is_huawei {
        let segments = edit_segments(data, selected_trak)?;
        match segments {
            None => candidates.push((requested_us, "presentation-identity")),
            Some(_) => {
                if let Some(mapped) = movie_to_media_us(data, selected_trak, ts, requested_us)? {
                    candidates.push((mapped, "presentation-edit-list"));
                }
            }
        }
    }
    // Nearest-sample match per candidate, half duration + 2 us, dedup by frame.
    let mut matches: Vec<(usize, i64, &'static str, i128)> = Vec::new();
    for (cand_us, label) in &candidates {
        let mut best: Option<(i128, usize)> = None;
        for (i, (pts, _)) in sorted.iter().enumerate() {
            let diff = (*pts as i128 * 1_000_000 - *cand_us as i128 * ts as i128).abs();
            match best {
                Some((b, _)) if diff >= b => {}
                _ => best = Some((diff, i)),
            }
        }
        let (diff, i) = match best {
            Some(v) => v,
            None => continue,
        };
        let duration = match duration_at(&sorted, i) {
            Some(d) if d > 0 => d as i128,
            _ => continue,
        };
        if 2 * diff <= duration * 1_000_000 + 4 * ts as i128
            && !matches.iter().any(|(f, _, _, _)| *f == i)
        {
            matches.push((i, *cand_us, label, diff));
        }
    }
    if matches.len() > 1 {
        return fail("封面时间歧义：媒体时间线与编辑列表指向不同帧；请指定时间域 media 或 presentation");
    }
    let (frame, mapped_us, label, diff) = match matches.into_iter().next() {
        Some(m) => m,
        None => {
            if req.video_track == "secondary" {
                return fail("辅助轨无法对应原始封面时间；未修改封面或替换为主轨。可单独导出辅助轨");
            }
            return fail("封面时间超出视频媒体/编辑列表时间线；不自动回退中点");
        }
    };
    let (frame_time_ticks, _) = sorted[frame];
    let frame_duration_ticks = match duration_at(&sorted, frame) {
        Some(d) => d,
        None => return fail("无法确定封面帧时长"),
    };
    Ok(CoverMapping {
        frame,
        frame_time_us: ticks_to_us(frame_time_ticks, ts),
        frame_duration_us: ticks_to_us(frame_duration_ticks, ts),
        mapped_us,
        domain: label.to_string(),
        policy,
        error_us: ((diff + ts as i128 / 2) / ts as i128) as i64,
    })
}
