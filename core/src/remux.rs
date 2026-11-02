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

use crate::mp4::{self, bx, full, BoxRef, Movie};
use crate::{fail, fail_as, ErrorKind, Result};
use std::collections::HashSet;

/// Output movie timescale. Internal: part of the file format, not API.
const MOVIE_TIMESCALE: u32 = 1_000_000;

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
    let found: Vec<BoxRef> = children(d, parent)?
        .into_iter()
        .filter(|b| b.is(t))
        .collect();
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
    let v = (span as u128 * MOVIE_TIMESCALE as u128 + timescale as u128 / 2) / timescale as u128;
    if v > u32::MAX as u128 {
        return fail_as(ErrorKind::Unsupported, "重封装时长超出 MOV 版本 0 范围");
    }
    Ok(v as u32)
}

/// Rescale an edit list to the output movie timescale, or None if the track
/// has no edts box. Segment durations are rounded to nearest; media times
/// and rates are copied verbatim (media time -1 marks an empty edit).
fn maybe_edts(d: &[u8], trak: &BoxRef, movie_ts: usize) -> Result<Option<Vec<u8>>> {
    let edts_list: Vec<BoxRef> = children(d, trak)?
        .into_iter()
        .filter(|b| b.is(b"edts"))
        .collect();
    if edts_list.is_empty() {
        return Ok(None);
    }
    if edts_list.len() > 1 {
        return fail("重封装重复出现 edts");
    }
    let elst_list: Vec<BoxRef> = children(d, &edts_list[0])?
        .into_iter()
        .filter(|b| b.is(b"elst"))
        .collect();
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
    let (record, at) = if version == 1 {
        (20usize, elst.payload + 8)
    } else {
        (12usize, elst.payload + 8)
    };
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

/// The tracks a remux keeps: video track `video_index` (among video tracks)
/// and every audio track, in file order. Other tracks (Google mett, hint,
/// text, still-image-time) are dropped.
fn kept_tracks(movie: &Movie, video_index: usize, audio: bool) -> Vec<&mp4::Track> {
    let mut video_seen = 0usize;
    movie
        .tracks
        .iter()
        .filter(|t| match t.samples.handler.as_str() {
            "vide" => {
                video_seen += 1;
                video_seen - 1 == video_index
            }
            "soun" => audio,
            _ => false,
        })
        .collect()
}

fn parse_tracks(
    d: &[u8],
    movie: &Movie,
    video_index: usize,
    audio: bool,
) -> Result<(Vec<TrackParts>, u32)> {
    let movie_ts = movie.timescale as usize;
    let mut kept: Vec<TrackParts> = Vec::new();
    let mut max_track_id = 0u32;
    if movie.video(video_index).is_err() {
        return fail_as(ErrorKind::InvalidArgument, "视频没有所选的视频轨");
    }
    for track in kept_tracks(movie, video_index, audio) {
        let (trak, table) = (&track.trak, &track.samples);
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
        if is_video && table.samples.is_empty() {
            return fail("视频没有样本，无法重封装");
        }
        if table.samples.iter().any(|s| s.presentation_time < 0) {
            return fail_as(ErrorKind::Unsupported, "视频含负时间样本，暂不无损重封装");
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
        let edts = maybe_edts(d, trak, movie_ts)?;
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
        return fail_as(ErrorKind::Unsupported, "重封装时长超出 MOV 版本 0 范围");
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
                return fail_as(
                    ErrorKind::Unsupported,
                    "重封装块偏移超出 32 位范围，请用 64 位源文件",
                );
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
    meta: Option<&[u8]>,
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
    if let Some(u) = meta {
        moov_body.extend(u);
    }
    Ok(bx(b"moov", &moov_body))
}

const CONTENT_IDENTIFIER_KEY: &[u8] = b"com.apple.quicktime.content.identifier";

/// Movie-level QuickTime metadata carrying the Live Photo content identifier,
/// in the form iPhone camera files and AVAssetWriter use (QuickTime File
/// Format, "Metadata"): `moov/meta` holding `hdlr(mdta)`, a `keys` box with
/// one `mdta` key, and an `ilst` whose item type is the 1-based key index
/// with a UTF-8 (`data` type 1, locale 0) value. QuickTime's `meta` atom has
/// no version/flags, unlike the ISO BMFF full box.
pub fn content_identifier_meta(uuid: &str) -> Vec<u8> {
    let mut hdlr_body = u32b(0).to_vec();
    hdlr_body.extend(b"mdta");
    hdlr_body.extend([0u8; 12]);
    hdlr_body.push(0);
    let hdlr = full(b"hdlr", 0, 0, &hdlr_body);
    let mut key = u32b(8 + CONTENT_IDENTIFIER_KEY.len() as u32).to_vec();
    key.extend(b"mdta");
    key.extend(CONTENT_IDENTIFIER_KEY);
    let mut keys_body = u32b(1).to_vec();
    keys_body.extend(key);
    let keys = full(b"keys", 0, 0, &keys_body);
    let mut data_body = u32b(0).to_vec();
    data_body.extend(uuid.as_bytes());
    let data = full(b"data", 0, 1, &data_body);
    let item = bx(&1u32.to_be_bytes(), &data);
    let mut meta_body = hdlr;
    meta_body.extend(keys);
    meta_body.extend(bx(b"ilst", &item));
    bx(b"meta", &meta_body)
}

/// Children of a `meta` box. QuickTime writes it without version/flags and
/// ISO BMFF with; a zero first word can only be version/flags, because a
/// child box of size 0 ("to end of file") cannot occur inside `moov`.
fn meta_children(d: &[u8], meta: &BoxRef) -> Result<Vec<BoxRef>> {
    let start = if meta.end - meta.payload >= 4 && d[meta.payload..meta.payload + 4] == [0; 4] {
        meta.payload + 4
    } else {
        meta.payload
    };
    mp4::boxes(d, start, meta.end)
}

/// UTF-8 value of a `data` atom (type indicator 1), or None for other types.
fn utf8_data(d: &[u8], data: &BoxRef) -> Result<Option<String>> {
    let body = &d[data.payload..data.end];
    if body.len() < 8 || body[..4] != [0, 0, 0, 1] {
        return Ok(None);
    }
    String::from_utf8(body[8..].to_vec())
        .map(Some)
        .map_err(|_| crate::Error::new("内容标识不是 UTF-8"))
}

/// Content identifier from QuickTime keyed metadata (`hdlr` + `keys` + `ilst`).
fn keyed_identifier(d: &[u8], meta: &BoxRef) -> Result<Option<String>> {
    let kids = meta_children(d, meta)?;
    let (Some(keys), Some(ilst)) = (
        kids.iter().find(|b| b.is(b"keys")),
        kids.iter().find(|b| b.is(b"ilst")),
    ) else {
        return Ok(None);
    };
    if keys.end - keys.payload < 8 {
        return fail("QuickTime keys 被截断");
    }
    let count = crate::binary::be(d, keys.payload + 4, 4)?;
    let mut at = keys.payload + 8;
    let mut index = None;
    for i in 0..count {
        if keys.end - at < 8 {
            return fail("QuickTime keys 被截断");
        }
        let size = crate::binary::be(d, at, 4)?;
        if size < 8 || size > keys.end - at {
            return fail("QuickTime keys 被截断");
        }
        if &d[at + 4..at + 8] == b"mdta" && &d[at + 8..at + size] == CONTENT_IDENTIFIER_KEY {
            index = Some(i as u32 + 1);
        }
        at += size;
    }
    let Some(index) = index else {
        return Ok(None);
    };
    for item in children(d, ilst)? {
        if d[item.start + 4..item.start + 8] != index.to_be_bytes() {
            continue;
        }
        for c in children(d, &item)? {
            if c.is(b"data") {
                if let Some(v) = utf8_data(d, &c)? {
                    return Ok(Some(v));
                }
            }
        }
    }
    Ok(None)
}

/// Content identifier from the `----` mean/name/data form this core wrote
/// before it switched to keyed metadata (`moov/udta/meta/ilst`).
fn freeform_identifier(d: &[u8], meta: &BoxRef) -> Result<Option<String>> {
    let Some(ilst) = meta_children(d, meta)?.into_iter().find(|b| b.is(b"ilst")) else {
        return Ok(None);
    };
    for item in children(d, &ilst)? {
        if !item.is(b"----") {
            continue;
        }
        let (mut mean, mut name, mut value) = (None, None, None);
        for c in children(d, &item)? {
            if c.end - c.payload < 4 {
                continue;
            }
            let body = &d[c.payload + 4..c.end];
            if c.is(b"mean") {
                mean = Some(body);
            } else if c.is(b"name") {
                name = Some(body);
            } else if c.is(b"data") {
                value = utf8_data(d, &c)?;
            }
        }
        if mean == Some(b"com.apple.quicktime.metadata".as_slice())
            && name == Some(CONTENT_IDENTIFIER_KEY)
        {
            if let Some(v) = value {
                return Ok(Some(v));
            }
        }
    }
    Ok(None)
}

/// Read the movie-level content identifier: keyed `moov/meta` first (iPhone,
/// AVAssetWriter, this writer), then keyed or freeform `moov/udta/meta`.
pub fn read_content_identifier(data: &[u8]) -> Result<Option<String>> {
    let top = mp4::boxes(data, 0, data.len())?;
    let Some(moov) = top.iter().find(|b| b.is(b"moov")) else {
        return Ok(None);
    };
    let kids = children(data, moov)?;
    for meta in kids.iter().filter(|b| b.is(b"meta")) {
        if let Some(v) = keyed_identifier(data, meta)? {
            return Ok(Some(v));
        }
    }
    for udta in kids.iter().filter(|b| b.is(b"udta")) {
        for meta in children(data, udta)?.iter().filter(|b| b.is(b"meta")) {
            if let Some(v) = keyed_identifier(data, meta)? {
                return Ok(Some(v));
            }
            if let Some(v) = freeform_identifier(data, meta)? {
                return Ok(Some(v));
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
    0x00, 0x00, 0x00, 0x60, 0x6d, 0x65, 0x62, 0x78, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x50, 0x6b, 0x65, 0x79, 0x73, 0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x30, 0x6b, 0x65, 0x79, 0x64, 0x6d, 0x64, 0x74, 0x61, 0x63, 0x6f, 0x6d, 0x2e,
    0x61, 0x70, 0x70, 0x6c, 0x65, 0x2e, 0x71, 0x75, 0x69, 0x63, 0x6b, 0x74, 0x69, 0x6d, 0x65, 0x2e,
    0x73, 0x74, 0x69, 0x6c, 0x6c, 0x2d, 0x69, 0x6d, 0x61, 0x67, 0x65, 0x2d, 0x74, 0x69, 0x6d, 0x65,
    0x00, 0x00, 0x00, 0x10, 0x64, 0x74, 0x79, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x41,
];

/// The single still-image-time sample: `[size 9][key 1][value 0xff]`.
const METT_SAMPLE: &[u8] = &[0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x01, 0xff];

/// The `gmhd`/`gmin` media header native meta tracks carry (verbatim).
const METT_GMHD: &[u8] = &[
    0x00, 0x00, 0x00, 0x20, 0x67, 0x6d, 0x68, 0x64, 0x00, 0x00, 0x00, 0x18, 0x67, 0x6d, 0x69, 0x6e,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
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
        return fail_as(
            ErrorKind::Cover,
            "封面时间超出 still-image-time 编辑列表范围",
        );
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
    let stsc = full(
        b"stsc",
        0,
        0,
        &[u32b(1), u32b(1), u32b(1), u32b(1)].concat(),
    );
    let stsz = full(
        b"stsz",
        0,
        0,
        &[u32b(0), u32b(1), u32b(METT_SAMPLE.len() as u32)].concat(),
    );
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
    for (i, v) in [0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000]
        .iter()
        .enumerate()
    {
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
    Ok(remux_mov(data, &Movie::parse(data)?, video_index, None, None)?.0)
}

/// Same as `remux_to_mov`, plus the movie-level content identifier
/// (`com.apple.quicktime.content.identifier`, ROADMAP item 9b first half).
/// No still-image-time track: use `remux_to_mov_with_identifier_and_mett`
/// when the cover time is known.
pub fn remux_to_mov_with_identifier(
    data: &[u8],
    video_index: usize,
    uuid: &str,
) -> Result<Vec<u8>> {
    if uuid.is_empty() {
        return fail_as(ErrorKind::InvalidArgument, "内容标识不能为空");
    }
    Ok(remux_mov(data, &Movie::parse(data)?, video_index, Some(uuid), None)?.0)
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
        return fail_as(ErrorKind::InvalidArgument, "内容标识不能为空");
    }
    Ok(remux_mov(
        data,
        &Movie::parse(data)?,
        video_index,
        Some(uuid),
        Some(cover_time_us),
    )?
    .0)
}

/// The MOV remux behind the `remux_to_mov*` functions, on an already parsed
/// source. Returns the output and its own parse, both already verified
/// sample by sample against the source (like Swift `movie()`), so callers
/// with more checks to run on the output need not parse it again.
pub fn remux_mov(
    data: &[u8],
    movie: &Movie,
    video_index: usize,
    identifier: Option<&str>,
    mett_cover_us: Option<i64>,
) -> Result<(Vec<u8>, Movie)> {
    remux_mov_with(data, movie, video_index, identifier, mett_cover_us, true)
}

/// [`remux_mov`] with a choice to drop the audio tracks (`audio: false`
/// gives a silent movie of the one video track, like Swift
/// `exportAuxiliary`).
pub fn remux_mov_with(
    data: &[u8],
    movie: &Movie,
    video_index: usize,
    identifier: Option<&str>,
    mett_cover_us: Option<i64>,
    audio: bool,
) -> Result<(Vec<u8>, Movie)> {
    remux_mov_policy(
        data,
        movie,
        video_index,
        identifier,
        mett_cover_us,
        audio,
        false,
    )
}

/// App compatibility route: for a simple leading delay, AVAssetWriter used
/// the media sample timeline. The generic remux APIs retain source edits.
pub(crate) fn remux_mov_media(
    data: &[u8],
    movie: &Movie,
    video_index: usize,
    identifier: Option<&str>,
    cover: i64,
) -> Result<(Vec<u8>, Movie)> {
    remux_mov_policy(
        data,
        movie,
        video_index,
        identifier,
        Some(cover),
        true,
        true,
    )
}

/// True only for untrimmed media starting at zero, optionally preceded by
/// empty edits. Never discard a trim, repeat, rate change or interior gap.
pub(crate) fn simple_media_timeline(data: &[u8], movie: &Movie, index: usize) -> Result<bool> {
    for track in kept_tracks(movie, index, true) {
        if let Some(edits) = mp4::edit_list(data, &track.trak)? {
            let edts = children(data, &track.trak)?
                .into_iter()
                .find(|b| b.is(b"edts"))
                .unwrap();
            let elst = children(data, &edts)?
                .into_iter()
                .find(|b| b.is(b"elst"))
                .unwrap();
            let version = data[elst.payload];
            if version > 1 {
                return Ok(false);
            }
            let record = if version == 1 { 20 } else { 12 };
            for i in 0..edits.len() {
                let at = elst.payload + 8 + i * record + record - 4;
                if data[at..at + 4] != [0, 1, 0, 0] {
                    return Ok(false);
                }
            }

            let real: Vec<_> = edits
                .iter()
                .enumerate()
                .filter(|(_, (_, start))| *start >= 0)
                .collect();
            if real.len() != 1 || real[0].0 + 1 != edits.len() || (real[0].1).1 != 0 {
                return Ok(false);
            }
            let end = track
                .samples
                .samples
                .iter()
                .map(|s| s.presentation_time as i128 + s.duration as i128)
                .max()
                .unwrap_or(0);
            if (real[0].1).0 as i128 * (track.samples.timescale as i128)
                < end * (movie.timescale as i128)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn remux_mov_policy(
    data: &[u8],
    movie: &Movie,
    video_index: usize,
    identifier: Option<&str>,
    mett_cover_us: Option<i64>,
    audio: bool,
    media: bool,
) -> Result<(Vec<u8>, Movie)> {
    let (mut tracks, mut max_track_id) = parse_tracks(data, movie, video_index, audio)?;
    if media {
        if !simple_media_timeline(data, movie, video_index)? {
            return fail("编辑时间线不能安全规范化");
        }
        for track in &mut tracks {
            track.edts = None;
        }
    }
    let meta = identifier.map(content_identifier_meta);
    if let Some(cover) = mett_cover_us {
        tracks.push(mett_track_parts(max_track_id + 1, cover)?);
        max_track_id += 1;
    }
    let mut ftyp_payload = b"qt  ".to_vec();
    ftyp_payload.extend(u32b(0));
    ftyp_payload.extend(b"qt  ");
    let out = assemble(
        &tracks,
        max_track_id,
        meta,
        bx(b"ftyp", &ftyp_payload),
        data,
    )?;
    // The added still-image-time track is not a source track and is ignored.
    let out_movie = Movie::parse(&out)?;
    verify(data, movie, video_index, audio, &out, &out_movie)?;
    Ok((out, out_movie))
}

/// MOV -> MP4 sample-copy remux (ROADMAP item 8, first half), mirroring
/// Swift `AndroidExport.remux`: exactly one video track and at most one
/// audio track, metadata (mett) tracks dropped, samples copied verbatim,
/// movie timescale 1_000_000, ftyp `mp42`. The edit list is copied, so
/// playback timing matches the source; raw sample-table times are unchanged.
pub fn remux_to_mp4(data: &[u8]) -> Result<Vec<u8>> {
    remux_mp4(data, &Movie::parse(data)?)
}

/// [`remux_to_mp4`] on an already parsed source.
pub fn remux_mp4(data: &[u8], movie: &Movie) -> Result<Vec<u8>> {
    let count = |h: &str| {
        movie
            .tracks
            .iter()
            .filter(|t| t.samples.handler == h)
            .count()
    };
    if count("vide") != 1 || count("soun") > 1 {
        return fail_as(ErrorKind::Unsupported, "实况视频轨道布局不受支持");
    }
    let (tracks, max_track_id) = parse_tracks(data, movie, 0, true)?;
    let mut ftyp_payload = b"mp42".to_vec();
    ftyp_payload.extend(u32b(0));
    ftyp_payload.extend(b"mp42");
    ftyp_payload.extend(b"isom");
    let out = assemble(
        &tracks,
        max_track_id,
        None,
        bx(b"ftyp", &ftyp_payload),
        data,
    )?;
    verify(data, movie, 0, true, &out, &Movie::parse(&out)?)?;
    Ok(out)
}

/// Shared two-pass assembly: build moov once with zero chunk offsets to
/// measure it, lay out mdat chunks, then rebuild moov with real offsets.
fn assemble(
    tracks: &[TrackParts],
    max_track_id: u32,
    meta: Option<Vec<u8>>,
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
    let moov_len = build_moov(
        tracks,
        &zero,
        movie_duration,
        max_track_id + 1,
        meta.as_deref(),
    )?
    .len();
    // Chunk offsets depend only on sample sizes, so compute them without
    // copying any sample bytes (perf item 5): the old code first copied
    // every sample into mdat_body, then bx() copied it again into the
    // output, plus growth re-copies of the unreserved buffer.
    let mut chunk_offsets: Vec<Vec<usize>> = Vec::new();
    let mut total_samples = 0usize;
    let mut at = ftyp.len() + moov_len + 8;
    for t in tracks {
        let mut offsets = Vec::new();
        for range in &t.chunks {
            offsets.push(at);
            for s in &t.samples[range.clone()] {
                at += s.size;
                total_samples += s.size;
            }
        }
        chunk_offsets.push(offsets);
    }
    let moov = build_moov(
        tracks,
        &chunk_offsets,
        movie_duration,
        max_track_id + 1,
        meta.as_deref(),
    )?;
    debug_assert_eq!(moov.len(), moov_len);
    // The mdat header and stco offsets are 32-bit: refuse instead of
    // silently truncating the box size.
    if total_samples
        .checked_add(8)
        .is_none_or(|n| n > u32::MAX as usize)
    {
        return fail_as(
            ErrorKind::Unsupported,
            "媒体数据超过 4 GB，无法写入 32 位 mdat",
        );
    }
    let mut out = ftyp;
    out.extend(moov);
    // Single copy: reserve the exact sample payload once, then write the
    // mdat header and every sample straight into the output, in the
    // same track/chunk/sample order as before.
    out.reserve(8 + total_samples);
    out.extend_from_slice(&((total_samples + 8) as u32).to_be_bytes());
    out.extend_from_slice(b"mdat");
    for t in tracks {
        let mut cursor = 0usize;
        for range in &t.chunks {
            for s in &t.samples[range.clone()] {
                if let Some(inline) = &t.inline_data {
                    out.extend_from_slice(&inline[cursor..cursor + s.size]);
                    cursor += s.size;
                } else {
                    out.extend_from_slice(&source[s.offset..s.offset + s.size]);
                }
            }
        }
    }
    Ok(out)
}

/// Verify a remux like Swift `movie()`: per kept track, sample count, sizes,
/// sample bytes and presentation times must match exactly.
/// Every public `remux_*` function already runs this on its output.
pub fn check_remux(source: &[u8], output: &[u8], video_index: usize) -> Result<()> {
    verify(
        source,
        &Movie::parse(source)?,
        video_index,
        true,
        output,
        &Movie::parse(output)?,
    )
}

fn verify(
    src: &[u8],
    src_movie: &Movie,
    video_index: usize,
    audio: bool,
    out: &[u8],
    out_movie: &Movie,
) -> Result<()> {
    let s_tracks = kept_tracks(src_movie, video_index, audio);
    // The output holds only the selected video track, so it is index 0 there.
    let o_tracks = kept_tracks(out_movie, 0, audio);
    if s_tracks.len() != o_tracks.len() {
        return fail_as(ErrorKind::VerifyFailed, "重封装校验失败：轨道数不同");
    }
    for (s, o) in s_tracks.iter().zip(&o_tracks) {
        let (s, o) = (&s.samples, &o.samples);
        if s.handler != o.handler || s.timescale != o.timescale {
            return fail_as(
                ErrorKind::VerifyFailed,
                "重封装校验失败：轨道类型或时间尺度不同",
            );
        }
        if s.samples.len() != o.samples.len() {
            return fail_as(ErrorKind::VerifyFailed, "重封装校验失败：样本数不同");
        }
        for (a, b) in s.samples.iter().zip(&o.samples) {
            if a.size != b.size {
                return fail_as(ErrorKind::VerifyFailed, "重封装校验失败：样本大小不同");
            }
            if a.presentation_time != b.presentation_time {
                return fail_as(ErrorKind::VerifyFailed, "重封装校验失败：显示时间漂移");
            }
            // Both files are in memory: compare the bytes themselves (exact,
            // and several times faster than hashing each side).
            if src[a.offset..a.offset + a.size] != out[b.offset..b.offset + b.size] {
                return fail_as(ErrorKind::VerifyFailed, "重封装校验失败：样本数据不同");
            }
        }
    }
    Ok(())
}

/// Track kinds kept by remux, for callers that need a summary.
pub fn kept_handlers(data: &[u8], video_index: usize) -> Result<Vec<String>> {
    let (tracks, _) = parse_tracks(data, &Movie::parse(data)?, video_index, true)?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in tracks {
        if seen.insert(t.handler.clone()) {
            out.push(t.handler);
        }
    }
    Ok(out)
}

/// Track id and display size (16.16 fixed point, as floats) from a track's
/// `tkhd`, like AVFoundation's `trackID` and `naturalSize`.
pub fn track_header(d: &[u8], trak: &BoxRef) -> Result<(u32, f64, f64)> {
    let tkhd = only(d, trak, b"tkhd", "tkhd")?;
    let (id, width, height, ..) = parse_tkhd(d, &tkhd)?;
    Ok((id, width as f64 / 65536.0, height as f64 / 65536.0))
}
