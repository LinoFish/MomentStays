//! Bounded ISO BMFF walk and Google cover-time descriptors (MotionVideoMetadata).
use crate::binary::{be, read};
use crate::{fail, fail_as, ErrorKind, Result};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub struct BoxRef {
    pub ty: [u8; 4],
    pub start: usize,
    pub payload: usize,
    pub end: usize,
}

impl BoxRef {
    pub fn is(&self, t: &[u8; 4]) -> bool {
        &self.ty == t
    }
    pub fn type_str(&self) -> String {
        String::from_utf8_lossy(&self.ty).into_owned()
    }
}

/// How strictly a box walk treats damaged framing.
///
/// `Strict` is the default for every production read and every rewrite: any
/// damaged header or length is an error, so nothing is silently skipped or
/// clipped. `Tolerant` is an explicit diagnostic: it keeps what it can frame
/// and reports whether anything was skipped. A tolerant walk is never
/// evidence that a file is safe to rewrite or convert losslessly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxParseMode {
    Strict,
    Tolerant,
}

/// Result of [`boxes_with_mode`].
#[derive(Clone, Debug)]
pub struct BoxWalk {
    pub boxes: Vec<BoxRef>,
    /// False when damaged bytes were skipped or a declared box was clipped.
    /// Even `true` only describes box framing, not sample or payload validity.
    pub complete: bool,
}

/// Boxes in `start..end`, strictly: every box must have a whole header and a
/// length that fits inside the range.
pub fn boxes(d: &[u8], start: usize, end: usize) -> Result<Vec<BoxRef>> {
    if end > d.len() || start > end {
        return fail("MP4 元数据范围无效");
    }
    let mut out = Vec::new();
    let mut p = start;
    while p < end {
        if end - p < 8 {
            return fail("MP4 元数据头被截断");
        }
        let mut size = be(d, p, 4)?;
        let mut header = 8;
        if size == 1 {
            if end - p < 16 {
                return fail("MP4 扩展数据头被截断");
            }
            size = be(d, p + 8, 8)?;
            header = 16;
        } else if size == 0 {
            size = end - p;
        }
        if size < header || size > end - p {
            return fail("MP4 元数据长度无效");
        }
        out.push(BoxRef {
            ty: [d[p + 4], d[p + 5], d[p + 6], d[p + 7]],
            start: p,
            payload: p + header,
            end: p + size,
        });
        p += size;
    }
    Ok(out)
}

/// Walk with an explicit recovery policy; see [`BoxParseMode`].
pub fn boxes_with_mode(d: &[u8], start: usize, end: usize, mode: BoxParseMode) -> Result<BoxWalk> {
    if mode == BoxParseMode::Strict {
        return boxes(d, start, end).map(|boxes| BoxWalk {
            boxes,
            complete: true,
        });
    }
    if end > d.len() || start > end {
        return fail("MP4 元数据范围无效");
    }
    let mut out = Vec::new();
    let mut p = start;
    let mut complete = true;
    while p < end {
        if end - p < 8 {
            complete = false;
            break;
        }
        let mut size = be(d, p, 4)?;
        let mut header = 8;
        if size == 1 {
            if end - p < 16 {
                complete = false;
                break;
            }
            size = be(d, p + 8, 8)?;
            header = 16;
        } else if size == 0 {
            size = end - p;
        }
        if size < header {
            complete = false;
            break;
        }
        let box_end = if size > end - p {
            complete = false;
            end
        } else {
            p + size
        };
        out.push(BoxRef {
            ty: [d[p + 4], d[p + 5], d[p + 6], d[p + 7]],
            start: p,
            payload: p + header,
            end: box_end,
        });
        p = box_end;
    }
    Ok(BoxWalk {
        boxes: out,
        complete,
    })
}

fn child(d: &[u8], parent: &BoxRef, t: &[u8; 4]) -> Result<BoxRef> {
    let found: Vec<BoxRef> = boxes(d, parent.payload, parent.end)?
        .into_iter()
        .filter(|b| b.is(t))
        .collect();
    if found.len() != 1 {
        return fail(format!(
            "Google 视频元数据缺少唯一的 {}",
            String::from_utf8_lossy(t)
        ));
    }
    Ok(found[0].clone())
}

fn word(d: &[u8], b: &BoxRef, offset: usize, width: usize) -> Result<usize> {
    if b.payload + offset + width > b.end {
        return fail("Google 视频元数据表被截断");
    }
    be(d, b.payload + offset, width)
}

struct Descriptor {
    tag: u8,
    payload: usize,
    end: usize,
}

fn descriptors(d: &[u8], start: usize, end: usize) -> Result<Vec<Descriptor>> {
    let mut p = start;
    let mut out = Vec::new();
    while p < end {
        let tag = d[p];
        p += 1;
        let (mut size, mut complete) = (0usize, false);
        for _ in 0..4 {
            if p >= end {
                return fail("Google 封面描述符被截断");
            }
            let v = d[p];
            p += 1;
            size = (size << 7) | (v & 127) as usize;
            if v & 128 == 0 {
                complete = true;
                break;
            }
        }
        if !complete || size > end - p {
            return fail("Google 封面描述符长度无效");
        }
        out.push(Descriptor {
            tag,
            payload: p,
            end: p + size,
        });
        p += size;
    }
    Ok(out)
}

fn image_samples(d: &[u8]) -> Result<Vec<Range<usize>>> {
    let top = boxes(d, 0, d.len())?;
    let moov = match top.iter().find(|b| b.is(b"moov")) {
        Some(m) => m.clone(),
        None => return Ok(vec![]),
    };
    let mut samples = Vec::new();
    for track in boxes(d, moov.payload, moov.end)?
        .into_iter()
        .filter(|b| b.is(b"trak"))
    {
        let mdia = child(d, &track, b"mdia")?;
        let handler = child(d, &mdia, b"hdlr")?;
        if handler.end - handler.payload < 12
            || &d[handler.payload + 8..handler.payload + 12] != b"meta"
        {
            continue;
        }
        let stbl = child(d, &child(d, &mdia, b"minf")?, b"stbl")?;
        let stsd = child(d, &stbl, b"stsd")?;
        if word(d, &stsd, 4, 4)? != 1 {
            continue;
        }
        let entries = boxes(d, stsd.payload + 8, stsd.end)?;
        if entries.len() != 1 || !entries[0].is(b"mett") {
            continue;
        }
        let e = &entries[0];
        let has = d[e.payload..e.end]
            .split(|b| *b == 0)
            .filter(|f| !f.is_empty())
            .any(|f| f == b"application/motionphoto-image-meta");
        if !has {
            continue;
        }
        let sizes = child(d, &stbl, b"stsz")?;
        let chunks = child(d, &stbl, b"stsc")?;
        if word(d, &sizes, 8, 4)? != 1
            || word(d, &chunks, 4, 4)? != 1
            || word(d, &chunks, 8, 4)? != 1
            || word(d, &chunks, 12, 4)? != 1
            || word(d, &chunks, 16, 4)? != 1
        {
            return fail("Google 封面元数据不是单样本布局");
        }
        let tables: Vec<BoxRef> = boxes(d, stbl.payload, stbl.end)?
            .into_iter()
            .filter(|b| b.is(b"stco") || b.is(b"co64"))
            .collect();
        if tables.len() != 1 || word(d, &tables[0], 4, 4)? != 1 {
            return fail("Google 封面块偏移无效");
        }
        let offset = word(d, &tables[0], 8, if tables[0].is(b"co64") { 8 } else { 4 })?;
        let fixed = word(d, &sizes, 4, 4)?;
        let size = if fixed == 0 {
            word(d, &sizes, 12, 4)?
        } else {
            fixed
        };
        if size == 0
            || offset > d.len()
            || size > d.len() - offset
            || !top
                .iter()
                .any(|b| b.is(b"mdat") && offset >= b.payload && offset + size <= b.end)
        {
            return fail("Google 封面样本超出媒体数据范围");
        }
        samples.push(offset..offset + size);
    }
    Ok(samples)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageTimes {
    pub primary: Option<i64>,
    pub high_resolution: Vec<Option<i64>>,
}

pub fn image_times(d: &[u8]) -> Result<Option<ImageTimes>> {
    let mut times = Vec::new();
    for sample in image_samples(d)? {
        let roots = descriptors(d, sample.start, sample.end)?;
        if roots.len() != 1 || roots[0].tag != 0xc0 {
            return fail("Google 封面根描述符无效");
        }
        let scores: Vec<Descriptor> = descriptors(d, roots[0].payload, roots[0].end)?
            .into_iter()
            .filter(|x| x.tag == 0xc1)
            .collect();
        if scores.len() != 1 || scores[0].end - scores[0].payload < 4 {
            return fail("Google 封面评分描述符无效");
        }
        let children = descriptors(d, scores[0].payload + 4, scores[0].end)?;
        let frames: Vec<&Descriptor> = children.iter().filter(|x| x.tag == 0xc3).collect();
        if frames.len() != 1 || frames[0].end - frames[0].payload != 12 {
            return fail("Google 原始封面时间描述符无效");
        }
        let signed = read(d, frames[0].payload + 4, 8, false)? as i64;
        let mut high = Vec::new();
        let tracks: Vec<&Descriptor> = children.iter().filter(|x| x.tag == 0xc2).collect();
        if tracks.len() > 1 {
            return fail("Google 高清候选时间描述符重复");
        }
        if let Some(t) = tracks.first() {
            if t.end - t.payload < 4 {
                return fail("Google 高清候选时间描述符被截断");
            }
            let count = be(d, t.payload, 4)?;
            let entries = descriptors(d, t.payload + 4, t.end)?;
            if entries.len() != count
                || !entries
                    .iter()
                    .all(|e| e.tag == 0xc3 && e.end - e.payload == 12)
            {
                return fail("Google 高清候选帧数与时间描述符不一致");
            }
            for e in &entries {
                let v = read(d, e.payload + 4, 8, false)? as i64;
                high.push(if v >= 0 { Some(v) } else { None });
            }
            let known: Vec<i64> = high.iter().flatten().copied().collect();
            if known.windows(2).any(|w| w[0] > w[1]) {
                return fail("Google 高清候选时间未按顺序排列");
            }
        }
        times.push(ImageTimes {
            primary: if signed >= 0 { Some(signed) } else { None },
            high_resolution: high,
        });
    }
    if times.iter().any(|t| Some(t) != times.first()) {
        return fail_as(ErrorKind::Cover, "视频中的封面时间描述符冲突");
    }
    Ok(times.into_iter().next())
}

/// One media sample, fully located: byte range, decode and presentation
/// time in the track timescale, and sync (keyframe) flag.
/// Upper bound for decode times in track ticks: room for a 32-bit ctts
/// offset or stts delta on top without leaving i64.
const MAX_TICKS: u64 = (i64::MAX as u64) - 2 * (u32::MAX as u64);

#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub offset: usize,
    pub size: usize,
    pub decode_time: u64,
    /// Decode duration (stts delta) in track timescale units.
    pub duration: u64,
    pub presentation_time: i64,
    pub sync: bool,
}

/// All samples of one track, in file order.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackSamples {
    pub handler: String,
    pub timescale: u32,
    pub samples: Vec<Sample>,
    /// Sample-index ranges per chunk, in decode order (mirrors stsc).
    pub chunks: Vec<std::ops::Range<usize>>,
}

fn find_box(d: &[u8], parent: &BoxRef, t: &[u8; 4]) -> Result<BoxRef> {
    let found: Vec<BoxRef> = boxes(d, parent.payload, parent.end)?
        .into_iter()
        .filter(|b| b.is(t))
        .collect();
    if found.len() != 1 {
        return fail(format!(
            "MP4 采样表缺少唯一的 {}",
            String::from_utf8_lossy(t)
        ));
    }
    Ok(found[0].clone())
}

fn find_box_opt(d: &[u8], parent: &BoxRef, t: &[u8; 4]) -> Result<Option<BoxRef>> {
    let found: Vec<BoxRef> = boxes(d, parent.payload, parent.end)?
        .into_iter()
        .filter(|b| b.is(t))
        .collect();
    if found.len() > 1 {
        return fail(format!("MP4 采样表重复出现 {}", String::from_utf8_lossy(t)));
    }
    Ok(found.into_iter().next())
}

/// Entries of a full box (version/flags + entry count + fixed-size records).
fn full_entries(d: &[u8], b: &BoxRef, record: usize) -> Result<Vec<usize>> {
    if b.end - b.payload < 8 {
        return fail("MP4 采样表被截断");
    }
    let count = be(d, b.payload + 4, 4)?;
    let at = b.payload + 8;
    if count > (b.end - at) / record {
        return fail("MP4 采样表被截断");
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let mut row = Vec::with_capacity(record / 4);
        for j in 0..record / 4 {
            row.push(be(d, at + i * record + j * 4, 4)?);
        }
        out.push(row);
    }
    // Flattened below by callers that need per-field access; kept as rows.
    Ok(out.into_iter().flatten().collect())
}

fn table_u32(d: &[u8], b: &BoxRef) -> Result<Vec<u32>> {
    if b.end - b.payload < 8 {
        return fail("MP4 采样表被截断");
    }
    let count = be(d, b.payload + 4, 4)?;
    let at = b.payload + 8;
    if count > (b.end - at) / 4 {
        return fail("MP4 采样表被截断");
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(be(d, at + i * 4, 4)? as u32);
    }
    Ok(out)
}

/// Sample tables of every track: stts/stsz/stsc/stco-or-co64, with optional
/// ctts (presentation offsets) and stss (sync samples). This is the reader
/// ROADMAP item 7 needs for cover time<->frame conversion and for remux:
/// it locates every sample byte range and its timestamps without decoding.
pub fn sample_tables(d: &[u8]) -> Result<Vec<TrackSamples>> {
    let moov = find_moov(d)?;
    Ok(tracks_in(d, &moov)?
        .into_iter()
        .map(|t| t.samples)
        .collect())
}

fn find_moov(d: &[u8]) -> Result<BoxRef> {
    match boxes(d, 0, d.len())?.into_iter().find(|b| b.is(b"moov")) {
        Some(m) => Ok(m),
        None => fail("MP4 缺少 moov"),
    }
}

/// One track of a [`Movie`]: its `trak` box and decoded sample tables.
#[derive(Clone, Debug)]
pub struct Track {
    pub trak: BoxRef,
    pub samples: TrackSamples,
}

/// An MP4/MOV parsed once: `moov`, movie timescale and every track's sample
/// tables. Pipelines pass this around instead of re-walking the file per
/// step (cover matching, remux, verification).
#[derive(Clone, Debug)]
pub struct Movie {
    pub moov: BoxRef,
    pub timescale: u32,
    pub tracks: Vec<Track>,
}

impl Movie {
    pub fn parse(d: &[u8]) -> Result<Movie> {
        let moov = find_moov(d)?;
        let timescale = movie_timescale(d, &moov)?;
        let tracks = tracks_in(d, &moov)?;
        Ok(Movie {
            moov,
            timescale,
            tracks,
        })
    }

    /// Video track `index` among video tracks (0 = first in file order).
    pub fn video(&self, index: usize) -> Result<&Track> {
        match self
            .tracks
            .iter()
            .filter(|t| t.samples.handler == "vide")
            .nth(index)
        {
            Some(t) => Ok(t),
            None => fail_as(ErrorKind::InvalidArgument, "视频没有所选的视频轨"),
        }
    }
}

/// Most samples accepted across all tracks of one file.
const MAX_SAMPLES: usize = 1_000_000;

fn tracks_in(d: &[u8], moov: &BoxRef) -> Result<Vec<Track>> {
    let mut tracks = Vec::new();
    let mut sample_budget = MAX_SAMPLES;
    for trak in boxes(d, moov.payload, moov.end)?
        .into_iter()
        .filter(|b| b.is(b"trak"))
    {
        let mdia = find_box(d, &trak, b"mdia")?;
        let mdhd = find_box(d, &mdia, b"mdhd")?;
        if mdhd.end - mdhd.payload < 4 {
            return fail("MP4 采样表被截断");
        }
        let version = d[mdhd.payload];
        let timescale_at = if version == 1 { 20 } else { 12 };
        if mdhd.end - mdhd.payload < timescale_at + 4 {
            return fail("MP4 采样表被截断");
        }
        let timescale = be(d, mdhd.payload + timescale_at, 4)? as u32;
        let hdlr = find_box(d, &mdia, b"hdlr")?;
        if hdlr.end - hdlr.payload < 12 {
            return fail("MP4 采样表被截断");
        }
        let handler = String::from_utf8_lossy(&d[hdlr.payload + 8..hdlr.payload + 12]).into_owned();
        let minf = find_box(d, &mdia, b"minf")?;
        let stbl = find_box(d, &minf, b"stbl")?;
        let stts = find_box(d, &stbl, b"stts")?;
        let stsc = find_box(d, &stbl, b"stsc")?;
        let stsz = find_box(d, &stbl, b"stsz")?;
        let stco = find_box_opt(d, &stbl, b"stco")?;
        let co64 = find_box_opt(d, &stbl, b"co64")?;
        let (chunk_offsets, _is64) = match (stco, co64) {
            (Some(_), Some(_)) => return fail("MP4 采样表同时出现 stco 与 co64"),
            (None, None) => return fail("MP4 采样表缺少块偏移"),
            (Some(b), None) => (
                table_u32(d, &b)?
                    .into_iter()
                    .map(|v| v as usize)
                    .collect::<Vec<_>>(),
                false,
            ),
            (None, Some(b)) => {
                if b.end - b.payload < 8 {
                    return fail("MP4 采样表被截断");
                }
                let count = be(d, b.payload + 4, 4)?;
                let at = b.payload + 8;
                if count > (b.end - at) / 8 {
                    return fail("MP4 采样表被截断");
                }
                let mut out = Vec::with_capacity(count);
                for i in 0..count {
                    out.push(crate::binary::int(d, at + i * 8, 8, false)?);
                }
                (out, true)
            }
        };
        // stsz: fixed size or per-sample sizes.
        if stsz.end - stsz.payload < 12 {
            return fail("MP4 采样表被截断");
        }
        let fixed = be(d, stsz.payload + 4, 4)?;
        let sample_count = be(d, stsz.payload + 8, 4)?;
        // Resource limit, checked before anything is sized by the count. A
        // few header bytes can claim billions of samples (or, with a fixed
        // sample size, tens of millions inside a modest file); every sample
        // costs ~100 bytes across the tables. Motion Photo clips have a few
        // hundred samples per track; the budget is shared by all tracks.
        if sample_count > sample_budget
            || sample_count > d.len()
            || (fixed != 0 && sample_count > d.len() / fixed)
        {
            return fail_as(ErrorKind::Unsupported, "MP4 样本数量超出安全资源限制");
        }
        sample_budget -= sample_count;
        // stsz entries start at payload+12, unlike the other tables.
        let sizes: Vec<usize> = if fixed != 0 {
            vec![fixed; sample_count]
        } else {
            let at = stsz.payload + 12;
            if sample_count > (stsz.end - at) / 4 {
                return fail("MP4 采样表被截断");
            }
            let mut out = Vec::with_capacity(sample_count);
            for i in 0..sample_count {
                out.push(be(d, at + i * 4, 4)?);
            }
            out
        };
        // stts: (sample_count, sample_delta) pairs.
        let stts_rows = full_entries(d, &stts, 8)?;
        // The rows must add up to the stsz sample count; check that before
        // expanding them (one 8-byte row can claim 2^32 samples).
        let rows_total: u64 = stts_rows
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| p[0] as u64)
            .sum();
        if rows_total != sample_count as u64 {
            return fail("MP4 采样表样本数不一致");
        }
        let mut decode_times: Vec<u64> = Vec::new();
        let mut durations: Vec<u64> = Vec::new();
        {
            let mut t: u64 = 0;
            for pair in stts_rows.as_chunks::<2>().0 {
                let (count, delta) = (pair[0], pair[1] as u64);
                for _ in 0..count {
                    decode_times.push(t);
                    durations.push(delta);
                    // Keep decode times (plus a ctts offset or stts delta)
                    // inside i64; only crafted tables get near this.
                    t = match t.checked_add(delta) {
                        Some(v) if v <= MAX_TICKS => v,
                        _ => return fail("MP4 时间戳超出范围"),
                    };
                }
            }
        }
        if sizes.len() != sample_count || decode_times.len() != sample_count {
            return fail("MP4 采样表样本数不一致");
        }
        // stsc: (first_chunk, samples_per_chunk, _) triples, 1-based chunks.
        let stsc_rows = full_entries(d, &stsc, 12)?;
        if stsc_rows.is_empty() && sample_count > 0 {
            return fail("MP4 采样表缺少块样本映射");
        }
        let triples: &[[usize; 3]] = stsc_rows.as_chunks::<3>().0;
        if triples.first().is_some_and(|r| r[0] != 1)
            || triples.windows(2).any(|w| w[0][0] >= w[1][0])
        {
            return fail("MP4 采样表块映射无效");
        }
        let total_chunks = chunk_offsets.len();
        let mut chunk_samples: Vec<usize> = Vec::new();
        {
            for (i, t) in triples.iter().enumerate() {
                let first = t[0];
                let last = triples.get(i + 1).map_or(total_chunks + 1, |n| n[0]);
                if first < 1 || first > total_chunks + 1 || last < first {
                    return fail("MP4 采样表块映射越界");
                }
                for _ in first..last {
                    chunk_samples.push(t[1]);
                }
            }
        }
        if chunk_samples.len() != total_chunks {
            return fail("MP4 采样表块数与块偏移数不一致");
        }
        if chunk_samples.iter().sum::<usize>() != sample_count {
            return fail("MP4 采样表块样本数与样本总数不一致");
        }
        // ctts: presentation offsets (version 0 unsigned, version 1 signed).
        let mut offsets: Vec<i64> = vec![0; sample_count];
        if let Some(ctts) = find_box_opt(d, &stbl, b"ctts")? {
            if ctts.end - ctts.payload < 4 {
                return fail("MP4 采样表被截断");
            }
            let version = d[ctts.payload];
            let rows = full_entries(d, &ctts, 8)?;
            let mut idx = 0usize;
            for pair in rows.as_chunks::<2>().0 {
                let (count, raw) = (pair[0], pair[1]);
                for _ in 0..count {
                    if idx >= sample_count {
                        return fail("MP4 采样表样本数不一致");
                    }
                    offsets[idx] = if version == 1 {
                        raw as u32 as i32 as i64
                    } else {
                        raw as i64
                    };
                    idx += 1;
                }
            }
            if idx != sample_count {
                return fail("MP4 采样表样本数不一致");
            }
        }
        // stss: 1-based sync sample numbers; absent means all sync.
        let mut sync = vec![true; sample_count];
        if let Some(stss) = find_box_opt(d, &stbl, b"stss")? {
            sync = vec![false; sample_count];
            for n in table_u32(d, &stss)? {
                if n < 1 || n as usize > sample_count {
                    return fail("MP4 采样表同步样本越界");
                }
                sync[n as usize - 1] = true;
            }
        }
        // Walk chunks in order, laying samples out by size.
        let mut samples = Vec::with_capacity(sample_count);
        let mut chunk_ranges: Vec<std::ops::Range<usize>> = Vec::with_capacity(total_chunks);
        let mut next = 0usize;
        for (ci, per) in chunk_samples.iter().enumerate() {
            let start = next;
            let mut at = chunk_offsets[ci];
            for _ in 0..*per {
                if next >= sample_count {
                    return fail("MP4 采样表样本数不一致");
                }
                let size = sizes[next];
                if size == 0 || at > d.len() || size > d.len() - at {
                    return fail("MP4 样本超出文件范围");
                }
                samples.push(Sample {
                    offset: at,
                    size,
                    decode_time: decode_times[next],
                    duration: durations[next],
                    presentation_time: decode_times[next] as i64 + offsets[next],
                    sync: sync[next],
                });
                at += size;
                next += 1;
            }
            chunk_ranges.push(start..next);
        }
        if next != sample_count {
            return fail("MP4 采样表样本数不一致");
        }
        tracks.push(Track {
            samples: TrackSamples {
                handler,
                timescale,
                samples,
                chunks: chunk_ranges,
            },
            trak,
        });
    }
    Ok(tracks)
}

/// Top-level box types of an embedded MP4, validating every size.
pub fn top_level_types(video: &[u8], prefix: &str) -> Result<Vec<String>> {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < video.len() {
        if pos + 8 > video.len() {
            return fail(format!("{prefix} 数据头被截断"));
        }
        let mut size = be(video, pos, 4)?;
        let mut header = 8;
        if size == 1 {
            size = be(video, pos + 8, 8)?;
            header = 16;
        } else if size == 0 {
            size = video.len() - pos;
        }
        if size < header || size > video.len() - pos {
            return fail(format!("{prefix} box 长度无效"));
        }
        out.push(String::from_utf8_lossy(&video[pos + 4..pos + 8]).into_owned());
        pos += size;
    }
    Ok(out)
}

/// Edit-list entries as (segment_duration in movie timescale, media_time
/// in track timescale, -1 for an empty edit), or None if the track has no
/// edts box.
pub fn edit_list(d: &[u8], trak: &BoxRef) -> Result<Option<Vec<(i64, i64)>>> {
    let edts_list: Vec<BoxRef> = boxes(d, trak.payload, trak.end)?
        .into_iter()
        .filter(|b| b.is(b"edts"))
        .collect();
    if edts_list.is_empty() {
        return Ok(None);
    }
    if edts_list.len() > 1 {
        return fail("MP4 轨道重复出现 edts");
    }
    let elst_list: Vec<BoxRef> = boxes(d, edts_list[0].payload, edts_list[0].end)?
        .into_iter()
        .filter(|b| b.is(b"elst"))
        .collect();
    if elst_list.len() != 1 {
        return fail("MP4 编辑列表缺少唯一的 elst");
    }
    let elst = &elst_list[0];
    if elst.end - elst.payload < 8 {
        return fail("MP4 elst 被截断");
    }
    let version = d[elst.payload];
    let count = crate::binary::be(d, elst.payload + 4, 4)?;
    let (record, at) = if version == 1 {
        (20usize, elst.payload + 8)
    } else {
        (12usize, elst.payload + 8)
    };
    if count > (elst.end - at) / record {
        return fail("MP4 elst 被截断");
    }
    let mut out = Vec::new();
    for i in 0..count {
        let p = at + i * record;
        if version == 1 {
            let dur_u = crate::binary::be(d, p, 8)? as u64;
            if dur_u > i64::MAX as u64 {
                return fail("MP4 编辑时长超出范围");
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

/// Movie timescale from `moov/mvhd` (version 0 or 1).
pub fn movie_timescale(d: &[u8], moov: &BoxRef) -> Result<u32> {
    let mvhd: Vec<BoxRef> = boxes(d, moov.payload, moov.end)?
        .into_iter()
        .filter(|b| b.is(b"mvhd"))
        .collect();
    if mvhd.len() != 1 {
        return fail("MP4 缺少唯一的 mvhd");
    }
    let mvhd = &mvhd[0];
    if mvhd.end <= mvhd.payload {
        return fail("MP4 mvhd 被截断");
    }
    let at = if d[mvhd.payload] == 1 { 20 } else { 12 };
    if mvhd.end - mvhd.payload < at + 4 {
        return fail("MP4 mvhd 被截断");
    }
    let ts = be(d, mvhd.payload + at, 4)? as u32;
    if ts == 0 {
        return fail("MP4 电影时间尺度无效");
    }
    Ok(ts)
}

/// An ISO BMFF box: 32-bit size, type, payload. Callers keep payloads far
/// below 4 GB (metadata boxes); `mdat`/`mpvd` sizes are checked where built.
pub(crate) fn bx(t: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(t);
    v.extend(payload);
    v
}

/// A full box: version, 24-bit flags, payload.
pub(crate) fn full(t: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = vec![
        version,
        ((flags >> 16) & 0xff) as u8,
        ((flags >> 8) & 0xff) as u8,
        (flags & 0xff) as u8,
    ];
    p.extend(payload);
    bx(t, &p)
}
