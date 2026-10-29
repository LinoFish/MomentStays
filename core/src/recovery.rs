//! Explicit, bounded recovery for malformed top-level MP4 box lengths.
//! A tolerant walk is diagnostic only. Conversion recovery never discards
//! bytes: only an oversized final box length may be corrected, and every
//! sample must remain fully contained in mdat. It does not validate pixels.
use crate::{
    binary, fail,
    mp4::{self, BoxParseMode, BoxRef},
    Result,
};

pub struct ConversionCandidate {
    bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

impl ConversionCandidate {
    /// The caller must obtain explicit user approval after showing warnings.
    pub fn approve(self, user_confirmed: bool) -> Result<Vec<u8>> {
        if !user_confirmed {
            return fail("未确认容错转换，未写入输出");
        }
        Ok(self.bytes)
    }
}

/// Report recovered boxes and warnings without modifying the source.
pub fn inspect_boxes(data: &[u8], mode: BoxParseMode) -> Result<(Vec<BoxRef>, Vec<String>)> {
    let walk = mp4::boxes_with_mode(data, 0, data.len(), mode)?;
    let warnings = if walk.complete {
        vec![]
    } else {
        vec!["容器长度异常或尾部不完整；当前为容错解析结果，不代表媒体完整或可以转换".to_string()]
    };
    Ok((walk.boxes, warnings))
}

/// Build a candidate before asking permission. No filesystem writes occur.
/// Partial headers, unknown trailing bytes, missing sample bytes, and damaged
/// nested tables cannot be repaired by this limited operation and are refused.
pub fn prepare_conversion(data: &[u8]) -> Result<ConversionCandidate> {
    let walk = mp4::boxes_with_mode(data, 0, data.len(), BoxParseMode::Tolerant)?;
    let last = walk
        .boxes
        .last()
        .ok_or_else(|| crate::Error::new("没有可恢复的 MP4 box"))?;
    if last.end != data.len() {
        return fail("尾部有无法识别的数据，未丢弃或猜测修复");
    }
    let mut bytes = data.to_vec();
    let size32 = binary::be(data, last.start, 4)?;
    let declared = if size32 == 1 {
        binary::be(data, last.start + 8, 8)?
    } else {
        size32
    };
    let actual = last.end - last.start;
    let mut warnings = vec![];
    if !walk.complete {
        if declared <= actual {
            return fail("损坏范围无法安全恢复");
        }
        if size32 == 1 {
            binary::put(actual, &mut bytes, last.start + 8, 8, false)?;
        } else {
            binary::put(actual, &mut bytes, last.start, 4, false)?;
        }
        warnings.push("最后一个 box 声明长度超出文件；仅修正长度字段，未补齐或删除媒体字节。媒体编码本身未解码验证".to_string());
    }
    let top = mp4::boxes(&bytes, 0, bytes.len())?;
    if top.first().map_or(true, |b| !b.is(b"ftyp"))
        || top.iter().filter(|b| b.is(b"moov")).count() != 1
        || !top.iter().any(|b| b.is(b"mdat"))
    {
        return fail("恢复后仍缺少有效 MP4 容器结构");
    }
    let tracks = mp4::sample_tables(&bytes)?;
    if !tracks
        .iter()
        .any(|t| t.handler == "vide" && !t.samples.is_empty())
    {
        return fail("恢复后没有可验证的视频样本");
    }
    if tracks
        .iter()
        .any(|t| t.handler != "vide" && t.handler != "soun")
        || tracks.iter().filter(|t| t.handler == "vide").count() != 1
    {
        return fail("容错转换仅接受单视频轨及音频轨，其他轨道未冒险丢弃");
    }
    if top.iter().any(|b| {
        ![b"ftyp", b"moov", b"mdat", b"free", b"skip"]
            .iter()
            .any(|t| b.is(t))
    }) {
        return fail("存在未知顶层载荷，未冒险容错转换");
    }
    let mut ranges = Vec::new();
    for track in &tracks {
        for sample in &track.samples {
            // sample_tables checked file bounds, but conversion also requires
            // ownership by a media payload, never by a box header or metadata.
            let end = sample
                .offset
                .checked_add(sample.size)
                .ok_or_else(|| crate::Error::new("样本范围溢出"))?;
            if !top
                .iter()
                .any(|b| b.is(b"mdat") && sample.offset >= b.payload && end <= b.end)
            {
                return fail("样本不在完整 mdat 载荷内，未尝试容错转换");
            }
            ranges.push(sample.offset..end);
        }
    }
    ranges.sort_by_key(|r| r.start);
    if ranges.windows(2).any(|r| r[0].end > r[1].start) {
        return fail("媒体样本重叠，未尝试容错转换");
    }
    Ok(ConversionCandidate { bytes, warnings })
}
