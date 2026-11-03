//! Live Photo movie writing without AVFoundation: Swift `movie()`,
//! `exportAuxiliary()` and `PhotoFormat.repairAuxiliaryDuration`.
//!
//! The movie is a sample-copy remux ([`crate::remux`]): the chosen video
//! track plus audio, the QuickTime keyed content identifier, and the
//! native-form still-image-time track on the cover frame. Every output is
//! read back and checked (samples, identifier, cover marker) before it is
//! returned. The timing report keeps the Swift keys.

use crate::engine::failure;
use crate::live::{self, TimeDomain};
use crate::model::Cover;
use crate::mp4::{self, BoxRef, Movie};
use crate::{live2motion, remux, sha256_hex, Error, Result};
use serde_json::{json, Value};

/// still-image-time sample length written by [`remux`] (1 tick at 600).
const MARKER_SECONDS: f64 = 1.0 / 600.0;

/// A Swift `movie(req)` request. `still` is the already written still,
/// whose MakerNote content identifier must equal `identifier`.
#[derive(Clone, Debug, Default)]
pub struct MovieRequest<'a> {
    pub still: Option<&'a [u8]>,
    pub identifier: &'a str,
    /// "primary" or "secondary".
    pub video_track: &'a str,
    pub timestamp_us: Option<i64>,
    pub cover_frame_index: Option<i64>,
    /// None, "high-res-candidate" or "video-metadata".
    pub timestamp_source: Option<&'a str>,
    /// "error" or "midpoint".
    pub missing_time: &'a str,
    /// "auto", "media" or "presentation".
    pub time_domain: &'a str,
}

fn video_tracks(movie: &Movie) -> Vec<&mp4::Track> {
    movie
        .tracks
        .iter()
        .filter(|t| t.samples.handler == "vide")
        .collect()
}

/// Concatenated sample bytes hash, sample order (Swift `trackInfo.hash`).
fn samples_hash(data: &[u8], t: &mp4::Track) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for s in &t.samples.samples {
        h.update(&data[s.offset..s.offset + s.size]);
    }
    crate::hex(&h.finalize())
}

/// Presentation-sorted (pts, duration-to-next) in track ticks.
fn sorted_frames(t: &mp4::Track) -> Vec<(i64, Option<i64>)> {
    let sorted = crate::cover::sorted_presentation(&t.samples);
    let last = t.samples.samples.last().map(|s| s.duration as i64);
    (0..sorted.len())
        .map(|i| {
            let d = if i + 1 < sorted.len() {
                Some(sorted[i + 1].0 - sorted[i].0)
            } else if last.is_some_and(|d| d > 0) {
                last
            } else if i > 0 {
                Some(sorted[i].0 - sorted[i - 1].0)
            } else {
                None
            };
            (sorted[i].0, d)
        })
        .collect()
}

fn ticks_to_seconds(ticks: i64, timescale: u32) -> f64 {
    ticks as f64 / timescale.max(1) as f64
}

fn domain(name: &str) -> TimeDomain {
    match name {
        "media" => TimeDomain::Media,
        "presentation" => TimeDomain::Presentation,
        _ => TimeDomain::Auto,
    }
}

/// Swift texts for the cover resolver's refusals.
pub(crate) fn cover_error(e: Error, secondary: bool) -> Error {
    let text = e.0.as_str();
    if text.starts_with("封面时间在媒体时间线与显示时间线上对应不同帧") {
        return Error::new("Ambiguous cover time: media and presentation timelines point to different frames; select --time-domain media or presentation");
    }
    if text.starts_with("封面时间不在视频媒体时间线") {
        if secondary {
            return Error::new(
                "辅助轨无法对应原始封面时间；未修改封面或替换为主轨。可单独导出辅助轨",
            );
        }
        return Error::new(
            "Cover PTS outside the video media/edit-list timelines; no automatic midpoint fallback",
        );
    }
    e
}

/// Swift `movie(req)`: returns the MOV bytes and the timing report.
pub fn movie(source: &[u8], req: &MovieRequest) -> Result<(Vec<u8>, Value)> {
    if req.identifier.is_empty() {
        return failure("Missing movie arguments");
    }
    if let Some(still) = req.still {
        if live::still_content_identifier(still)
            .ok()
            .flatten()
            .as_deref()
            != Some(req.identifier)
        {
            return failure("ImageIO could not read back the still ContentIdentifier");
        }
    }
    let parsed = Movie::parse(source)?;
    let videos = video_tracks(&parsed);
    if videos.is_empty() || videos.len() > 2 {
        return failure(format!(
            "Motion Photo requires a primary video track and at most one secondary track (found {})",
            videos.len()
        ));
    }
    let selection = if req.video_track.is_empty() {
        "primary"
    } else {
        req.video_track
    };
    if selection != "primary" && selection != "secondary" {
        return failure("未知的视频轨选择");
    }
    let secondary = selection == "secondary";
    if secondary && videos.len() < 2 {
        return failure("原片没有辅助视频轨，请选择主轨");
    }
    let chosen_index = usize::from(secondary);
    let chosen = videos[chosen_index];
    let frames = sorted_frames(chosen);
    if frames.is_empty() {
        return failure("Video has no samples");
    }
    if secondary && frames.len() < 2 {
        return failure(format!(
            "辅助轨只有 {} 帧，不能作为动态视频；请使用主轨或单独导出辅助轨",
            frames.len()
        ));
    }
    let kept_negative = parsed
        .tracks
        .iter()
        .filter(|t| std::ptr::eq(*t, chosen) || t.samples.handler == "soun")
        .any(|t| t.samples.samples.iter().any(|s| s.presentation_time < 0));
    if kept_negative {
        return failure(
            "Negative media sample PTS requires explicit edit-list handling; original retained",
        );
    }

    // Requested cover time and which timelines may interpret it.
    let ts = chosen.samples.timescale;
    let (requested_us, requested_seconds, policy, cover_domain) =
        if let Some(index) = req.cover_frame_index {
            let primary = sorted_frames(videos[0]);
            let Some(&(pts, _)) = usize::try_from(index).ok().and_then(|i| primary.get(i)) else {
                return failure("华为/荣耀原始封面帧号超出视频范围，未推测封面位置");
            };
            let pts_ts = videos[0].samples.timescale;
            let us = (pts as i128 * 1_000_000 / pts_ts.max(1) as i128) as i64;
            (
                us,
                ticks_to_seconds(pts, pts_ts),
                "source-huawei-frame",
                TimeDomain::Media,
            )
        } else if let Some(us) = req.timestamp_us.filter(|us| *us >= 0) {
            let policy = match req.timestamp_source {
                Some("high-res-candidate") => "selected-high-res-candidate",
                Some("user-frame") => "selected-user-frame",
                Some("video-metadata") => "source-video-metadata",
                _ => "source-xmp",
            };
            (us, us as f64 / 1e6, policy, domain(req.time_domain))
        } else if req.missing_time == "midpoint" {
            let (first, last) = (frames[0].0, frames[frames.len() - 1].0);
            let mid = first as f64 + (last - first) as f64 * 0.5;
            let seconds = mid / ts.max(1) as f64;
            (
                (seconds * 1e6).round() as i64,
                seconds,
                "explicit-midpoint",
                TimeDomain::Media,
            )
        } else {
            return failure(
                "No valid source cover timestamp; use --missing-time midpoint only if intended",
            );
        };
    let cover = live::resolve_cover_in(
        source,
        &parsed,
        Cover::Time(requested_us),
        cover_domain,
        chosen_index,
    )
    .map_err(|e| cover_error(e, secondary))?;
    let frame_duration = frames[cover.frame]
        .1
        .filter(|d| *d > 0)
        .ok_or_else(|| Error::new("Cannot determine cover frame duration"))?;

    let media_timeline = remux::simple_media_timeline(source, &parsed, chosen_index)?;
    let marker_us = if media_timeline {
        cover.frame_time_us
    } else {
        cover.marker_us
    };
    let (mov, out) = if media_timeline {
        remux::remux_mov_media(
            source,
            &parsed,
            chosen_index,
            Some(req.identifier),
            marker_us,
        )?
    } else {
        remux::remux_mov(
            source,
            &parsed,
            chosen_index,
            Some(req.identifier),
            Some(marker_us),
        )?
    };
    // Read back: identifier, one marker on the cover frame.
    if remux::read_content_identifier(&mov)?.as_deref() != Some(req.identifier) {
        return failure("Output content identifier verification failed");
    }
    if live2motion::mett_cover_time_in(&mov, &out)? != Some(marker_us) {
        return failure("Written cover timestamp did not survive MOV readback");
    }
    let back = live::resolve_cover_in(
        &mov,
        &out,
        Cover::Time(marker_us),
        TimeDomain::Presentation,
        0,
    )?;
    if back.frame != cover.frame {
        return failure("Written cover timestamp did not survive MOV readback");
    }

    // Track report: video first, then audio, like AVFoundation's lists.
    let out_tracks: Vec<&mp4::Track> = out
        .tracks
        .iter()
        .filter(|t| t.samples.handler == "vide")
        .chain(out.tracks.iter().filter(|t| t.samples.handler == "soun"))
        .collect();
    let tracks: Vec<Value> = out_tracks
        .iter()
        .map(|t| {
            json!({
                "type": t.samples.handler,
                "packets": t.samples.samples.len(),
                "sha256": samples_hash(&mov, t),
                "max_pts_shift_seconds": 0.0,
            })
        })
        .collect();
    let ids: Vec<u32> = videos
        .iter()
        .map(|t| remux::track_header(source, &t.trak).map(|h| h.0))
        .collect::<Result<_>>()?;
    let (chosen_id, width, height) = remux::track_header(source, &chosen.trak)?;
    let timing = json!({
        "requested_seconds": requested_seconds,
        "mapped_source_seconds": cover.mapped_us.unwrap_or(requested_us) as f64 / 1e6,
        "timestamp_domain": cover.domain,
        "cover_seconds": marker_us as f64 / 1e6,
        "source_frame_duration_seconds": ticks_to_seconds(frame_duration, ts),
        "cover_duration_seconds": MARKER_SECONDS,
        "snap_error_seconds": cover.error_us.unwrap_or(0) as f64 / 1e6,
        "timing_policy": policy,
        "tracks": tracks,
        "identifier": req.identifier,
        "source_video_track_count": videos.len(),
        "primary_source_track_id": ids[0],
        "secondary_source_track_ids": ids[1..].to_vec(),
        "video_selection_policy": if secondary { "explicit-secondary" } else { "google-primary-first" },
        "selected_source_track_id": chosen_id,
        "selected_video_role": selection,
        "selected_frame_count": frames.len(),
        "selected_width": width,
        "selected_height": height,
    });
    Ok((mov, timing))
}

/// Swift `exportAuxiliary`: the secondary video track alone, silent, no
/// cover marker and no identifier.
pub fn export_auxiliary(source: &[u8]) -> Result<(Vec<u8>, Value)> {
    let parsed = Movie::parse(source)?;
    let videos = video_tracks(&parsed);
    if videos.len() != 2 {
        return failure("原片没有唯一的辅助视频轨，无法单独导出");
    }
    let track = videos[1];
    let samples = &track.samples.samples;
    // AVFoundation cannot read zero-duration samples; the batch repairs them
    // only when the user allows it (repair_auxiliary_duration).
    let total: u64 = samples.iter().map(|s| s.duration).sum();
    if samples.is_empty() || samples.iter().any(|s| s.presentation_time < 0) || total == 0 {
        return failure(
            "系统无法读取辅助轨有效样本；若为零时长样本，可在高级设置中允许修复后单独导出",
        );
    }
    let (mov, out) = remux::remux_mov_with(source, &parsed, 1, None, None, false)?;
    let out_videos = video_tracks(&out);
    if out.tracks.len() != 1 || out_videos.len() != 1 {
        return failure("辅助视频轨数校验失败");
    }
    let hash = samples_hash(&mov, out_videos[0]);
    if hash != samples_hash(source, track) {
        return failure("辅助视频压缩数据校验失败");
    }
    let (id, width, height) = remux::track_header(source, &track.trak)?;
    let info = json!({
        "source_track_id": id,
        "frames": samples.len(),
        "width": width,
        "height": height,
        "duration_seconds": total as f64 / track.samples.timescale.max(1) as f64,
        "compressed_sha256": hash,
        "max_pts_shift_seconds": 0.0,
        "audio_included": false,
    });
    Ok((mov, info))
}

fn child(d: &[u8], parent: &BoxRef, t: &[u8; 4]) -> Result<BoxRef> {
    let found: Vec<BoxRef> = mp4::boxes(d, parent.payload, parent.end)?
        .into_iter()
        .filter(|b| b.is(t))
        .collect();
    if found.len() != 1 {
        return failure(format!(
            "Google 视频元数据缺少唯一的 {}",
            String::from_utf8_lossy(t)
        ));
    }
    Ok(found[0].clone())
}

fn word(d: &[u8], b: &BoxRef, at: usize, width: usize) -> Result<usize> {
    if b.payload + at + width > b.end {
        return failure("Google 视频元数据表被截断");
    }
    crate::binary::be(d, b.payload + at, width)
}

fn put32(d: &mut [u8], at: usize, v: u64) -> Result<()> {
    let v = u32::try_from(v).map_err(|_| Error::new("无法安全修正文件偏移"))?;
    d.get_mut(at..at + 4)
        .ok_or_else(|| Error::new("无法安全修正文件偏移"))?
        .copy_from_slice(&v.to_be_bytes());
    Ok(())
}

/// Swift `PhotoFormat.repairAuxiliaryDuration`: some GCam files carry one
/// real auxiliary sample with zero stts/mdhd/tkhd duration. Patch only
/// those fixed-size fields with the primary track's shortest positive
/// frame duration. None when the file does not have that layout.
pub fn repair_auxiliary_duration(data: &[u8]) -> Result<Option<(Vec<u8>, Value)>> {
    let top = mp4::boxes(data, 0, data.len())?;
    let Some(moov) = top.iter().find(|b| b.is(b"moov")) else {
        return Ok(None);
    };
    let mut videos = Vec::new();
    for t in mp4::boxes(data, moov.payload, moov.end)?
        .into_iter()
        .filter(|b| b.is(b"trak"))
    {
        let mdia = child(data, &t, b"mdia")?;
        let h = child(data, &mdia, b"hdlr")?;
        if h.payload + 12 <= h.end && &data[h.payload + 8..h.payload + 12] == b"vide" {
            videos.push(t);
        }
    }
    if videos.len() != 2 {
        return Ok(None);
    }
    let main_media = child(data, &videos[0], b"mdia")?;
    let aux_media = child(data, &videos[1], b"mdia")?;
    let main_header = child(data, &main_media, b"mdhd")?;
    let aux_header = child(data, &aux_media, b"mdhd")?;
    let track_header = child(data, &videos[1], b"tkhd")?;
    let movie_header = child(data, moov, b"mvhd")?;
    if ![&main_header, &aux_header, &track_header, &movie_header]
        .iter()
        .all(|b| b.payload < b.end && data[b.payload] == 0)
    {
        return Ok(None);
    }
    let main_table = child(data, &child(data, &main_media, b"minf")?, b"stbl")?;
    let aux_table = child(data, &child(data, &aux_media, b"minf")?, b"stbl")?;
    let times = child(data, &aux_table, b"stts")?;
    let sizes = child(data, &aux_table, b"stsz")?;
    let chunks = child(data, &aux_table, b"stsc")?;
    if !(word(data, &times, 4, 4)? == 1
        && word(data, &times, 8, 4)? == 1
        && word(data, &times, 12, 4)? == 0
        && word(data, &sizes, 8, 4)? == 1
        && word(data, &aux_header, 16, 4)? == 0
        && word(data, &track_header, 20, 4)? == 0)
    {
        return Ok(None);
    }
    let in_trak = mp4::boxes(data, videos[1].payload, videos[1].end)?;
    let in_stbl = mp4::boxes(data, aux_table.payload, aux_table.end)?;
    if in_trak.iter().any(|b| b.is(b"edts"))
        || in_stbl.iter().any(|b| b.is(b"ctts"))
        || word(data, &chunks, 4, 4)? != 1
        || word(data, &chunks, 8, 4)? != 1
        || word(data, &chunks, 12, 4)? != 1
        || word(data, &chunks, 16, 4)? != 1
    {
        return failure("零时长辅助轨的布局不能安全修复");
    }
    let offsets: Vec<&BoxRef> = in_stbl
        .iter()
        .filter(|b| b.is(b"stco") || b.is(b"co64"))
        .collect();
    if offsets.len() != 1 || word(data, offsets[0], 4, 4)? != 1 {
        return failure("零时长辅助轨偏移无效");
    }
    let offset = word(
        data,
        offsets[0],
        8,
        if offsets[0].is(b"co64") { 8 } else { 4 },
    )?;
    let fixed = word(data, &sizes, 4, 4)?;
    let size = if fixed == 0 {
        word(data, &sizes, 12, 4)?
    } else {
        fixed
    };
    if !(size > 0
        && offset <= data.len()
        && size <= data.len() - offset
        && top
            .iter()
            .any(|b| b.is(b"mdat") && offset >= b.payload && offset + size <= b.end))
    {
        return failure("零时长辅助轨的压缩样本范围无效");
    }
    let main_times = child(data, &main_table, b"stts")?;
    let count = word(data, &main_times, 4, 4)?;
    if count == 0 || count > (main_times.end - main_times.payload).saturating_sub(8) / 8 {
        return failure("主轨帧长表无效");
    }
    let mut step: Option<usize> = None;
    for i in 0..count {
        let delta = word(data, &main_times, 12 + i * 8, 4)?;
        if delta > 0 {
            step = Some(step.map_or(delta, |s| s.min(delta)));
        }
    }
    let main_scale = word(data, &main_header, 12, 4)?;
    let aux_scale = word(data, &aux_header, 12, 4)?;
    let movie_scale = word(data, &movie_header, 12, 4)?;
    let Some(step) = step.filter(|_| main_scale > 0 && aux_scale > 0 && movie_scale > 0) else {
        return failure("无法从主轨确定有效帧长");
    };
    let seconds = step as f64 / main_scale as f64;
    let aux_value = (seconds * aux_scale as f64).ceil();
    let movie_value = (seconds * movie_scale as f64).ceil();
    if !(aux_value.is_finite()
        && movie_value.is_finite()
        && aux_value <= u32::MAX as f64
        && movie_value <= u32::MAX as f64)
    {
        return failure("修复辅助轨时长超出容器范围");
    }
    let aux_ticks = (aux_value as u64).max(1);
    let movie_ticks = (movie_value as u64).max(1);
    let mut repaired = data.to_vec();
    put32(&mut repaired, times.payload + 12, aux_ticks)?;
    put32(&mut repaired, aux_header.payload + 16, aux_ticks)?;
    put32(&mut repaired, track_header.payload + 20, movie_ticks)?;
    let info = json!({
        "method": "primary-shortest-positive-frame-duration",
        "original_duration_seconds": 0,
        "repaired_duration_seconds": aux_ticks as f64 / aux_scale as f64,
        "sample_pts_unchanged": true,
        "original_sample_sha256": sha256_hex(&data[offset..offset + size]),
    });
    Ok(Some((repaired, info)))
}
