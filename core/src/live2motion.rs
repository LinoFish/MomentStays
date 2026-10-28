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
use crate::mp4::{self, BoxRef};
use crate::pymotion;
use crate::remux;
use crate::{fail, Result};

pub struct LivePhotoInput {
    pub still_jpeg: Vec<u8>,
    pub movie_mov: Vec<u8>,
}

pub fn assemble(input: &LivePhotoInput, vendor: Vendor, cover: Cover) -> Result<Vec<u8>> {
    let video = remux::remux_to_mp4(&input.movie_mov)?;
    let still = pymotion::live_still_clean(&input.still_jpeg)?;
    let model = MotionModel { still, video, cover, source_format: "apple-live".to_string() };
    model::write(&model, vendor)
}

fn children(d: &[u8], parent: &BoxRef) -> Result<Vec<BoxRef>> {
    mp4::boxes(d, parent.payload, parent.end)
}

fn only(d: &[u8], parent: &BoxRef, t: &[u8; 4], what: &str) -> Result<BoxRef> {
    let found: Vec<BoxRef> = children(d, parent)?.into_iter().filter(|b| b.is(t)).collect();
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
    let tables = mp4::sample_tables(mov)?;
    let top = mp4::boxes(mov, 0, mov.len())?;
    let moov = match top.iter().find(|b| b.is(b"moov")) {
        Some(m) => m.clone(),
        None => return Ok(None),
    };
    let traks: Vec<BoxRef> = children(mov, &moov)?.into_iter().filter(|b| b.is(b"trak")).collect();
    if traks.len() != tables.len() {
        return fail("still-image-time 轨道数与采样表不一致");
    }
    let mvhd = only(mov, &moov, b"mvhd", "mvhd")?;
    let movie_ts = {
        let v = mov[mvhd.payload];
        let at = if v == 1 { 20 } else { 12 };
        if mvhd.end - mvhd.payload < at + 4 {
            return fail("still-image-time mvhd 被截断");
        }
        crate::binary::be(mov, mvhd.payload + at, 4)? as i64
    };
    if movie_ts <= 0 {
        return fail("still-image-time 电影时间尺度无效");
    }
    let mut found: Option<usize> = None;
    for (i, trak) in traks.iter().enumerate() {
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
            entry.windows(STILL_IMAGE_TIME_KEYD.len()).any(|w| w == STILL_IMAGE_TIME_KEYD)
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
    let table = &tables[i];
    if table.samples.len() != 1 {
        return fail("still-image-time 标记数量异常");
    }
    let media_time = table.samples[0].presentation_time;
    let track_ts = table.timescale as i64;
    if track_ts <= 0 {
        return fail("still-image-time 轨道时间尺度无效");
    }
    let movie_time = match edts_entries(mov, &traks[i])? {
        None => (media_time * movie_ts + track_ts / 2) / track_ts,
        Some(entries) => {
            let mut movie_cursor: i64 = 0;
            let mut mapped: Option<i64> = None;
            for (seg_dur, media_start) in entries {
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
    Ok(Some((movie_time * 1_000_000 + movie_ts / 2) / movie_ts))
}

/// Edit-list entries as (segment_duration in movie timescale, media_time
/// in track timescale, -1 for an empty edit), or None if the track has no
/// edts box.
fn edts_entries(d: &[u8], trak: &BoxRef) -> Result<Option<Vec<(i64, i64)>>> {
    let edts_list: Vec<BoxRef> = children(d, trak)?.into_iter().filter(|b| b.is(b"edts")).collect();
    if edts_list.is_empty() {
        return Ok(None);
    }
    if edts_list.len() > 1 {
        return fail("still-image-time 重复出现 edts");
    }
    let elst_list: Vec<BoxRef> = children(d, &edts_list[0])?.into_iter().filter(|b| b.is(b"elst")).collect();
    if elst_list.len() != 1 {
        return fail("still-image-time 编辑列表缺少唯一的 elst");
    }
    let elst = &elst_list[0];
    if elst.end - elst.payload < 8 {
        return fail("still-image-time elst 被截断");
    }
    let version = d[elst.payload];
    let count = crate::binary::be(d, elst.payload + 4, 4)?;
    let (record, at) = if version == 1 { (20usize, elst.payload + 8) } else { (12usize, elst.payload + 8) };
    if count > (elst.end - at) / record {
        return fail("still-image-time elst 被截断");
    }
    let mut out = Vec::new();
    for i in 0..count {
        let p = at + i * record;
        if version == 1 {
            let dur_u = crate::binary::be(d, p, 8)? as u64;
            if dur_u > i64::MAX as u64 {
                return fail("still-image-time 编辑时长超出范围");
            }
            // Media time is signed (empty edits use -1); reinterpret the bits.
            let media = crate::binary::be(d, p + 8, 8)? as u64 as i64;
            out.push((dur_u as i64, media));
        } else {
            let dur = crate::binary::be(d, p, 4)? as i64;
            let media = crate::binary::be(d, p + 4, 4)? as u32 as i32 as i64;
            out.push((dur, media));
        }
    }
    Ok(Some(out))
}
