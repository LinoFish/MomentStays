//! HEIC writers: XMP rewrite and Motion Photo assembly (ROADMAP A3).
//!
//! Mirrors Swift `HEIFMetadata.write` + `AndroidExport.assemble` (HEIC
//! path) byte-for-byte in layout: item payloads are repacked into a
//! single `mdat`, `iloc`/`iinf`/`iref` are rebuilt (never patched in
//! place, so no offset guessing), and the coding hash must be unchanged
//! after an XMP rewrite. The assembled file is `still + mpvd{video +
//! tail}` where the tail is the Samsung SEF directory for Samsung and
//! empty for Google; the mpvd video pointer is the absolute file offset
//! of the video start (still length + 8), exactly as Swift computes it.
//!
//! Only Google and Samsung targets are supported, like Swift
//! `AndroidExport.convert` (other vendors have no verified HEIC layout).
//! Validation mirrors Swift: parse-back via `format::parse` (video
//! bytes, still coding hash, cover time) after every write.

use crate::format;
use crate::heif;
use crate::model::Vendor;
use crate::mp4::{self, BoxRef};
use crate::pyjpeg;
use crate::pymotion::{visit_mut, CAMERA, CONTAINER, ITEM, RDF, XMPMETA};
use crate::pyxml::{self, Element};
use crate::{fail, Result};
use std::collections::HashSet;

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

fn be(v: u64, w: usize) -> Vec<u8> {
    v.to_be_bytes()[8 - w..].to_vec()
}

/// The motion XMP document for a HEIC still, mirroring Swift
/// `AndroidExport.xmp` with `heic: true`: GCamera motion keys plus a
/// Container directory whose Primary item is `image/heic` with
/// `Padding 8` (the mpvd header) and whose MotionPhoto item carries the
/// video length. No GainMap item: Swift's HEIC path passes no gain
/// length.
fn motion_doc_heic(time: i64, video_len: usize) -> Element {
    let mut desc = Element::new(RDF, "Description");
    desc.set_attr(CAMERA, "MotionPhoto", "1");
    desc.set_attr(CAMERA, "MotionPhotoVersion", "1");
    desc.set_attr(CAMERA, "MotionPhotoPresentationTimestampUs", &time.to_string());
    let mut dir = Element::new(CONTAINER, "Directory");
    let mut seq = Element::new(RDF, "Seq");
    let mut primary = Element::new(CONTAINER, "Item");
    primary.set_attr(ITEM, "Semantic", "Primary");
    primary.set_attr(ITEM, "Mime", "image/heic");
    primary.set_attr(ITEM, "Length", "0");
    primary.set_attr(ITEM, "Padding", "8");
    let mut li = Element::new(RDF, "li");
    li.set_attr(RDF, "parseType", "Resource");
    li.children.push(primary);
    seq.children.push(li);
    let mut motion = Element::new(CONTAINER, "Item");
    motion.set_attr(ITEM, "Semantic", "MotionPhoto");
    motion.set_attr(ITEM, "Mime", "video/mp4");
    motion.set_attr(ITEM, "Length", &video_len.to_string());
    let mut li2 = Element::new(RDF, "li");
    li2.set_attr(RDF, "parseType", "Resource");
    li2.children.push(motion);
    seq.children.push(li2);
    dir.children.push(seq);
    desc.children.push(dir);
    let mut rdf = Element::new(RDF, "RDF");
    rdf.children.push(desc);
    let mut root = Element::new(XMPMETA, "xmpmeta");
    root.children.push(rdf);
    root
}

fn one<'a>(children: &'a [BoxRef], t: &[u8; 4], what: &str) -> Result<&'a BoxRef> {
    let found: Vec<&BoxRef> = children.iter().filter(|b| b.is(t)).collect();
    if found.len() != 1 {
        return fail(format!("HEIC 缺少唯一的{what}"));
    }
    Ok(found[0])
}

fn primary_id(still: &[u8], children: &[BoxRef]) -> Result<usize> {
    let pitm = one(children, b"pitm", "pitm")?;
    if pitm.end - pitm.payload < 6 || still[pitm.payload] > 1 {
        return fail("HEIC 主图引用无效");
    }
    let id_width = if still[pitm.payload] == 0 { 2 } else { 4 };
    crate::binary::be(still, pitm.payload + 4, id_width)
}

/// Repack a HEIC still's item payloads into one `mdat`, rebuilding
/// `iloc` (v2, 8-byte offsets/lengths, no base/index) and `iinf`, and
/// rebuilding `iref` only when adding an item (old version/flags +
/// entries kept, new `cdsc` reference appended). `replacements` maps
/// item id to new bytes; `added` is a new item (id, `infe` entry body,
/// payload) plus the primary id it references via `cdsc`. Other meta
/// children are copied verbatim; `idat` is dropped because every
/// payload moves into `mdat`. The coding hash must be unchanged, like
/// Swift's guard — Exif and mime items are excluded from it, so XMP
/// rewrites and identifier injects pass while any coding change fails.
fn repack(
    still: &[u8],
    layout: &heif::Layout,
    replacements: &[(usize, Vec<u8>)],
    added: Option<(usize, Vec<u8>, Vec<u8>, usize)>,
) -> Result<Vec<u8>> {
    let children = &layout.children;
    let ftyp = match layout.boxes.iter().find(|b| b.is(b"ftyp")) {
        Some(b) => b.clone(),
        None => return fail("HEIC 缺少 ftyp"),
    };
    let prefix = still[ftyp.start..ftyp.end].to_vec();
    let mut payload: Vec<u8> = Vec::new();
    let mut locations: Vec<(usize, usize, usize)> = Vec::new();
    for item in &layout.items {
        let bytes = match replacements.iter().find(|(id, _)| *id == item.id) {
            Some((_, b)) => b.clone(),
            None => heif::item_bytes(still, item),
        };
        locations.push((item.id, prefix.len() + 8 + payload.len(), bytes.len()));
        payload.extend_from_slice(&bytes);
    }
    if let Some((new_id, _, bytes, _)) = &added {
        locations.push((*new_id, prefix.len() + 8 + payload.len(), bytes.len()));
        payload.extend_from_slice(bytes);
    }
    // Rebuild iloc (v2, 8-byte offsets/lengths, no base/index).
    let mut iloc_body = vec![2u8, 0, 0, 0, 0x88, 0];
    iloc_body.extend(be(locations.len() as u64, 4));
    for (id, offset, length) in &locations {
        iloc_body.extend(be(*id as u64, 4));
        iloc_body.extend([0u8, 0, 0, 0, 0, 1]);
        iloc_body.extend(be(*offset as u64, 8));
        iloc_body.extend(be(*length as u64, 8));
    }
    let iloc = bx(b"iloc", &iloc_body);
    // Rebuild iinf (v0, 2-byte count); append the new infe when adding.
    let iinf_box = one(children, b"iinf", "iinf")?;
    let old_count_width = if still[iinf_box.payload] == 0 { 2 } else { 4 };
    let mut infe = still[iinf_box.payload + 4 + old_count_width..iinf_box.end].to_vec();
    if let Some((_, infe_entry, _, _)) = &added {
        infe.extend(bx(b"infe", infe_entry));
    }
    let mut iinf_body = vec![0u8, 0, 0, 0];
    iinf_body.extend(be(locations.len() as u64, 2));
    iinf_body.extend(infe);
    let iinf = bx(b"iinf", &iinf_body);
    // Rebuild iref only when adding a mime item: keep the old
    // version/flags + entries, append the new cdsc reference.
    let mut iref_new: Vec<u8> = Vec::new();
    if let Some((new_id, _, _, target)) = &added {
        let existing = children.iter().find(|b| b.is(b"iref"));
        let version = match existing {
            Some(r) => still[r.payload],
            None => {
                if *new_id > 65535 || *target > 65535 {
                    1
                } else {
                    0
                }
            }
        };
        if version > 1 || (version == 0 && (*new_id > 65535 || *target > 65535)) {
            return fail("HEIC 元数据引用 ID 超限");
        }
        let width = if version == 0 { 2 } else { 4 };
        let mut cdsc_body = be(*new_id as u64, width);
        cdsc_body.extend([0u8, 1]);
        cdsc_body.extend(be(*target as u64, width));
        let cdsc = bx(b"cdsc", &cdsc_body);
        let mut p = match existing {
            Some(r) => still[r.payload..r.end].to_vec(),
            None => vec![version, 0, 0, 0],
        };
        p.extend(cdsc);
        iref_new = bx(b"iref", &p);
    }
    let mut meta_children_out: Vec<u8> = Vec::new();
    let mut iref_appended = false;
    for child in children {
        if child.is(b"iloc") {
            meta_children_out.extend(&iloc);
        } else if child.is(b"iinf") {
            meta_children_out.extend(&iinf);
        } else if child.is(b"iref") && !iref_new.is_empty() {
            meta_children_out.extend(&iref_new);
            iref_appended = true;
        } else if !child.is(b"idat") {
            meta_children_out.extend(&still[child.start..child.end]);
        }
    }
    if !iref_new.is_empty() && !iref_appended {
        meta_children_out.extend(&iref_new);
    }
    // `full` already adds the version/flags word: the meta payload is
    // exactly the children, no extra zero word.
    let mut out = prefix;
    out.extend(bx(b"mdat", &payload));
    out.extend(full(b"meta", 0, 0, &meta_children_out));
    if heif::coding_hash(&out)? != heif::coding_hash(still)? {
        return fail("HEIC 图像、增益图或方向/色彩属性发生变化");
    }
    Ok(out)
}

/// Rewrite a HEIC still's primary XMP mime item, mirroring Swift
/// `HEIFMetadata.write`. When the still has no primary-associated mime
/// item, a new one is created (new id, new `infe`, new `cdsc` reference).
/// Item payloads are repacked into one `mdat`; `iloc` (v2, 8-byte
/// offsets/lengths), `iinf` and `iref` are rebuilt. The coding hash must
/// be unchanged, like Swift's guard.
pub fn write_xmp(still: &[u8], xmp: &[u8]) -> Result<Vec<u8>> {
    let layout = heif::layout(still)?;
    let top_ok = layout.boxes.iter().all(|b| {
        b.is(b"ftyp") || b.is(b"meta") || b.is(b"mdat") || b.is(b"free")
    });
    if !top_ok {
        return fail("HEIC 含未知顶层结构，未冒险修改编码或私有引用");
    }
    let children = &layout.children;
    let primary = primary_id(still, children)?;
    // Mime items referenced from the primary image via iref cdsc entries.
    let mut primary_descriptions: Vec<usize> = Vec::new();
    if let Some(iref) = children.iter().find(|b| b.is(b"iref")) {
        if iref.end - iref.payload < 4 || still[iref.payload] > 1 {
            return fail("HEIC iref 版本不受支持");
        }
        let width = if still[iref.payload] == 0 { 2 } else { 4 };
        for r in mp4::boxes(still, iref.payload + 4, iref.end)? {
            if !r.is(b"cdsc") {
                continue;
            }
            if r.end - r.payload < width + 2 {
                return fail("HEIC 元数据引用被截断");
            }
            let from = crate::binary::be(still, r.payload, width)?;
            let count = crate::binary::be(still, r.payload + width, 2)?;
            if r.end - r.payload != width + 2 + count * width {
                return fail("HEIC 元数据引用长度无效");
            }
            for i in 0..count {
                let to = crate::binary::be(still, r.payload + width + 2 + i * width, width)?;
                if to == primary {
                    primary_descriptions.push(from);
                }
            }
        }
    }
    let mime: Vec<&heif::Item> = layout.items.iter().filter(|i| i.ty == "mime").collect();
    let associated: Vec<&&heif::Item> = mime.iter().filter(|i| primary_descriptions.contains(&i.id)).collect();
    if associated.len() > 1 {
        return fail("HEIC 主图有多份 XMP，未推测替换");
    }
    let selected = associated.first().copied().copied();
    let new_id = match selected {
        Some(item) => item.id,
        None => layout.items.iter().map(|i| i.id).max().unwrap_or(0) + 1,
    };
    if new_id > u32::MAX as usize || layout.items.len() >= 4096 {
        return fail("HEIC 项目数量超限");
    }
    match selected {
        Some(item) => repack(still, &layout, &[(item.id, xmp.to_vec())], None),
        None => {
            let mut infe_entry = vec![3u8, 0, 0, 0];
            infe_entry.extend(be(new_id as u64, 4));
            infe_entry.extend([0u8, 0]);
            infe_entry.extend(b"mime\0application/rdf+xml\0\0");
            repack(still, &layout, &[], Some((new_id, infe_entry, xmp.to_vec(), primary)))
        }
    }
}

fn strip_exif_hdr(v: &[u8]) -> Result<Vec<u8>> {
    if !v.starts_with(pyjpeg::EXIF_HDR) {
        return fail("EXIF 标识结果缺少 Exif 头");
    }
    Ok(v[pyjpeg::EXIF_HDR.len()..].to_vec())
}

/// Swift `PhotoFormat.liveStill(uuid, updateIdentifier:true)` for HEIC,
/// in the same order as the JPEG version: static-only truncation, motion
/// XMP strip, MakerNote identifier inject. Parity is defined on the
/// coding hash (unchanged: only Exif/XMP items move) plus semantic
/// fields (motion keys gone, MakerNote tag 17 reads back the uuid).
///
/// A motion HEIC input is truncated to its still part first (mpvd/sefd
/// + video dropped); a plain HEIC still is used as-is. Inputs that are
/// neither — corrupt motion files included, since their still part
/// fails `layout` while the motion box is still attached — are refused
/// rather than half-cleaned. A still without an Exif item gains a
/// minimal one (new id, new `infe`, new `cdsc` reference), mirroring
/// the JPEG version creating an EXIF segment when absent.
pub fn live_still_heic(still: &[u8], identifier: &str) -> Result<Vec<u8>> {
    if still.len() < 12 || &still[4..8] != b"ftyp" {
        return fail("不是 HEIC 文件");
    }
    let brand = &still[8..12];
    if ![b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1", b"msf1"]
        .iter()
        .any(|b| brand == *b)
    {
        return fail("不是 HEIC 文件");
    }
    let base: Vec<u8> = match format::parse(still) {
        Ok(back) => back.still,
        Err(_) => {
            heif::layout(still)?;
            still.to_vec()
        }
    };
    let layout = heif::layout(&base)?;
    let top_ok = layout.boxes.iter().all(|b| {
        b.is(b"ftyp") || b.is(b"meta") || b.is(b"mdat") || b.is(b"free")
    });
    if !top_ok {
        return fail("HEIC 含未知顶层结构，未冒险修改编码或私有引用");
    }
    let primary = primary_id(&base, &layout.children)?;
    // Motion XMP strip: same key sets as `pymotion::live_still_clean`.
    let motion_names = [
        "MotionPhoto",
        "MotionPhotoVersion",
        "MotionPhotoPresentationTimestampUs",
        "MicroVideo",
        "MicroVideoVersion",
        "MicroVideoOffset",
        "MicroVideoPresentationTimestampUs",
        "MediaDataOffset",
    ];
    let vendor_names = [
        "MotionPhotoPrimaryPresentationTimestampUs",
        "MotionPhotoOwner",
        "OLivePhotoVersion",
        "VideoLength",
        "MediaDataOffset",
        "VMotionPhotoVersion",
        "VMotionPhotoSource",
        "VMediaKitVersion",
    ];
    let mut motion: HashSet<(String, String)> = HashSet::new();
    for n in motion_names {
        motion.insert((CAMERA.to_string(), n.to_string()));
    }
    for ns in ["http://ns.oplus.com/photos/1.0/camera/", "http://ns.vivo.com/photos/1.0/camera/"] {
        for n in vendor_names {
            motion.insert((ns.to_string(), n.to_string()));
        }
    }
    let mut replacements: Vec<(usize, Vec<u8>)> = Vec::new();
    for item in layout.items.iter().filter(|i| i.ty == "mime") {
        let raw = heif::item_bytes(&base, item);
        let mut end = raw.len();
        while end > 0 && raw[end - 1] == 0 {
            end -= 1;
        }
        let mut root = pyxml::parse(&raw[..end])?;
        let mut touched = false;
        visit_mut(&mut root, &mut |el| {
            let before = el.attrib.len();
            el.attrib.retain(|(n, l, _)| !motion.contains(&(n.clone(), l.clone())));
            if el.attrib.len() != before {
                touched = true;
            }
        });
        let removed = pyxml::remove_children_where(&mut root, &|child| {
            motion.contains(&(child.ns.clone(), child.local.clone()))
        });
        if removed > 0 {
            touched = true;
        }
        let removed = pyxml::remove_children_where(&mut root, &|child| {
            child.ns == RDF
                && child.local == "li"
                && pyxml::values(child, ITEM, "Semantic").iter().any(|v| v == "MotionPhoto")
        });
        if removed > 0 {
            touched = true;
        }
        if touched {
            replacements.push((item.id, pyxml::tostring(&root)));
        }
    }
    // MakerNote identifier inject (same TIFF surgery as the JPEG version).
    let exifs: Vec<&heif::Item> = layout.items.iter().filter(|i| i.ty == "Exif").collect();
    let mut added = None;
    match exifs.len() {
        0 => {
            let new_tiff = strip_exif_hdr(&pyjpeg::exif_identifier(None, identifier)?)?;
            let new_id = layout.items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            if new_id > u32::MAX as usize || layout.items.len() >= 4096 {
                return fail("HEIC 项目数量超限");
            }
            let mut infe_entry = vec![3u8, 0, 0, 0];
            infe_entry.extend(be(new_id as u64, 4));
            infe_entry.extend([0u8, 0]);
            infe_entry.extend(b"Exif\0");
            let mut bytes = vec![0u8, 0, 0, 0];
            bytes.extend(new_tiff);
            added = Some((new_id, infe_entry, bytes, primary));
        }
        1 => {
            let raw = heif::item_bytes(&base, exifs[0]);
            if raw.len() < 4 {
                return fail("HEIC EXIF 被截断");
            }
            let off = crate::binary::be(&raw, 0, 4)?;
            if off > raw.len() - 4 {
                return fail("HEIC EXIF TIFF 偏移越界");
            }
            let old_tiff = &raw[4 + off..];
            let mut payload = pyjpeg::EXIF_HDR.to_vec();
            payload.extend_from_slice(old_tiff);
            let new_tiff = strip_exif_hdr(&pyjpeg::exif_identifier(Some(&payload), identifier)?)?;
            let mut bytes = vec![0u8, 0, 0, 0];
            bytes.extend(new_tiff);
            replacements.push((exifs[0].id, bytes));
        }
        _ => return fail("HEIC 有多份 EXIF 项目，未推测替换"),
    }
    repack(&base, &layout, &replacements, added)
}

fn samsung_sef(video_start: usize, length: usize) -> Result<Vec<u8>> {
    if video_start > u32::MAX as usize || length > u32::MAX as usize {
        return fail("三星 SEF 4 GiB 范围超限");
    }
    let tag: [u8; 4] = [0, 0, 0x30, 0x0a];
    let mut record = tag.to_vec();
    record.extend_from_slice(&16u32.to_le_bytes());
    record.extend_from_slice(b"MotionPhoto_Data");
    record.extend_from_slice(b"mpv2");
    record.extend_from_slice(&(video_start as u32).to_be_bytes());
    record.extend_from_slice(&(length as u32).to_be_bytes());
    let mut directory = b"SEFH".to_vec();
    directory.extend_from_slice(&107u32.to_le_bytes());
    directory.extend_from_slice(&1u32.to_le_bytes());
    directory.extend_from_slice(&tag);
    directory.extend_from_slice(&(record.len() as u32).to_le_bytes());
    directory.extend_from_slice(&(record.len() as u32).to_le_bytes());
    let mut out = record;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(b"SEFT");
    Ok(out)
}

/// Assemble a HEIC Motion Photo, mirroring Swift
/// `AndroidExport.assemble` (HEIC path): rewrite the still's XMP, then
/// append `mpvd{video + tail}`. Only Google and Samsung are supported;
/// the video start pointer is `image.len() + 8` (after the mpvd header).
/// The output is validated by parsing it back: video bytes, still
/// coding hash and cover time must all match.
pub fn assemble_heic(still: &[u8], video: &[u8], vendor: Vendor, time: i64) -> Result<Vec<u8>> {
    if vendor != Vendor::Google && vendor != Vendor::Samsung {
        return fail("此厂商的 HEIC 动态封装尚无可验证方案；请选择 Google 或三星");
    }
    if time < 0 {
        return fail("HEIC 封面时间无效");
    }
    let doc = motion_doc_heic(time, video.len());
    let image = write_xmp(still, &pyxml::tostring(&doc))?;
    let image_hash = heif::coding_hash(&image)?;
    let start = image.len() + 8;
    let tail = match vendor {
        Vendor::Samsung => samsung_sef(start, video.len())?,
        _ => Vec::new(),
    };
    let mut mpvd_body = video.to_vec();
    mpvd_body.extend_from_slice(&tail);
    let mut out = image;
    out.extend(bx(b"mpvd", &mpvd_body));
    // Parse-back validation, like Swift's convert guards.
    let back = format::parse(&out)?;
    if back.video(&out) != video {
        return fail("HEIC 封装读回校验失败：视频数据不同");
    }
    if heif::coding_hash(&back.still)? != image_hash {
        return fail("HEIC 封装读回校验失败：图像编码数据不同");
    }
    if back.timestamp != Some(time) {
        return fail("HEIC 封装读回校验失败：封面时间不同");
    }
    Ok(out)
}
