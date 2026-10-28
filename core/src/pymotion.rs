//! Port of `motionlive/photo.py` and `motionlive/hdr.py`: Google XMP parsing,
//! lossless Live Photo still metadata, legacy Apple HDR handling.
//!
//! Error strings match the Python implementation, since this module backs the
//! Python CLI replacement (not the Swift-parity `format` module).

use crate::pyjpeg::{self, Tiff};
use crate::pyxml::{self, Element};
use crate::{fail, hex, Result};
use md5::{Digest, Md5};
use sha2::Sha256;
use std::collections::{HashMap, HashSet};

pub const XMP_HDR: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
pub const EXTENDED_HDR: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
pub const NOTE: &str = "http://ns.adobe.com/xmp/note/";
pub const CAMERA: &str = "http://ns.google.com/photos/1.0/camera/";
pub const CONTAINER: &str = "http://ns.google.com/photos/1.0/container/";
pub const ITEM: &str = "http://ns.google.com/photos/1.0/container/item/";
pub const HDR: &str = "http://ns.adobe.com/hdr-gain-map/1.0/";
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const XMPMETA: &str = "adobe:ns:meta/";
pub const APPLE: &str = "http://ns.apple.com/HDRGainMap/1.0/";
pub const PIXEL: &str = "http://ns.apple.com/pixeldatainfo/1.0/";

pub fn md5_upper(b: &[u8]) -> String {
    hex(&Md5::digest(b)).to_uppercase()
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest as _;
    hex(&Sha256::digest(b))
}

/// Python int() on an XMP value: surrounding whitespace, sign, ASCII digits,
/// single underscores between digits (as int() allows). Returns None when the
/// value is not an integer at all.
fn py_int(s: &str) -> Option<i64> {
    let t = s.trim();
    let (sign, digits) = if let Some(rest) = t.strip_prefix('-') {
        (-1i64, rest)
    } else if let Some(rest) = t.strip_prefix('+') {
        (1i64, rest)
    } else {
        (1i64, t)
    };
    if digits.is_empty() { return None; }
    let mut prev_underscore = false;
    for (i, c) in digits.chars().enumerate() {
        if c == '_' {
            if i == 0 || prev_underscore { return None; }
            prev_underscore = true;
        } else if c.is_ascii_digit() {
            prev_underscore = false;
        } else {
            return None;
        }
    }
    if prev_underscore { return None; }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
    clean.parse::<i64>().ok().map(|v| sign * v)
}

/// Python str(float): integral floats print with ".0" (Rust prints "2").
fn py_float_str(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

pub(crate) fn visit_mut(e: &mut Element, f: &mut dyn FnMut(&mut Element)) {
    f(e);
    for c in &mut e.children {
        visit_mut(c, f);
    }
}

// ---------------------------------------------------------------------------
// XMP packets
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Packet {
    pub seg_start: usize,
    pub guid: Option<String>,
    pub fragments: Vec<usize>,
    pub root: Element,
}

fn document(xml: &[u8]) -> Result<Element> {
    let mut end = xml.len();
    while end > 0 && xml[end - 1] == 0 {
        end -= 1;
    }
    pyxml::parse(&xml[..end])
}

pub fn xmp_packets(data: &[u8]) -> Result<Vec<Packet>> {
    let mut result = Vec::new();
    let mut groups: std::collections::BTreeMap<String, Vec<(usize, usize, usize, Vec<u8>)>> =
        std::collections::BTreeMap::new();
    for s in pyjpeg::segments(data)? {
        if s.marker != 0xE1 {
            continue;
        }
        let pl = s.payload(data);
        if pl.starts_with(EXTENDED_HDR) {
            let p = EXTENDED_HDR.len();
            if pl.len() < p + 40 {
                return fail("Truncated Extended XMP header");
            }
            let guid_bytes = &pl[p..p + 32];
            if !guid_bytes.iter().all(|b| b.is_ascii()) {
                return fail("Invalid Extended XMP GUID");
            }
            let guid = String::from_utf8_lossy(guid_bytes).to_uppercase();
            if guid.len() != 32 || !guid.chars().all(|c| c.is_ascii_hexdigit()) {
                return fail("Invalid Extended XMP GUID");
            }
            let size = u32::from_be_bytes([pl[p + 32], pl[p + 33], pl[p + 34], pl[p + 35]]) as usize;
            let offset = u32::from_be_bytes([pl[p + 36], pl[p + 37], pl[p + 38], pl[p + 39]]) as usize;
            let chunk = pl[p + 40..].to_vec();
            if size == 0 || size > data.len() || offset > size || chunk.len() > size - offset {
                return fail("Invalid Extended XMP chunk bounds");
            }
            groups.entry(guid).or_default().push((s.start, size, offset, chunk));
        } else if pl.starts_with(XMP_HDR) {
            result.push(Packet {
                seg_start: s.start,
                guid: None,
                fragments: Vec::new(),
                root: document(&pl[XMP_HDR.len()..])?,
            });
        }
    }
    for (guid, chunks) in groups.iter() {
        let mut chunks = chunks.clone();
        chunks.sort_by_key(|c| c.2);
        let size = chunks[0].1;
        let mut xml: Vec<u8> = Vec::new();
        for (_, total, offset, chunk) in &chunks {
            if *total != size || *offset != xml.len() {
                return fail("Missing, overlapping or inconsistent Extended XMP chunks");
            }
            xml.extend_from_slice(chunk);
        }
        if xml.len() != size || md5_upper(&xml) != *guid {
            return fail("Extended XMP length/checksum mismatch");
        }
        let mut fragments: Vec<usize> = chunks.iter().map(|c| c.0).collect();
        fragments.sort_unstable();
        let seg_start = fragments[0];
        result.push(Packet {
            seg_start,
            guid: Some(guid.clone()),
            fragments,
            root: document(&xml)?,
        });
    }
    for p in result.iter().filter(|p| p.guid.is_none()) {
        for g in pyxml::values(&p.root, NOTE, "HasExtendedXMP") {
            if !groups.contains_key(&g.to_uppercase()) {
                return fail("Missing referenced Extended XMP");
            }
        }
    }
    Ok(result)
}

pub fn integer_unique(packets: &[Packet], name: &str) -> Result<Option<i64>> {
    let mut found: Vec<String> = Vec::new();
    for p in packets {
        found.extend(pyxml::values(&p.root, CAMERA, name));
    }
    let mut parsed: Vec<i64> = Vec::new();
    for v in &found {
        match py_int(v) {
            Some(n) => parsed.push(n),
            None => return fail(format!("Invalid {name}")),
        }
    }
    if parsed.iter().collect::<HashSet<_>>().len() > 1 {
        return fail(format!("Conflicting {name}"));
    }
    Ok(parsed.first().copied())
}

pub(crate) fn xmp_rewrites(packets: &mut [Packet], changed: &mut HashSet<usize>) -> Result<HashMap<usize, Vec<u8>>> {
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut ids: Vec<(String, String)> = Vec::new();
    let mut ordered: Vec<usize> = changed.iter().copied().collect();
    ordered.sort_unstable();
    for i in ordered {
        let packet = &packets[i];
        let guid = match &packet.guid {
            Some(g) => g.clone(),
            None => continue,
        };
        let xml = pyxml::tostring(&packet.root);
        let new_guid = md5_upper(&xml);
        ids.push((guid, new_guid.clone()));
        let maximum = 65533 - EXTENDED_HDR.len() - 40;
        let mut encoded = Vec::new();
        let mut offset = 0usize;
        while offset < xml.len() {
            let end = (offset + maximum).min(xml.len());
            let mut seg = EXTENDED_HDR.to_vec();
            seg.extend_from_slice(new_guid.as_bytes());
            seg.extend_from_slice(&(xml.len() as u32).to_be_bytes());
            seg.extend_from_slice(&(offset as u32).to_be_bytes());
            seg.extend_from_slice(&xml[offset..end]);
            encoded.extend_from_slice(&pyjpeg::app(0xE1, &seg)?);
            offset = end;
        }
        for f in &packet.fragments {
            replacements.insert(*f, Vec::new());
        }
        replacements.insert(packet.seg_start, encoded);
    }
    for (old, new) in ids {
        let mut linked = false;
        for (i, packet) in packets.iter_mut().enumerate() {
            if packet.guid.is_some() {
                continue;
            }
            let mut hit = false;
            visit_mut(&mut packet.root, &mut |el| {
                if let Some(v) = el.get_attr(NOTE, "HasExtendedXMP") {
                    if v.to_uppercase() == old {
                        el.set_attr(NOTE, "HasExtendedXMP", &new);
                        hit = true;
                    }
                }
                if el.ns == NOTE && el.local == "HasExtendedXMP" {
                    if el.text.as_deref().unwrap_or("").to_uppercase() == old {
                        el.text = Some(new.clone());
                        hit = true;
                    }
                }
            });
            if hit {
                changed.insert(i);
                linked = true;
            }
        }
        if !linked {
            return fail("Modified Extended XMP has no main-packet reference");
        }
    }
    let changed_now: Vec<usize> = changed.iter().copied().collect();
    for i in changed_now {
        if packets[i].guid.is_none() {
            let mut payload = XMP_HDR.to_vec();
            payload.extend_from_slice(&pyxml::tostring(&packets[i].root));
            replacements.insert(packets[i].seg_start, pyjpeg::app(0xE1, &payload)?);
        }
    }
    Ok(replacements)
}

// ---------------------------------------------------------------------------
// Google Motion Photo parse / still rewriting
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct MotionPhoto {
    pub still: Vec<u8>,
    pub video: Vec<u8>,
    pub timestamp_us: Option<i64>,
    pub timestamp_tag: Option<String>,
    pub hdr: String,
    pub warnings: Vec<String>,
}

pub fn parse(data: &[u8]) -> Result<MotionPhoto> {
    let packets = xmp_packets(data)?;
    let motion = integer_unique(&packets, "MotionPhoto")?;
    let micro = integer_unique(&packets, "MicroVideo")?;
    if motion == Some(0) || (motion != Some(1) && micro != Some(1)) {
        return fail("Not a marked Google Motion Photo");
    }
    let mut lengths: Vec<i64> = Vec::new();
    let mut gainmap = false;
    for p in &packets {
        let mut elems = Vec::new();
        p.root.preorder(&mut elems);
        for el in elems {
            // Support both attribute and element XMP forms of Container:Item.
            let mut semantic = el.get_attr(ITEM, "Semantic").map(|s| s.to_string());
            let mut length = el.get_attr(ITEM, "Length").map(|s| s.to_string());
            if el.ns == CONTAINER && el.local == "Item" && semantic.is_none() {
                semantic = el.find_child_text(ITEM, "Semantic").map(|s| s.to_string());
                length = el.find_child_text(ITEM, "Length").map(|s| s.to_string());
            }
            if semantic.as_deref() == Some("MotionPhoto") {
                match length.as_deref().and_then(py_int) {
                    Some(n) => lengths.push(n),
                    None => return fail("Missing/invalid MotionPhoto item length"),
                }
            }
            if semantic.as_deref() == Some("GainMap") {
                gainmap = true;
            }
        }
    }
    if lengths.iter().collect::<HashSet<_>>().len() > 1 {
        return fail("Conflicting MotionPhoto lengths");
    }
    let length = match lengths.first() {
        Some(l) => Some(*l),
        None => integer_unique(&packets, "MicroVideoOffset")?,
    };
    let length = match length {
        Some(l) if l >= 16 && (l as u128) < data.len() as u128 => l as usize,
        _ => return fail("Cannot locate embedded video from XMP"),
    };
    let start = data.len() - length;
    let still = data[..start].to_vec();
    let video = data[start..].to_vec();
    let segs = pyjpeg::segments(data)?;
    if start < segs.last().map(|s| s.end).unwrap_or(0) {
        return fail("Video overlaps JPEG");
    }
    if video[4..8] != *b"ftyp" {
        return fail("XMP offset does not point to an MP4 ftyp box");
    }
    // Strict box walk; never search video bytes for JPEG EOI or ftyp signatures.
    let mut pos = 0usize;
    let mut boxes: Vec<[u8; 4]> = Vec::new();
    while pos < video.len() {
        if pos + 8 > video.len() {
            return fail("Truncated MP4 header");
        }
        let mut size = u32::from_be_bytes([video[pos], video[pos + 1], video[pos + 2], video[pos + 3]]) as usize;
        let mut header = 8usize;
        if size == 1 {
            if pos + 16 > video.len() {
                return fail("Truncated MP4 large size");
            }
            size = u64::from_be_bytes(video[pos + 8..pos + 16].try_into().unwrap()) as usize;
            header = 16;
        } else if size == 0 {
            size = video.len() - pos;
        }
        if size < header || pos + size > video.len() {
            return fail("Invalid MP4 box size");
        }
        boxes.push(video[pos + 4..pos + 8].try_into().unwrap());
        pos += size;
    }
    if !boxes.contains(b"moov") || !boxes.contains(b"mdat") {
        return fail("MP4 missing moov/mdat");
    }
    let mut timestamp = integer_unique(&packets, "MotionPhotoPresentationTimestampUs")?;
    let mut tag = "MotionPhotoPresentationTimestampUs";
    if timestamp.map_or(true, |t| t < 0) {
        let legacy = integer_unique(&packets, "MicroVideoPresentationTimestampUs")?;
        if legacy.map_or(false, |t| t >= 0) {
            timestamp = legacy;
            tag = "MicroVideoPresentationTimestampUs";
        } else {
            timestamp = None;
            tag = "";
        }
    }
    // Version is an unambiguous Ultra HDR signal; auxiliary payload is verified.
    let ultra = packets.iter().any(|p| !pyxml::values(&p.root, HDR, "Version").is_empty());
    let aux = pyjpeg::mpf_images(&still)?;
    let mut warnings = Vec::new();
    let hdr;
    if ultra {
        if aux.len() < 2 {
            return fail("Ultra HDR declaration without an MPF auxiliary image");
        }
        for r in aux.iter().skip(1) {
            pyjpeg::segments(&still[r.clone()])?;
        }
        hdr = "ultra-hdr".to_string();
    } else {
        hdr = "legacy-or-sdr".to_string();
        if gainmap {
            warnings.push("GainMap container item without Ultra HDR Version; inspect legacy HDR".to_string());
        }
    }
    Ok(MotionPhoto {
        still,
        video,
        timestamp_us: timestamp,
        timestamp_tag: timestamp.map(|_| tag.to_string()),
        hdr,
        warnings,
    })
}

pub fn live_still(still: &[u8], identifier: &str) -> Result<Vec<u8>> {
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut insert: Vec<u8> = Vec::new();
    let mut has_exif = false;
    for s in pyjpeg::segments(still)? {
        if s.marker == 0xE1 && s.payload(still).starts_with(pyjpeg::EXIF_HDR) {
            if has_exif {
                return fail("Multiple EXIF segments");
            }
            replacements.insert(s.start, pyjpeg::app(0xE1, &pyjpeg::exif_identifier(Some(s.payload(still)), identifier)?)?);
            has_exif = true;
        }
    }
    if !has_exif {
        insert = pyjpeg::app(0xE1, &pyjpeg::exif_identifier(None, identifier)?)?;
    }
    let motion_keys: HashSet<(String, String)> = [
        "MotionPhoto", "MotionPhotoVersion", "MotionPhotoPresentationTimestampUs",
        "MicroVideo", "MicroVideoVersion", "MicroVideoOffset", "MicroVideoPresentationTimestampUs",
    ]
    .iter()
    .map(|n| (CAMERA.to_string(), n.to_string()))
    .collect();
    let mut packets = xmp_packets(still)?;
    let mut changed: HashSet<usize> = HashSet::new();
    for (i, packet) in packets.iter_mut().enumerate() {
        let mut touched = false;
        visit_mut(&mut packet.root, &mut |el| {
            let before = el.attrib.len();
            el.attrib.retain(|(n, l, _)| !motion_keys.contains(&(n.clone(), l.clone())));
            if el.attrib.len() != before {
                touched = true;
            }
        });
        let removed = pyxml::remove_children_where(&mut packet.root, &|child| {
            motion_keys.contains(&(child.ns.clone(), child.local.clone()))
        });
        if removed > 0 {
            touched = true;
        }
        if touched {
            changed.insert(i);
        }
        // Remove only MotionPhoto items; keep GainMap and its HDR metadata.
        let removed = pyxml::remove_children_where(&mut packet.root, &|child| {
            if !(child.ns == RDF && child.local == "li") {
                return false;
            }
            let mut subtree = Vec::new();
            child.preorder(&mut subtree);
            subtree.iter().any(|e| {
                e.get_attr(ITEM, "Semantic") == Some("MotionPhoto")
                    || (e.ns == ITEM && e.local == "Semantic" && e.text.as_deref() == Some("MotionPhoto"))
            })
        });
        if removed > 0 {
            changed.insert(i);
        }
    }
    replacements.extend(xmp_rewrites(&mut packets, &mut changed)?);
    let output = pyjpeg::edit_header(still, &replacements, &insert)?;
    let before = pyjpeg::mpf_images(still)?;
    let after = pyjpeg::mpf_images(&output)?;
    let before_hashes: Vec<String> = before.get(1..).unwrap_or(&[]).iter().map(|r| sha256_hex(&still[r.clone()])).collect();
    let after_hashes: Vec<String> = after.get(1..).unwrap_or(&[]).iter().map(|r| sha256_hex(&output[r.clone()])).collect();
    if before_hashes != after_hashes {
        return fail("Auxiliary image bytes changed");
    }
    Ok(output)
}

/// Swift `PhotoFormat.liveStill(uuid:"", updateIdentifier:false)` for JPEG:
/// keep the still, strip only motion metadata. Steps, in Swift order:
/// 1. `staticOnly`: keep only the image ranges (primary JPEG through EOI,
///    plus MPF auxiliary images when present) and drop trailing data
///    (appended video tails, stale vendor markers). Non-contiguous image
///    ranges are refused rather than rebuilt. The Honor private EHDR
///    extension is not handled here: it is metadata Swift re-appends, and
///    no verified Rust EHDR writer exists yet.
/// 2. Strip motion XMP: GCamera keys (MotionPhoto, MotionPhotoVersion,
///    MotionPhotoPresentationTimestampUs, MicroVideo, MicroVideoVersion,
///    MicroVideoOffset, MicroVideoPresentationTimestampUs, MediaDataOffset)
///    and OPPO/vivo keys (MotionPhotoPrimaryPresentationTimestampUs,
///    MotionPhotoOwner, OLivePhotoVersion, VideoLength, MediaDataOffset,
///    VMotionPhotoVersion, VMotionPhotoSource, VMediaKitVersion), as
///    attributes or elements, plus any rdf:li whose Item:Semantic values
///    contain "MotionPhoto".
/// 3. EXIF is left untouched (updateIdentifier is false: no identifier is
///    injected or replaced), and auxiliary (gain-map) bytes must hash-match
///    before/after, like Swift.
/// HEIC stills are refused: HEIC output needs the HEIC writers (ROADMAP
/// stretch item), and a HEIC still cannot be cleaned into a JPEG without
/// re-encoding the still itself.
pub fn live_still_clean(still: &[u8]) -> Result<Vec<u8>> {
    if still.len() >= 12 && &still[4..8] == b"ftyp" {
        let brand = &still[8..12];
        if [b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1", b"msf1"]
            .iter()
            .any(|b| brand == *b)
        {
            return fail("HEIC 静态图暂不支持 Live→Motion 清理，需 HEIC 写入支持");
        }
    }
    let ranges = pyjpeg::mpf_images(still)?;
    let end = if ranges.is_empty() {
        let segs = pyjpeg::segments(still)?;
        match segs.last() {
            Some(s) => s.end,
            None => return fail("Not a JPEG"),
        }
    } else {
        if ranges[0].start != 0 {
            return fail("图片范围不连续，拒绝清理");
        }
        for w in ranges.windows(2) {
            if w[1].start != w[0].end {
                return fail("图片范围不连续，拒绝清理");
            }
        }
        ranges.last().map(|r| r.end).unwrap_or(0)
    };
    let truncated = &still[..end];
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
    let mut packets = xmp_packets(truncated)?;
    let mut changed: HashSet<usize> = HashSet::new();
    for (i, packet) in packets.iter_mut().enumerate() {
        let mut touched = false;
        visit_mut(&mut packet.root, &mut |el| {
            let before = el.attrib.len();
            el.attrib.retain(|(n, l, _)| !motion.contains(&(n.clone(), l.clone())));
            if el.attrib.len() != before {
                touched = true;
            }
        });
        let removed = pyxml::remove_children_where(&mut packet.root, &|child| {
            motion.contains(&(child.ns.clone(), child.local.clone()))
        });
        if removed > 0 {
            touched = true;
        }
        let removed = pyxml::remove_children_where(&mut packet.root, &|child| {
            child.ns == RDF
                && child.local == "li"
                && pyxml::values(child, ITEM, "Semantic").iter().any(|v| v == "MotionPhoto")
        });
        if removed > 0 {
            touched = true;
        }
        if touched {
            changed.insert(i);
        }
    }
    let replacements = xmp_rewrites(&mut packets, &mut changed)?;
    let output = pyjpeg::edit_header(truncated, &replacements, &[])?;
    let before = pyjpeg::mpf_images(truncated)?;
    let after = pyjpeg::mpf_images(&output)?;
    let before_hashes: Vec<String> = before.get(1..).unwrap_or(&[]).iter().map(|r| sha256_hex(&truncated[r.clone()])).collect();
    let after_hashes: Vec<String> = after.get(1..).unwrap_or(&[]).iter().map(|r| sha256_hex(&output[r.clone()])).collect();
    if before_hashes != after_hashes {
        return fail("Auxiliary image bytes changed");
    }
    Ok(output)
}

pub fn ultra_from_apple(still: &[u8], gain_jpeg: &[u8], stops: f64) -> Result<Vec<u8>> {
    // Repackage a converted legacy gain map; primary JPEG coding is unchanged.
    let mut gain_root = Element::new(XMPMETA, "xmpmeta");
    let mut rdf = Element::new(RDF, "RDF");
    let mut desc = Element::new(RDF, "Description");
    desc.set_attr(HDR, "Version", "1.0");
    desc.set_attr(HDR, "GainMapMin", "0");
    desc.set_attr(HDR, "GainMapMax", &py_float_str(stops));
    desc.set_attr(HDR, "Gamma", "1");
    desc.set_attr(HDR, "OffsetSDR", "0");
    desc.set_attr(HDR, "OffsetHDR", "0");
    desc.set_attr(HDR, "HDRCapacityMin", "0");
    desc.set_attr(HDR, "HDRCapacityMax", &py_float_str(stops));
    desc.set_attr(HDR, "BaseRenditionIsHDR", "False");
    rdf.children.push(desc);
    gain_root.children.push(rdf);
    let mut gain = gain_jpeg.get(..2).unwrap_or(gain_jpeg).to_vec();
    let mut gain_payload = XMP_HDR.to_vec();
    gain_payload.extend_from_slice(&pyxml::tostring(&gain_root));
    gain.extend_from_slice(&pyjpeg::app(0xE1, &gain_payload)?);
    gain.extend_from_slice(gain_jpeg.get(2..).unwrap_or(&[]));

    let primary_end = pyjpeg::segments(still)?.last().map(|s| s.end).unwrap_or(0);
    let mut primary = still[..primary_end].to_vec();
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    for s in pyjpeg::segments(&primary)? {
        if s.marker == 0xE2 && s.payload(&primary).starts_with(pyjpeg::MPF_HDR) {
            replacements.insert(s.start, Vec::new());
        }
    }
    let mut root = Element::new(XMPMETA, "xmpmeta");
    let mut rdf2 = Element::new(RDF, "RDF");
    let mut desc2 = Element::new(RDF, "Description");
    desc2.set_attr(HDR, "Version", "1.0");
    let mut dir = Element::new(CONTAINER, "Directory");
    let mut seq = Element::new(RDF, "Seq");
    for (semantic, length) in [("Primary", 0usize), ("GainMap", gain.len())] {
        let mut li = Element::new(RDF, "li");
        li.set_attr(RDF, "parseType", "Resource");
        let mut item = Element::new(CONTAINER, "Item");
        item.set_attr(ITEM, "Semantic", semantic);
        item.set_attr(ITEM, "Mime", "image/jpeg");
        item.set_attr(ITEM, "Length", &length.to_string());
        li.children.push(item);
        seq.children.push(li);
    }
    dir.children.push(seq);
    desc2.children.push(dir);
    rdf2.children.push(desc2.clone());
    root.children.push(rdf2);
    // Merge with original XMP without duplicating Container:Directory.
    let packets = xmp_packets(&primary)?;
    for packet in &packets {
        let mut old_root = packet.root.clone();
        pyxml::remove_children_where(&mut old_root, &|c| c.ns == CONTAINER && c.local == "Directory");
        let old_rdf = match old_root.find_child_mut(RDF, "RDF") {
            Some(r) => r,
            None => return fail("XMP RDF missing"),
        };
        old_rdf.children.push(desc2.clone());
        let mut payload = XMP_HDR.to_vec();
        payload.extend_from_slice(&pyxml::tostring(&old_root));
        replacements.insert(packet.seg_start, pyjpeg::app(0xE1, &payload)?);
        break;
    }
    let addition = if packets.is_empty() {
        let mut payload = XMP_HDR.to_vec();
        payload.extend_from_slice(&pyxml::tostring(&root));
        pyjpeg::app(0xE1, &payload)?
    } else {
        Vec::new()
    };
    // Existing MPF is deliberately replaced with a fresh two-image index.
    primary = pyjpeg::edit_header(&primary, &replacements, &addition)?;
    // MPF TIFF: version, image count, 32-byte MPEntry array.
    let mut tiff = b"MM\0*\0\0\0\x08".to_vec();
    tiff.extend_from_slice(&3u16.to_be_bytes());
    tiff.extend_from_slice(&0xB000u16.to_be_bytes());
    tiff.extend_from_slice(&7u16.to_be_bytes());
    tiff.extend_from_slice(&4u32.to_be_bytes());
    tiff.extend_from_slice(b"0100");
    tiff.extend_from_slice(&0xB001u16.to_be_bytes());
    tiff.extend_from_slice(&4u16.to_be_bytes());
    tiff.extend_from_slice(&1u32.to_be_bytes());
    tiff.extend_from_slice(&2u32.to_be_bytes());
    tiff.extend_from_slice(&0xB002u16.to_be_bytes());
    tiff.extend_from_slice(&7u16.to_be_bytes());
    tiff.extend_from_slice(&32u32.to_be_bytes());
    tiff.extend_from_slice(&50u32.to_be_bytes());
    tiff.extend_from_slice(&[0u8; 4]);
    let mpf_len = 4 + 4 + tiff.len() + 32;
    let final_size = primary.len() + mpf_len;
    // New MPF is immediately after SOI; TIFF origin is at absolute offset 10.
    for (attr, size, off) in [(0x20030000u32, final_size as u32, 0u32), (0u32, gain.len() as u32, (final_size - 10) as u32)] {
        tiff.extend_from_slice(&attr.to_be_bytes());
        tiff.extend_from_slice(&size.to_be_bytes());
        tiff.extend_from_slice(&off.to_be_bytes());
        tiff.extend_from_slice(&0u16.to_be_bytes());
        tiff.extend_from_slice(&0u16.to_be_bytes());
    }
    let mut mpf_payload = pyjpeg::MPF_HDR.to_vec();
    mpf_payload.extend_from_slice(&tiff);
    let mut output = primary.get(..2).unwrap_or(&primary).to_vec();
    output.extend_from_slice(&pyjpeg::app(0xE2, &mpf_payload)?);
    output.extend_from_slice(primary.get(2..).unwrap_or(&[]));
    output.extend_from_slice(&gain);
    if pyjpeg::image_hash(still)? != pyjpeg::image_hash(&output)? {
        return fail("Primary JPEG changed during legacy HDR conversion");
    }
    let out_images = pyjpeg::mpf_images(&output)?;
    match out_images.get(1) {
        Some(r) if &output[r.clone()] == gain.as_slice() => {}
        _ => return fail("Generated Ultra HDR MPF index is invalid"),
    }
    Ok(output)
}

pub fn legacy_color_profile(still: &[u8], srgb_icc: &[u8]) -> Result<Vec<u8>> {
    // Ultra HDR requires an ICC profile. Only infer sRGB from explicit EXIF sRGB.
    let segs = pyjpeg::segments(still)?;
    if segs.iter().any(|s| s.marker == 0xE2 && s.payload(still).starts_with(b"ICC_PROFILE\0")) {
        return Ok(still.to_vec());
    }
    for s in &segs {
        if s.marker != 0xE1 || !s.payload(still).starts_with(pyjpeg::EXIF_HDR) {
            continue;
        }
        let t = Tiff::new(&s.payload(still)[6..])?;
        let (root, _) = t.root()?;
        let ptr = Tiff::tag(&root, t.little, 0x8769);
        if let Some(p) = ptr {
            let (exif, _) = t.directory(p.value(t.little))?;
            let color = Tiff::tag(&exif, t.little, 0xA001);
            if let Some(c) = color {
                if c.tag(t.little) == 0xA001 && c.ty(t.little) == 3 && c.count(t.little) == 1 {
                    let v = if t.little {
                        (c.raw[8] as usize) | ((c.raw[9] as usize) << 8)
                    } else {
                        ((c.raw[8] as usize) << 8) | c.raw[9] as usize
                    };
                    if v == 1 {
                        let mut icc = b"ICC_PROFILE\0\x01\x01".to_vec();
                        icc.extend_from_slice(srgb_icc);
                        return pyjpeg::edit_header(still, &HashMap::new(), &pyjpeg::app(0xE2, &icc)?);
                    }
                }
            }
        }
    }
    fail("Legacy HDR has no ICC profile or explicit EXIF sRGB; color space not guessed")
}

// ---------------------------------------------------------------------------
// Legacy Apple HDR (motionlive/hdr.py)
// ---------------------------------------------------------------------------

pub struct LegacyGain {
    pub gain: Vec<u8>,
    pub stops: f64,
}

fn tiff_u16(d: &[u8], p: usize, little: bool) -> usize {
    if little { (d[p] as usize) | ((d[p + 1] as usize) << 8) } else { ((d[p] as usize) << 8) | d[p + 1] as usize }
}
fn tiff_u32(d: &[u8], p: usize, little: bool) -> usize {
    if little {
        (d[p] as usize) | ((d[p + 1] as usize) << 8) | ((d[p + 2] as usize) << 16) | ((d[p + 3] as usize) << 24)
    } else {
        ((d[p] as usize) << 24) | ((d[p + 1] as usize) << 16) | ((d[p + 2] as usize) << 8) | d[p + 3] as usize
    }
}

pub fn legacy_apple(still: &[u8]) -> Result<(Option<LegacyGain>, String)> {
    let images = pyjpeg::mpf_images(still)?;
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    for r in images.get(1..).unwrap_or(&[]) {
        let image = &still[r.clone()];
        for packet in xmp_packets(image)? {
            if pyxml::values(&packet.root, PIXEL, "AuxiliaryImageType")
                .iter()
                .any(|v| v == "urn:com:apple:photo:2020:aux:hdrgainmap")
            {
                if pyxml::values(&packet.root, APPLE, "HDRGainMapVersion") != vec!["65536".to_string()] {
                    return Ok((None, "Unsupported legacy Apple gain-map version; original HDR retained".to_string()));
                }
                candidates.push(image.to_vec());
            }
        }
    }
    if candidates.is_empty() {
        return Ok((None, "No recognized legacy Apple gain map; SDR is not expanded".to_string()));
    }
    if candidates.len() != 1 {
        return Ok((None, "Multiple legacy HDR maps; original retained".to_string()));
    }
    for segment in pyjpeg::segments(still)? {
        if segment.marker != 0xE1 || !segment.payload(still).starts_with(pyjpeg::EXIF_HDR) {
            continue;
        }
        let t = Tiff::new(&segment.payload(still)[6..])?;
        let (root, _) = t.root()?;
        let ptr = match Tiff::tag(&root, t.little, 0x8769) {
            Some(p) => p,
            None => continue,
        };
        let (exif, _) = t.directory(ptr.value(t.little))?;
        let entry = match Tiff::tag(&exif, t.little, 0x927C) {
            Some(e) => e,
            None => continue,
        };
        let ty = entry.ty(t.little);
        let count = entry.count(t.little);
        let offset = entry.value(t.little);
        if ty != 7 || offset + count > t.data.len() {
            return fail("Invalid legacy MakerNote");
        }
        let note = &t.data[offset..offset + count];
        if !note.starts_with(b"Apple iOS\0\0\x01") {
            continue;
        }
        let little = match note.get(12..14) {
            Some(b"MM") => false,
            Some(b"II") => true,
            _ => return fail("Invalid Apple MakerNote byte order"),
        };
        if note.len() < 16 {
            return fail("Invalid Apple MakerNote byte order");
        }
        let n = tiff_u16(note, 14, little);
        if 16 + n * 12 + 4 > note.len() {
            return fail("Truncated Apple MakerNote");
        }
        let mut scalars: HashMap<usize, f64> = HashMap::new();
        for i in 0..n {
            let at = 16 + i * 12;
            let tag = tiff_u16(note, at, little);
            let field_type = tiff_u16(note, at + 2, little);
            let length = tiff_u32(note, at + 4, little);
            if (tag != 33 && tag != 48) || length != 1 {
                continue;
            }
            if field_type == 11 {
                let bits = tiff_u32(note, at + 8, little);
                scalars.insert(tag, f32::from_bits(bits as u32) as f64);
            } else if field_type == 5 || field_type == 10 || field_type == 12 {
                let position = tiff_u32(note, at + 8, little);
                if position < 16 + n * 12 + 4 || position + 8 > note.len() {
                    return fail("Invalid Apple headroom scalar offset");
                }
                if field_type == 12 {
                    let bits = if little {
                        let mut b = [0u8; 8];
                        b.copy_from_slice(&note[position..position + 8]);
                        u64::from_le_bytes(b)
                    } else {
                        let mut b = [0u8; 8];
                        b.copy_from_slice(&note[position..position + 8]);
                        u64::from_be_bytes(b)
                    };
                    scalars.insert(tag, f64::from_bits(bits));
                } else {
                    let (num, den) = if field_type == 10 {
                        let num = tiff_u32(note, position, little) as i32;
                        let den = tiff_u32(note, position + 4, little) as i32;
                        (num as f64, den as f64)
                    } else {
                        let num = tiff_u32(note, position, little) as f64;
                        let den = tiff_u32(note, position + 4, little) as f64;
                        (num, den)
                    };
                    if den == 0.0 {
                        return fail("Zero Apple headroom denominator");
                    }
                    scalars.insert(tag, num / den);
                }
            } else {
                continue;
            }
        }
        if let (Some(a), Some(b)) = (scalars.get(&33).copied(), scalars.get(&48).copied()) {
            if !a.is_finite() || !b.is_finite() {
                return fail("Non-finite legacy HDR headroom");
            }
            let stops = if a < 1.0 {
                if b <= 0.01 { -20.0 * b + 1.8 } else { -0.101 * b + 1.601 }
            } else if b <= 0.01 {
                -70.0 * b + 3.0
            } else {
                -0.303 * b + 2.303
            }
            .max(0.0);
            if stops > 0.0 && stops <= 16.0 {
                return Ok((Some(LegacyGain { gain: candidates[0].clone(), stops }), "Existing legacy Apple HDR".to_string()));
            }
        }
    }
    Ok((None, "Legacy HDR map present, but valid Apple 33/48 headroom unavailable; original retained".to_string()))
}
