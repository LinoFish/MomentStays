//! Experimental OPPO UHDR reader, independently implemented from media bytes.
//! See docs/EXPERIMENTAL_OPPO_UHDR.md for evidence and deliberately narrow scope.
//! No LHDR reconstruction or imported converter code.
mod container;
mod gain;

use crate::{fail, heif, jpeg, Result};
use serde_json::Value;
use std::collections::HashSet;

pub const PROFILE: &str = "oppo-uhdr-scalar-hevc444-3";
const FOOTER: &[u8] = b"\0jxrs";
const MAX_MANIFEST: usize = 256 * 1024;

pub struct Source<'a> {
    pub base: &'a [u8],
    pub gain: &'a [u8],
    pub boost: f64,
    pub gain_width: usize,
    pub gain_height: usize,
    pub gain_channels: u8,
}

/// Only the observed EOF-anchored manifest is admitted. Length includes the
/// nine-byte footer, and each payload offset counts back from the JSON start.
/// A missing marker is not evidence of UHDR. A broken declared tail is an error.
pub fn read(data: &[u8]) -> Result<Option<Source<'_>>> {
    if !heif::matches(data) || data.len() < 9 || &data[data.len() - 9..data.len() - 4] != FOOTER {
        return Ok(None);
    }
    if data.len() > 256 << 20 {
        return fail("OPPO UHDR 超过 256 MiB 安全上限");
    }
    let n = u32::from_le_bytes(data[data.len() - 4..].try_into().unwrap()) as usize;
    if !(11..=MAX_MANIFEST).contains(&n) || n > data.len() {
        return fail("OPPO 私有尾部长度无效");
    }
    let start = data.len() - n;
    // JSON duplicate object keys are rejected before serde_json can overwrite
    // them (this protocol only needs name, offset, length and version).
    let entries: Vec<Entry> = serde_json::from_slice(&data[start..data.len() - 9])
        .map_err(|_| crate::Error::new("OPPO 私有尾部目录无效或字段重复"))?;
    if entries.is_empty() || entries.len() > 256 {
        return fail("OPPO 私有尾部条目数量无效");
    }
    let mut names = HashSet::new();
    let mut ranges = Vec::new();
    let mut info = None;
    let mut gain = None;
    for e in entries {
        if e.name.is_empty()
            || e.name.len() > 256
            || !names.insert(e.name.clone())
            || e.version != 1
            || e.length == 0
            || e.offset > start
            || e.length > e.offset
        {
            return fail("OPPO 私有条目版本、名称或范围无效");
        }
        let range = start - e.offset..start - e.offset + e.length;
        if ranges
            .iter()
            .any(|r: &std::ops::Range<usize>| r.start < range.end && range.start < r.end)
        {
            return fail("OPPO 私有条目重叠");
        }
        match e.name.as_str() {
            "local.uhdr.gainmap.info" => info = Some(range.clone()),
            "local.uhdr.gainmap.data" => gain = Some(range.clone()),
            "hdr.transform.data"
            | "master.mode.preset.info"
            | "private.emptyspace"
            | "watermark.color"
            | "watermark.device"
            | "watermark.master.params"
            | "watermark.params" => {}
            _ => return fail("OPPO 私有目录含未验证资源；首版不处理实况、人像或未知附属数据"),
        }
        ranges.push(range);
    }
    let (info, gain) = match (info, gain) {
        (Some(info), Some(gain)) => (info, gain),
        (None, None) => return Ok(None),
        _ => return fail("OPPO UHDR 增益图或参数缺失"),
    };
    let earliest = ranges.iter().map(|r| r.start).min().unwrap();
    // Walk the standard HEIF prefix, never scan image payloads for box names.
    // QTI and all following private bytes are outside the rewritten image.
    let mut end = 0usize;
    while end + 8 <= earliest {
        let mut size = u32::from_be_bytes(data[end..end + 4].try_into().unwrap()) as usize;
        let ty = &data[end + 4..end + 8];
        if ![&b"ftyp"[..], b"meta", b"free", b"skip", b"mdat"].contains(&ty) {
            break;
        }
        let header = if size == 1 {
            if earliest - end < 16 {
                return fail("OPPO HEIF 扩展长度被截断");
            }
            size = usize::try_from(u64::from_be_bytes(
                data[end + 8..end + 16].try_into().unwrap(),
            ))
            .map_err(|_| crate::Error::new("OPPO HEIF 长度溢出"))?;
            16
        } else {
            8
        };
        if size < header || size > earliest - end {
            return fail("OPPO HEIF 前缀长度无效");
        }
        end += size;
    }
    if end == 0 || ranges.iter().any(|r| r.start < end) {
        return fail("OPPO 私有载荷与 HEIF 重叠");
    }
    let base = &data[..end];
    let layout = heif::layout(base)?;
    if layout.items.iter().any(|i| i.ty == "tmap") {
        return fail("OPPO 原片已含标准 HDR，不叠加第二组增益图");
    }
    if !crate::scan::confirmed_static(base)
        || crate::live::still_content_identifier(base)?.is_some()
    {
        return fail("OPPO UHDR 首版仅接受无实况声明的完整静态 HEIC");
    }
    let params = &data[info];
    if params.len() != 80 {
        return fail("OPPO UHDR 参数块长度尚不支持");
    }
    let mut f = [0f32; 20];
    for (v, b) in f.iter_mut().zip(params.as_chunks::<4>().0) {
        *v = f32::from_le_bytes(*b);
    }
    // Two independently inspected camera files agree on this scalar profile.
    // Do not infer general RGB/gamma/offset field semantics from equal values.
    let ones = [0, 1, 2, 3, 7, 8, 9, 16];
    if !f.iter().all(|v| v.is_finite())
        || ones.iter().any(|&i| f[i] != 1.0)
        || f[10..16].iter().any(|&v| v != 0.0)
        || f[19] != 0.0
        || !(1.0..=16.0).contains(&f[4])
        || f[4] == 1.0
        || [5, 6, 17, 18].iter().any(|&i| f[i] != f[4])
    {
        return fail("OPPO UHDR 参数不属于已验证的共享增益配置，未猜测 RGB、Gamma 或偏移");
    }
    let gain = &data[gain];
    let (gain_width, gain_height, gain_channels) = jpeg_size(gain)?;
    Ok(Some(Source {
        base,
        gain,
        boost: f[4] as f64,
        gain_width,
        gain_height,
        gain_channels,
    }))
}

#[derive(serde::Deserialize)]
struct Entry {
    name: String,
    offset: usize,
    length: usize,
    version: u32,
}

fn jpeg_size(data: &[u8]) -> Result<(usize, usize, u8)> {
    if jpeg::image_end(data)? != data.len() {
        return fail("OPPO UHDR 增益图不是唯一完整 JPEG");
    }
    let segs = jpeg::segments(data)?;
    let frames: Vec<_> = segs
        .iter()
        .filter(|s| [0xc0, 0xc1, 0xc2].contains(&s.marker))
        .collect();
    if frames.len() != 1 {
        return fail("OPPO UHDR 增益图 JPEG 帧无效");
    }
    let p = frames[0].payload(data);
    if p.len() < 6 || p[0] != 8 || ![1, 3].contains(&p[5]) || p.len() != 6 + 3 * p[5] as usize {
        return fail("OPPO UHDR 增益图仅支持 8 位单通道或 RGB JPEG");
    }
    let h = u16::from_be_bytes([p[1], p[2]]) as usize;
    let w = u16::from_be_bytes([p[3], p[4]]) as usize;
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return fail("OPPO UHDR 增益图尺寸超出支持范围");
    }
    Ok((w, h, p[5]))
}

/// Preserve the primary codestream; encode decoded gain RGB as lossless HEVC
/// 4:4:4 PCM. Host codecs validate both the gain-only HEIF and final SDR decode.
pub fn convert(native: &dyn crate::engine::Native, source: &Source<'_>) -> Result<Vec<u8>> {
    if !source.boost.is_finite()
        || source.boost <= 1.0
        || source.boost > 16.0
        || jpeg_size(source.gain)? != (source.gain_width, source.gain_height, source.gain_channels)
        || source.base.len().saturating_add(source.gain.len()) > 256 << 20
    {
        return fail("OPPO UHDR 来源参数或尺寸无效");
    }
    if native.image_decodes(source.base) != Some(true)
        || native.image_decodes(source.gain) != Some(true)
    {
        return fail("OPPO UHDR 主图或增益图无法完成原生解码验证");
    }
    let donor = gain::graph(source)?;
    if native.image_decodes(&donor) != Some(true) {
        return fail("OPPO HEVC 4:4:4 增益图无法完成原生解码验证");
    }
    let out = container::attach(source, &donor)?;
    if native.image_decodes(&out) != Some(true) {
        return fail("OPPO 标准 HDR HEIC 输出无法解码");
    }
    verify_preserved(source.base, &out)?;
    Ok(out)
}

pub fn verify_preserved(base: &[u8], output: &[u8]) -> Result<()> {
    container::verify_preserved(base, output)
}

/// ISO/TS 21496-1 v0 metadata in a HEIF ToneMapImage v0 payload. This narrow
/// profile uses the base colour space, common channel parameters, unity gamma,
/// zero offsets, SDR headroom 1 and HDR headroom from the observed source.
fn tone_map(boost: f64) -> Vec<u8> {
    let stops = boost.log2();
    let mut out = vec![0, 0, 0, 0, 0, 0x40];
    for value in [0.0, stops, 0.0, stops, 1.0, 0.0, 0.0] {
        out.extend(((value * 1_000_000.0).round() as i32).to_be_bytes());
        out.extend(1_000_000u32.to_be_bytes());
    }
    out
}

/// User-visible evidence, not a claim of full colour/Photos equivalence.
pub fn report(source: &Source<'_>) -> Value {
    serde_json::json!({"input":"oppo-private-uhdr","output":"iso-hdr-heic",
        "status":"converted-experimental","profile":PROFILE,"boost":source.boost,
        "gain_dimensions":[source.gain_width,source.gain_height],"primary_reencoded":false,
        "gain_reencoded":true,"gain_encoding":"hevc-444-pcm","gain_decoded_rgb_preserved":true,
        "private_tail_preserved":false,
        "reason":"OPPO UHDR 已实验性转换为 ISO HDR HEIC；主图 HEVC 保留，增益图由软件编码为 HEVC 4:4:4 PCM，保留解码后的 RGB 像素但文件明显增大；Apple HDR 显示兼容性仍待实测，原私有尾部仅保留在来源文件中"})
}
