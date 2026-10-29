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
use crate::{fail, Result};
use std::collections::HashSet;

/// Output movie timescale. Internal: part of the file format, not API.
const MOVIE_TIMESCALE: u32 = 1_000_000;

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
    /// tkhd flags (parsed tracks: 7 = enabled|in_movie|in_preview).
    tkhd_flags: u32,
    /// tkhd duration in movie timescale, overriding the sample-span-derived
    /// value when set (synthetic still-image-time track: its edit list
    /// extends past the single sample span by the empty edit).
    movie_duration_override: Option<u32>,
    /// Inline sample bytes for synthetic tracks (still-image-time); when
    /// set, assemble() copies from here instead of the source file.
    inline_data: Option<Vec<u8>>,
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

fn parse_tracks(d: &[u8], video_index: usize, keep_audio: bool) -> Result<(Vec<TrackParts>, u32)> {
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
        } else if !keep_audio {
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
            tkhd_flags: 7,
            movie_duration_override: None,
            inline_data: None,
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
    full(b"tkhd", 0, t.tkhd_flags, &p)
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

fn build_moov(
    tracks: &[TrackParts],
    chunk_offsets: &[Vec<usize>],
    movie_duration: u32,
    next_track_id: u32,
    udta: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let mut moov_body = mvhd_box(movie_duration, next_track_id);
    for (t, offsets) in tracks.iter().zip(chunk_offsets.iter()) {
        let span = track_duration_span(&t.samples);
        let duration_movie = match t.movie_duration_override {
            Some(d) => d,
            None => rescale_to_movie(span, t.timescale)?,
        };
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
    if let Some(u) = udta {
        moov_body.extend(u);
    }
    Ok(bx(b"moov", &moov_body))
}

/// Movie-level QuickTime metadata carrying the Live Photo content identifier:
/// moov/udta/meta/hdlr(mdta) + ilst/----/mean + name + data (UTF-8, type 1).
/// Layout follows the QuickTime File Format metadata chapter and ffmpeg's
/// `mov_read_custom` reader (mean/name/data full boxes, data locale 0).
pub fn content_identifier_udta(uuid: &str) -> Vec<u8> {
    let mut hdlr_body = u32b(0).to_vec();
    hdlr_body.extend(b"mdta");
    hdlr_body.extend([0u8; 12]);
    hdlr_body.push(0);
    let hdlr = full(b"hdlr", 0, 0, &hdlr_body);
    let mean = full(b"mean", 0, 0, b"com.apple.quicktime.metadata");
    let name = full(b"name", 0, 0, b"com.apple.quicktime.content.identifier");
    let mut data_body = u32b(0).to_vec();
    data_body.extend(uuid.as_bytes());
    let data = full(b"data", 0, 1, &data_body);
    let mut item = mean;
    item.extend(name);
    item.extend(data);
    let ilst = bx(b"ilst", &bx(b"----", &item));
    let mut meta_body = hdlr;
    meta_body.extend(ilst);
    bx(b"udta", &full(b"meta", 0, 0, &meta_body))
}

/// Read the content identifier back from a MOV's udta/meta/ilst, if present.
pub fn read_content_identifier(data: &[u8]) -> Result<Option<String>> {
    let top = mp4::boxes(data, 0, data.len())?;
    let moov = match top.iter().find(|b| b.is(b"moov")) {
        Some(m) => m.clone(),
        None => return Ok(None),
    };
    let udta = match children(data, &moov)?.into_iter().find(|b| b.is(b"udta")) {
        Some(u) => u,
        None => return Ok(None),
    };
    let meta = match children(data, &udta)?.into_iter().find(|b| b.is(b"meta")) {
        Some(m) => m,
        None => return Ok(None),
    };
    // `meta` is a full box: its children start after version/flags.
    let ilst = match mp4::boxes(data, meta.payload + 4, meta.end)?.into_iter().find(|b| b.is(b"ilst")) {
        Some(i) => i,
        None => return Ok(None),
    };
    for item in children(data, &ilst)? {
        if !item.is(b"----") {
            continue;
        }
        let mut mean = None;
        let mut name = None;
        let mut value = None;
        for c in children(data, &item)? {
            if c.end - c.payload < 4 {
                continue;
            }
            let body = &data[c.payload + 4..c.end];
            if c.is(b"mean") {
                mean = Some(body.to_vec());
            } else if c.is(b"name") {
                name = Some(body.to_vec());
            } else if c.is(b"data") && body.len() >= 4 {
                value = Some(body[4..].to_vec());
            }
        }
        if mean.as_deref() == Some(b"com.apple.quicktime.metadata".as_slice())
            && name.as_deref() == Some(b"com.apple.quicktime.content.identifier".as_slice())
        {
            if let Some(v) = value {
                return Ok(Some(String::from_utf8(v).map_err(|_| crate::Error::new("内容标识不是 UTF-8"))?));
            }
        }
    }
    Ok(None)
}

/// Still-image-time timed-metadata track (ROADMAP item 9b2), in the form
/// iPhone camera files actually write: handler `meta`, sample entry
/// `mebx` with a `keys` box carrying the `mdta` /
/// `com.apple.quicktime.still-image-time` key, one 9-byte sample
/// (`[size 9][key 1][value 0xff = int8 -1]`), mdhd timescale 600, and an
/// edit list whose empty edit runs up to the cover point.
///
/// No public sample with an AVAssetWriter-style `mett` entry was found:
/// every verified native file uses `mebx` (see `tests/SOURCES.md`), so
/// the writer replicates the native `mebx` form, whose stsd entry and
/// sample bytes below are pinned by three independent sources
/// (osxphotos MIT, logseq-livephotos-plugin MIT, plus an unlicensed
/// corroborating file). The sample VALUE is the native -1, not Swift
/// `movie()`'s 0; Swift's reader accepts {-1, 0, 255}, and the cover time
/// comes from the edit list, never from the value. Photos recognition of
/// this synthetic track is a Needs Mac check (ROADMAP item 9b2).
/// Native still-track timescale. Internal, like MOVIE_TIMESCALE.
const METT_TRACK_TIMESCALE: u32 = 600;

/// The 1-key `mebx` stsd entry, byte-identical in all verified sources
/// (logseq demo, makzan file, and the first key of the osxphotos 3-key
/// entry). Do not "improve" these bytes: they are copied, not designed.
const METT_STSD_ENTRY: &[u8] = &[
    0x00, 0x00, 0x00, 0x60, 0x6d, 0x65, 0x62, 0x78, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x50, 0x6b, 0x65, 0x79, 0x73,
    0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30,
    0x6b, 0x65, 0x79, 0x64, 0x6d, 0x64, 0x74, 0x61, 0x63, 0x6f, 0x6d, 0x2e,
    0x61, 0x70, 0x70, 0x6c, 0x65, 0x2e, 0x71, 0x75, 0x69, 0x63, 0x6b, 0x74,
    0x69, 0x6d, 0x65, 0x2e, 0x73, 0x74, 0x69, 0x6c, 0x6c, 0x2d, 0x69, 0x6d,
    0x61, 0x67, 0x65, 0x2d, 0x74, 0x69, 0x6d, 0x65, 0x00, 0x00, 0x00, 0x10,
    0x64, 0x74, 0x79, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x41,
];

/// The single still-image-time sample: `[size 9][key 1][value 0xff]`.
const METT_SAMPLE: &[u8] = &[0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x01, 0xff];

/// The `gmhd`/`gmin` media header native meta tracks carry (verbatim).
const METT_GMHD: &[u8] = &[
    0x00, 0x00, 0x00, 0x20, 0x67, 0x6d, 0x68, 0x64, 0x00, 0x00, 0x00, 0x18,
    0x67, 0x6d, 0x69, 0x6e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x80, 0x00,
    0x80, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Build the synthetic still-image-time track for a cover at
/// `cover_time_us` microseconds in the output movie timeline
/// (MOVIE_TIMESCALE = 1_000_000, so microseconds map 1:1 to movie units).
/// The track timescale stays 600 like native files; the edit list's
/// second segment is 1/600 s in movie units (1667, rounded like
/// `rescale_to_movie`), and a zero cover gets a single-segment edit list
/// (no zero-length empty edit).
fn mett_track_parts(track_id: u32, cover_time_us: i64) -> Result<TrackParts> {
    if cover_time_us < 0 || cover_time_us > u32::MAX as i64 {
        return fail("封面时间超出 still-image-time 编辑列表范围");
    }
    let cover = cover_time_us as u32;
    let sample_span_movie = rescale_to_movie(1, METT_TRACK_TIMESCALE)?;
    let mut elst_body = Vec::new();
    let mut segments = 0u32;
    if cover > 0 {
        elst_body.extend(u32b(cover));
        elst_body.extend(0xffffffffu32.to_be_bytes());
        elst_body.extend(u16b(1));
        elst_body.extend(u16b(0));
        segments += 1;
    }
    elst_body.extend(u32b(sample_span_movie));
    elst_body.extend(u32b(0));
    elst_body.extend(u16b(1));
    elst_body.extend(u16b(0));
    segments += 1;
    let mut count_body = u32b(segments).to_vec();
    count_body.extend(elst_body);
    let edts = bx(b"edts", &full(b"elst", 0, 0, &count_body));
    let mut stsd_body = u32b(1).to_vec();
    stsd_body.extend(METT_STSD_ENTRY);
    let stsd = full(b"stsd", 0, 0, &stsd_body);
    let stts = full(b"stts", 0, 0, &[u32b(1), u32b(1), u32b(1)].concat());
    let stsc = full(b"stsc", 0, 0, &[u32b(1), u32b(1), u32b(1), u32b(1)].concat());
    let stsz = full(b"stsz", 0, 0, &[u32b(0), u32b(1), u32b(METT_SAMPLE.len() as u32)].concat());
    let stco = full(b"stco", 0, 0, &[u32b(1), u32b(0)].concat());
    let url = full(b"url ", 0, 1, &[]);
    let mut dref_body = u32b(1).to_vec();
    dref_body.extend(url);
    let dinf = bx(b"dinf", &full(b"dref", 0, 0, &dref_body));
    let mut hdlr_body = u32b(0).to_vec();
    hdlr_body.extend(b"meta");
    hdlr_body.extend([0u8; 12]);
    hdlr_body.push(0);
    let hdlr = full(b"hdlr", 0, 0, &hdlr_body);
    let mut matrix = [0u8; 36];
    for (i, v) in [0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000].iter().enumerate() {
        matrix[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    Ok(TrackParts {
        handler: "meta".to_string(),
        timescale: METT_TRACK_TIMESCALE,
        track_id,
        width: 0,
        height: 0,
        layer: 0,
        alternate_group: 0,
        volume: 0,
        matrix,
        mdhd_language: [0x55, 0xc4],
        hdlr,
        edts: Some(edts),
        minf_others: vec![METT_GMHD.to_vec(), dinf],
        stbl_ordered: vec![stsd, stts, stsc, stsz, stco],
        stco_is64: false,
        stco_pos: 4,
        samples: vec![mp4::Sample {
            offset: 0,
            size: METT_SAMPLE.len(),
            decode_time: 0,
            duration: 1,
            presentation_time: 0,
            sync: true,
        }],
        chunks: vec![0..1],
        // Native still tracks set enabled|in_movie|in_preview|in_poster.
        tkhd_flags: 0x0f,
        movie_duration_override: Some(cover + sample_span_movie),
        inline_data: Some(METT_SAMPLE.to_vec()),
    })
}

/// Remux an MP4 to a QuickTime MOV: chosen video track (0-based among video
/// tracks) plus all audio tracks, samples copied verbatim.
pub fn remux_to_mov(data: &[u8], video_index: usize) -> Result<Vec<u8>> {
    remux_impl(data, video_index, None, None)
}

/// Same as `remux_to_mov`, plus the movie-level content identifier
/// (`com.apple.quicktime.content.identifier`, ROADMAP item 9b first half).
/// No still-image-time track: use `remux_to_mov_with_identifier_and_mett`
/// when the cover time is known.
pub fn remux_to_mov_with_identifier(data: &[u8], video_index: usize, uuid: &str) -> Result<Vec<u8>> {
    if uuid.is_empty() {
        return fail("内容标识不能为空");
    }
    remux_impl(data, video_index, Some(uuid), None)
}

/// Same as `remux_to_mov_with_identifier`, plus the native-form
/// still-image-time track for a cover at `cover_time_us` microseconds
/// (ROADMAP item 9b2). The output is a complete Live Photo MOV modulo
/// Photos recognition, which is a Needs Mac check.
pub fn remux_to_mov_with_identifier_and_mett(
    data: &[u8],
    video_index: usize,
    uuid: &str,
    cover_time_us: i64,
) -> Result<Vec<u8>> {
    if uuid.is_empty() {
        return fail("内容标识不能为空");
    }
    remux_impl(data, video_index, Some(uuid), Some(cover_time_us))
}

fn remux_impl(
    data: &[u8],
    video_index: usize,
    identifier: Option<&str>,
    mett_cover_us: Option<i64>,
) -> Result<Vec<u8>> {
    remux_impl_audio(data, video_index, identifier, mett_cover_us, true)
}

/// Same as [`remux_to_mov`], but audio tracks are dropped: mirrors Swift
/// `PhotoFormat.exportAuxiliary`, which remuxes only the auxiliary video
/// track into a silent MOV.
pub fn remux_to_mov_video_only(data: &[u8], video_index: usize) -> Result<Vec<u8>> {
    remux_impl_audio(data, video_index, None, None, false)
}

fn remux_impl_audio(
    data: &[u8],
    video_index: usize,
    identifier: Option<&str>,
    mett_cover_us: Option<i64>,
    keep_audio: bool,
) -> Result<Vec<u8>> {
    let (mut tracks, mut max_track_id) = parse_tracks(data, video_index, keep_audio)?;
    let udta = identifier.map(content_identifier_udta);
    if let Some(cover) = mett_cover_us {
        tracks.push(mett_track_parts(max_track_id + 1, cover)?);
        max_track_id += 1;
    }
    let mut ftyp_payload = b"qt  ".to_vec();
    ftyp_payload.extend(u32b(0));
    ftyp_payload.extend(b"qt  ");
    assemble(&tracks, max_track_id, udta, bx(b"ftyp", &ftyp_payload), data)
}

/// MOV -> MP4 sample-copy remux (ROADMAP item 8, first half), mirroring
/// Swift `AndroidExport.remux`: exactly one video track and at most one
/// audio track, metadata (mett) tracks dropped, samples copied verbatim,
/// movie timescale 1_000_000, ftyp `mp42`. The edit list is copied, so
/// playback timing matches the source; raw sample-table times are unchanged.
pub fn remux_to_mp4(data: &[u8]) -> Result<Vec<u8>> {
    let tables = mp4::sample_tables(data)?;
    let n_video = tables.iter().filter(|t| t.handler == "vide").count();
    let n_audio = tables.iter().filter(|t| t.handler == "soun").count();
    if n_video != 1 || n_audio > 1 {
        return fail("实况视频轨道布局不受支持");
    }
    let (tracks, max_track_id) = parse_tracks(data, 0, true)?;
    let mut ftyp_payload = b"mp42".to_vec();
    ftyp_payload.extend(u32b(0));
    ftyp_payload.extend(b"mp42");
    ftyp_payload.extend(b"isom");
    assemble(&tracks, max_track_id, None, bx(b"ftyp", &ftyp_payload), data)
}

/// Shared two-pass assembly: build moov once with zero chunk offsets to
/// measure it, lay out mdat chunks, then rebuild moov with real offsets.
fn assemble(
    tracks: &[TrackParts],
    max_track_id: u32,
    udta: Option<Vec<u8>>,
    ftyp: Vec<u8>,
    source: &[u8],
) -> Result<Vec<u8>> {
    // First build with zero offsets to measure moov, then rebuild with real ones.
    let zero: Vec<Vec<usize>> = tracks.iter().map(|t| vec![0; t.chunks.len()]).collect();
    let movie_duration = tracks
        .iter()
        .map(|t| rescale_to_movie(track_duration_span(&t.samples), t.timescale))
        .collect::<Result<Vec<u32>>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    let moov_len = build_moov(tracks, &zero, movie_duration, max_track_id + 1, udta.as_deref())?.len();
    let mut chunk_offsets: Vec<Vec<usize>> = Vec::new();
    let mut mdat_body: Vec<u8> = Vec::new();
    let mut at = ftyp.len() + moov_len + 8;
    for t in tracks {
        let mut offsets = Vec::new();
        let mut cursor = 0usize;
        for range in &t.chunks {
            offsets.push(at);
            for s in &t.samples[range.clone()] {
                if let Some(inline) = &t.inline_data {
                    mdat_body.extend_from_slice(&inline[cursor..cursor + s.size]);
                    cursor += s.size;
                } else {
                    mdat_body.extend_from_slice(&source[s.offset..s.offset + s.size]);
                }
                at += s.size;
            }
        }
        chunk_offsets.push(offsets);
    }
    let moov = build_moov(tracks, &chunk_offsets, movie_duration, max_track_id + 1, udta.as_deref())?;
    debug_assert_eq!(moov.len(), moov_len);
    let mut out = ftyp;
    out.extend(moov);
    out.extend(bx(b"mdat", &mdat_body));
    Ok(out)
}

/// Verify a remux like Swift `movie()`: per kept track, sample count, sizes,
/// concatenated sample hash and presentation times must match exactly.
pub fn check_remux(source: &[u8], output: &[u8], video_index: usize) -> Result<()> {
    let (src_tracks, _) = parse_tracks(source, video_index, true)?;
    // The output holds only the selected video track, so it is index 0 there.
    let (out_tracks, _) = parse_tracks(output, 0, true)?;
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
        // Stream the hash over the samples: the old code collected
        // every sample byte into one Vec first, doubling peak memory
        // on large videos for no benefit (P2 zero-copy).
        let hash = |samples: &[mp4::Sample], d: &[u8]| {
            use sha2::Digest as _;
            let mut h = sha2::Sha256::new();
            for x in samples {
                h.update(&d[x.offset..x.offset + x.size]);
            }
            crate::hex(&h.finalize())
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
    let (tracks, _) = parse_tracks(data, video_index, true)?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in tracks {
        if seen.insert(t.handler.clone()) {
            out.push(t.handler);
        }
    }
    Ok(out)
}

/// Result of [`repair_auxiliary_duration`], mirroring Swift's repair report.
#[derive(Clone, Debug)]
pub struct AuxRepair {
    pub repaired: Vec<u8>,
    pub method: &'static str,
    pub original_duration_seconds: f64,
    pub repaired_duration_seconds: f64,
    pub sample_pts_unchanged: bool,
    pub original_sample_sha256: String,
}

fn aux_child(data: &[u8], parent: &BoxRef, name: &[u8; 4]) -> Result<BoxRef> {
    let found: Vec<BoxRef> = mp4::boxes(data, parent.payload, parent.end)?
        .into_iter()
        .filter(|b| b.is(name))
        .collect();
    if found.len() != 1 {
        return fail(format!(
            "辅助轨修复缺少唯一的 {}",
            String::from_utf8_lossy(name)
        ));
    }
    Ok(found[0].clone())
}

fn aux_word(data: &[u8], b: &BoxRef, at: usize) -> Result<u32> {
    Ok(crate::binary::be(data, b.payload + at, 4)? as u32)
}

/// Port of Swift `PhotoFormat.repairAuxiliaryDuration`, byte for byte in
/// guard order: some GCam files carry one real compressed auxiliary sample
/// but zero stts, mdhd and tkhd duration. Only those fixed-size fields are
/// repaired, using the primary track's shortest positive frame duration.
/// Returns None when the layout is not a two-video-track file (not
/// applicable); errors when the layout looks repairable but unsafe.
/// Sample presentation times are never touched.
pub fn repair_auxiliary_duration(data: &[u8]) -> Result<Option<AuxRepair>> {
    let top = mp4::boxes(data, 0, data.len())?;
    let moov = match top.iter().find(|b| b.is(b"moov")) {
        Some(m) => m.clone(),
        None => return Ok(None),
    };
    let mut videos: Vec<BoxRef> = Vec::new();
    for trak in mp4::boxes(data, moov.payload, moov.end)?
        .into_iter()
        .filter(|b| b.is(b"trak"))
    {
        let mdia = aux_child(data, &trak, b"mdia")?;
        let hdlr = aux_child(data, &mdia, b"hdlr")?;
        if hdlr.end - hdlr.payload >= 12 && &data[hdlr.payload + 8..hdlr.payload + 12] == b"vide" {
            videos.push(trak);
        }
    }
    if videos.len() != 2 {
        return Ok(None);
    }
    let main_media = aux_child(data, &videos[0], b"mdia")?;
    let aux_media = aux_child(data, &videos[1], b"mdia")?;
    let main_header = aux_child(data, &main_media, b"mdhd")?;
    let aux_header = aux_child(data, &aux_media, b"mdhd")?;
    let track_header = aux_child(data, &videos[1], b"tkhd")?;
    let movie_header = aux_child(data, &moov, b"mvhd")?;
    for h in [&main_header, &aux_header, &track_header, &movie_header] {
        if h.payload >= h.end || data[h.payload] != 0 {
            return Ok(None);
        }
    }
    let main_minf = aux_child(data, &main_media, b"minf")?;
    let aux_minf = aux_child(data, &aux_media, b"minf")?;
    let main_table = aux_child(data, &main_minf, b"stbl")?;
    let aux_table = aux_child(data, &aux_minf, b"stbl")?;
    let times = aux_child(data, &aux_table, b"stts")?;
    let sizes = aux_child(data, &aux_table, b"stsz")?;
    let chunks = aux_child(data, &aux_table, b"stsc")?;
    if aux_word(data, &times, 4)? != 1
        || aux_word(data, &times, 8)? != 1
        || aux_word(data, &times, 12)? != 0
        || aux_word(data, &sizes, 8)? != 1
        || aux_word(data, &aux_header, 16)? != 0
        || aux_word(data, &track_header, 20)? != 0
    {
        return Ok(None);
    }
    if mp4::boxes(data, videos[1].payload, videos[1].end)?
        .iter()
        .any(|b| b.is(b"edts"))
        || mp4::boxes(data, aux_table.payload, aux_table.end)?
            .iter()
            .any(|b| b.is(b"ctts"))
        || aux_word(data, &chunks, 4)? != 1
        || aux_word(data, &chunks, 8)? != 1
        || aux_word(data, &chunks, 12)? != 1
        || aux_word(data, &chunks, 16)? != 1
    {
        return fail("零时长辅助轨的布局不能安全修复");
    }
    let offsets: Vec<BoxRef> = mp4::boxes(data, aux_table.payload, aux_table.end)?
        .into_iter()
        .filter(|b| b.is(b"stco") || b.is(b"co64"))
        .collect();
    if offsets.len() != 1 || aux_word(data, &offsets[0], 4)? != 1 {
        return fail("零时长辅助轨偏移无效");
    }
    let is64 = offsets[0].is(b"co64");
    let offset = if is64 {
        crate::binary::be(data, offsets[0].payload + 8, 8)? as usize
    } else {
        aux_word(data, &offsets[0], 8)? as usize
    };
    let fixed = aux_word(data, &sizes, 4)?;
    let size = if fixed == 0 {
        aux_word(data, &sizes, 12)?
    } else {
        fixed
    } as usize;
    if size == 0
        || offset > data.len()
        || size > data.len() - offset
        || !top.iter().any(|b| {
            b.is(b"mdat") && offset >= b.payload && offset + size <= b.end
        })
    {
        return fail("零时长辅助轨的压缩样本范围无效");
    }
    let main_times = aux_child(data, &main_table, b"stts")?;
    let count = aux_word(data, &main_times, 4)? as usize;
    if count == 0 || count > (main_times.end - main_times.payload - 8) / 8 {
        return fail("主轨帧长表无效");
    }
    let mut positive: Vec<u32> = Vec::new();
    for i in 0..count {
        let delta = aux_word(data, &main_times, 12 + i * 8)?;
        if delta > 0 {
            positive.push(delta);
        }
    }
    let main_scale = aux_word(data, &main_header, 12)?;
    let aux_scale = aux_word(data, &aux_header, 12)?;
    let movie_scale = aux_word(data, &movie_header, 12)?;
    let step = match positive.iter().min() {
        Some(s) => *s,
        None => return fail("无法从主轨确定有效帧长"),
    };
    if main_scale == 0 || aux_scale == 0 || movie_scale == 0 {
        return fail("无法从主轨确定有效帧长");
    }
    let seconds = step as f64 / main_scale as f64;
    let aux_value = (seconds * aux_scale as f64).ceil();
    let movie_value = (seconds * movie_scale as f64).ceil();
    if !aux_value.is_finite()
        || !movie_value.is_finite()
        || aux_value > u32::MAX as f64
        || movie_value > u32::MAX as f64
    {
        return fail("修复辅助轨时长超出容器范围");
    }
    let aux_ticks = (aux_value as u32).max(1);
    let movie_ticks = (movie_value as u32).max(1);
    let mut repaired = data.to_vec();
    let put = |buf: &mut Vec<u8>, at: usize, v: u32| {
        buf[at..at + 4].copy_from_slice(&v.to_be_bytes());
    };
    put(&mut repaired, times.payload + 12, aux_ticks);
    put(&mut repaired, aux_header.payload + 16, aux_ticks);
    put(&mut repaired, track_header.payload + 20, movie_ticks);
    Ok(Some(AuxRepair {
        repaired,
        method: "primary-shortest-positive-frame-duration",
        original_duration_seconds: 0.0,
        repaired_duration_seconds: aux_ticks as f64 / aux_scale as f64,
        sample_pts_unchanged: true,
        original_sample_sha256: crate::sha256_hex(&data[offset..offset + size]),
    }))
}
