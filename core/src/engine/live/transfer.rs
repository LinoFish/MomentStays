//! Google Motion Photo -> Honor (Swift `AndroidTransfer`): accept only a
//! Google-standard JPEG whose every byte is explained, rewrite it with the
//! Google layout, and prove that nothing but the motion declarations moved.

use super::motion as android;
use crate::engine::options::{AndroidOptions, ConversionOptions};
use crate::engine::output::{self, Stage};
use crate::engine::paths::stem;
use crate::engine::scanrow::{self, ScanRow, SourceVersion};
use crate::engine::{failure, hash_file, read, Native, StopToken, Timings};
use crate::live::{self, TimeDomain};
use crate::model::Cover;
use crate::mp4::Movie;
use crate::pymotion;
use crate::pyxml::Element;
use crate::xmp::{self, CAMERA, CONTAINER, HDR, ITEM, NOTE, RDF};
use crate::{format, jpeg, sha256_hex, Error, MotionPhoto, Result};
use serde_json::{json, Value};
use std::path::Path;

pub use crate::engine::contracts::TRANSFER_REVISION as REVISION;
const XMPMETA: &str = "adobe:ns:meta/";
const OPPO: &str = "http://ns.oplus.com/photos/1.0/camera/";
const VIVO: &str = "http://ns.vivo.com/photos/1.0/camera/";
const MPF: &[u8] = b"MPF\0";

/// Swift `AndroidTransfer.rejection`: every refusal starts with its class.
pub fn rejection(e: Error) -> Error {
    let classified = [
        "来源不支持",
        "文件损坏",
        "封面时间不明确",
        "结构无法无损保留",
    ]
    .iter()
    .any(|p| e.0.starts_with(p));
    if classified {
        e
    } else {
        Error::new(format!("文件损坏：{}", e.0))
    }
}

/// Every element in pre-order with its parent (None for the root).
fn walk<'a>(
    e: &'a Element,
    parent: Option<&'a Element>,
    out: &mut Vec<(&'a Element, Option<&'a Element>)>,
) {
    out.push((e, parent));
    for c in &e.children {
        walk(c, Some(e), out);
    }
}

/// Swift `MLXMP.property`: attribute first, else a direct child's text.
fn property<'a>(e: &'a Element, ns: &str, name: &str) -> Option<&'a str> {
    e.get_attr(ns, name).or_else(|| {
        e.find_child(ns, name)
            .map(|c| c.text.as_deref().unwrap_or(""))
    })
}

/// Swift `AndroidTransfer.inspect`.
pub fn inspect<'a>(native: &dyn Native, data: &'a [u8]) -> Result<MotionPhoto<'a>> {
    inspect_structure(native, data).map_err(rejection)
}

fn inspect_structure<'a>(native: &dyn Native, data: &'a [u8]) -> Result<MotionPhoto<'a>> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return failure("来源不支持：暂仅支持 Google → 荣耀的 Motion Photo JPEG，不支持 HEIC");
    }
    if native.image_decodes(data) == Some(false) {
        return failure("文件损坏：JPEG 图像无法完整读取");
    }
    let packets = xmp::packets(data)?;
    let modern = xmp::integer(&packets, "MotionPhoto")?;
    let legacy = xmp::integer(&packets, "MicroVideo")?;
    if modern != Some(1) && legacy != Some(1) {
        return failure("来源不支持：缺少 Google Motion Photo 声明");
    }
    if !(modern.is_none() || modern == Some(1)) || !(legacy.is_none() || legacy == Some(1)) {
        return failure("文件损坏：Google 动态声明冲突");
    }
    for key in ["MotionPhotoVersion", "MicroVideoVersion"] {
        if let Some(v) = xmp::integer(&packets, key)? {
            if v != 1 {
                return failure("来源不支持：未知 Google 动态版本");
            }
        }
    }
    let trees = pymotion::xmp_packets(data)?;
    for p in &trees {
        let mut all = Vec::new();
        walk(&p.root, None, &mut all);
        for (e, _) in &all {
            let mut names: Vec<(&str, &str, Option<&str>)> =
                vec![(e.ns.as_str(), e.local.as_str(), e.text.as_deref())];
            names.extend(
                e.attrib
                    .iter()
                    .map(|(n, l, v)| (n.as_str(), l.as_str(), Some(v.as_str()))),
            );
            for (uri, local, value) in names {
                let lower = uri.to_lowercase();
                if uri == OPPO
                    || uri == VIVO
                    || ["samsung", "xiaomi", "huawei", "honor"]
                        .iter()
                        .any(|v| lower.contains(v))
                {
                    return failure("来源不支持：检测到其他厂商动态结构");
                }
                if uri == HDR && local == "Version" && value.map(str::trim) != Some("1.0") {
                    return failure("结构无法无损保留：未知增益图版本");
                }
            }
        }
    }
    let photo = format::parse(data)?;
    if photo.format != "google-compatible" || photo.cover_frame.is_some() {
        return failure("来源不支持：不是 Google 标准动态封装");
    }
    let mut ranges = jpeg::image_ranges(data)?;
    ranges.sort_by_key(|r| r.start);
    if !(ranges.len() == 1 || (ranges.len() == 2 && photo.ultra)) {
        return failure("结构无法无损保留：附加图片无法确认是增益图");
    }
    let mut end = 0;
    for r in &ranges {
        if r.start != end {
            return failure("结构无法无损保留：图片之间存在未解释载荷");
        }
        end = r.end;
    }
    let video = photo.video(data);
    if end + video.len() != data.len() || &data[end..] != video {
        return failure("结构无法无损保留：存在额外尾标、间隙或未解释载荷");
    }
    if let Some(offset) = xmp::integer(&packets, "MicroVideoOffset")? {
        if offset != video.len() as i64 {
            return failure("文件损坏：旧版偏移与视频长度冲突");
        }
    }
    let mut directories: Vec<&Element> = Vec::new();
    for p in &trees {
        let mut all = Vec::new();
        walk(&p.root, None, &mut all);
        directories.extend(
            all.iter()
                .map(|(e, _)| *e)
                .filter(|e| e.ns == CONTAINER && e.local == "Directory"),
        );
    }
    if directories.len() > 1 || (modern == Some(1) && directories.len() != 1) {
        return failure("结构无法无损保留：Google 目录缺失或重复");
    }
    const PROPS: [&str; 4] = ["Semantic", "Mime", "Length", "Padding"];
    if let Some(directory) = directories.first() {
        let mut nodes = Vec::new();
        walk(directory, None, &mut nodes);
        for (node, _) in &nodes {
            let (uri, name) = (node.ns.as_str(), node.local.as_str());
            let structural = (uri == CONTAINER && ["Directory", "Item"].contains(&name))
                || (uri == RDF && ["Seq", "li"].contains(&name));
            let prop = uri == ITEM && PROPS.contains(&name);
            if !(structural || prop) {
                return failure("结构无法无损保留：目录包含未知字段");
            }
            for (n, l, v) in &node.attrib {
                let known = n == ITEM && PROPS.contains(&l.as_str());
                let resource = n == RDF && l == "parseType" && v == "Resource";
                if !(known || resource) {
                    return failure("结构无法无损保留：目录包含未知属性");
                }
            }
        }
        let items: Vec<&Element> = nodes
            .iter()
            .map(|(e, _)| *e)
            .filter(|e| property(e, ITEM, "Semantic").is_some())
            .collect();
        let semantics: Vec<&str> = items
            .iter()
            .filter_map(|e| property(e, ITEM, "Semantic"))
            .collect();
        let expected: &[&str] = if ranges.len() == 2 {
            &["Primary", "GainMap", "MotionPhoto"]
        } else {
            &["Primary", "MotionPhoto"]
        };
        if semantics != expected {
            return failure("结构无法无损保留：目录含未知或重复载荷");
        }
        for (i, item) in items.iter().enumerate() {
            for name in PROPS {
                if crate::pyxml::values(item, ITEM, name).len() > 1 {
                    return failure("文件损坏：目录属性重复或冲突");
                }
            }
            let semantic = semantics[i];
            let want = match semantic {
                "Primary" => 0,
                "GainMap" => ranges[1].len(),
                _ => video.len(),
            };
            let length = property(item, ITEM, "Length").unwrap_or("0");
            let padding = property(item, ITEM, "Padding").unwrap_or("0");
            let mime = property(item, ITEM, "Mime");
            let want_mime = if semantic == "MotionPhoto" {
                "video/mp4"
            } else {
                "image/jpeg"
            };
            if xmp::swift_int(length) != Some(want as i64)
                || xmp::swift_int(padding) != Some(0)
                || mime != Some(want_mime)
            {
                return failure("文件损坏：目录长度、类型或填充与实际载荷不符");
            }
        }
    }
    let times: Vec<i64> = [
        xmp::integer(&packets, "MotionPhotoPresentationTimestampUs")?,
        xmp::integer(&packets, "MicroVideoPresentationTimestampUs")?,
    ]
    .into_iter()
    .flatten()
    .collect();
    if times.is_empty()
        || times.iter().any(|t| *t < 0)
        || times.iter().any(|t| *t != times[0])
        || photo.timestamp != Some(times[0])
    {
        return failure("封面时间不明确：时间缺失、无效或声明冲突");
    }
    let result = android::assemble(
        &photo.still,
        video,
        &AndroidOptions::vendor("google"),
        times[0],
        0,
    )?;
    validate(data, &photo, &result)?;
    Ok(photo)
}

fn rewritten(ns: &str, name: &str) -> bool {
    (ns == CONTAINER && name == "Directory")
        || (ns == HDR && name == "Version")
        || (ns == NOTE && name == "HasExtendedXMP")
        || (ns == CAMERA
            && [
                "MotionPhoto",
                "MotionPhotoVersion",
                "MotionPhotoPresentationTimestampUs",
                "MicroVideo",
                "MicroVideoVersion",
                "MicroVideoOffset",
                "MicroVideoPresentationTimestampUs",
            ]
            .contains(&name))
}

/// A canonical property tree (prefixes and packet grouping ignored).
fn tree(e: &Element) -> Option<String> {
    if rewritten(&e.ns, &e.local) {
        return None;
    }
    let mut attributes: Vec<String> = e
        .attrib
        .iter()
        .filter(|(n, l, _)| !rewritten(n, l))
        .map(|(n, l, v)| format!("{n}:{l}={v}"))
        .collect();
    attributes.sort();
    let nested: Vec<String> = e.children.iter().filter_map(tree).collect();
    let text = if e.children.is_empty() {
        e.text.clone().unwrap_or_default()
    } else {
        String::new()
    };
    Some(json!([e.ns, e.local, attributes, nested, text]).to_string())
}

/// Swift `AndroidTransfer.metadata`: everything that must survive.
pub fn metadata(still: &[u8]) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for s in jpeg::segments(still)? {
        let p = s.payload(still);
        if !((0xE0..=0xEF).contains(&s.marker) || s.marker == 0xFE) {
            continue;
        }
        if s.marker == 0xE2 && p.starts_with(MPF) {
            continue;
        }
        if s.marker == 0xE1 && (p.starts_with(xmp::HEADER) || p.starts_with(xmp::EXTENDED)) {
            continue;
        }
        values.push(format!("{}:{}", s.marker, sha256_hex(p)));
    }
    for packet in pymotion::xmp_packets(still)? {
        let mut all = Vec::new();
        walk(&packet.root, None, &mut all);
        // Ancestor test: inside an rdf:Description whose parent is rdf:RDF.
        fn inside_resource(path: &[&Element]) -> bool {
            path.windows(2).any(|w| {
                w[0].ns == RDF
                    && w[0].local == "RDF"
                    && w[1].ns == RDF
                    && w[1].local == "Description"
            })
        }
        fn visit<'a>(e: &'a Element, path: &mut Vec<&'a Element>, values: &mut Vec<String>) {
            let wrapper = (e.ns == XMPMETA && e.local == "xmpmeta")
                || (e.ns == RDF && (e.local == "RDF" || e.local == "Description"));
            if !wrapper && !inside_resource(path) {
                if let Some(t) = tree(e) {
                    values.push(format!("outside-resource:{t}"));
                }
            }
            if wrapper && e.local != "Description" {
                for (n, l, v) in &e.attrib {
                    if l != "xmptk" {
                        values.push(format!("wrapper:{n}:{l}={v}"));
                    }
                }
            }
            let is_resource = e.ns == RDF
                && e.local == "Description"
                && path.last().is_some_and(|p| p.local == "RDF");
            if is_resource {
                for (n, l, v) in &e.attrib {
                    if rewritten(n, l) || (n == RDF && l == "about" && v.is_empty()) {
                        continue;
                    }
                    values.push(format!("{n}:{l}={v}"));
                }
                for c in &e.children {
                    if let Some(t) = tree(c) {
                        values.push(t);
                    }
                }
            }
            path.push(e);
            for c in &e.children {
                visit(c, path, values);
            }
            path.pop();
        }
        visit(&packet.root, &mut Vec::new(), &mut values);
    }
    values.sort();
    Ok(values)
}

/// Swift `AndroidTransfer.validate`.
pub fn validate(source: &[u8], photo: &MotionPhoto, result: &[u8]) -> Result<()> {
    let after = format::parse(result)?;
    let ok = after.video(result) == photo.video(source)
        && after.timestamp == photo.timestamp
        && format::coding_hash(&after.still)? == format::coding_hash(&photo.still)?
        && format::auxiliary_hashes(&after.still)? == format::auxiliary_hashes(&photo.still)?
        && metadata(&after.still)? == metadata(&photo.still)?;
    if !ok {
        return failure("结构无法无损保留：图像、EXIF、增益图或必要元数据校验失败");
    }
    Ok(())
}

/// Swift `AndroidTransfer.cover`: the recorded time must land on one frame.
pub fn cover(video: &[u8], timestamp: i64) -> Result<Value> {
    let movie = Movie::parse(video).map_err(rejection)?;
    if movie.video(0).is_err() {
        return failure("文件损坏：视频缺少画面轨");
    }
    let m = live::resolve_cover_in(video, &movie, Cover::Time(timestamp), TimeDomain::Auto, 0)
        .map_err(|e| {
            let e = crate::engine::movie::cover_error(e, false);
            Error::new(format!("封面时间不明确：{}", e.0))
        })?;
    Ok(json!({
        "cover_timestamp_us": timestamp,
        "mapped_seconds": m.mapped_us.unwrap_or(timestamp) as f64 / 1e6,
        "domain": m.domain,
        "video_sha256": sha256_hex(video),
    }))
}

/// Swift `AndroidTransfer.scanFile`.
pub fn scan_file(
    native: &dyn Native,
    source: &Path,
    relative: &str,
    bytes: Option<&[u8]>,
) -> ScanRow {
    let before = SourceVersion::read(source);
    let result = (|| -> Result<ScanRow> {
        let owned;
        let data = match bytes {
            Some(b) => b,
            None => {
                owned = read(source)?;
                &owned
            }
        };
        let photo = inspect(native, data)?;
        let ts = photo.timestamp.unwrap_or(0);
        cover(photo.video(data), ts)?;
        Ok(ScanRow {
            source: source.to_path_buf(),
            relative: relative.to_string(),
            is_motion: true,
            hdr: if photo.ultra {
                "Ultra HDR · 保留"
            } else {
                "原始 JPEG"
            }
            .into(),
            cover: format!("{:.6} 秒", ts as f64 / 1e6),
            detail: "Google → 荣耀；原视频与源封面保持不变".into(),
            manufacturer: "Google 标准".into(),
            manufacturer_detail: "依据 XMP、目录及实际媒体结构".into(),
            ..ScanRow::default()
        })
    })();
    match result {
        Ok(mut row) => {
            row.version = before.filter(|v| Some(*v) == SourceVersion::read(source));
            row
        }
        Err(e) => {
            let base = scanrow::scan_file(native, source, relative, bytes);
            if base.is_static_hdr() || base.burst.is_some() {
                return base;
            }
            ScanRow {
                is_motion: false,
                detail: rejection(e).0,
                ..base
            }
        }
    }
}

/// Swift `AndroidTransfer.convert`.
pub fn convert(
    native: &dyn Native,
    source: &Path,
    target: &Path,
    options: &ConversionOptions,
    token: &StopToken,
    overwrite: bool,
    timings: &Timings,
) -> Result<Value> {
    let hash = hash_file(source)?;
    if !overwrite && target.exists() {
        let previous = output::verify(target)?;
        if previous["implementation"].as_str() != Some(REVISION)
            || previous["source_sha256"].as_str() != Some(hash.as_str())
            || !crate::engine::same_json(&previous["options"], &options.json())
        {
            return failure("已有输出的来源或设置不同，请选择覆盖或跳过");
        }
        return Ok(
            json!({"status": "resumed", "source": source.to_string_lossy(), "output": target.to_string_lossy(), "detail": "已校验 Google → 荣耀结果，跳过重复转换"}),
        );
    }
    if token.is_cancelled() {
        return failure("已停止");
    }
    let stage = Stage::new(target)?;
    let data = read(source)?;
    let photo = timings.measure("input_validation", || inspect(native, &data))?;
    let ts = photo.timestamp.unwrap_or(0);
    let timing = cover(photo.video(&data), ts)?;
    let result = android::assemble(
        &photo.still,
        photo.video(&data),
        &AndroidOptions::vendor("google"),
        ts,
        0,
    )?;
    let name = format!("{}.MP.jpg", stem(source));
    let file = stage.path().join(&name);
    crate::engine::write(&file, &result)?;
    validate(&data, &photo, &read(&file)?)?;
    let hdr =
        json!({"status": "preserved", "reason": "JPEG、EXIF、原视频、源封面及增益图保持不变"});
    let manifest = json!({
        "implementation": REVISION,
        "source": source.to_string_lossy(),
        "source_sha256": hash,
        "options": options.json(),
        "output_files": {"motionphoto": name},
        "output_sha256": {name.clone(): sha256_hex(&result)},
        "timing": timing,
        "hdr": hdr,
    });
    crate::engine::write_json(&manifest, &stage.path().join("manifest.json"))?;
    output::publish(
        native,
        &stage,
        target,
        source,
        &hash,
        token,
        overwrite,
        options.export_data_files,
    )?;
    Ok(json!({
        "status": "converted",
        "source": source.to_string_lossy(),
        "output": target.to_string_lossy(),
        "output_files": {"motionphoto": name},
        "timing": timing,
        "hdr": hdr,
        "detail": "Google → 荣耀：已无损保留原视频和源封面并校验",
        "verification": manifest,
    }))
}
