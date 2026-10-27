//! Bounded ISO BMFF walk and Google cover-time descriptors (MotionVideoMetadata).
use crate::binary::{be, read};
use crate::{fail, Result};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub struct BoxRef { pub ty: [u8; 4], pub payload: usize, pub end: usize }

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
        out.push(BoxRef { ty: [d[p + 4], d[p + 5], d[p + 6], d[p + 7]], payload: p + header, end: p + size });
        p += size;
    }
    Ok(out)
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
