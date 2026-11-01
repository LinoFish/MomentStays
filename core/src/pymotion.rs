//! Port of `motionlive/photo.py` and `motionlive/hdr.py`: Google XMP parsing,
//! lossless Live Photo still metadata, legacy Apple HDR handling.
//!
//! Error strings match the Python implementation, since this module backs the
//! Python CLI replacement (not the Swift-parity `format` module).

use crate::pyjpeg;
use crate::pyxml::{self, Element};
use crate::tiff::{self, Tiff};
use crate::{fail, fail_as, hex, ErrorKind, Result};
use sha2::Sha256;
use std::collections::{HashMap, HashSet};

pub use crate::xmp::{
    md5_upper, APPLE, CAMERA, CONTAINER, EXTENDED as EXTENDED_HDR, HDR, HEADER as XMP_HDR, ITEM,
    NOTE, PIXEL, RDF, XMPMETA,
};

fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest as _;
    hex(&Sha256::digest(b))
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

/// XMP packets as rewrite targets: a view over [`crate::xmp::packets`], the
/// crate's single XMP reader (Extended XMP reassembly, GUID/MD5 checks,
/// HasExtendedXMP references), keeping the segment starts and parsed tree.
pub fn xmp_packets(data: &[u8]) -> Result<Vec<Packet>> {
    Ok(crate::xmp::packets(data)?
        .into_iter()
        .map(|p| Packet {
            seg_start: p.segment.start,
            fragments: p.fragments.iter().map(|f| f.start).collect(),
            guid: p.guid,
            root: p.root,
        })
        .collect())
}

pub(crate) fn xmp_rewrites(
    packets: &mut [Packet],
    changed: &mut HashSet<usize>,
) -> Result<HashMap<usize, Vec<u8>>> {
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
                if el.ns == NOTE
                    && el.local == "HasExtendedXMP"
                    && el.text.as_deref().unwrap_or("").to_uppercase() == old
                {
                    el.text = Some(new.clone());
                    hit = true;
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

pub fn live_still(still: &[u8], identifier: &str) -> Result<Vec<u8>> {
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut insert: Vec<u8> = Vec::new();
    let mut has_exif = false;
    for s in pyjpeg::segments(still)? {
        if s.marker == 0xE1 && s.payload(still).starts_with(pyjpeg::EXIF_HDR) {
            if has_exif {
                return fail("Multiple EXIF segments");
            }
            replacements.insert(
                s.start,
                pyjpeg::app(
                    0xE1,
                    &pyjpeg::exif_identifier(Some(s.payload(still)), identifier)?,
                )?,
            );
            has_exif = true;
        }
    }
    if !has_exif {
        insert = pyjpeg::app(0xE1, &pyjpeg::exif_identifier(None, identifier)?)?;
    }
    let motion_keys: HashSet<(String, String)> = [
        "MotionPhoto",
        "MotionPhotoVersion",
        "MotionPhotoPresentationTimestampUs",
        "MicroVideo",
        "MicroVideoVersion",
        "MicroVideoOffset",
        "MicroVideoPresentationTimestampUs",
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
            el.attrib
                .retain(|(n, l, _)| !motion_keys.contains(&(n.clone(), l.clone())));
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
                    || (e.ns == ITEM
                        && e.local == "Semantic"
                        && e.text.as_deref() == Some("MotionPhoto"))
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
    let before_hashes: Vec<String> = before
        .get(1..)
        .unwrap_or(&[])
        .iter()
        .map(|r| sha256_hex(&still[r.clone()]))
        .collect();
    let after_hashes: Vec<String> = after
        .get(1..)
        .unwrap_or(&[])
        .iter()
        .map(|r| sha256_hex(&output[r.clone()]))
        .collect();
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
        if [
            b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1",
            b"msf1",
        ]
        .iter()
        .any(|b| brand == *b)
        {
            return fail_as(
                ErrorKind::Unsupported,
                "HEIC 静态图暂不支持 Live→Motion 清理，需 HEIC 写入支持",
            );
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
    for ns in [
        "http://ns.oplus.com/photos/1.0/camera/",
        "http://ns.vivo.com/photos/1.0/camera/",
    ] {
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
            el.attrib
                .retain(|(n, l, _)| !motion.contains(&(n.clone(), l.clone())));
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
                && pyxml::values(child, ITEM, "Semantic")
                    .iter()
                    .any(|v| v == "MotionPhoto")
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
    let before_hashes: Vec<String> = before
        .get(1..)
        .unwrap_or(&[])
        .iter()
        .map(|r| sha256_hex(&truncated[r.clone()]))
        .collect();
    let after_hashes: Vec<String> = after
        .get(1..)
        .unwrap_or(&[])
        .iter()
        .map(|r| sha256_hex(&output[r.clone()]))
        .collect();
    if before_hashes != after_hashes {
        return fail("Auxiliary image bytes changed");
    }
    Ok(output)
}

/// Swift `PhotoFormat.liveStill(uuid, updateIdentifier:true)` for JPEG:
/// clean first (static-only truncation + full motion XMP strip, like
/// `live_still_clean`), then inject the EXIF MakerNote content
/// identifier (like `live_still`). Parity is defined on coding data
/// (unchanged: only EXIF/XMP header segments move) plus semantic XMP
/// fields (motion keys gone) and the MakerNote tag 17 reading back as
/// the uuid, mirroring Swift `movie()`'s still check. HEIC stills are
/// refused by `live_still_clean`.
pub fn live_still_with_identifier(still: &[u8], identifier: &str) -> Result<Vec<u8>> {
    let cleaned = live_still_clean(still)?;
    live_still(&cleaned, identifier)
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
    // Only the first XMP packet is merged (the Python reference breaks after one).
    if let Some(packet) = packets.first() {
        let mut old_root = packet.root.clone();
        pyxml::remove_children_where(&mut old_root, &|c| {
            c.ns == CONTAINER && c.local == "Directory"
        });
        let old_rdf = match old_root.find_child_mut(RDF, "RDF") {
            Some(r) => r,
            None => return fail("XMP RDF missing"),
        };
        old_rdf.children.push(desc2.clone());
        let mut payload = XMP_HDR.to_vec();
        payload.extend_from_slice(&pyxml::tostring(&old_root));
        replacements.insert(packet.seg_start, pyjpeg::app(0xE1, &payload)?);
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
    for (attr, size, off) in [
        (0x20030000u32, final_size as u32, 0u32),
        (0u32, gain.len() as u32, (final_size - 10) as u32),
    ] {
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
    if segs
        .iter()
        .any(|s| s.marker == 0xE2 && s.payload(still).starts_with(b"ICC_PROFILE\0"))
    {
        return Ok(still.to_vec());
    }
    for s in &segs {
        if s.marker != 0xE1 || !s.payload(still).starts_with(pyjpeg::EXIF_HDR) {
            continue;
        }
        let t = Tiff::new(&s.payload(still)[6..])?;
        let (root, _) = t.root()?;
        let ptr = tiff::find(&root, 0x8769);
        if let Some(p) = ptr {
            let (exif, _) = t.directory(p.value_u32(t.little))?;
            let color = tiff::find(&exif, 0xA001);
            if let Some(c) = color {
                if c.tag == 0xA001 && c.ty == 3 && c.count == 1 {
                    let v = if t.little {
                        (c.raw[8] as usize) | ((c.raw[9] as usize) << 8)
                    } else {
                        ((c.raw[8] as usize) << 8) | c.raw[9] as usize
                    };
                    if v == 1 {
                        let mut icc = b"ICC_PROFILE\0\x01\x01".to_vec();
                        icc.extend_from_slice(srgb_icc);
                        return pyjpeg::edit_header(
                            still,
                            &HashMap::new(),
                            &pyjpeg::app(0xE2, &icc)?,
                        );
                    }
                }
            }
        }
    }
    fail("Legacy HDR has no ICC profile or explicit EXIF sRGB; color space not guessed")
}
