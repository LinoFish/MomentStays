//! Live Photo -> Motion Photo assembly (ROADMAP item 8).
//!
//! Mirrors Swift `AndroidExport.convert` minus HDR preparation (the caller
//! passes the prepared still): remux the MOV to MP4 (`remux_to_mp4`),
//! clean the still (`pymotion::live_still_clean`), then `model::write`
//! for the vendor JPEG. `model::write` already validates parse-back
//! (video bytes, still coding hash, cover), which is Swift's
//! `decoded.video == video` guard and cover check combined.
//!
//! Cover kinds follow `model::write`: Huawei needs `Cover::Frame`, other
//! vendors need `Cover::Time`. Convert with `cover.rs` first when the
//! available form does not match the target vendor.

use crate::model::{self, Cover, MotionModel, Vendor};
use crate::mp4::{self, BoxRef, Movie};
use crate::pymotion;
use crate::remux;
use crate::{fail, Result};

pub struct LivePhotoInput {
    pub still_jpeg: Vec<u8>,
    pub movie_mov: Vec<u8>,
}

pub fn assemble(input: &LivePhotoInput, vendor: Vendor, cover: Cover) -> Result<Vec<u8>> {
    let (still, video) = prepare(&input.still_jpeg, &input.movie_mov)?;
    let model = MotionModel {
        still,
        video,
        cover,
        source_format: "apple-live".to_string(),
    };
    model::write(&model, vendor)
}

/// The two inputs of a vendor writer for a Live Photo pair: the cleaned
/// still and the MOV remuxed to MP4 (already verified sample by sample by
/// `remux_to_mp4`). Feed them to `model::plan` to stream the output to a
/// file instead of building it in memory with [`assemble`].
pub fn prepare(still_jpeg: &[u8], movie_mov: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    prepare_parsed(still_jpeg, movie_mov, &Movie::parse(movie_mov)?)
}

/// [`prepare`] with the MOV already parsed.
pub fn prepare_parsed(
    still_jpeg: &[u8],
    movie_mov: &[u8],
    movie: &Movie,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let video = remux::remux_mp4(movie_mov, movie)?;
    let still = pymotion::live_still_clean(still_jpeg)?;
    Ok((still, video))
}

fn children(d: &[u8], parent: &BoxRef) -> Result<Vec<BoxRef>> {
    mp4::boxes(d, parent.payload, parent.end)
}

fn only(d: &[u8], parent: &BoxRef, t: &[u8; 4], what: &str) -> Result<BoxRef> {
    let found: Vec<BoxRef> = children(d, parent)?
        .into_iter()
        .filter(|b| b.is(t))
        .collect();
    if found.len() != 1 {
        return fail(format!("still-image-time 缺少唯一的{what}"));
    }
    Ok(found[0].clone())
}

/// Key box identifying a still-image-time `mebx` entry: `keyd` +
/// namespace `mdta` + key `com.apple.quicktime.still-image-time`.
/// Native iPhone files carry three `mebx` meta tracks
/// (video-orientation, live-photo-info, still-image-time); only the key
/// string tells them apart, so a bare `mebx` match is not enough.
const STILL_IMAGE_TIME_KEYD: &[u8] = b"keydmdtacom.apple.quicktime.still-image-time";

/// Cover time in microseconds from a Live Photo MOV's still-image-time
/// track: the single marker sample's presentation time, mapped through
/// the track's edit list into the movie timeline. Returns None when the
/// file has no marker track.
///
/// Two entry forms are accepted: Swift `movie()`'s `mett` entry, and the
/// native iPhone `mebx` entry (ROADMAP item 9b2), recognized only when
/// its `keys` box carries the still-image-time key above.
///
/// The sample VALUE is deliberately not checked: native files write -1
/// (`0xff`), Swift `movie()` writes 0, and Swift's reader accepts
/// {-1, 0, 255}. Timing comes only from the sample tables and the edit
/// list, never from payload bytes.
pub fn mett_cover_time(mov: &[u8]) -> Result<Option<i64>> {
    mett_cover_time_in(mov, &Movie::parse(mov)?)
}

/// [`mett_cover_time`] on an already parsed movie.
pub fn mett_cover_time_in(mov: &[u8], movie: &Movie) -> Result<Option<i64>> {
    let movie_ts = movie.timescale as i64;
    let mut found: Option<usize> = None;
    for (i, track) in movie.tracks.iter().enumerate() {
        let trak = &track.trak;
        let mdia = only(mov, trak, b"mdia", "mdia")?;
        let hdlr = only(mov, &mdia, b"hdlr", "hdlr")?;
        if hdlr.end - hdlr.payload < 12 || &mov[hdlr.payload + 8..hdlr.payload + 12] != b"meta" {
            continue;
        }
        let minf = only(mov, &mdia, b"minf", "minf")?;
        let stbl = only(mov, &minf, b"stbl", "stbl")?;
        let stsd = only(mov, &stbl, b"stsd", "stsd")?;
        let entries = mp4::boxes(mov, stsd.payload + 8, stsd.end)?;
        let is_marker = if entries.len() == 1 && entries[0].is(b"mett") {
            true
        } else if entries.len() == 1 && entries[0].is(b"mebx") {
            let entry = &mov[entries[0].start..entries[0].end];
            entry
                .windows(STILL_IMAGE_TIME_KEYD.len())
                .any(|w| w == STILL_IMAGE_TIME_KEYD)
        } else {
            false
        };
        if is_marker {
            if found.is_some() {
                return fail("多个 still-image-time 轨道");
            }
            found = Some(i);
        }
    }
    let i = match found {
        Some(i) => i,
        None => return Ok(None),
    };
    let table = &movie.tracks[i].samples;
    if table.samples.len() != 1 {
        return fail("still-image-time 标记数量异常");
    }
    let media_time = table.samples[0].presentation_time;
    let track_ts = table.timescale as i64;
    if track_ts <= 0 {
        return fail("still-image-time 轨道时间尺度无效");
    }
    // i128 throughout: edit durations (v1: up to 2^63) times a 32-bit
    // timescale cannot overflow, so crafted values fail instead of wrapping.
    let (media_time, movie_ts, track_ts) = (media_time as i128, movie_ts as i128, track_ts as i128);
    let movie_time: i128 = match mp4::edit_list(mov, &movie.tracks[i].trak)? {
        None => (media_time * movie_ts + track_ts / 2) / track_ts,
        Some(entries) => {
            let mut movie_cursor: i128 = 0;
            let mut mapped: Option<i128> = None;
            for (seg_dur, media_start) in entries {
                let (seg_dur, media_start) = (seg_dur as i128, media_start as i128);
                if media_start < 0 {
                    movie_cursor += seg_dur;
                    continue;
                }
                let seg_track = (seg_dur * track_ts + movie_ts / 2) / movie_ts;
                if media_start <= media_time && media_time < media_start + seg_track {
                    mapped = Some(movie_cursor + (media_time - media_start) * movie_ts / track_ts);
                    break;
                }
                movie_cursor += seg_dur;
            }
            match mapped {
                Some(m) => m,
                None => return fail("still-image-time 标记不在编辑列表内"),
            }
        }
    };
    if movie_time < 0 {
        return fail("still-image-time 标记时间为负");
    }
    let us = (movie_time * 1_000_000 + movie_ts / 2) / movie_ts;
    match i64::try_from(us) {
        Ok(us) => Ok(Some(us)),
        Err(_) => fail("still-image-time 标记时间超出范围"),
    }
}
