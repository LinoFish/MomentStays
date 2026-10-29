//! Bounded ISO BMFF walk and Google cover-time descriptors (MotionVideoMetadata).
use crate::binary::{be, read};
use crate::{fail, Result};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub struct BoxRef { pub ty: [u8; 4], pub start: usize, pub payload: usize, pub end: usize }

impl BoxRef {
    pub fn is(&self, t: &[u8; 4]) -> bool { &self.ty == t }
    pub fn type_str(&self) -> String { String::from_utf8_lossy(&self.ty).into_owned() }
}

pub fn boxes(d: &[u8], start: usize, end: usize) -> Result<Vec<BoxRef>> {
    if end > d.len() || start > end { return fail("MP4 元数据范围无效"); }
    let mut out = Vec::new();
    let mut p = start;
    while p < end {
        if end - p < 8 { return fail("MP4 元数据头被截断"); }
        let mut size = be(d, p, 4)?;
        let mut header = 8;
        if size == 1 {
            if end - p < 16 { return fail("MP4 扩展数据头被截断"); }
            size = be(d, p + 8, 8)?;
            header = 16;
        } else if size == 0 { size = end - p; }
        if size < header || size > end - p { return fail("MP4 元数据长度无效"); }
        out.push(BoxRef { ty: [d[p + 4], d[p + 5], d[p + 6], d[p + 7]], start: p, payload: p + header, end: p + size });
        p += size;
    }
    Ok(out)
}

/// Strict validation is the default for production reads and every rewrite.
/// Tolerant walks are explicit diagnostics only; they do not validate media.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxParseMode { Strict, Tolerant }

#[derive(Clone, Debug)]
pub struct BoxWalk {
    pub boxes: Vec<BoxRef>,
    /// False when damaged bytes were skipped or a declared box was clipped.
    /// Even true only describes box framing, not sample or payload validity.
    pub complete: bool,
}

/// Walk with an explicit recovery policy. A partial walk must never be used
/// as evidence that a source is safe to rewrite or losslessly convert.
pub fn boxes_with_mode(d: &[u8], start: usize, end: usize, mode: BoxParseMode) -> Result<BoxWalk> {
    if mode == BoxParseMode::Strict {
        return boxes(d, start, end).map(|boxes| BoxWalk { boxes, complete: true });
    }
    if end > d.len() || start > end { return fail("MP4 元数据范围无效"); }
    let mut out = Vec::new();
    let mut p = start;
    let mut complete = true;
    while p < end {
        if end - p < 8 { complete = false; break; }
        let mut size = be(d, p, 4)?;
        let mut header = 8;
        if size == 1 {
            if end - p < 16 { complete = false; break; }
            size = be(d, p + 8, 8)?;
            header = 16;
        } else if size == 0 { size = end - p; }
        if size < header { complete = false; break; }
        let box_end = if size > end - p { complete = false; end } else { p + size };
        out.push(BoxRef { ty: [d[p + 4], d[p + 5], d[p + 6], d[p + 7]], start: p, payload: p + header, end: box_end });
        p = box_end;
    }
    Ok(BoxWalk { boxes: out, complete })
}

fn child(d: &[u8], parent: &BoxRef, t: &[u8; 4]) -> Result<BoxRef> {
    let found: Vec<BoxRef> = boxes(d, parent.payload, parent.end)?.into_iter().filter(|b| b.is(t)).collect();
    if found.len() != 1 { return fail(format!("Google 视频元数据缺少唯一的 {}", String::from_utf8_lossy(t))); }
    Ok(found[0].clone())
}

fn word(d: &[u8], b: &BoxRef, offset: usize, width: usize) -> Result<usize> {
    if b.payload + offset + width > b.end { return fail("Google 视频元数据表被截断"); }
    be(d, b.payload + offset, width)
}

struct Descriptor { tag: u8, payload: usize, end: usize }

fn descriptors(d: &[u8], start: usize, end: usize) -> Result<Vec<Descriptor>> {
    let mut p = start;
    let mut out = Vec::new();
    while p < end {
        let tag = d[p];
        p += 1;
        let (mut size, mut complete) = (0usize, false);
        for _ in 0..4 {
            if p >= end { return fail("Google 封面描述符被截断"); }
            let v = d[p];
            p += 1;
            size = (size << 7) | (v & 127) as usize;
            if v & 128 == 0 { complete = true; break; }
        }
        if !complete || size > end - p { return fail("Google 封面描述符长度无效"); }
        out.push(Descriptor { tag, payload: p, end: p + size });
        p += size;
    }
    Ok(out)
}

fn image_samples(d: &[u8]) -> Result<Vec<Range<usize>>> {
    let top = boxes(d, 0, d.len())?;
    let moov = match top.iter().find(|b| b.is(b"moov")) { Some(m) => m.clone(), None => return Ok(vec![]) };
    let mut samples = Vec::new();
    for track in boxes(d, moov.payload, moov.end)?.into_iter().filter(|b| b.is(b"trak")) {
        let mdia = child(d, &track, b"mdia")?;
        let handler = child(d, &mdia, b"hdlr")?;
        if handler.end - handler.payload < 12 || &d[handler.payload + 8..handler.payload + 12] != b"meta" { continue; }
        let stbl = child(d, &child(d, &mdia, b"minf")?, b"stbl")?;
        let stsd = child(d, &stbl, b"stsd")?;
        if word(d, &stsd, 4, 4)? != 1 { continue; }
        let entries = boxes(d, stsd.payload + 8, stsd.end)?;
        if entries.len() != 1 || !entries[0].is(b"mett") { continue; }
        let e = &entries[0];
        let has = d[e.payload..e.end].split(|b| *b == 0).filter(|f| !f.is_empty())
            .any(|f| f == b"application/motionphoto-image-meta");
        if !has { continue; }
        let sizes = child(d, &stbl, b"stsz")?;
        let chunks = child(d, &stbl, b"stsc")?;
        if word(d, &sizes, 8, 4)? != 1 || word(d, &chunks, 4, 4)? != 1 || word(d, &chunks, 8, 4)? != 1
            || word(d, &chunks, 12, 4)? != 1 || word(d, &chunks, 16, 4)? != 1 {
            return fail("Google 封面元数据不是单样本布局");
        }
        let tables: Vec<BoxRef> = boxes(d, stbl.payload, stbl.end)?.into_iter().filter(|b| b.is(b"stco") || b.is(b"co64")).collect();
        if tables.len() != 1 || word(d, &tables[0], 4, 4)? != 1 { return fail("Google 封面块偏移无效"); }
        let offset = word(d, &tables[0], 8, if tables[0].is(b"co64") { 8 } else { 4 })?;
        let fixed = word(d, &sizes, 4, 4)?;
        let size = if fixed == 0 { word(d, &sizes, 12, 4)? } else { fixed };
        if size == 0 || offset > d.len() || size > d.len() - offset
            || !top.iter().any(|b| b.is(b"mdat") && offset >= b.payload && offset + size <= b.end) {
            return fail("Google 封面样本超出媒体数据范围");
        }
        samples.push(offset..offset + size);
    }
    Ok(samples)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageTimes { pub primary: Option<i64>, pub high_resolution: Vec<Option<i64>> }

pub fn image_times(d: &[u8]) -> Result<Option<ImageTimes>> {
    let mut times = Vec::new();
    for sample in image_samples(d)? {
        let roots = descriptors(d, sample.start, sample.end)?;
        if roots.len() != 1 || roots[0].tag != 0xc0 { return fail("Google 封面根描述符无效"); }
        let scores: Vec<Descriptor> = descriptors(d, roots[0].payload, roots[0].end)?.into_iter().filter(|x| x.tag == 0xc1).collect();
        if scores.len() != 1 || scores[0].end - scores[0].payload < 4 { return fail("Google 封面评分描述符无效"); }
        let children = descriptors(d, scores[0].payload + 4, scores[0].end)?;
        let frames: Vec<&Descriptor> = children.iter().filter(|x| x.tag == 0xc3).collect();
        if frames.len() != 1 || frames[0].end - frames[0].payload != 12 { return fail("Google 原始封面时间描述符无效"); }
        let signed = read(d, frames[0].payload + 4, 8, false)? as i64;
        let mut high = Vec::new();
        let tracks: Vec<&Descriptor> = children.iter().filter(|x| x.tag == 0xc2).collect();
        if tracks.len() > 1 { return fail("Google 高清候选时间描述符重复"); }
        if let Some(t) = tracks.first() {
            if t.end - t.payload < 4 { return fail("Google 高清候选时间描述符被截断"); }
            let count = be(d, t.payload, 4)?;
            let entries = descriptors(d, t.payload + 4, t.end)?;
            if entries.len() != count || !entries.iter().all(|e| e.tag == 0xc3 && e.end - e.payload == 12) {
                return fail("Google 高清候选帧数与时间描述符不一致");
            }
            for e in &entries {
                let v = read(d, e.payload + 4, 8, false)? as i64;
                high.push(if v >= 0 { Some(v) } else { None });
            }
            let known: Vec<i64> = high.iter().flatten().copied().collect();
            if known.windows(2).any(|w| w[0] > w[1]) { return fail("Google 高清候选时间未按顺序排列"); }
        }
        times.push(ImageTimes { primary: if signed >= 0 { Some(signed) } else { None }, high_resolution: high });
    }
    if times.iter().any(|t| Some(t) != times.first()) { return fail("视频中的封面时间描述符冲突"); }
    Ok(times.into_iter().next())
}

/// One media sample, fully located: byte range, decode and presentation
/// time in the track timescale, and sync (keyframe) flag.
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
        return fail(format!("MP4 采样表缺少唯一的 {}", String::from_utf8_lossy(t)));
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
    let top = boxes(d, 0, d.len())?;
    let moov = match top.iter().find(|b| b.is(b"moov")) {
        Some(m) => m.clone(),
        None => return fail("MP4 缺少 moov"),
    };
    let mut tracks = Vec::new();
    // Bound expansion of run-length encoded timing tables across all tracks.
    let mut sample_budget = 1_000_000usize;
    for trak in boxes(d, moov.payload, moov.end)?.into_iter().filter(|b| b.is(b"trak")) {
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
            (Some(b), None) => (table_u32(d, &b)?.into_iter().map(|v| v as usize).collect::<Vec<_>>(), false),
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
        if sample_count > sample_budget || sample_count > d.len()
            || (fixed != 0 && sample_count > d.len() / fixed) {
            return fail("MP4 样本数量超出安全资源限制");
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
        let mut decode_times: Vec<u64> = Vec::new();
        let mut durations: Vec<u64> = Vec::new();
        {
            let mut t: u64 = 0;
            for pair in stts_rows.chunks_exact(2) {
                let (count, delta) = (pair[0], pair[1] as u64);
                if count > sample_count - decode_times.len() {
                    return fail("MP4 采样表样本数不一致");
                }
                for _ in 0..count {
                    decode_times.push(t);
                    durations.push(delta);
                    t = t.checked_add(delta).filter(|v| *v <= i64::MAX as u64)
                        .ok_or_else(|| crate::Error::new("MP4 解码时间溢出"))?;
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
        if stsc_rows.chunks_exact(3).next().map_or(false, |r| r[0] != 1)
            || stsc_rows.chunks_exact(3).collect::<Vec<_>>().windows(2).any(|w| w[0][0] >= w[1][0])
        {
            return fail("MP4 采样表块映射无效");
        }
        let total_chunks = chunk_offsets.len();
        let mut chunk_samples: Vec<usize> = Vec::new();
        {
            let triples: Vec<&[usize]> = stsc_rows.chunks_exact(3).collect();
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
            for pair in rows.chunks_exact(2) {
                let (count, raw) = (pair[0], pair[1]);
                for _ in 0..count {
                    if idx >= sample_count {
                        return fail("MP4 采样表样本数不一致");
                    }
                    offsets[idx] = if version == 1 { raw as u32 as i32 as i64 } else { raw as i64 };
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
                    presentation_time: (decode_times[next] as i64).checked_add(offsets[next])
                        .ok_or_else(|| crate::Error::new("MP4 呈现时间溢出"))?,
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
        tracks.push(TrackSamples { handler, timescale, samples, chunks: chunk_ranges });
    }
    Ok(tracks)
}

/// Top-level box types of an embedded MP4, validating every size.
pub fn top_level_types(video: &[u8], prefix: &str) -> Result<Vec<String>> {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < video.len() {
        if pos + 8 > video.len() { return fail(format!("{prefix} 数据头被截断")); }
        let mut size = be(video, pos, 4)?;
        let mut header = 8;
        if size == 1 { size = be(video, pos + 8, 8)?; header = 16; } else if size == 0 { size = video.len() - pos; }
        if size < header || size > video.len() - pos { return fail(format!("{prefix} box 长度无效")); }
        out.push(String::from_utf8_lossy(&video[pos + 4..pos + 8]).into_owned());
        pos += size;
    }
    Ok(out)
}
