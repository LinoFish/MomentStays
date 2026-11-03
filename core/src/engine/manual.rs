//! Direction-limited legacy Huawei input and source-bound manual main-track covers.
use crate::{format, jpeg, mp4, xmp, Error, Result};
use std::borrow::Cow;

fn field(bytes: &[u8]) -> Result<&str> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::new("旧式华为尾标不是 ASCII"))?;
    let value = text.trim_end_matches(' ');
    if value.is_empty() || !value.is_ascii() || value.as_bytes().contains(&b' ') {
        return Err(Error::new("旧式华为尾标字段无效"));
    }
    Ok(value)
}
fn number(text: &str) -> Result<usize> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::new("旧式华为尾标数字无效"));
    }
    text.parse().map_err(|_| Error::new("旧式华为尾标数字溢出"))
}

/// This entry point must be used only by the Android -> Apple input route.
pub fn parse(data: &[u8]) -> Result<format::MotionPhoto<'_>> {
    let baseline = match format::parse(data) {
        Ok(photo) => return Ok(photo),
        Err(error) => error,
    };
    if baseline.0 != "华为/荣耀 LIVE_ 尾标无效" || data.len() < 60 {
        return Err(baseline);
    }
    // A broken versioned trailer must never fall through to the legacy path.
    if data[data.len() - 60] == b'v' {
        return Err(baseline);
    }
    let tail = &data[data.len() - 40..];
    let pair = field(&tail[..20])?;
    let (a, b) = pair
        .split_once(':')
        .ok_or_else(|| Error::new("旧式华为尾标缺少双数字字段"))?;
    let _raw_pair = (number(a)?, number(b)?);
    // The two numbers are retained in the source, not interpreted as a
    // timestamp or a frame: both interpretations remain ambiguous here.
    let length = number(
        field(&tail[20..])?
            .strip_prefix("LIVE_")
            .ok_or_else(|| Error::new("旧式华为 LIVE_ 尾标缺失"))?,
    )?;
    let end = data.len() - 40;
    let start = end
        .checked_sub(length)
        .ok_or_else(|| Error::new("旧式华为视频长度越界"))?;
    if length < 16 || start < jpeg::image_end(data)? {
        return Err(Error::new("旧式华为视频与静态图片重叠"));
    }
    // Narrow candidate: no contradictory XMP and no unverified gain-map
    // interpretation. Preserve every prefix byte, including opaque ctrace.
    if !xmp::packets(data)?.is_empty() || jpeg::image_ranges(&data[..start])?.len() != 1 {
        return Err(Error::new("旧式华为 XMP/附加图片结构尚未验证"));
    }
    let image_end = jpeg::image_end(data)?;
    let gap = &data[image_end..start];
    if !gap.is_empty()
        && (gap.len() != 5420
            || &gap[gap.len() - 8..] != b"ctrace\0\0"
            || u32::from_le_bytes(gap[gap.len() - 12..gap.len() - 8].try_into().unwrap()) as usize
                != gap.len() - 12)
    {
        return Err(Error::new("旧式华为 ctrace 尾框或长度未经验证"));
    }
    let video = &data[start..end];
    let boxes = mp4::boxes(video, 0, video.len())?;
    if boxes.first().is_none_or(|b| !b.is(b"ftyp"))
        || boxes.iter().filter(|b| b.is(b"ftyp")).count() != 1
        || boxes.iter().filter(|b| b.is(b"moov")).count() != 1
        || !boxes.iter().any(|b| b.is(b"mdat"))
    {
        return Err(Error::new("旧式华为视频缺少唯一 ftyp/moov 或 mdat"));
    }
    let movie = mp4::Movie::parse(video)?;
    movie.video(0)?;
    for track in &movie.tracks {
        if track.samples.samples.is_empty()
            || !track.samples.samples.iter().all(|s| {
                boxes.iter().any(|b| {
                    b.is(b"mdat")
                        && s.offset >= b.payload
                        && s.offset.checked_add(s.size).is_some_and(|end| end <= b.end)
                })
            })
        {
            return Err(Error::new("旧式华为视频采样范围无效"));
        }
    }
    Ok(format::MotionPhoto {
        still: Cow::Borrowed(&data[..image_end]),
        video: start..end,
        timestamp: None,
        timestamp_tag: None,
        ultra: false,
        cover_frame: None,
        format: "huawei-honor-legacy".into(),
    })
}

pub const FORMAT: &str = "huawei-honor-legacy";
pub const COVER: &str = "未确定（需选帧）";
use crate::engine::scanrow::SourceVersion;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub source_sha256: String,
    pub source_version: String,
    pub track_index: usize,
    pub frame_index: i64,
}

impl Selection {
    pub fn json(&self) -> Value {
        json!({"source_sha256":self.source_sha256,"source_version":self.source_version,"track_index":self.track_index,"frame_index":self.frame_index})
    }
    pub fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            source_sha256: v["source_sha256"].as_str()?.into(),
            source_version: v["source_version"].as_str()?.into(),
            track_index: usize::try_from(v["track_index"].as_u64()?).ok()?,
            frame_index: v["frame_index"].as_i64()?,
        })
    }
}

pub fn version(path: &Path) -> Result<String> {
    let v = SourceVersion::read(path).ok_or_else(|| Error::new("来源不存在或不可读"))?;
    Ok(format!(
        "{}:{}:{}:{}:{}",
        v.device, v.inode, v.size, v.modified_ns, v.changed_ns
    ))
}

pub fn resolve(data: &[u8], path: &Path, selection: &Selection) -> Result<crate::live::CoverMatch> {
    if selection.source_sha256 != crate::sha256_hex(data)
        || selection.source_version != version(path)?
        || selection.track_index != 0
    {
        return Err(Error::new(
            "人工封面与来源版本、SHA 或主视频轨不匹配，请重新选帧",
        ));
    }
    let photo = parse(data)?;
    if photo.format != FORMAT {
        return Err(Error::new("此输入不使用旧华为人工选帧策略"));
    }
    let video = photo.video(data);
    let movie = mp4::Movie::parse(video)?;
    crate::live::resolve_cover_in(
        video,
        &movie,
        crate::model::Cover::Frame(selection.frame_index),
        crate::live::TimeDomain::Presentation,
        0,
    )
}

pub fn frames(path: &Path, output: &Path) -> Result<Value> {
    let stamp = version(path)?;
    let data = crate::engine::read(path)?;
    let photo = parse(&data)?;
    if photo.format != FORMAT {
        return Err(Error::new("此输入无需旧华为人工主轨选帧"));
    }
    let video = photo.video(&data);
    let movie = mp4::Movie::parse(video)?;
    let count = crate::cover::sorted_presentation(&movie.video(0)?.samples).len();
    let visible: Vec<crate::live::CoverMatch> = (0..count).filter_map(|index| {
        crate::live::resolve_cover_in(video,&movie,crate::model::Cover::Frame(index as i64),crate::live::TimeDomain::Presentation,0).ok()
    }).collect();
    if visible.is_empty() || stamp != version(path)? {
        return Err(Error::new("来源已变化或没有可选画面"));
    }
    let frames: Vec<Value> = visible.iter().map(|m|json!({"frame_index":m.frame,"presented_us":m.marker_us,"media_us":m.frame_time_us})).collect();
    let (hints, issues, raw) = legacy_hints(&data,video,&movie,&visible)?;
    if stamp != version(path)? { return Err(Error::new("来源已变化，请重新读取候选")); }
    crate::engine::write(output, video)?;
    Ok(json!({"source_sha256":crate::sha256_hex(&data),"source_version":stamp,"track_index":0,
        "frames":frames,"source_hints":hints,"hint_issues":issues,"raw_hint_field":raw,
        "hint_policy":"unconfirmed-millisecond-hypotheses; media/presentation domains unverified"}))
}

// Only hypotheses from bytes the source actually contains. Neither unit nor
// domain of the legacy pair is known. Never feed these into default conversion.
fn legacy_hints(data: &[u8], video: &[u8], movie: &mp4::Movie,
                visible: &[crate::live::CoverMatch]) -> Result<(Vec<Value>,Vec<Value>,String)> {
    use crate::live::{resolve_cover_in,TimeDomain};
    let raw = field(&data[data.len()-40..data.len()-20])?.to_owned();
    let mut grouped = std::collections::BTreeMap::<(i64,usize),Vec<Value>>::new();
    let mut issues = Vec::new();
    for (component,value) in raw.split(':').enumerate() {
        let assumed = number(value)?.checked_mul(1000).and_then(|n|i64::try_from(n).ok());
        for (domain,name) in [(TimeDomain::Presentation,"presentation"),(TimeDomain::Media,"media")] {
            let origin = json!({"kind":"legacy-huawei-40-pair","raw_field":raw,"component":component+1,
                "raw_value":value,"interpretation":"milliseconds-hypothesis","time_domain":name,"assumed_us":assumed});
            let result = assumed.ok_or_else(||Error::new("源字段按毫秒尝试时超出范围")).and_then(|us| {
                let mut times = visible.iter().map(|m| if domain == TimeDomain::Media {m.frame_time_us} else {m.marker_us});
                let low = times.next().unwrap_or(0);
                let (low,high) = times.fold((low,low),|(lo,hi),t|(lo.min(t),hi.max(t)));
                if us < low || us > high { return Err(Error::new("源字段的可能时间超出可见采样范围")); }
                resolve_cover_in(video,movie,crate::model::Cover::Time(us),domain,0)
            });
            match result {
                Ok(m) => grouped.entry((m.marker_us,m.frame)).or_default().push(origin),
                Err(e) => issues.push(json!({"source":origin,"issue":e.0})),
            }
        }
    }
    let hints = grouped.into_iter().map(|((time,frame),sources)|json!({"frame_index":frame,
        "presented_us":time,"verified":false,"sources":sources})).collect();
    Ok((hints,issues,raw))
}

/// All source bytes are reversible, including the original MP4 container. These
/// files are opaque archives, never resources submitted to Photos.
pub fn archive(
    data: &[u8],
    photo: &format::MotionPhoto<'_>,
    folder: &Path,
) -> Result<std::collections::BTreeMap<String, String>> {
    let mut names = std::collections::BTreeMap::new();
    let image_end = jpeg::image_end(data)?;
    let mut save = |role: String, name: String, bytes: &[u8]| -> Result<()> {
        crate::engine::write(&folder.join(&name), bytes)?;
        names.insert(role, name);
        Ok(())
    };
    save("legacy_original".into(), "source-original.bin".into(), data)?;
    save(
        "legacy_private".into(),
        "source-private.bin".into(),
        &data[image_end..photo.video.start],
    )?;
    save(
        "legacy_tail".into(),
        "source-tail.bin".into(),
        &data[photo.video.end..],
    )?;
    for (i, seg) in jpeg::segments(&photo.still)?
        .into_iter()
        .filter(|s| s.marker == 0xe1 && s.payload(&photo.still).starts_with(jpeg::EXIF))
        .enumerate()
    {
        save(
            format!("legacy_exif_{i}"),
            format!("source-exif-{i}.bin"),
            &photo.still[seg.start..seg.end],
        )?;
    }
    Ok(names)
}
