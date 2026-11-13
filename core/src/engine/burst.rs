//! Evidence-based Honor / Google / Huawei / Samsung JPEG burst migration.
//! Filename protocol and Apple tags adapted from LinoFish/MomentPort (upstream GPL-3.0-or-later),
//! cb927369c7555d1b38e465d4184c3d64334a2c8d, honor.rs / makernote.rs.
//! Container editing, hashing, TIFF, UUIDs and atomic publication belong to MomentStays.
use super::{failure, hash_file, io_error, output, paths, write, write_json, Native, StopToken};
use crate::{jpeg, pyjpeg, scan, sha256_hex, tiff, Error, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

const REVISION: &str = "honor-burst-2";
const GOOGLE_REVISION: &str = "google-burst-1";
const HUAWEI_REVISION: &str = "huawei-burst-1";
const SAMSUNG_REVISION: &str = "samsung-burst-1";
mod google;
mod samsung;

fn source_format(value: &Value) -> &str {
    value["source_format"].as_str().unwrap_or("honor-burst")
}
fn revision(format: &str) -> Result<&'static str> {
    match format {
        "honor-burst" => Ok(REVISION),
        "google-burst" => Ok(GOOGLE_REVISION),
        "huawei-burst" => Ok(HUAWEI_REVISION),
        "samsung-burst" => Ok(SAMSUNG_REVISION),
        _ => failure("未知连拍来源格式"),
    }
}
pub fn source_make(plan: &Value) -> &'static str {
    match source_format(plan) {
        "google-burst" => "Google",
        "huawei-burst" => "HUAWEI",
        "samsung-burst" => "samsung",
        _ => "",
    }
}
/// Name-only prefilter, not evidence of a supported brand or group.
pub fn filename_candidate(name: &str) -> bool {
    filename(name, "Google").is_some() || samsung::filename(name).is_some()
}
pub const MAX_IMAGE: u64 = 256 << 20;
fn image_read(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| io_error(path, e))?;
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE + 65537)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error(path, e))?;
    if bytes.len() as u64 > MAX_IMAGE + 65536 {
        return failure("连拍资源超过安全上限");
    }
    Ok(bytes)
}

/// Strict filename evidence, never a claim about the original camera's total count.
pub fn filename(name: &str, make: &str) -> Option<Value> {
    if make.trim().eq_ignore_ascii_case("samsung") {
        return samsung::filename(name);
    }
    let google = make.trim().eq_ignore_ascii_case("Google");
    let huawei = make.trim().eq_ignore_ascii_case("HUAWEI");
    if !google && !huawei && !make.trim().is_empty() && !make.trim().eq_ignore_ascii_case("HONOR") {
        return None;
    }
    let upper = name.to_ascii_uppercase();
    let (stem, ext) = upper.rsplit_once('.')?;
    if !matches!(ext, "JPG" | "JPEG") {
        return None;
    }
    let (id, index) = stem.strip_prefix("IMG_")?.split_once("_BURST")?;
    if id.len() != 15
        || id.as_bytes()[8] != b'_'
        || !id
            .bytes()
            .enumerate()
            .all(|(i, c)| i == 8 || c.is_ascii_digit())
    {
        return None;
    }
    let year: u32 = id[..4].parse().ok()?;
    let month: usize = id[4..6].parse().ok()?;
    let day: u32 = id[6..8].parse().ok()?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days[month - 1]
        || id[9..11].parse::<u32>().ok()? > 23
        || id[11..13].parse::<u32>().ok()? > 59
        || id[13..].parse::<u32>().ok()? > 59
    {
        return None;
    }
    let (index, cover) = index
        .strip_suffix("_COVER")
        .map_or((index, false), |v| (v, true));
    if index.len() != 3 || !index.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let index: u16 = index.parse().ok()?;
    if index == 0 && !google {
        return None;
    }
    let mut frame = json!({"source_group":format!("IMG_{id}"),"index":index,"source_cover":cover,"evidence":"honor-filename-v1"});
    if google {
        frame["evidence"] = json!("google-filename-xmp-v1");
        frame["source_format"] = json!("google-burst");
    }
    if huawei {
        frame["evidence"] = json!("huawei-filename-v1");
        frame["source_format"] = json!("huawei-burst");
    }
    Some(frame)
}

pub fn inspect(file: &Path, data: &[u8]) -> Option<Value> {
    let (make, model) = crate::vendor::make_model(data);
    let mut frame = filename(file.file_name()?.to_str()?, &make)?;
    let google = frame["source_format"] == "google-burst";
    let samsung = frame["source_format"] == "samsung-burst";
    let brand = match source_format(&frame) {
        "google-burst" => Some("Google"),
        "huawei-burst" => Some("Huawei"),
        "samsung-burst" => Some("Samsung"),
        _ => None,
    };
    let namespace = paths::normalized(file.parent()?);
    frame["group_id"] = json!(sha256_hex(
        format!(
            "{}\0{}",
            if let Some(brand) = brand {
                format!("{}\0{brand}\0{model}", namespace.display())
            } else {
                namespace.display().to_string()
            },
            frame["source_group"].as_str()?
        )
        .as_bytes()
    ));
    // Keep evidence visible even when the combination is unsupported.
    let support = (|| -> Result<()> {
        if google {
            google::validate(data, &mut frame)?;
        }
        if samsung {
            samsung::validate(data, &mut frame)?;
        }
        if data.len() as u64 > MAX_IMAGE || (!samsung && !scan::confirmed_static(data)) {
            return failure("连拍仅支持完整静态 JPEG；实况组合尚未验证");
        }
        if crate::live::still_content_identifier(data)?.is_some() {
            return failure("连拍含 Apple 实况配对标识；此组合尚未验证，未丢弃配对资源");
        }
        // Validate container framing; unknown but intact HDR resources are retained.
        let _ = jpeg::auxiliary(data)?;
        let _ = jpeg::coding_hash(data)?;
        Ok(())
    })();
    frame["source_sha256"] = json!(sha256_hex(data));
    frame["supported"] = json!(support.is_ok());
    frame["reason"] = json!(support.err().map(|e| e.0).unwrap_or_default());
    Some(frame)
}

/// Group only evidence already supplied by scan. O(n log n), no image reread.
pub fn plans(rows: &[super::scanrow::ScanRow]) -> Vec<Value> {
    let mut groups: BTreeMap<String, Vec<&super::scanrow::ScanRow>> = BTreeMap::new();
    for row in rows {
        if let Some(id) = row.burst.as_ref().and_then(|v| v["group_id"].as_str()) {
            groups.entry(id.into()).or_default().push(row);
        }
    }
    groups.into_iter().map(|(id, mut rows)| {
        rows.sort_by_key(|r| r.burst.as_ref().and_then(|v| v["index"].as_u64()).unwrap_or(0));
        let google = rows[0].burst.as_ref().unwrap()["source_format"] == "google-burst";
        let format = source_format(rows[0].burst.as_ref().unwrap());
        let samsung = format == "samsung-burst";
        let members: Vec<Value> = rows.iter().map(|r| {
            let frame = r.burst.as_ref().unwrap();
            let mut member = json!({"source":paths::json(&r.source),"source_sha256":frame["source_sha256"],"index":frame["index"],
                "source_cover":frame["source_cover"],"version":r.version.map(|v| format!("{}:{}:{}:{}:{}",v.device,v.inode,v.size,v.modified_ns,v.changed_ns))});
            if google { member["google_group"] = frame["google_group"].clone(); }
            if samsung { member["vendor_group"] = frame["vendor_group"].clone(); }
            member
        }).collect();
        let covers: Vec<_> = rows.iter().filter(|r| r.burst.as_ref().unwrap()["source_cover"] == true).collect();
        let consecutive = members.iter().enumerate().all(|(i,m)| m["index"].as_u64() == Some((i + usize::from(!google)) as u64));
        let ordered = members.windows(2).all(|pair| pair[0]["index"].as_u64() < pair[1]["index"].as_u64());
        let unsupported = rows.iter().find(|r| r.burst.as_ref().unwrap()["supported"] != true);
        let google_conflict = google && members.iter().any(|m| m["google_group"] != members[0]["google_group"]);
        let samsung_conflict = samsung && members.iter().any(|m| m["vendor_group"] != members[0]["vendor_group"]);
        let reason = if let Some(row) = unsupported { row.burst.as_ref().unwrap()["reason"].as_str().unwrap_or("来源不支持") }
            else if google_conflict { "Google 连拍成员的 XMP 组标识缺失或冲突，请检查完整原片" }
            else if samsung_conflict { "三星连拍成员的 SEF 连拍数据不一致，请检查完整原片" }
            else if !ordered { "帧序号重复，无法确定原始顺序；可显式按独立照片导出" }
            else if covers.len() > 1 { "发现多个原始 COVER；请选择代表帧" } else { "" };
        let representative = if covers.len() == 1 { paths::json(&covers[0].source) } else { Value::Null };
        let label = rows[0].burst.as_ref().unwrap()["source_group"].clone();
        let mut identity_members = members.clone();
        for member in &mut identity_members { if let Some(source) = member["source"].as_str() { member["source"] = paths::json(&paths::normalized(Path::new(source))); } }
        let signature = sha256_hex(serde_json::to_string(&identity_members).unwrap_or_default().as_bytes());
        let mut plan = json!({"id":id,"label":label,"members":members,"signature":signature,"representative":representative,
            "reason":reason,"ordered":ordered,"has_gaps":!consecutive,"supported":unsupported.is_none() && !google_conflict && !samsung_conflict,"source_cover_count":covers.len(),
            "completeness":"observed-only","original_total_known":false});
        if format != "honor-burst" { plan["source_format"] = json!(format); }
        plan
    }).collect()
}

fn sources(plan: &Value) -> Result<Vec<PathBuf>> {
    let members = plan["members"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 999)
        .ok_or_else(|| Error::new("连拍成员清单无效"))?;
    members
        .iter()
        .map(|m| {
            m["source"]
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| Error::new("连拍成员路径无效"))
        })
        .collect()
}

// Directory version cache: enumerate each unchanged directory once, index
// labels in O(n), then validate each group in O(member count). Bounded across
// long-lived C ABI hosts; file content versions/hashes are checked separately.
struct Inventory {
    version: super::identity::SourceVersion,
    groups: HashMap<String, HashSet<PathBuf>>,
}
fn inventory(parent: &Path, label: &str, make: &str) -> Result<HashSet<PathBuf>> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, String), Inventory>>> = OnceLock::new();
    let parent = paths::normalized(parent);
    let key = (parent.clone(), make.to_string());
    let version = super::identity::SourceVersion::read_directory(&parent)
        .ok_or_else(|| Error::new("连拍来源目录不可读取"))?;
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some(previous) = cache.get(&key) {
        if previous.version == version {
            return Ok(previous.groups.get(label).cloned().unwrap_or_default());
        }
    }
    let mut groups: HashMap<String, HashSet<PathBuf>> = HashMap::new();
    for (i, e) in std::fs::read_dir(&parent)
        .map_err(|e| io_error(&parent, e))?
        .enumerate()
    {
        if i >= 100_000 {
            return failure("来源目录超过安全枚举上限");
        }
        let e = e.map_err(|e| io_error(&parent, e))?;
        if let Some(frame) = filename(&e.file_name().to_string_lossy(), make) {
            if let Some(id) = frame["source_group"].as_str() {
                groups
                    .entry(id.into())
                    .or_default()
                    .insert(paths::normalized(&e.path()));
            }
        }
    }
    if super::identity::SourceVersion::read_directory(&parent) != Some(version) {
        return failure("读取期间连拍来源目录发生变化，请重新检查");
    }
    let result = groups.get(label).cloned().unwrap_or_default();
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.insert(key, Inventory { version, groups });
    Ok(result)
}

/// Revalidate membership as well as content versions. Only directories in the plan
/// are enumerated; no guessed groups, symlink traversal, or unbounded image reads.
fn validate_inventory(plan: &Value) -> Result<()> {
    let files = sources(plan)?;
    let parent = files[0]
        .parent()
        .ok_or_else(|| Error::new("连拍来源目录无效"))?;
    let expected: HashSet<_> = files.iter().map(|p| paths::normalized(p)).collect();
    if expected.len() != files.len() || files.iter().any(|p| p.parent() != Some(parent)) {
        return failure("连拍来源重复或跨目录");
    }
    let label = plan["label"]
        .as_str()
        .ok_or_else(|| Error::new("连拍来源组无效"))?;
    let actual = inventory(parent, label, source_make(plan))?;
    if actual != expected {
        return failure("连拍成员范围发生变化或仅扫描了部分组，请添加完整来源目录后重新检查");
    }
    for (file, member) in files.iter().zip(plan["members"].as_array().unwrap()) {
        let v = super::identity::SourceVersion::read(file)
            .ok_or_else(|| Error::new("连拍成员缺失或不是普通文件"))?;
        let signature = format!(
            "{}:{}:{}:{}:{}",
            v.device, v.inode, v.size, v.modified_ns, v.changed_ns
        );
        if member["version"] != signature {
            return failure("连拍来源版本发生变化，请重新扫描");
        }
        if v.size > MAX_IMAGE {
            return failure("连拍单图超过 256 MiB 安全上限");
        }
    }
    Ok(())
}

fn uuid(seed: &str) -> String {
    let mut hex = sha256_hex(seed.as_bytes());
    hex.replace_range(12..13, "5");
    hex.replace_range(16..17, "8");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
    .to_uppercase()
}

fn note_fields(data: &[u8]) -> Result<(Option<String>, Option<String>)> {
    let mut result = (None, None);
    for s in jpeg::segments(data)? {
        if s.marker != 0xe1 || !s.payload(data).starts_with(jpeg::EXIF) {
            continue;
        }
        let t = tiff::Tiff::new(&s.payload(data)[6..])?;
        let (root, _) = t.root()?;
        if let Some(p) = tiff::find(&root, 0x8769) {
            let (exif, _) = t.directory(p.value_u32(t.little))?;
            if let Some(e) = tiff::find(&exif, 0x927c) {
                let at = e.value_u32(t.little);
                let note = t
                    .data
                    .get(
                        at..at
                            .checked_add(e.count)
                            .ok_or_else(|| Error::new("MakerNote 长度无效"))?,
                    )
                    .ok_or_else(|| Error::new("MakerNote 被截断"))?;
                if !note.starts_with(b"Apple iOS\0\0\x01") || note.len() < 20 {
                    continue;
                }
                let little = match &note[12..14] {
                    b"MM" => false,
                    b"II" => true,
                    _ => return failure("Apple MakerNote 字节序无效"),
                };
                let n = tiff::u16_at(note, 14, little) as usize;
                if 16 + n * 12 + 4 > note.len() {
                    return failure("Apple MakerNote 目录被截断");
                }
                for i in 0..n {
                    let at = 16 + i * 12;
                    let tag = tiff::u16_at(note, at, little);
                    if ![0xb, 0x2b].contains(&tag) {
                        continue;
                    }
                    let len = tiff::u32_at(note, at + 4, little) as usize;
                    let p = tiff::u32_at(note, at + 8, little) as usize;
                    if tiff::u16_at(note, at + 2, little) != 2 || len != 37 {
                        return failure("连拍标识类型或长度无效");
                    }
                    let raw = note
                        .get(p..p.saturating_add(len))
                        .filter(|r| r[36] == 0)
                        .ok_or_else(|| Error::new("连拍标识偏移无效"))?;
                    let v = std::str::from_utf8(&raw[..36])
                        .map_err(|_| Error::new("连拍标识编码无效"))?
                        .to_string();
                    if tag == 0xb {
                        result.0 = Some(v);
                    } else {
                        result.1 = Some(v);
                    }
                }
            }
        }
    }
    Ok(result)
}

fn mark(data: &[u8], group: &str, photo: &str) -> Result<Vec<u8>> {
    let segs = jpeg::segments(data)?;
    let exifs: Vec<_> = segs
        .iter()
        .filter(|s| s.marker == 0xe1 && s.payload(data).starts_with(jpeg::EXIF))
        .collect();
    if exifs.len() > 1 {
        return failure("JPEG 包含多个 EXIF，未猜测活动目录");
    }
    let payload = pyjpeg::exif_note(exifs.first().map(|s| s.payload(data)), |old| {
        pyjpeg::apple_note(old, &[(0xb, group), (0x2b, photo)])
    })?;
    let app = pyjpeg::app(0xe1, &payload)?;
    let mut replacements = HashMap::new();
    let insertion = if let Some(s) = exifs.first() {
        replacements.insert(s.start, app);
        Vec::new()
    } else {
        app
    };
    let out = pyjpeg::edit_header(data, &replacements, &insertion)?;
    if note_fields(&out)? != (Some(group.into()), Some(photo.into())) {
        return failure("连拍标记读回失败");
    }
    if data[jpeg::image_end(data)?..] != out[jpeg::image_end(&out)?..] {
        return failure("JPEG 尾部资源发生变化");
    }
    Ok(out)
}

#[derive(Default, Clone, Copy)]
pub struct Choice<'a> {
    pub representative: Option<&'a str>,
    pub independent: bool,
}

/// One group is one output transaction. Selected members are never silently filled.
// Keep the publication boundary explicit, like output::publish.
#[allow(clippy::too_many_arguments)]
pub fn convert_with_policy(
    native: &dyn Native,
    plan: &Value,
    selected: &[PathBuf],
    choice: Choice<'_>,
    target: &Path,
    token: &StopToken,
    overwrite: bool,
    legacy_hdr: bool,
) -> Result<Value> {
    let google = plan["source_format"] == "google-burst";
    let format = source_format(plan);
    let revision = revision(format)?;
    let Choice {
        representative,
        independent,
    } = choice;
    let mut selected: Vec<_> = selected.iter().map(|p| paths::normalized(p)).collect();
    selected.sort();
    let representative_path = representative
        .or_else(|| plan["representative"].as_str())
        .map(|p| paths::normalized(Path::new(p)));
    let representative = representative_path.as_ref().and_then(|p| p.to_str());
    validate_inventory(plan)?;
    if plan["supported"] != true {
        return failure("此连拍含尚未验证的组合，未剥离特性");
    }
    let all: Vec<_> = sources(plan)?
        .iter()
        .map(|p| paths::normalized(p))
        .collect();
    let selected_set: HashSet<_> = selected.iter().collect();
    let all_set: HashSet<_> = all.iter().collect();
    if selected.is_empty()
        || selected_set.len() != selected.len()
        || selected.iter().any(|p| !all_set.contains(p))
    {
        return failure("连拍选取范围无效");
    }
    let mut indices = Vec::new();
    let mut vendor_groups = HashSet::new();
    for (file, member) in all.iter().zip(plan["members"].as_array().unwrap()) {
        let evidence =
            inspect(file, &image_read(file)?).ok_or_else(|| Error::new("连拍来源证据已失效"))?;
        if evidence["supported"] != true
            || evidence["group_id"] != plan["id"]
            || evidence["index"] != member["index"]
            || evidence["source_cover"] != member["source_cover"]
            || source_format(&evidence) != format
        {
            return failure("连拍来源证据与扫描计划不符，请重新检查");
        }
        if let Some(index) = evidence["index"].as_u64() {
            indices.push(index);
        }
        if google {
            vendor_groups.insert(evidence["google_group"].to_string());
        }
        if format == "samsung-burst" {
            vendor_groups.insert(evidence["vendor_group"].to_string());
        }
    }
    if vendor_groups.len() > 1 {
        return failure("连拍成员的厂商分组证据缺失或冲突");
    }
    let ordered = indices.len() == all.len() && indices.windows(2).all(|p| p[0] < p[1]);
    if !independent && (selected.len() != all.len() || !ordered) {
        return failure(
            "连拍帧序号重复或仅选了部分已扫描成员；请检查选取范围，或显式按独立照片导出",
        );
    }
    if !independent
        && all.len() > 1
        && !representative.is_some_and(|p| selected.iter().any(|f| f.to_str() == Some(p)))
    {
        return failure("请选择一张有效的连拍代表帧");
    }
    let mut hashes = BTreeMap::new();
    for (file, member) in all.iter().zip(plan["members"].as_array().unwrap()) {
        if token.is_cancelled() {
            return failure("已停止，整组未发布");
        }
        let hash = hash_file(file)?;
        let expected = member["source_sha256"].as_str();
        if expected != Some(hash.as_str()) {
            return failure("连拍原片 SHA-256 与扫描计划不符，请重新检查");
        }
        hashes.insert(file.to_string_lossy().into_owned(), hash);
    }
    let processing_kind = if independent || selected.len() == 1 {
        "original-copy"
    } else {
        "burst-marker-conversion"
    };
    let identity = json!({"group":plan["id"],"members":hashes,"selected":selected,"representative":representative,"independent":independent,"hdr_policy":super::hdr::POLICY,"legacy_hdr":legacy_hdr});
    let group = uuid(&identity.to_string());
    if !overwrite && target.exists() {
        let m = output::verify(target)?;
        if m["implementation"] != revision || m["burst"]["identity"] != identity {
            return failure("已有连拍结果与来源、范围或代表帧不同，请显式覆盖");
        }
        validate_inventory(plan)?;
        if token.is_cancelled() {
            return failure("已停止");
        }
        return Ok(
            json!({"processing_kind":processing_kind,"status":"resumed","source":paths::json(&all[0]),"output":paths::json(target),"output_files":m["output_files"],"burst":m["burst"],"hdr":m["hdr"],"detail":format!("已校验复用；{}",super::hdr::summary(&m["hdr"])),"processing_detail":if processing_kind == "original-copy" {"独立照片原样输出已校验复用；未转换连拍标记"} else {"整组文件已校验；图库成组状态需单独回查"}}),
        );
    }
    let stage = output::Stage::new(target)?;
    let mut names = BTreeMap::new();
    let mut outputs = BTreeMap::new();
    let mut members = Vec::new();
    let mut folded = HashSet::new();
    let mut hdr_counts = json!({"converted":0,"preserved":0,"ignored":0,"not-applicable":0});
    for (i, file) in all.iter().filter(|f| selected_set.contains(f)).enumerate() {
        if token.is_cancelled() {
            return failure("已停止，整组未发布");
        }
        let data = image_read(file)?;
        if sha256_hex(&data) != hashes[&file.to_string_lossy().into_owned()] {
            return failure("连拍原片发生变化");
        }
        let evidence = inspect(file, &data).ok_or_else(|| Error::new("连拍证据已失效"))?;
        if evidence["supported"] != true || evidence["group_id"] != plan["id"] {
            return failure("连拍实际内容不受支持");
        }
        if native.image_decodes(&data) == Some(false) {
            return failure("连拍原图无法解码");
        }
        let photo = uuid(&format!(
            "{group}\0{}\0{}",
            file.display(),
            hashes[&file.to_string_lossy().into_owned()]
        ));
        let (prepared, hdr) = if processing_kind == "original-copy" {
            (
                data.clone(),
                json!({"status":"not-applicable","outcome":"not-applicable","policy":super::hdr::POLICY,"reason":"独立照片原样输出，未执行 HDR 转换"}),
            )
        } else {
            let processed = super::hdr::process(
                native,
                data.clone(),
                super::hdr::Options {
                    try_private_hdr: legacy_hdr,
                    already_ultra_hdr: super::hdr::is_ultra(&data)?,
                    allow_legacy_conversion: true,
                },
            )?;
            (processed.image, Value::Object(processed.report))
        };
        let hdr_key = hdr["outcome"].as_str().unwrap_or("ignored");
        hdr_counts[hdr_key] = json!(hdr_counts[hdr_key].as_u64().unwrap_or(0) + 1);
        let out = if processing_kind == "original-copy" {
            prepared.clone()
        } else {
            mark(&prepared, &group, &photo)?
        };
        if crate::format::auxiliary_hashes(&out)? != crate::format::auxiliary_hashes(&prepared)?
            || super::hdr::honor_private(&out)? != super::hdr::honor_private(&prepared)?
        {
            return failure("连拍标记写入改变 HDR 或私有资源");
        }
        if native.image_decodes(&out) == Some(false)
            || jpeg::coding_hash(&data)? != jpeg::coding_hash(&out)?
        {
            return failure("连拍输出图像完整性校验失败");
        }
        let name = paths::file_name(file);
        if !folded.insert(name.to_lowercase()) {
            return failure("连拍文件名冲突");
        }
        write(&stage.path().join(&name), &out)?;
        let _ = super::copy_mtime(file, &stage.path().join(&name));
        names.insert(format!("burst_frame_{i:03}"), name.clone());
        outputs.insert(name.clone(), sha256_hex(&out));
        let original = format!("source-burst-{i:03}.bin");
        write(&stage.path().join(&original), &data)?;
        names.insert(format!("burst_original_{i:03}"), original.clone());
        outputs.insert(original, sha256_hex(&data));
        members.push(json!({"source":paths::json(file),"file":name,"index":evidence["index"],"source_cover":evidence["source_cover"],"hdr":hdr,"prepared_auxiliary_sha256":crate::format::auxiliary_hashes(&prepared)?,"private_extension_sha256":super::hdr::honor_private(&prepared)?.map(|v|sha256_hex(&v)),"representative":representative == file.to_str(),"photo_identifier":if independent || selected.len()==1 {Value::Null} else {json!(photo)}}));
    }
    let burst = json!({"identity":identity,"group_id":plan["id"],"uuid":if independent || selected.len()==1 {Value::Null} else {json!(group)},"members":members,
        "independent":independent,"has_gaps":plan["has_gaps"],"completeness":"observed-only","original_total_known":false,"library_status":"not-checked"});
    let hdr = json!({"policy":super::hdr::POLICY,"status":"per-member","counts":hdr_counts,
        "reason":format!("HDR 转换 {} / 保留 {} / 忽略 {}；逐成员详情可查看",hdr_counts["converted"],hdr_counts["preserved"],hdr_counts["ignored"])});
    let m = json!({"processing_kind":processing_kind,"source_format":format,"implementation":revision,"output_files":names,"output_sha256":outputs,"burst":burst,"timing":{"applicable":false},"hdr":hdr});
    write_json(&m, &stage.path().join(output::MANIFEST))?;
    output::verify(stage.path())?;
    validate_inventory(plan)?;
    for file in &all {
        if hash_file(file)? != hashes[&file.to_string_lossy().into_owned()] {
            return failure("转换期间连拍原片变化，整组未发布");
        }
    }
    validate_inventory(plan)?;
    // Reuse publication's final cancellation/atomic exchange; group sources were all checked above.
    output::publish(
        &stage,
        target,
        &all[0],
        &hashes[&all[0].to_string_lossy().into_owned()],
        token,
        overwrite,
        true,
    )?;
    Ok(
        json!({"processing_kind":processing_kind,"status":"converted","source":paths::json(&all[0]),"output":paths::json(target),"output_files":names,"burst":burst,"hdr":hdr,"detail":format!("{}；{}",if processing_kind == "original-copy" {"独立照片原样输出；未转换连拍标记"} else {"连拍整组已生成并校验；图库成组需单独回查"},hdr["reason"].as_str().unwrap_or("")),"processing_detail":if processing_kind == "original-copy" {"独立照片原样输出；未转换连拍标记"} else {"连拍整组文件已生成并校验；图库成组状态需单独回查"}}),
    )
}

/// Called by the existing manifest verifier, including delivery and resume.
pub fn verify(folder: &Path, manifest: &Value) -> Result<()> {
    let burst = &manifest["burst"];
    let members = burst["members"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 999)
        .ok_or_else(|| Error::new("连拍输出成员无效"))?;
    let names = manifest["output_files"]
        .as_object()
        .ok_or_else(|| Error::new("连拍输出角色无效"))?;
    let google = manifest["source_format"] == "google-burst";
    let format = source_format(manifest);
    let valid_revision = manifest["implementation"] == revision(format)?
        || (format == "honor-burst" && manifest["implementation"] == "honor-burst-1");
    if names.len() != members.len() * 2 || !valid_revision {
        return failure("连拍输出缺少成员或原片归档");
    }
    let marked = burst["independent"] == false && members.len() > 1;
    let mut ids = HashSet::new();
    let mut representatives = 0;
    let mut vendor_groups = HashSet::new();
    for (i, m) in members.iter().enumerate() {
        let role = format!("burst_frame_{i:03}");
        let original_role = format!("burst_original_{i:03}");
        let name = m["file"]
            .as_str()
            .ok_or_else(|| Error::new("连拍文件名无效"))?;
        if names.get(&role) != Some(&json!(name))
            || names.get(&original_role) != Some(&json!(format!("source-burst-{i:03}.bin")))
        {
            return failure("连拍成员角色不匹配");
        }
        let out = image_read(&folder.join(name))?;
        let original = image_read(&folder.join(format!("source-burst-{i:03}.bin")))?;
        let source = m["source"]
            .as_str()
            .ok_or_else(|| Error::new("连拍原片身份无效"))?;
        if format != "honor-burst" {
            let evidence = inspect(Path::new(source), &original)
                .ok_or_else(|| Error::new("连拍归档缺少来源证据"))?;
            if source_format(&evidence) != format
                || evidence["supported"] != true
                || evidence["index"] != m["index"]
                || evidence["source_cover"] != m["source_cover"]
            {
                return failure("连拍归档证据与清单不符");
            }
            if google {
                vendor_groups.insert(evidence["google_group"].to_string());
            }
            if format == "samsung-burst" {
                vendor_groups.insert(evidence["vendor_group"].to_string());
                let mut output_evidence = json!({});
                samsung::validate(&out, &mut output_evidence)?;
                if output_evidence["vendor_group"] != evidence["vendor_group"] {
                    return failure("三星连拍输出的 SEF 连拍证据变化");
                }
                let original_end = jpeg::image_ranges(&original)?.last().unwrap().end;
                let output_end = jpeg::image_ranges(&out)?.last().unwrap().end;
                if original[original_end..] != out[output_end..] {
                    return failure("三星连拍 SEF 尾部资源未原样保留");
                }
            }
        }
        if burst["identity"]["members"][source] != sha256_hex(&original)
            || jpeg::coding_hash(&out)? != jpeg::coding_hash(&original)?
        {
            return failure("连拍原片归档或图像编码校验失败");
        }
        if manifest["implementation"] != "honor-burst-1" {
            if crate::format::auxiliary_hashes(&out)?
                != m["prepared_auxiliary_sha256"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
                || json!(super::hdr::honor_private(&out)?.map(|v| sha256_hex(&v)))
                    != m["private_extension_sha256"]
            {
                return failure("连拍 HDR 输出资源与处理记录不一致");
            }
            if m["hdr"]["outcome"] != "converted"
                && crate::format::auxiliary_hashes(&out)?
                    != crate::format::auxiliary_hashes(&original)?
            {
                return failure("未转换 HDR 的连拍资源发生变化");
            }
        }
        if marked {
            let (group, photo) = note_fields(&out)?;
            if group.as_deref() != burst["uuid"].as_str()
                || photo.as_deref() != m["photo_identifier"].as_str()
                || photo.is_none()
                || !ids.insert(photo)
            {
                return failure("连拍组标识或逐帧标识不一致");
            }
        } else if out != original {
            return failure("独立照片或单帧输出不应改写连拍标记");
        }
        if m["representative"] == true {
            representatives += 1;
        }
    }
    if vendor_groups.len() > 1 {
        return failure("连拍归档包含冲突的厂商组证据");
    }
    if marked && representatives != 1 {
        return failure("连拍必须恰有一张代表帧");
    }
    Ok(())
}

/// Compatibility entry point for existing callers; batch supplies its actual switch.
pub fn convert(
    native: &dyn Native,
    plan: &Value,
    selected: &[PathBuf],
    choice: Choice<'_>,
    target: &Path,
    token: &StopToken,
    overwrite: bool,
) -> Result<Value> {
    convert_with_policy(
        native, plan, selected, choice, target, token, overwrite, true,
    )
}
