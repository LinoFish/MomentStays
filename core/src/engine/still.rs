//! Still writers: Swift `PhotoFormat.liveStill` and `PhotoHDR`
//! (`upgradeISO`, `colorProfile`, `ultra`, `packGainMap`) plus the gain
//! math of `legacyGain` / `AndroidHDR.convert`. Coded image bytes are never
//! changed; every writer checks the coding hash before returning.

use crate::engine::{failure, Gray, Native};
use crate::pymotion::{self, visit_mut, Packet};
use crate::pyxml::{self, Element};
use crate::tiff::{self, Tiff};
use crate::xmp::{CAMERA, CONTAINER, HDR, HEADER as XMP_HEADER, ITEM, RDF, XMPMETA};
use crate::{heif, heifwrite, jpeg, pyjpeg, sha256_hex, Result};
use std::collections::{HashMap, HashSet};

const OPPO: &str = "http://ns.oplus.com/photos/1.0/camera/";
const VIVO: &str = "http://ns.vivo.com/photos/1.0/camera/";
const ISO: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
const MPF: &[u8] = b"MPF\0";
const ICC: &[u8] = b"ICC_PROFILE\0";

/// Swift `Double.description`: "2.0", "0.5", "1e-05", "1e+16".
pub fn swift_double(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() {
            "nan".into()
        } else if v > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    let a = v.abs();
    if v != 0.0 && !(1e-4..1e16).contains(&a) {
        let s = format!("{v:e}");
        let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let (sign, digits) = match exp.strip_prefix('-') {
            Some(d) => ('-', d),
            None => ('+', exp),
        };
        return format!("{mantissa}e{sign}{digits:0>2}");
    }
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

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

/// Swift `PhotoHDR.colorProfile`: tag sRGB only when EXIF says sRGB.
pub fn color_profile(still: &[u8], srgb: &[u8]) -> Result<Vec<u8>> {
    let segs = jpeg::segments(still)?;
    if segs
        .iter()
        .any(|s| s.marker == 0xE2 && s.payload(still).starts_with(ICC))
    {
        return Ok(still.to_vec());
    }
    for s in &segs {
        if s.marker != 0xE1 || !s.payload(still).starts_with(jpeg::EXIF) {
            continue;
        }
        let t = Tiff::new(&s.payload(still)[6..])?;
        let (root, _) = t.root()?;
        let Some(p) = tiff::find(&root, 0x8769) else {
            continue;
        };
        let (exif, _) = t.directory(p.value_u32(t.little))?;
        if let Some(c) = tiff::find(&exif, 0xA001) {
            let v = tiff::u16_at(&c.raw, 8, t.little);
            if c.ty == 3 && c.count == 1 && v == 1 {
                let mut icc = ICC.to_vec();
                icc.extend([1, 1]);
                icc.extend(srgb);
                return pyjpeg::edit_header(still, &HashMap::new(), &pyjpeg::app(0xE2, &icc)?);
            }
        }
    }
    failure("旧 HDR 缺少 ICC 或明确的 sRGB 色彩声明，保留原始格式")
}

fn hdr_description(attrs: &[(&str, String)]) -> Element {
    let mut desc = Element::new(RDF, "Description");
    for (k, v) in attrs {
        desc.set_attr(HDR, k, v);
    }
    desc
}

fn xmpmeta(desc: Element) -> Element {
    let mut root = Element::new(XMPMETA, "xmpmeta");
    let mut rdf = Element::new(RDF, "RDF");
    rdf.children.push(desc);
    root.children.push(rdf);
    root
}

fn xmp_app1(doc: &[u8]) -> Result<Vec<u8>> {
    let mut payload = XMP_HEADER.to_vec();
    payload.extend_from_slice(doc);
    pyjpeg::app(0xE1, &payload)
}

/// Insert an APP segment right after SOI.
fn after_soi(image: &[u8], segment: &[u8]) -> Vec<u8> {
    let mut out = image[..2.min(image.len())].to_vec();
    out.extend_from_slice(segment);
    out.extend_from_slice(image.get(2..).unwrap_or(&[]));
    out
}

/// Swift `PhotoHDR.packGainMap`: the primary JPEG (MPF removed, Google
/// GainMap directory merged into its first XMP packet) plus `gain`, under
/// a fresh two-image MPF index.
pub fn pack_gain_map(still: &[u8], gain: &[u8]) -> Result<Vec<u8>> {
    let end = jpeg::segments(still)?
        .last()
        .map(|s| s.end)
        .unwrap_or(still.len());
    let primary = &still[..end];
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    for s in jpeg::segments(primary)? {
        if s.marker == 0xE2 && s.payload(primary).starts_with(MPF) {
            replacements.insert(s.start, Vec::new());
        }
    }
    let mut desc = hdr_description(&[("Version", "1.0".into())]);
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
    desc.children.push(dir);
    let packets: Vec<Packet> = pymotion::xmp_packets(primary)?;
    let mut insert = Vec::new();
    if let Some(first) = packets.first() {
        let mut root = first.root.clone();
        pyxml::remove_children_where(&mut root, &|c| c.ns == CONTAINER && c.local == "Directory");
        let mut placed = false;
        visit_mut(&mut root, &mut |el| {
            if !placed && el.ns == RDF && el.local == "RDF" {
                el.children.push(desc.clone());
                placed = true;
            }
        });
        if !placed {
            return failure("XMP RDF 结构无效");
        }
        replacements.insert(first.seg_start, xmp_app1(&pyxml::tostring(&root))?);
    } else {
        insert = xmp_app1(&pyxml::tostring(&xmpmeta(desc)))?;
    }
    let cleaned = pyjpeg::edit_header(primary, &replacements, &insert)?;
    // MPF APP2 (90 bytes): header, IFD of three fields, two MPEntries.
    let mut tiff = b"MM\0*\0\0\0\x08".to_vec();
    tiff.extend(3u16.to_be_bytes());
    for (tag, ty, count, value) in [
        (0xB000u16, 7u16, 4u32, *b"0100"),
        (0xB001, 4, 1, 2u32.to_be_bytes()),
        (0xB002, 7, 32, 50u32.to_be_bytes()),
    ] {
        tiff.extend(tag.to_be_bytes());
        tiff.extend(ty.to_be_bytes());
        tiff.extend(count.to_be_bytes());
        tiff.extend(value);
    }
    tiff.extend([0u8; 4]);
    let final_size = cleaned.len() + 90;
    let (Ok(final32), Ok(gain32)) = (u32::try_from(final_size), u32::try_from(gain.len())) else {
        return failure("旧 HDR 转换后的图像校验失败");
    };
    for (flags, size, offset) in [(0x2003_0000u32, final32, 0u32), (0, gain32, final32 - 10)] {
        tiff.extend(flags.to_be_bytes());
        tiff.extend(size.to_be_bytes());
        tiff.extend(offset.to_be_bytes());
        tiff.extend([0u8; 4]);
    }
    let mut mpf = MPF.to_vec();
    mpf.extend(tiff);
    let mut output = after_soi(&cleaned, &pyjpeg::app(0xE2, &mpf)?);
    output.extend_from_slice(gain);
    if jpeg::coding_hash(still)? != jpeg::coding_hash(&output)?
        || jpeg::auxiliary(&output)?.len() != 2
    {
        return failure("旧 HDR 转换后的图像校验失败");
    }
    Ok(output)
}

/// Swift `PhotoHDR.ultra`: gain-map XMP added to `gain_jpeg`, then packed.
pub fn ultra(still: &[u8], gain_jpeg: &[u8], stops: f64) -> Result<Vec<u8>> {
    let s = swift_double(stops);
    let desc = hdr_description(&[
        ("Version", "1.0".into()),
        ("GainMapMin", "0".into()),
        ("GainMapMax", s.clone()),
        ("Gamma", "1".into()),
        ("OffsetSDR", "0".into()),
        ("OffsetHDR", "0".into()),
        ("HDRCapacityMin", "0".into()),
        ("HDRCapacityMax", s),
        ("BaseRenditionIsHDR", "False".into()),
    ]);
    let gain = after_soi(gain_jpeg, &xmp_app1(&pyxml::tostring(&xmpmeta(desc)))?);
    pack_gain_map(still, &gain)
}

/// Swift `PhotoHDR.upgradeISO`: add Adobe/Google gain-map XMP to an
/// existing ISO 21496-1 gain map (both codestreams untouched). Ok(None)
/// when the layout is not the single-gain-map form this can verify.
pub fn upgrade_iso(still: &[u8]) -> Result<Option<Vec<u8>>> {
    let images = jpeg::auxiliary(still)?;
    if images.len() != 2 {
        return Ok(None);
    }
    let gain = &still[images[1].clone()];
    let packets: Vec<jpeg::Segment> = jpeg::segments(gain)?
        .into_iter()
        .filter(|s| s.marker == 0xE2 && s.payload(gain).starts_with(ISO))
        .collect();
    if packets.len() != 1 {
        return Ok(None);
    }
    let bytes = &packets[0].payload(gain)[ISO.len()..];
    if bytes.len() < 5 || bytes[0..2] != [0, 0] {
        return failure("ISO HDR 增益图版本尚不支持，原数据已保留");
    }
    let flags = bytes[4];
    let channels = if flags & 128 == 0 { 1 } else { 3 };
    if flags & 64 == 0 || flags & 4 != 0 || flags & 0x33 != 0 {
        return failure("ISO HDR 色彩空间或反向增益不适合无损补充 Ultra HDR 元数据，原格式已保留");
    }
    let mut pos = 5usize;
    let mut word = |signed: bool| -> Result<f64> {
        let Some(b) = bytes.get(pos..pos + 4) else {
            return failure("文件元数据被截断");
        };
        pos += 4;
        let raw = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        Ok(if signed {
            raw as i32 as f64
        } else {
            raw as f64
        })
    };
    let common = if flags & 8 != 0 {
        let c = word(false)?;
        if c == 0.0 {
            return failure("ISO HDR 参数分母为零");
        }
        Some(c)
    } else {
        None
    };
    let mut ratio = |signed: bool| -> Result<f64> {
        let n = word(signed)?;
        let d = match common {
            Some(c) => c,
            None => word(false)?,
        };
        if d <= 0.0 {
            return failure("ISO HDR 参数分母为零");
        }
        Ok(n / d)
    };
    let capacity_min = ratio(false)?;
    let capacity_max = ratio(false)?;
    let mut values: Vec<[f64; 5]> = Vec::new();
    for _ in 0..channels {
        values.push([
            ratio(true)?,
            ratio(true)?,
            ratio(false)?,
            ratio(true)?,
            ratio(true)?,
        ]);
    }
    let valid = |v: &[f64; 5]| {
        v.iter().all(|x| x.is_finite())
            && v[0] >= -16.0
            && v[1] <= 16.0
            && v[1] >= v[0]
            && v[2] > 0.0
            && v[2] <= 16.0
            && v[3].abs() <= 1.0
            && v[4].abs() <= 1.0
    };
    if !(pos == bytes.len()
        && capacity_min >= 0.0
        && capacity_max > capacity_min
        && capacity_max <= 16.0
        && values.iter().all(valid))
    {
        return failure("ISO HDR 增益参数无效或超出支持范围，原数据已保留");
    }
    let field = |name: &str, vals: Vec<f64>| -> String {
        if vals.len() == 1 {
            return format!("<hdrgm:{name}>{}</hdrgm:{name}>", swift_double(vals[0]));
        }
        let items: String = vals
            .iter()
            .map(|v| format!("<rdf:li>{}</rdf:li>", swift_double(*v)))
            .collect();
        format!("<hdrgm:{name}><rdf:Seq>{items}</rdf:Seq></hdrgm:{name}>")
    };
    let mut fields = String::new();
    for (i, name) in [
        "GainMapMin",
        "GainMapMax",
        "Gamma",
        "OffsetSDR",
        "OffsetHDR",
    ]
    .iter()
    .enumerate()
    {
        fields += &field(name, values.iter().map(|v| v[i]).collect());
    }
    let xml = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF}\"><rdf:Description xmlns:hdrgm=\"{HDR}\" hdrgm:Version=\"1.0\" hdrgm:BaseRenditionIsHDR=\"False\" hdrgm:HDRCapacityMin=\"{}\" hdrgm:HDRCapacityMax=\"{}\">{fields}</rdf:Description></rdf:RDF></x:xmpmeta>",
        swift_double(capacity_min),
        swift_double(capacity_max)
    );
    let new_gain = pyjpeg::edit_header(gain, &HashMap::new(), &xmp_app1(xml.as_bytes())?)?;
    let output = pack_gain_map(still, &new_gain)?;
    let after = jpeg::auxiliary(&output)?;
    if jpeg::coding_hash(gain)? != jpeg::coding_hash(&output[after[1].clone()])? {
        return failure("ISO HDR 增益图压缩数据发生变化");
    }
    Ok(Some(output))
}

/// Apple Rec.709-encoded linear boost to Ultra HDR log gain (Swift
/// `legacyGain` / `AndroidHDR.convert` lookup table).
pub fn gain_lookup(stops: f64) -> [u8; 256] {
    let headroom = 2f64.powf(stops);
    let mut t = [0u8; 256];
    for (i, slot) in t.iter_mut().enumerate() {
        let v = i as f64 / 255.0;
        let linear = if v < 0.081 {
            v / 4.5
        } else {
            ((v + 0.099) / 1.099).powf(1.0 / 0.45)
        };
        let g = ((1.0 + (headroom - 1.0) * linear).log2() / stops * 255.0).round();
        *slot = g.clamp(0.0, 255.0) as u8;
    }
    t
}

/// Map a gray gain image through [`gain_lookup`] (tight stride).
pub fn convert_gain(image: &Gray, stops: f64) -> Result<Gray> {
    if image.stride < image.width || image.pixels.len() / image.stride.max(1) < image.height {
        return failure("增益图数据被截断");
    }
    let lut = gain_lookup(stops);
    let mut pixels = vec![0u8; image.width * image.height];
    for y in 0..image.height {
        let row = &image.pixels[y * image.stride..y * image.stride + image.width];
        for (x, p) in row.iter().enumerate() {
            pixels[y * image.width + x] = lut[*p as usize];
        }
    }
    Ok(Gray {
        width: image.width,
        height: image.height,
        stride: image.width,
        pixels,
    })
}

/// Swift `legacyGain`: the converted gain-map JPEG and the map's size, or
/// Ok(None) = "unchanged" (not an 8-bit monochrome JPEG).
pub fn legacy_gain(
    native: &dyn Native,
    old_map: &[u8],
    stops: f64,
) -> Result<Option<(Vec<u8>, usize, usize)>> {
    if !(stops.is_finite() && stops > 0.0 && stops <= 16.0) {
        return Ok(None);
    }
    let Some(image) = native.decode_gray_jpeg(old_map)? else {
        return Ok(None);
    };
    let converted = convert_gain(&image, stops)?;
    Ok(Some((
        native.encode_gray_jpeg(&converted, 1.0)?,
        image.width,
        image.height,
    )))
}

/// Swift `PhotoHDR.honorPrivate` on a whole still (None for HEIC).
pub fn honor_private(data: &[u8]) -> Result<Option<Vec<u8>>> {
    if heif::matches(data) {
        return Ok(None);
    }
    let end = jpeg::image_end(data)?;
    Ok(crate::hdr::honor_private(data, end)?.map(<[u8]>::to_vec))
}
