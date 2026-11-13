//! Neutral Motion Photo model and vendor writers (ROADMAP A1/A3, JPEG path).
//!
//! A [`MotionModel`] is built from any file [`crate::format::parse`]
//! understands (Google / Samsung SEF / Huawei-Honor LIVE_ / OPPO / vivo) and
//! can be written back out for any of those vendors. Video bytes are copied,
//! never re-encoded; the still keeps its coding data (only XMP segments are
//! rewritten, with MPF offsets repaired by [`crate::pyjpeg::edit_header`]).
//!
//! Deliberate refusals, matching the product rule of never guessing cover:
//! a target that needs a cover time cannot be built from a frame-only model
//! (Huawei), and vice versa. Time<->frame conversion needs the MP4
//! sample-table reader (ROADMAP item 7) and is not implemented here.
//! Output is validated by parsing it back: video bytes, still coding hash
//! and cover must all match, mirroring Swift `AndroidExport.convert`.

use crate::format::{self, MotionPhoto};
use crate::jpeg;
use crate::pyjpeg;
use crate::pymotion::{self, CAMERA, CONTAINER, HDR, ITEM, NOTE, RDF, XMPMETA};
use crate::pyxml::{self, Element};
use crate::{fail, fail_as, ErrorKind, Result};
use std::collections::{HashMap, HashSet};

pub const OPPO: &str = "http://ns.oplus.com/photos/1.0/camera/";
pub const VIVO: &str = "http://ns.vivo.com/photos/1.0/camera/";

/// Cover position: a presentation time in microseconds, or a Huawei frame index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cover {
    Time(i64),
    Frame(i64),
}

/// Vendor-neutral Motion Photo: still JPEG bytes, MP4 bytes, cover.
#[derive(Clone, Debug)]
pub struct MotionModel {
    pub still: Vec<u8>,
    pub video: Vec<u8>,
    pub cover: Cover,
    pub source_format: String,
}

/// The cover a parsed file carries: its Huawei frame if present, else its time.
pub fn cover_of(p: &MotionPhoto) -> Result<Cover> {
    if let Some(f) = p.cover_frame {
        Ok(Cover::Frame(f))
    } else if let Some(t) = p.timestamp {
        Ok(Cover::Time(t))
    } else {
        fail_as(ErrorKind::Cover, "缺少封面时间或帧号，无法无损转换")
    }
}

impl MotionModel {
    pub fn from_parsed(p: &MotionPhoto, source: &[u8]) -> Result<Self> {
        let cover = cover_of(p)?;
        Ok(MotionModel {
            still: p.still.to_vec(),
            video: p.video(source).to_vec(),
            cover,
            source_format: p.format.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Google,
    Samsung,
    Huawei,
    Oppo,
    Vivo,
}

impl Vendor {
    pub fn label(&self) -> &'static str {
        match self {
            Vendor::Google => "google",
            Vendor::Samsung => "samsung",
            Vendor::Huawei => "huawei",
            Vendor::Oppo => "oppo",
            Vendor::Vivo => "vivo",
        }
    }
}

const MOTION_KEYS: &[&str] = &[
    "MotionPhoto",
    "MotionPhotoVersion",
    "MotionPhotoPresentationTimestampUs",
    "MicroVideo",
    "MicroVideoVersion",
    "MicroVideoOffset",
    "MicroVideoPresentationTimestampUs",
];

const VENDOR_KEYS: &[&str] = &[
    "MotionPhotoPrimaryPresentationTimestampUs",
    "MotionPhotoOwner",
    "OLivePhotoVersion",
    "VideoLength",
    "MediaDataOffset",
    "VMotionPhotoVersion",
    "VMotionPhotoSource",
    "VMediaKitVersion",
];

fn is_motion_attr(ns: &str, local: &str) -> bool {
    (ns == CAMERA && MOTION_KEYS.contains(&local))
        || ((ns == OPPO || ns == VIVO) && VENDOR_KEYS.contains(&local))
        || (ns == HDR && local == "Version")
}

/// Strip old motion metadata; opaque HDR version fields remain when the
/// production HDR policy cannot identify a standard gain map. Mirrors `liveStill` +
/// `unifiedJPEG`: GCamera motion keys, vendor motion keys, hdrgm Version,
/// MotionPhoto directory items and whole Container directories.
fn clean_packet(root: &mut Element, rewrite_hdr: bool) -> bool {
    let mut touched = false;
    pymotion::visit_mut(root, &mut |el| {
        let before = el.attrib.len();
        el.attrib.retain(|(n, l, _)| {
            !(is_motion_attr(n, l) && (rewrite_hdr || n != HDR || l != "Version"))
        });
        if el.attrib.len() != before {
            touched = true;
        }
    });
    if pyxml::remove_children_where(root, &|child| {
        child.ns == CAMERA && MOTION_KEYS.contains(&child.local.as_str())
    }) > 0
    {
        touched = true;
    }
    if pyxml::remove_children_where(root, &|child| {
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
    }) > 0
    {
        touched = true;
    }
    if pyxml::remove_children_where(root, &|child| {
        child.ns == CONTAINER && child.local == "Directory"
    }) > 0
    {
        touched = true;
    }
    touched
}

/// Collect Extended XMP references (old GUIDs) from standard packets so the
/// rewritten document keeps linking its extended packets.
fn extended_refs(packets: &[pymotion::Packet]) -> Vec<String> {
    let mut refs: Vec<String> = Vec::new();
    for p in packets.iter().filter(|p| p.guid.is_none()) {
        let mut found = Vec::new();
        pymotion::visit_mut(&mut p.root.clone(), &mut |el| {
            if let Some(v) = el.get_attr(NOTE, "HasExtendedXMP") {
                found.push(v.to_string());
            }
            if el.ns == NOTE && el.local == "HasExtendedXMP" {
                if let Some(t) = &el.text {
                    found.push(t.clone());
                }
            }
        });
        for g in found {
            if !refs.iter().any(|r| r.eq_ignore_ascii_case(&g)) {
                refs.push(g);
            }
        }
    }
    refs
}

fn item(semantic: &str, mime: &str, length: usize) -> Element {
    let mut it = Element::new(CONTAINER, "Item");
    it.set_attr(ITEM, "Semantic", semantic);
    it.set_attr(ITEM, "Mime", mime);
    it.set_attr(ITEM, "Length", &length.to_string());
    it
}

fn li(item: Element) -> Element {
    let mut li = Element::new(RDF, "li");
    li.set_attr(RDF, "parseType", "Resource");
    li.children.push(item);
    li
}

/// The motion XMP document, mirroring Swift `AndroidExport.xmp`: one
/// Description carrying the vendor extras, the GCamera motion keys and the
/// Container directory (Primary, optional GainMap, MotionPhoto).
fn motion_doc(
    target: Vendor,
    time: Option<i64>,
    video_len: usize,
    gain_len: usize,
    declare_hdr: bool,
) -> Element {
    let mut desc = Element::new(RDF, "Description");
    match target {
        Vendor::Oppo => {
            desc.set_attr(
                OPPO,
                "MotionPhotoPrimaryPresentationTimestampUs",
                &time.unwrap_or(0).to_string(),
            );
            desc.set_attr(OPPO, "VideoLength", &video_len.to_string());
        }
        Vendor::Vivo => {
            desc.set_attr(VIVO, "VMotionPhotoVersion", "2");
            desc.set_attr(VIVO, "VMotionPhotoSource", "1");
        }
        _ => {}
    }
    if gain_len > 0 && declare_hdr {
        desc.set_attr(HDR, "Version", "1.0");
    }
    desc.set_attr(CAMERA, "MotionPhoto", "1");
    desc.set_attr(CAMERA, "MotionPhotoVersion", "1");
    if let Some(t) = time {
        desc.set_attr(CAMERA, "MotionPhotoPresentationTimestampUs", &t.to_string());
    }
    let mut dir = Element::new(CONTAINER, "Directory");
    let mut seq = Element::new(RDF, "Seq");
    let mut primary = item("Primary", "image/jpeg", 0);
    primary.set_attr(ITEM, "Padding", "0");
    seq.children.push(li(primary));
    if gain_len > 0 {
        seq.children
            .push(li(item("GainMap", "image/jpeg", gain_len)));
    }
    seq.children
        .push(li(item("MotionPhoto", "video/mp4", video_len)));
    dir.children.push(seq);
    desc.children.push(dir);
    let mut rdf = Element::new(RDF, "RDF");
    rdf.children.push(desc);
    let mut root = Element::new(XMPMETA, "xmpmeta");
    root.children.push(rdf);
    root
}

/// Rewrite the still's XMP to the target vendor's motion document, preserving
/// other Descriptions and Extended XMP links, then repair MPF offsets.
fn build_still(
    still: &[u8],
    target: Vendor,
    time: Option<i64>,
    video_len: usize,
    gain_len: usize,
    strict_hdr: bool,
) -> Result<Vec<u8>> {
    let rewrite_hdr = !strict_hdr || crate::hdr::known_ultra_version(still)?;
    let mut packets = pymotion::xmp_packets(still)?;
    let standard: Vec<usize> = packets
        .iter()
        .enumerate()
        .filter(|(_, p)| p.guid.is_none())
        .map(|(i, _)| i)
        .collect();
    if standard.is_empty() && packets.iter().any(|p| p.guid.is_some()) {
        return fail("Extended XMP 缺少主包，无法安全重写");
    }
    let source_scopes: Vec<_> = packets
        .iter()
        .map(|p| pyxml::namespace_uris(&p.root))
        .collect();
    let mut copied_scopes = Vec::new();
    let mut changed: HashSet<usize> = HashSet::new();
    for (i, p) in packets.iter_mut().enumerate() {
        if clean_packet(&mut p.root, rewrite_hdr) {
            changed.insert(i);
        }
    }
    let refs = extended_refs(&packets);
    let mut doc = motion_doc(target, time, video_len, gain_len, rewrite_hdr);
    {
        let rdf = doc.find_child_mut(RDF, "RDF").unwrap();
        for g in &refs {
            let mut link = Element::new(NOTE, "HasExtendedXMP");
            link.text = Some(g.clone());
            rdf.children[0].children.push(link);
        }
        for i in &standard {
            let root = &packets[*i].root;
            if let Some(r) = root.find_child(RDF, "RDF") {
                for d in r
                    .children
                    .iter()
                    .filter(|c| c.ns == RDF && c.local == "Description")
                {
                    rdf.children.push(d.clone());
                    copied_scopes.push(source_scopes[*i].clone());
                }
            }
        }
    }
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut insert: Vec<u8> = Vec::new();
    if let Some(first) = standard.first() {
        packets[*first].root = doc;
        changed.insert(*first);
        for i in standard.iter().skip(1) {
            replacements.insert(packets[*i].seg_start, Vec::new());
        }
    } else {
        let mut payload = pymotion::XMP_HDR.to_vec();
        payload.extend_from_slice(&if target == Vendor::Google {
            pyxml::tostring_motion_xmp(&doc, &copied_scopes)
        } else {
            pyxml::tostring(&doc)
        });
        insert = pyjpeg::app(0xE1, &payload)?;
    }
    let mut rewrites = pymotion::xmp_rewrites(&mut packets, &mut changed)?;
    if target == Vendor::Google {
        if let Some(first) = standard.first() {
            let mut payload = pymotion::XMP_HDR.to_vec();
            // Serialize after Extended-XMP GUID references have been updated.
            payload.extend(pyxml::tostring_motion_xmp(
                &packets[*first].root,
                &copied_scopes,
            ));
            rewrites.insert(packets[*first].seg_start, pyjpeg::app(0xE1, &payload)?);
        }
    }
    for (k, v) in replacements {
        rewrites.insert(k, v);
    }
    let out = pyjpeg::edit_header(still, &rewrites, &insert)?;
    let before = pyjpeg::mpf_images(still)?;
    let after = pyjpeg::mpf_images(&out)?;
    let hash = |d: &[u8], rs: &[std::ops::Range<usize>]| {
        rs.iter()
            .map(|r| crate::sha256_hex(&d[r.clone()]))
            .collect::<Vec<_>>()
    };
    if hash(still, before.get(1..).unwrap_or(&[])) != hash(&out, after.get(1..).unwrap_or(&[])) {
        return fail("辅助图数据变化");
    }
    Ok(out)
}

pub(crate) fn samsung_sef(video_start: usize, length: usize) -> Result<Vec<u8>> {
    if video_start > u32::MAX as usize || length > u32::MAX as usize {
        return fail_as(ErrorKind::Unsupported, "三星 SEF 4 GiB 范围超限");
    }
    let tag: [u8; 4] = [0, 0, 0x30, 0x0a];
    let mut record = tag.to_vec();
    record.extend_from_slice(&16u32.to_le_bytes());
    record.extend_from_slice(b"MotionPhoto_Data");
    record.extend_from_slice(b"mpv2");
    // SEF directory fields are little-endian, but the mpv2 video pointer and
    // length are big-endian (the parser reads them with `be`).
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

fn huawei_tail(frame: i64, video_len: usize) -> Result<Vec<u8>> {
    fn field(text: &str) -> Result<Vec<u8>> {
        if text.len() > 20 {
            return fail("厂商尾标数值超限");
        }
        let mut v = text.as_bytes().to_vec();
        v.resize(20, b' ');
        Ok(v)
    }
    let mut out = field(&format!("v2_f{frame}"))?;
    out.extend_from_slice(&field("0:0")?);
    out.extend_from_slice(&field(&format!("LIVE_{}", video_len + 20))?);
    Ok(out)
}

/// Write a model out for `target`, then parse it back and verify video bytes,
/// still coding hash and cover, exactly like Swift `AndroidExport.convert`.
pub fn write(model: &MotionModel, target: Vendor) -> Result<Vec<u8>> {
    write_inner(model, target, true)
}

/// Build the output without the read-back verification (perf item 6):
/// byte-identical to `write`, but skips the re-parse that doubles the
/// parse cost on large stills. `write` keeps verifying by default;
/// batch callers that verify separately can use this.
pub fn write_unverified(model: &MotionModel, target: Vendor) -> Result<Vec<u8>> {
    write_inner(model, target, false)
}

fn write_inner(model: &MotionModel, target: Vendor, verify: bool) -> Result<Vec<u8>> {
    let plan = plan(&model.still, &model.video, model.cover, target)?;
    let out = plan.concat(&model.video);
    if verify {
        plan.verify(&out, &model.video)?;
    }
    Ok(out)
}

/// Output layout of a Motion Photo writer: the file is `head ++ video ++
/// tail`. The video is never copied into the plan, so callers can stream
/// the three parts straight to a file (`output::write_atomic`) without
/// building the whole output in memory.
#[derive(Clone, Debug)]
pub struct Plan {
    /// Still image (plus any container header that precedes the video).
    pub head: Vec<u8>,
    /// Vendor trailer after the video (SEF directory, LIVE_ tail, or empty).
    pub tail: Vec<u8>,
    /// `head[..image_len]` is the still whose coding hash must survive.
    pub(crate) image_len: usize,
    pub(crate) cover: Cover,
    /// Prefix for read-back error texts ("安卓" / "HEIC ").
    pub(crate) label: &'static str,
}

impl Plan {
    /// Total output size for a video of `video_len` bytes.
    pub fn len(&self, video_len: usize) -> usize {
        self.head.len() + video_len + self.tail.len()
    }

    /// The whole output in one buffer (one copy of the video).
    pub fn concat(&self, video: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len(video.len()));
        out.extend_from_slice(&self.head);
        out.extend_from_slice(video);
        out.extend_from_slice(&self.tail);
        out
    }

    /// Parse `out` back and check video bytes, still coding hash and cover,
    /// like Swift `AndroidExport.convert`. `out` is the written file, so
    /// this also catches anything lost between building and writing.
    pub fn verify(&self, out: &[u8], video: &[u8]) -> Result<()> {
        let label = self.label;
        let back = format::parse(out)?;
        if back.video(out) != video {
            return fail_as(
                ErrorKind::VerifyFailed,
                format!("{label}封装读回校验失败：视频数据不同"),
            );
        }
        if format::coding_hash(&back.still)? != format::coding_hash(&self.head[..self.image_len])? {
            return fail_as(
                ErrorKind::VerifyFailed,
                format!("{label}封装读回校验失败：图像编码数据不同"),
            );
        }
        match self.cover {
            Cover::Time(t) if back.timestamp != Some(t) => fail_as(
                ErrorKind::VerifyFailed,
                format!("{label}封装读回校验失败：封面时间不同"),
            ),
            Cover::Frame(f) if back.cover_frame != Some(f) => fail_as(
                ErrorKind::VerifyFailed,
                format!("{label}封装读回校验失败：封面帧号不同"),
            ),
            _ => Ok(()),
        }
    }
}

/// Swift `AndroidExport.assemble` (JPEG path): the XMP always records the
/// cover time, and Huawei additionally gets the `LIVE_` tail with the cover
/// frame number. Read-back checks the frame for Huawei, the time otherwise.
pub fn plan_swift(
    still: &[u8],
    video: &[u8],
    time: i64,
    frame: i64,
    target: Vendor,
) -> Result<Plan> {
    let aux = jpeg::auxiliary(still)?;
    if aux.len() > 2 {
        return fail_as(
            ErrorKind::Unsupported,
            "动态照片含多个辅助 JPEG，无法安全建立目录",
        );
    }
    let gain_len = aux.get(1).map(|r| r.len()).unwrap_or(0);
    let head = build_still(still, target, Some(time), video.len(), gain_len, true)?;
    let (tail, cover) = match target {
        Vendor::Samsung => (samsung_sef(head.len(), video.len())?, Cover::Time(time)),
        Vendor::Huawei => (huawei_tail(frame, video.len())?, Cover::Frame(frame)),
        _ => (Vec::new(), Cover::Time(time)),
    };
    Ok(Plan {
        image_len: head.len(),
        head,
        tail,
        cover,
        label: "安卓",
    })
}

/// Plan a vendor JPEG Motion Photo for `still` + `video` (borrowed; nothing
/// is copied but the rewritten still). Same refusals as [`write`].
pub fn plan(still: &[u8], video: &[u8], cover: Cover, target: Vendor) -> Result<Plan> {
    let (time, frame) = match (target, cover) {
        (Vendor::Huawei, Cover::Frame(f)) => (None, f),
        (Vendor::Huawei, Cover::Time(_)) => {
            return fail_as(
                ErrorKind::Cover,
                "华为/荣耀目标需要封面帧号；时间→帧号转换尚未实现",
            )
        }
        (_, Cover::Time(t)) => (Some(t), 0),
        (_, Cover::Frame(_)) => {
            return fail_as(
                ErrorKind::Cover,
                "目标厂商需要封面时间；帧号→时间转换尚未实现",
            )
        }
    };
    let aux = jpeg::auxiliary(still)?;
    if aux.len() > 2 {
        return fail_as(
            ErrorKind::Unsupported,
            "动态照片含多个辅助 JPEG，无法安全建立目录",
        );
    }
    let gain_len = aux.get(1).map(|r| r.len()).unwrap_or(0);
    let head = build_still(still, target, time, video.len(), gain_len, false)?;
    let tail = match target {
        Vendor::Samsung => samsung_sef(head.len(), video.len())?,
        Vendor::Huawei => huawei_tail(frame, video.len())?,
        _ => Vec::new(),
    };
    Ok(Plan {
        image_len: head.len(),
        head,
        tail,
        cover,
        label: "安卓",
    })
}
