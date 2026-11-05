//! Live Photo still-image pairing metadata. HDR rendering and packaging live
//! in `engine::hdr`; this module only edits pairing/motion metadata.
use crate::engine::failure;
use crate::pymotion::{self, visit_mut};
use crate::pyxml;
use crate::xmp::{CAMERA, ITEM, RDF};
use crate::{heif, heifwrite, jpeg, pyjpeg, sha256_hex, Result};
use std::collections::{HashMap, HashSet};
const OPPO: &str = "http://ns.oplus.com/photos/1.0/camera/";
const VIVO: &str = "http://ns.vivo.com/photos/1.0/camera/";

// Compatibility exports; new callers use the dedicated HDR module.
pub use super::hdr::{
    color_profile, convert_gain, gain_lookup, honor_private, legacy_gain, pack_gain_map,
    swift_double, ultra, ultra_with_offset, upgrade_iso,
};

fn aux_hashes(data: &[u8]) -> Result<Vec<String>> {
    Ok(jpeg::auxiliary(data)?
        .iter()
        .skip(1)
        .map(|r| sha256_hex(&data[r.clone()]))
        .collect())
}

fn is_motion_name(ns: &str, name: &str) -> bool {
    const KEYS: [&str; 8] = [
        "MotionPhoto",
        "MotionPhotoVersion",
        "MotionPhotoPresentationTimestampUs",
        "MicroVideo",
        "MicroVideoVersion",
        "MicroVideoOffset",
        "MicroVideoPresentationTimestampUs",
        "MediaDataOffset",
    ];
    const VENDOR: [&str; 8] = [
        "MotionPhotoPrimaryPresentationTimestampUs",
        "MotionPhotoOwner",
        "OLivePhotoVersion",
        "VideoLength",
        "MediaDataOffset",
        "VMotionPhotoVersion",
        "VMotionPhotoSource",
        "VMediaKitVersion",
    ];
    (ns == CAMERA && KEYS.contains(&name)) || ((ns == OPPO || ns == VIVO) && VENDOR.contains(&name))
}

/// Swift `PhotoFormat.liveStill(original, uuid:, updateIdentifier:)`:
/// static image ranges only, motion XMP removed, and (when
/// `update_identifier`) the Apple MakerNote content identifier written.
/// HEIC always gets the identifier (Swift `PhotoHEIF.liveStill`).
pub fn live_still(original: &[u8], uuid: &str, update_identifier: bool) -> Result<Vec<u8>> {
    if heif::matches(original) {
        return heifwrite::live_still_heic(original, uuid);
    }
    let data = jpeg::static_only(original)?;
    let segs = jpeg::segments(&data)?;
    let exifs: Vec<&jpeg::Segment> = segs
        .iter()
        .filter(|s| s.marker == 0xE1 && s.payload(&data).starts_with(jpeg::EXIF))
        .collect();
    if exifs.len() > 1 {
        return failure("JPEG 包含多个 EXIF 数据段");
    }
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut insertion = Vec::new();
    if update_identifier {
        match exifs.first() {
            Some(s) => {
                replacements.insert(
                    s.start,
                    pyjpeg::app(
                        0xE1,
                        &pyjpeg::exif_identifier(Some(s.payload(&data)), uuid)?,
                    )?,
                );
            }
            None => insertion = pyjpeg::app(0xE1, &pyjpeg::exif_identifier(None, uuid)?)?,
        }
    }
    let mut packets = pymotion::xmp_packets(&data)?;
    let mut changed: HashSet<usize> = HashSet::new();
    for (i, packet) in packets.iter_mut().enumerate() {
        let mut touched = false;
        visit_mut(&mut packet.root, &mut |el| {
            let before = el.attrib.len();
            el.attrib.retain(|(n, l, _)| !is_motion_name(n, l));
            touched |= el.attrib.len() != before;
        });
        touched |=
            pyxml::remove_children_where(&mut packet.root, &|c| is_motion_name(&c.ns, &c.local))
                > 0;
        touched |= pyxml::remove_children_where(&mut packet.root, &|c| {
            c.ns == RDF
                && c.local == "li"
                && pyxml::values(c, ITEM, "Semantic")
                    .iter()
                    .any(|v| v == "MotionPhoto")
        }) > 0;
        if touched {
            changed.insert(i);
        }
    }
    replacements.extend(pymotion::xmp_rewrites(&mut packets, &mut changed)?);
    let result = pyjpeg::edit_header(&data, &replacements, &insertion)?;
    if aux_hashes(&data)? != aux_hashes(&result)? {
        return failure("增益图字节校验失败");
    }
    Ok(result)
}
