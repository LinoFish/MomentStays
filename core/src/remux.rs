//! MP4 -> MOV sample-copy remux without re-encoding (ROADMAP item 9a).
//!
//! Strategy: keep everything timing-related verbatim (stts/stsc/stsz/ctts/stss
//! boxes, track timescales, sample order and chunk grouping) and only rebuild
//! chunk offsets (stco/co64) for the new mdat layout, plus fresh mvhd/tkhd/mdhd
//! headers. Sample bytes are copied, never decoded. The result is a QuickTime
//! movie (ftyp `qt  `, movie timescale 1_000_000 like Swift `movie()`), with
//! the chosen video track plus all audio tracks; other tracks (Google mett,
//! hint, text) are dropped. The edit list is copied with segment durations
//! rescaled to the 1_000_000 movie timescale, so the output playback timeline
//! matches the source (verified on a real file: ffprobe pts/dts identical).
//!
//! Validation mirrors Swift `movie()`: per kept track, sample count, sizes,
//! concatenated sample bytes hash and presentation times must match exactly
//! (shift 0 here, since tables are copied verbatim, stricter than Swift's
//! one-tick-plus-1us tolerance). Negative presentation times are refused,
//! like Swift's "no explicit edit-list handling" rule.

use crate::mp4::{self, BoxRef};
use crate::{fail, sha256_hex, Result};
use std::collections::HashSet;

pub const MOVIE_TIMESCALE: u32 = 1_000_000;

fn bx(t: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(t);
    v.extend(payload);
    v
}

fn full(t: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = vec![version, ((flags >> 16) & 0xff) as u8, ((flags >> 8) & 0xff) as u8, (flags & 0xff) as u8];
    p.extend(payload);
    bx(t, &p)
}

fn u32b(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

fn u16b(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

fn children(d: &[u8], parent: &BoxRef) -> Result<Vec<BoxRef>> {
    mp4::boxes(d, parent.payload, parent.end)
}

fn only(d: &[u8], parent: &BoxRef, t: &[u8; 4], what: &str) -> Result<BoxRef> {
    let found: Vec<BoxRef> = children(d, parent)?.into_iter().filter(|b| b.is(t)).collect();
    if found.len() != 1 {
        return fail(format!("重封装缺少唯一的{what}"));
    }
    Ok(found[0].clone())
}

/// Parsed per-track pieces needed to rebuild the track.
struct TrackParts {
    handler: String,
    timescale: u32,
    track_id: u32,
    width: u32,
    height: u32,
    layer: u16,
    alternate_group: u16,
    volume: u16,
    matrix: [u8; 36],
    mdhd_language: [u8; 2],
    hdlr: Vec<u8>,
    /// Rescaled edit list (edts box) if the source track had one; segment
    /// durations are converted to the 1_000_000 movie timescale, media times
    /// (track timescale) are kept verbatim. Absent when the source had none.
    edts: Option<Vec<u8>>,
    minf_others: Vec<Vec<u8>>,
    stbl_ordered: Vec<Vec<u8>>,
    stco_is64: bool,
    stco_pos: usize,
    samples: Vec<mp4::Sample>,
    chunks: Vec<std::ops::Range<usize>>,
}

fn parse_tkhd(d: &[u8], tkhd: &BoxRef) -> Result<(u32, u32, u32, u16, u16, u16, [u8; 36])> {
    let p = tkhd.payload;
    if tkhd.end - p < 4 {
        return fail("重封装 tkhd 被截断");
    }
    let version = d[p];
    let (id_at, layer_at, matrix_at, width_at) = if version == 1 {
        (20usize, 44usize, 52usize, 88usize)
    } else {
        (12usize, 32usize, 40usize, 76usize)
    };
    if tkhd.end - p < width_at + 8 {
        return fail("重封装 tkhd 被截断");
    }
    let track_id = crate::binary::be(d, p + id_at, 4)? as u32;
    let layer = crate::binary::be(d, p + layer_at, 2)? as u16;
    let alt = crate::binary::be(d, p + layer_at + 2, 2)? as u16;
    let volume = crate::binary::be(d, p + layer_at + 4, 2)? as u16;
    let mut matrix = [0u8; 36];
    matrix.copy_from_slice(&d[p + matrix_at..p + matrix_at + 36]);
    let width = crate::binary::be(d, p + width_at, 4)? as u32;
    let height = crate::binary::be(d, p + width_at + 4, 4)? as u32;
    Ok((track_id, width, height, layer, alt, volume, matrix))
}

fn track_duration_span(samples: &[mp4::Sample]) -> u64 {
    match samples.last() {
        Some(s) => s.decode_time + s.duration,
        None => 0,
    }
}

fn rescale_to_movie(span: u64, timescale: u32) -> Result<u32> {
    if timescale == 0 {
        return fail("重封装时间尺度无效");
    }
    let v = (span * MOVIE_TIMESCALE as u64 + timescale as u64 / 2) / timescale as u64;
    if v > u32::MAX as u64 {
        return fail("重封装时长超出 MOV 版本 0 范围");
    }
    Ok(v as u32)
}

/// Rescale an edit list to the output movie timescale, or None if the track
/// has no edts box. Segment durations are rounded to nearest; media times
/// and rates are copied verbatim (media time -1 marks an empty edit).
fn maybe_edts(d: &[u8], trak: &BoxRef, movie_ts: usize) -> Result<Option<Vec<u8>>> {
    let edts_list: Vec<BoxRef> = children(d, trak)?.into_iter().filter(|b| b.is(b"edts")).collect();
    if edts_list.is_empty() {
        return Ok(None);
    }
    if edts_list.len() > 1 {
        return fail("重封装重复出现 edts");
    }
    let elst_list: Vec<BoxRef> = children(d, &edts_list[0])?.into_iter().filter(|b| b.is(b"elst")).collect();
    if elst_list.len() != 1 {
        return fail("重封装编辑列表缺少唯一的 elst");
    }
    let elst = &elst_list[0];
    if elst.end - elst.payload < 8 {
        return fail("重封装 elst 被截断");
    }
    let version = d[elst.payload];
    let flags = ((d[elst.payload + 1] as u32) << 16)
        | ((d[elst.payload + 2] as u32) << 8)
        | d[elst.payload + 3] as u32;
    let count = crate::binary::be(d, elst.payload + 4, 4)?;
    let (record, at) = if version == 1 { (20usize, elst.payload + 8) } else { (12usize, elst.payload + 8) };
    if count > (elst.end - at) / record {
        return fail("重封装 elst 被截断");
    }
    let mut body = Vec::new();
    body.extend(u32b(count as u32));
    for i in 0..count {
        let p = at + i * record;
        if version == 1 {
            let dur = crate::binary::int(d, p, 8, false)? as u64;
            let scaled = dur
                .checked_mul(MOVIE_TIMESCALE as u64)
                .and_then(|v| v.checked_add(movie_ts as u64 / 2))
                .map(|v| v / movie_ts as u64)
                .ok_or_else(|| crate::Error::new("重封装编辑时长溢出"))?;
            body.extend(scaled.to_be_bytes());
            body.extend_from_slice(&d[p + 8..p + 20]);
        } else {
            let dur = crate::binary::be(d, p, 4)? as u64;
            let scaled = (dur * MOVIE_TIMESCALE as u64 + movie_ts as u64 / 2) / movie_ts as u64;
            if scaled > u32::MAX as u64 {
                return fail("重封装编辑时长超出范围");
            }
            body.extend(u32b(scaled as u32));
            body.extend_from_slice(&d[p + 4..p + 12]);
        }
    }
    let elst_new = full(b"elst", version, flags, &body);
    Ok(Some(bx(b"edts", &elst_new)))
}

fn parse_tracks(d: &[u8], video_index: usize) -> Result<(Vec<TrackParts>, u32)> {
    let tables = mp4::sample_tables(d)?;
    let top = mp4::boxes(d, 0, d.len())?;
    let moov = match top.iter().find(|b| b.is(b"moov")) {
        Some(m) => m.clone(),
        None => return fail("重封装缺少 moov"),
    };
    let traks: Vec<BoxRef> = children(d, &moov)?.into_iter().filter(|b| b.is(b"trak")).collect();
    if traks.len() != tables.len() {
        return fail("重封装轨道数与采样表不一致");
    }
    // Source movie timescale, needed to rescale edit-list segment durations.
    let mvhd = only(d, &moov, b"mvhd", "mvhd")?;
    let movie_ts = {
        let v = d[mvhd.payload];
        let at = if v == 1 { 20 } else { 12 };
        if mvhd.end - mvhd.payload < at + 4 {
            return fail("重封装 mvhd 被截断");
        }
        crate::binary::be(d, mvhd.payload + at, 4)?
    };
    if movie_ts == 0 {
        return fail("重封装电影时间尺度无效");
    }
    let mut video_seen = 0usize;
    let mut kept: Vec<TrackParts> = Vec::new();
    let mut max_track_id = 0u32;
    for (trak, table) in traks.iter().zip(tables.iter()) {
        let tkhd = only(d, trak, b"tkhd", "tkhd")?;
        let mdia = only(d, trak, b"mdia", "mdia")?;
        let mdhd = only(d, &mdia, b"mdhd", "mdhd")?;
        let hdlr = only(d, &mdia, b"hdlr", "hdlr")?;
        let minf = only(d, &mdia, b"minf", "minf")?;
        let stbl = only(d, &minf, b"stbl", "stbl")?;
        if table.handler != String::from_utf8_lossy(&d[hdlr.payload + 8..hdlr.payload + 12]) {
            return fail("重封装轨道与采样表不一致");
        }
        let is_video = table.handler == "vide";
        let is_audio = table.handler == "soun";
        if is_video {
            if video_seen != video_index {
                video_seen += 1;
                continue;
            }
            video_seen += 1;
        } else if !is_audio {
            continue;
        }
        if is_video && table.samples.is_empty() {
            return fail("视频没有样本，无法重封装");
        }
        if table.samples.iter().any(|s| s.presentation_time < 0) {
            return fail("视频含负时间样本，暂不无损重封装");
        }
        let (track_id, width, height, layer, alt, volume, matrix) = parse_tkhd(d, &tkhd)?;
        max_track_id = max_track_id.max(track_id);
        let mdhd_version = d[mdhd.payload];
        let lang_at = if mdhd_version == 1 { 32 } else { 20 };
        if mdhd.end - mdhd.payload < lang_at + 2 {
            return fail("重封装 mdhd 被截断");
        }
        let mut lang = [0u8; 2];
        lang.copy_from_slice(&d[mdhd.payload + lang_at..mdhd.payload + lang_at + 2]);
        // Edit list: rescale segment durations to MOVIE_TIMESCALE, keep
        // media times and rates verbatim.
        let edts = match maybe_edts(d, trak, movie_ts)? {
            Some(b) => Some(b),
            None => None,
        };
        let mut minf_others = Vec::new();
        for c in children(d, &minf)? {
            if !c.is(b"stbl") {
                minf_others.push(d[c.start..c.end].to_vec());
            }
        }
        let mut stbl_ordered = Vec::new();
        let mut stco_is64 = false;
        let mut stco_pos = 0usize;
        for (i, c) in children(d, &stbl)?.iter().enumerate() {
            if c.is(b"stco") || c.is(b"co64") {
                stco_is64 = c.is(b"co64");
                stco_pos = i;
            }
            stbl_ordered.push(d[c.start..c.end].to_vec());
        }
        kept.push(TrackParts {
            handler: table.handler.clone(),
            timescale: table.timescale,
            track_id,
            width,
            height,
            layer,
            alternate_group: alt,
            volume,
            matrix,
            mdhd_language: lang,
            hdlr: d[hdlr.start..hdlr.end].to_vec(),
            edts,
            minf_others,
            stbl_ordered,
            stco_is64,
            stco_pos,
            samples: table.samples.clone(),
            chunks: table.chunks.clone(),
        });
    }
    if !kept.iter().any(|t| t.handler == "vide") {
        return fail("重封装未找到选定的视频轨");
    }
    Ok((kept, max_track_id))
}

fn mvhd_box(duration: u32, next_track_id: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(u32b(0));
    p.extend(u32b(0));
    p.extend(u32b(MOVIE_TIMESCALE));
    p.extend(u32b(duration));
    p.extend(u32b(0x00010000));
    p.extend(u16b(0x0100));
    p.extend(u16b(0));
    p.extend([0u8; 8]);
    p.extend(u32b(0x00010000));
    p.extend(u32b(0));
    p.extend(u32b(0));
    p.extend(u32b(0));
    p.extend(u32b(0x00010000));
    p.extend(u32b(0));
    p.extend(u32b(0));
    p.extend(u32b(0));
    p.extend(u32b(0x40000000));
    p.extend([0u8; 24]);
    p.extend(u32b(next_track_id));
    full(b"mvhd", 0, 0, &p)
}

fn tkhd_box(t: &TrackParts, duration_movie: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(u32b(0));
    p.extend(u32b(0));
    p.extend(u32b(t.track_id));
    p.extend(u32b(0));
    p.extend(u32b(duration_movie));
    p.extend([0u8; 8]);
    p.extend(u16b(t.layer));
    p.extend(u16b(t.alternate_group));
    p.extend(u16b(t.volume));
    p.extend(u16b(0));
    p.extend(t.matrix);
    p.extend(u32b(t.width));
    p.extend(u32b(t.height));
    full(b"tkhd", 0, 7, &p)
}

fn mdhd_box(t: &TrackParts, duration_track: u64) -> Result<Vec<u8>> {
    if duration_track > u32::MAX as u64 {
        return fail("重封装时长超出 MOV 版本 0 范围");
    }
    let mut p = Vec::new();
    p.extend(u32b(0));
    p.extend(u32b(0));
    p.extend(u32b(t.timescale));
    p.extend(u32b(duration_track as u32));
    p.extend(t.mdhd_language);
    p.extend(u16b(0));
    Ok(full(b"mdhd", 0, 0, &p))
}

fn stco_box(is64: bool, offsets: &[usize]) -> Result<Vec<u8>> {
    let mut p = Vec::new();
    p.extend(u32b(offsets.len() as u32));
    for o in offsets {
        if is64 {
            p.extend((*o as u64).to_be_bytes());
        } else {
            if *o > u32::MAX as usize {
                return fail("重封装块偏移超出 32 位范围，请用 64 位源文件");
            }
            p.extend(u32b(*o as u32));
        }
    }
    Ok(full(if is64 { b"co64" } else { b"stco" }, 0, 0, &p))
}

fn build_moov(tracks: &[TrackParts], chunk_offsets: &[Vec<usize>], movie_duration: u32, next_track_id: u32) -> Result<Vec<u8>> {
    let mut moov_body = mvhd_box(movie_duration, next_track_id);
    for (t, offsets) in tracks.iter().zip(chunk_offsets.iter()) {
        let span = track_duration_span(&t.samples);
        let duration_movie = rescale_to_movie(span, t.timescale)?;
        let mut mdia_body = mdhd_box(t, span)?;
        mdia_body.extend(t.hdlr.clone());
        let mut stbl_boxes = t.stbl_ordered.clone();
        stbl_boxes[t.stco_pos] = stco_box(t.stco_is64, offsets)?;
        let mut minf_body = Vec::new();
        for b in &t.minf_others {
            minf_body.extend(b);
        }
        minf_body.extend(bx(b"stbl", &stbl_boxes.concat()));
        mdia_body.extend(bx(b"minf", &minf_body));
        let mut trak_body = tkhd_box(t, duration_movie);
        if let Some(edts) = &t.edts {
            trak_body.extend(edts);
        }
        trak_body.extend(bx(b"mdia", &mdia_body));
        moov_body.extend(bx(b"trak", &trak_body));
    }
    Ok(bx(b"moov", &moov_body))
}

/// Remux an MP4 to a QuickTime MOV: chosen video track (0-based among video
/// tracks) plus all audio tracks, samples copied verbatim.
pub fn remux_to_mov(data: &[u8], video_index: usize) -> Result<Vec<u8>> {
    let (tracks, max_track_id) = parse_tracks(data, video_index)?;
    // First build with zero offsets to measure moov, then rebuild with real ones.
    let zero: Vec<Vec<usize>> = tracks.iter().map(|t| vec![0; t.chunks.len()]).collect();
    let movie_duration = tracks
        .iter()
        .map(|t| rescale_to_movie(track_duration_span(&t.samples), t.timescale))
        .collect::<Result<Vec<u32>>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    let moov_len = build_moov(&tracks, &zero, movie_duration, max_track_id + 1)?.len();
    let mut ftyp_payload = b"qt  ".to_vec();
    ftyp_payload.extend(u32b(0));
    ftyp_payload.extend(b"qt  ");
    let ftyp = bx(b"ftyp", &ftyp_payload);
    let mut chunk_offsets: Vec<Vec<usize>> = Vec::new();
    let mut mdat_body: Vec<u8> = Vec::new();
    let mut at = ftyp.len() + moov_len + 8;
    for t in &tracks {
        let mut offsets = Vec::new();
        for range in &t.chunks {
            offsets.push(at);
            for s in &t.samples[range.clone()] {
                mdat_body.extend_from_slice(&data[s.offset..s.offset + s.size]);
                at += s.size;
            }
        }
        chunk_offsets.push(offsets);
    }
    let moov = build_moov(&tracks, &chunk_offsets, movie_duration, max_track_id + 1)?;
    debug_assert_eq!(moov.len(), moov_len);
    let mut out = ftyp;
    out.extend(moov);
    out.extend(bx(b"mdat", &mdat_body));
    Ok(out)
}

/// Verify a remux like Swift `movie()`: per kept track, sample count, sizes,
/// concatenated sample hash and presentation times must match exactly.
pub fn check_remux(source: &[u8], output: &[u8], video_index: usize) -> Result<()> {
    let (src_tracks, _) = parse_tracks(source, video_index)?;
    // The output holds only the selected video track, so it is index 0 there.
    let (out_tracks, _) = parse_tracks(output, 0)?;
    if src_tracks.len() != out_tracks.len() {
        return fail("重封装校验失败：轨道数不同");
    }
    for (s, o) in src_tracks.iter().zip(out_tracks.iter()) {
        if s.handler != o.handler || s.timescale != o.timescale {
            return fail("重封装校验失败：轨道类型或时间尺度不同");
        }
        if s.samples.len() != o.samples.len() {
            return fail("重封装校验失败：样本数不同");
        }
        let hash = |samples: &[mp4::Sample], d: &[u8]| {
            let mut h = String::new();
            let mut all = Vec::new();
            for x in samples {
                all.extend_from_slice(&d[x.offset..x.offset + x.size]);
            }
            h.push_str(&sha256_hex(&all));
            h
        };
        if hash(&s.samples, source) != hash(&o.samples, output) {
            return fail("重封装校验失败：样本数据不同");
        }
        if s.samples.iter().map(|x| x.size).collect::<Vec<_>>()
            != o.samples.iter().map(|x| x.size).collect::<Vec<_>>()
        {
            return fail("重封装校验失败：样本大小不同");
        }
        if s.samples.iter().map(|x| x.presentation_time).collect::<Vec<_>>()
            != o.samples.iter().map(|x| x.presentation_time).collect::<Vec<_>>()
        {
            return fail("重封装校验失败：显示时间漂移");
        }
    }
    Ok(())
}

/// Track kinds kept by remux, for callers that need a summary.
pub fn kept_handlers(data: &[u8], video_index: usize) -> Result<Vec<String>> {
    let (tracks, _) = parse_tracks(data, video_index)?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in tracks {
        if seen.insert(t.handler.clone()) {
            out.push(t.handler);
        }
    }
    Ok(out)
}
