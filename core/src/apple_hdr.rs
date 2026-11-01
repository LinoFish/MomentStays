//! Apple HDR reading, byte level (no pixel decode).
//!
//! What the apps used to read through ImageIO and Swift, read from the file
//! bytes instead:
//!
//! * the Apple MakerNote HDR scalars (tags 33 and 48) and the headroom in
//!   stops they define, for JPEG and HEIC stills;
//! * the legacy Apple gain map in a JPEG (the MPF image whose XMP says
//!   `urn:com:apple:photo:2020:aux:hdrgainmap`, version 65536), with the same
//!   decisions and reason texts as the Swift `PhotoHDR.legacy`;
//! * the structure of a HEIC: the Apple gain-map auxiliary item (`auxC`
//!   property + `ipma` association + `auxl` link to the primary item), an ISO
//!   `tmap` item, the `hdrgainmap` marker, and the 10-bit / PQ / HLG signals
//!   that mean a stream may be HDR even without a gain map.
//!
//! Not here: decoding the HEVC gain-map image itself. That needs a video
//! decoder and stays with the OS (ImageIO); everything it needs to know
//! beforehand comes from [`read`].

use crate::binary::be;
use crate::heif::{self, Layout};
use crate::mp4::{boxes, BoxRef};
use crate::tiff::{self, Tiff};
use crate::xmp::{self, APPLE, PIXEL};
use crate::{fail, jpeg, pyxml, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

/// The auxiliary image type that marks an Apple HDR gain map.
pub const GAIN_MAP_TYPE: &str = "urn:com:apple:photo:2020:aux:hdrgainmap";

const NOTE_PREFIX: &[u8] = b"Apple iOS\0\0\x01";

// Reason texts, shared with the Swift reference (`PhotoHDR.legacy`).
const R_VERSION: &str = "旧版增益图版本尚不支持，已保留原始数据";
const R_AUX_NO_PARAMS: &str = "已有辅助图，但缺少可验证的 HDR 亮度参数；原样保留，不推测增益";
const R_NO_DATA: &str = "没有可恢复的 HDR 增益数据；SDR 保持原样";
const R_MANY: &str = "存在多个旧增益图，未推测其含义";
const R_FOUND: &str = "检测到可恢复的旧 Apple HDR";
const R_INCOMPLETE: &str = "旧增益图亮度参数不完整，已保留原始数据";
const R_HEIC_REFUSE: &str =
    "此 HEIC HDR 不能在 macOS 12 安全提取；请保留 HEIC 原编码，或提供支持的 Apple HDR 增益图";
const R_HEIC_SDR: &str = "未检测到可恢复 HDR；不生成增益图";
const R_HEIC_MAP: &str = "检测到 Apple HEIC HDR 增益图（像素由系统解码）";

/// What the Apple MakerNote says about HDR.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Maker {
    /// An Apple MakerNote was found.
    pub present: bool,
    /// Tag 33 / tag 48 exist (ImageIO exposes them as `"33"` / `"48"`).
    pub has_33: bool,
    pub has_48: bool,
    /// Headroom in stops from tags 33 and 48, when both are usable.
    pub stops: Option<f64>,
}

/// A legacy Apple gain map found in a JPEG.
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyGain {
    /// The gain map as a JPEG (the MPF image).
    pub gain: Vec<u8>,
    pub stops: f64,
}

/// Verdict of [`read`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// No Apple HDR data; nothing to preserve or convert.
    None,
    /// JPEG with a usable legacy gain map and headroom.
    Legacy,
    /// HEIC with an Apple gain-map auxiliary image; the pixels are decoded
    /// by the OS, `stops` comes from the MakerNote.
    GainMap,
    /// HEIC that may carry HDR this crate cannot interpret (ISO `tmap`,
    /// 10-bit / PQ / HLG, an unrecognized gain map): keep the original.
    Refuse,
}

impl Status {
    pub fn code(self) -> &'static str {
        match self {
            Status::None => "none",
            Status::Legacy => "legacy",
            Status::GainMap => "gainmap",
            Status::Refuse => "refuse",
        }
    }
}

/// HEIC structure facts behind a [`Report`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeicFacts {
    pub primary_item: Option<usize>,
    /// Item id of the Apple gain map that belongs to the primary image.
    pub gain_map_item: Option<usize>,
    /// An ISO 21496-1 `tmap` item exists.
    pub iso_tmap: bool,
    /// The property boxes mention `hdrgainmap` anywhere.
    pub marker: bool,
    /// 10-bit HEVC, or a PQ / HLG transfer function (or no property boxes
    /// to check): the stream may be HDR even without a gain map.
    pub hdr_signal: bool,
}

/// Everything [`read`] learned about a still.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// `"jpeg"` or `"heic"`.
    pub container: &'static str,
    pub status: Status,
    pub reason: String,
    /// Headroom in stops (legacy gain map, or the HEIC MakerNote).
    pub stops: Option<f64>,
    /// JPEG: the legacy gain map image, when `status` is `Legacy`.
    pub gain: Option<Vec<u8>>,
    pub maker: Maker,
    pub heic: Option<HeicFacts>,
}

impl Report {
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "container": self.container,
            "status": self.status.code(),
            "reason": self.reason,
            "stops": self.stops,
            "maker": {
                "present": self.maker.present,
                "has_33": self.maker.has_33,
                "has_48": self.maker.has_48,
                "stops": self.maker.stops,
            },
        });
        if let Some(h) = &self.heic {
            v["heic"] = json!({
                "primary_item": h.primary_item,
                "gain_map_item": h.gain_map_item,
                "iso_tmap": h.iso_tmap,
                "marker": h.marker,
                "hdr_signal": h.hdr_signal,
            });
        }
        v
    }
}

/// Read the Apple HDR facts of a JPEG or HEIC still.
pub fn read(d: &[u8]) -> Result<Report> {
    if heif::matches(d) {
        read_heic(d)
    } else {
        read_jpeg(d)
    }
}

/// Headroom in stops from the Apple MakerNote, for either container.
pub fn stops(d: &[u8]) -> Result<Option<f64>> {
    Ok(read(d)?.maker.stops)
}

// ---------------------------------------------------------------------------
// MakerNote
// ---------------------------------------------------------------------------

/// Headroom in stops from tags 33 (`a`) and 48 (`b`), Apple's piecewise model.
/// `None` unless both are finite and the result is in (0, 16].
pub fn stops_from(a: f64, b: f64) -> Option<f64> {
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    let raw = if a < 1.0 {
        if b <= 0.01 {
            -20.0 * b + 1.8
        } else {
            -0.101 * b + 1.601
        }
    } else if b <= 0.01 {
        -70.0 * b + 3.0
    } else {
        -0.303 * b + 2.303
    };
    let stops = raw.max(0.0);
    (stops > 0.0 && stops <= 16.0).then_some(stops)
}

/// Parse an Apple MakerNote blob. `None` when it is not an Apple note.
fn parse_note(note: &[u8]) -> Result<Option<(Maker, Option<(f64, f64)>)>> {
    if !note.starts_with(NOTE_PREFIX) || note.len() < 16 {
        return Ok(None);
    }
    let little = match &note[12..14] {
        b"II" => true,
        b"MM" => false,
        _ => return fail("Apple MakerNote 字节序无效"),
    };
    // The note is a TIFF-style directory at offset 14 whose offsets count
    // from the start of the note.
    let t = Tiff { data: note, little };
    let (entries, _) = t.directory(14)?;
    let mut maker = Maker {
        present: true,
        ..Maker::default()
    };
    let mut values: HashMap<usize, f64> = HashMap::new();
    for f in &entries {
        if f.tag == 33 {
            maker.has_33 = true;
        }
        if f.tag == 48 {
            maker.has_48 = true;
        }
        if !(f.tag == 33 || f.tag == 48) || f.count != 1 {
            continue;
        }
        match f.ty {
            11 => {
                let bits = tiff::u32_at(&f.value, 0, little);
                values.insert(f.tag, f32::from_bits(bits) as f64);
            }
            5 | 10 | 12 => {
                let at = f.offset(little)?;
                if at < 16 + entries.len() * 12 + 4 || at + 8 > note.len() {
                    return fail("旧 HDR 亮度参数偏移无效");
                }
                if f.ty == 12 {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&note[at..at + 8]);
                    let bits = if little {
                        u64::from_le_bytes(b)
                    } else {
                        u64::from_be_bytes(b)
                    };
                    values.insert(f.tag, f64::from_bits(bits));
                } else {
                    let num = tiff::u32_at(note, at, little);
                    let den = tiff::u32_at(note, at + 4, little);
                    if den == 0 {
                        return fail("旧 HDR 亮度参数分母为零");
                    }
                    let v = if f.ty == 10 {
                        (num as i32) as f64 / (den as i32) as f64
                    } else {
                        num as f64 / den as f64
                    };
                    values.insert(f.tag, v);
                }
            }
            _ => {}
        }
    }
    let pair = match (values.get(&33), values.get(&48)) {
        (Some(a), Some(b)) => Some((*a, *b)),
        _ => None,
    };
    if let Some((a, b)) = pair {
        maker.stops = stops_from(a, b);
    }
    Ok(Some((maker, pair)))
}

/// MakerNote inside a TIFF (the Exif block): root IFD -> Exif IFD -> 0x927C.
/// `Ok(None)` when there is no Exif IFD or no MakerNote tag.
fn note_in_tiff(tiff_bytes: &[u8]) -> Result<Option<&[u8]>> {
    let t = Tiff::new(tiff_bytes)?;
    let (root, _) = t.root()?;
    let Some(ptr) = tiff::find(&root, 0x8769) else {
        return Ok(None);
    };
    let (fields, _) = t.directory(ptr.offset(t.little)?)?;
    let Some(f) = tiff::find(&fields, 0x927C) else {
        return Ok(None);
    };
    let at = f.offset(t.little)?;
    if f.ty != 7 || at.checked_add(f.count).is_none_or(|e| e > t.data.len()) {
        return fail("旧 MakerNote 无效");
    }
    Ok(Some(&t.data[at..at + f.count]))
}

// ---------------------------------------------------------------------------
// JPEG
// ---------------------------------------------------------------------------

fn read_jpeg(d: &[u8]) -> Result<Report> {
    let segs = jpeg::segments(d)?;
    let (legacy, reason) = legacy_from_segments(d, &segs)?;
    // Best-effort summary for callers that only want the scalars; the
    // decision above already surfaced any error it needed.
    let maker = jpeg_maker(d, &segs).ok().flatten().unwrap_or_default();
    let (status, stops, gain) = match legacy {
        Some(l) => (Status::Legacy, Some(l.stops), Some(l.gain)),
        None => (Status::None, maker.stops, None),
    };
    Ok(Report {
        container: "jpeg",
        status,
        reason: reason.to_string(),
        stops,
        gain,
        maker,
        heic: None,
    })
}

/// The first Apple MakerNote among the APP1 Exif segments.
fn jpeg_maker(d: &[u8], segs: &[jpeg::Segment]) -> Result<Option<Maker>> {
    for s in segs {
        let pl = s.payload(d);
        if s.marker != 225 || !pl.starts_with(jpeg::EXIF) {
            continue;
        }
        let Some(note) = note_in_tiff(&pl[jpeg::EXIF.len()..])? else {
            continue;
        };
        if let Some((maker, _)) = parse_note(note)? {
            return Ok(Some(maker));
        }
    }
    Ok(None)
}

/// The legacy Apple gain map of a JPEG, with Swift `PhotoHDR.legacy`'s
/// decisions and reason texts.
pub fn legacy(still: &[u8]) -> Result<(Option<LegacyGain>, &'static str)> {
    let segs = jpeg::segments(still)?;
    legacy_from_segments(still, &segs)
}

/// [`legacy`] over a precomputed segment list.
pub(crate) fn legacy_from_segments(
    still: &[u8],
    segs: &[jpeg::Segment],
) -> Result<(Option<LegacyGain>, &'static str)> {
    let ranges = jpeg::auxiliary_from_segments(still, segs)?;
    let mut maps: Vec<&[u8]> = Vec::new();
    for r in ranges.iter().skip(1) {
        let image = &still[r.clone()];
        for packet in xmp::packets(image)? {
            let is_gain = pyxml::values(&packet.root, PIXEL, "AuxiliaryImageType")
                .iter()
                .any(|v| v == GAIN_MAP_TYPE);
            if is_gain {
                if pyxml::values(&packet.root, APPLE, "HDRGainMapVersion") != ["65536"] {
                    return Ok((None, R_VERSION));
                }
                maps.push(image);
            }
        }
    }
    if maps.len() != 1 {
        let has_aux = ranges.len() > 1;
        let reason = if maps.is_empty() {
            if has_aux {
                R_AUX_NO_PARAMS
            } else {
                R_NO_DATA
            }
        } else {
            R_MANY
        };
        return Ok((None, reason));
    }
    for s in segs {
        let pl = s.payload(still);
        if s.marker != 225 || !pl.starts_with(jpeg::EXIF) {
            continue;
        }
        let Some(note) = note_in_tiff(&pl[jpeg::EXIF.len()..])? else {
            continue;
        };
        let Some((maker, _)) = parse_note(note)? else {
            continue;
        };
        if let Some(stops) = maker.stops {
            return Ok((
                Some(LegacyGain {
                    gain: maps[0].to_vec(),
                    stops,
                }),
                R_FOUND,
            ));
        }
    }
    Ok((None, R_INCOMPLETE))
}

// ---------------------------------------------------------------------------
// HEIC
// ---------------------------------------------------------------------------

fn read_heic(d: &[u8]) -> Result<Report> {
    let layout = heif::layout_lenient(d)?;
    let facts = heic_facts(d, &layout)?;
    let maker = heic_maker(d, &layout).ok().flatten().unwrap_or_default();
    let (status, reason) = if facts.gain_map_item.is_some() {
        (Status::GainMap, R_HEIC_MAP)
    } else if facts.marker || facts.iso_tmap || facts.hdr_signal {
        (Status::Refuse, R_HEIC_REFUSE)
    } else {
        (Status::None, R_HEIC_SDR)
    };
    Ok(Report {
        container: "heic",
        status,
        reason: reason.to_string(),
        stops: maker.stops,
        gain: None,
        maker,
        heic: Some(facts),
    })
}

/// The MakerNote in the HEIC's single `Exif` item.
fn heic_maker(d: &[u8], layout: &Layout) -> Result<Option<Maker>> {
    let exif: Vec<_> = layout.items.iter().filter(|i| i.ty == "Exif").collect();
    if exif.len() != 1 {
        return Ok(None);
    }
    let raw = heif::item_bytes(d, exif[0]);
    if raw.len() < 12 {
        return fail("HEIC EXIF 被截断");
    }
    let off = be(&raw, 0, 4)?;
    if off > raw.len() - 12 {
        return fail("HEIC EXIF TIFF 偏移越界");
    }
    let Some(note) = note_in_tiff(&raw[4 + off..])? else {
        return Ok(None);
    };
    Ok(parse_note(note)?.map(|(m, _)| m))
}

fn child<'a>(list: &'a [BoxRef], t: &[u8; 4]) -> Option<&'a BoxRef> {
    list.iter().find(|b| b.is(t))
}

fn heic_facts(d: &[u8], layout: &Layout) -> Result<HeicFacts> {
    let mut facts = HeicFacts {
        iso_tmap: layout.items.iter().any(|i| i.ty == "tmap"),
        ..HeicFacts::default()
    };
    // pitm: the primary item.
    if let Some(p) = child(&layout.children, b"pitm") {
        if p.end - p.payload >= 6 {
            facts.primary_item = Some(if d[p.payload] == 0 {
                be(d, p.payload + 4, 2)?
            } else {
                be(d, p.payload + 4, 4)?
            });
        }
    }
    let Some(iprp) = child(&layout.children, b"iprp") else {
        // Nothing to check the properties of: stay conservative.
        facts.hdr_signal = true;
        return Ok(facts);
    };
    facts.marker = d[iprp.payload..iprp.end]
        .windows(b"hdrgainmap".len())
        .any(|w| w == b"hdrgainmap");
    let parts = boxes(d, iprp.payload, iprp.end)?;
    let Some(ipco) = child(&parts, b"ipco") else {
        facts.hdr_signal = true;
        return Ok(facts);
    };
    let props = boxes(d, ipco.payload, ipco.end)?;
    for p in &props {
        let len = p.end - p.payload;
        if p.is(b"hvcC") && len >= 19 && d[p.payload + 17] & 7 > 0 {
            facts.hdr_signal = true;
        }
        if p.is(b"colr")
            && len >= 10
            && &d[p.payload..p.payload + 4] == b"nclx"
            && matches!(be(d, p.payload + 6, 2)?, 16 | 18)
        {
            facts.hdr_signal = true;
        }
    }
    // Which items carry an Apple gain-map auxC property.
    let gain_props: Vec<usize> = props
        .iter()
        .enumerate()
        .filter(|(_, p)| p.is(b"auxC") && aux_type(d, p) == GAIN_MAP_TYPE)
        .map(|(i, _)| i + 1)
        .collect();
    if gain_props.is_empty() {
        return Ok(facts);
    }
    let assoc = associations(d, &parts)?;
    let mut items: Vec<usize> = assoc
        .iter()
        .filter(|(_, idx)| idx.iter().any(|i| gain_props.contains(i)))
        .map(|(id, _)| *id)
        .collect();
    items.sort_unstable();
    // The gain map must belong to the primary image: when the file says
    // what each auxiliary image refers to, it has to be the primary.
    let links = aux_links(d, &layout.children)?;
    items.retain(|id| match (links.get(id), facts.primary_item) {
        (Some(to), Some(primary)) => to.contains(&primary),
        _ => true,
    });
    if items.len() == 1 {
        facts.gain_map_item = Some(items[0]);
    } else if items.len() > 1 {
        // Several candidates: do not pick one. The caller sees a marker
        // without a gain-map item and keeps the original.
        facts.marker = true;
    }
    Ok(facts)
}

/// `auxC` payload: version/flags, then the NUL-terminated aux type URN.
fn aux_type(d: &[u8], p: &BoxRef) -> String {
    if p.end - p.payload < 5 {
        return String::new();
    }
    let body = &d[p.payload + 4..p.end];
    let z = body.iter().position(|b| *b == 0).unwrap_or(body.len());
    String::from_utf8_lossy(&body[..z]).into_owned()
}

/// `ipma`: item id -> 1-based property indices.
fn associations(d: &[u8], parts: &[BoxRef]) -> Result<HashMap<usize, Vec<usize>>> {
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for ipma in parts.iter().filter(|b| b.is(b"ipma")) {
        let mut p = ipma.payload;
        if ipma.end - p < 8 {
            return fail("HEIC ipma 被截断");
        }
        let version = d[p];
        let wide = be(d, p + 1, 3)? & 1 == 1;
        let count = be(d, p + 4, 4)?;
        p += 8;
        // Every entry is at least id + count byte.
        if count > (ipma.end - p) / 3 {
            return fail("HEIC ipma 被截断");
        }
        for _ in 0..count {
            let w = if version < 1 { 2 } else { 4 };
            let id = be(d, p, w)?;
            p += w;
            let n = *d
                .get(p)
                .ok_or_else(|| crate::Error::new("HEIC ipma 被截断"))? as usize;
            p += 1;
            for _ in 0..n {
                let index = if wide {
                    let v = be(d, p, 2)?;
                    p += 2;
                    v & 0x7FFF
                } else {
                    let v = be(d, p, 1)?;
                    p += 1;
                    v & 0x7F
                };
                out.entry(id).or_default().push(index);
            }
        }
        if p > ipma.end {
            return fail("HEIC ipma 被截断");
        }
    }
    Ok(out)
}

/// `iref` `auxl` references: auxiliary item id -> the items it belongs to.
fn aux_links(d: &[u8], children: &[BoxRef]) -> Result<HashMap<usize, Vec<usize>>> {
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    let Some(iref) = child(children, b"iref") else {
        return Ok(out);
    };
    if iref.end - iref.payload < 4 {
        return fail("HEIC iref 被截断");
    }
    let w = if d[iref.payload] == 0 { 2 } else { 4 };
    for r in boxes(d, iref.payload + 4, iref.end)? {
        if !r.is(b"auxl") {
            continue;
        }
        let mut p = r.payload;
        let from = be(d, p, w)?;
        let n = be(d, p + w, 2)?;
        p += w + 2;
        if n > (r.end - p) / w {
            return fail("HEIC iref 被截断");
        }
        for i in 0..n {
            out.entry(from).or_default().push(be(d, p + i * w, w)?);
        }
    }
    Ok(out)
}
