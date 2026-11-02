//! XMP packets (standard + Extended XMP) and namespace-aware lookups (MLXMP).
use crate::binary::be;
use crate::jpeg::{segments, Segment};
use crate::pyxml::{self, Element};
use crate::{fail, Error, Result};
use md5::{Digest, Md5};
use std::collections::BTreeMap;

pub const HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
pub const EXTENDED: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
pub const CAMERA: &str = "http://ns.google.com/photos/1.0/camera/";
pub const ITEM: &str = "http://ns.google.com/photos/1.0/container/item/";
pub const HDR: &str = "http://ns.adobe.com/hdr-gain-map/1.0/";
pub const NOTE: &str = "http://ns.adobe.com/xmp/note/";
pub const CONTAINER: &str = "http://ns.google.com/photos/1.0/container/";
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const XMPMETA: &str = "adobe:ns:meta/";
pub const APPLE: &str = "http://ns.apple.com/HDRGainMap/1.0/";
pub const PIXEL: &str = "http://ns.apple.com/pixeldatainfo/1.0/";

#[derive(Clone, Debug)]
struct Entry {
    ns: String,
    name: String,
    value: String,
}

/// Parsed facts of one XMP document, flattened so lookups need no tree.
#[derive(Clone, Debug)]
pub struct Packet {
    pub segment: Segment,
    pub guid: Option<String>,
    pub fragments: Vec<Segment>,
    pub xml: String,
    /// The parsed document, for callers that rewrite it.
    pub root: Element,
    entries: Vec<Entry>,
    /// Per element: (Item:Semantic, Item:Length) as `property()` reads them.
    items: Vec<(Option<String>, Option<String>)>,
}

/// All text inside `e` in document order (its text, then each child's text
/// and tail), like ElementTree `itertext`.
fn text_of(e: &Element, out: &mut String) {
    if let Some(t) = &e.text {
        out.push_str(t);
    }
    for c in &e.children {
        text_of(c, out);
        if let Some(t) = &c.tail {
            out.push_str(t);
        }
    }
}

fn text(e: &Element) -> String {
    let mut s = String::new();
    text_of(e, &mut s);
    s
}

/// An XMP property of `e`: the attribute, else the first direct child
/// element's text.
fn property(e: &Element, ns: &str, name: &str) -> Option<String> {
    if let Some(v) = e.get_attr(ns, name) {
        return Some(v.to_string());
    }
    e.find_child(ns, name).map(text)
}

/// Parse one XMP document with the crate's single XML parser (`pyxml`, which
/// the writers also use, so reading and rewriting accept the same input).
fn document(
    bytes: &[u8],
) -> Result<(
    String,
    Element,
    Vec<Entry>,
    Vec<(Option<String>, Option<String>)>,
)> {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == 0 {
        end -= 1;
    }
    let xml = std::str::from_utf8(&bytes[..end])
        .map_err(|_| Error::new("XMP 不是有效的 UTF-8"))?
        .to_string();
    let root =
        pyxml::parse(xml.as_bytes()).map_err(|e| Error::new(format!("XMP 解析失败：{e}")))?;
    let mut elems = Vec::new();
    root.preorder(&mut elems);
    let mut entries = Vec::new();
    let mut items = Vec::new();
    for e in elems {
        if !e.ns.is_empty() {
            entries.push(Entry {
                ns: e.ns.clone(),
                name: e.local.clone(),
                value: text(e).trim().to_string(),
            });
        }
        for (ns, name, value) in &e.attrib {
            if !ns.is_empty() {
                entries.push(Entry {
                    ns: ns.clone(),
                    name: name.clone(),
                    value: value.clone(),
                });
            }
        }
        items.push((property(e, ITEM, "Semantic"), property(e, ITEM, "Length")));
    }
    Ok((xml, root, entries, items))
}

impl Packet {
    pub fn values(&self, ns: &str, name: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.ns == ns && e.name == name)
            .map(|e| e.value.as_str())
            .collect()
    }
    pub fn items(&self) -> &[(Option<String>, Option<String>)] {
        &self.items
    }
}

pub fn md5_upper(b: &[u8]) -> String {
    Md5::digest(b).iter().map(|x| format!("{x:02X}")).collect()
}

pub fn packets(data: &[u8]) -> Result<Vec<Packet>> {
    let segs = segments(data)?;
    packets_from_segments(data, &segs)
}

/// `packets` over a precomputed segment list (perf item 3): the caller
/// guarantees `segs` is exactly `segments(data)`.
pub(crate) fn packets_from_segments(data: &[u8], segs: &[Segment]) -> Result<Vec<Packet>> {
    let mut out = Vec::new();
    let mut groups: BTreeMap<String, Vec<(Segment, usize, usize, Vec<u8>)>> = BTreeMap::new();
    for s in segs.iter().cloned() {
        if s.marker != 225 {
            continue;
        }
        let pl = s.payload(data);
        if pl.starts_with(EXTENDED) {
            let p = EXTENDED.len();
            if pl.len() < p + 40 {
                return fail("Extended XMP 数据头被截断");
            }
            let guid = String::from_utf8_lossy(&pl[p..p + 32]).to_uppercase();
            if guid.chars().count() != 32 || !guid.chars().all(|c| c.is_ascii_hexdigit()) {
                return fail("Extended XMP 标识无效");
            }
            let size = be(pl, p + 32, 4)?;
            let offset = be(pl, p + 36, 4)?;
            let bytes = pl[p + 40..].to_vec();
            if size == 0 || size > data.len() || offset > size || bytes.len() > size - offset {
                return fail("Extended XMP 分片偏移无效");
            }
            groups
                .entry(guid)
                .or_default()
                .push((s, size, offset, bytes));
        } else if pl.starts_with(HEADER) {
            let (xml, root, entries, items) = document(&pl[HEADER.len()..])?;
            out.push(Packet {
                segment: s,
                guid: None,
                fragments: vec![],
                xml,
                root,
                entries,
                items,
            });
        }
    }
    for (guid, chunks) in groups.iter() {
        let mut chunks = chunks.clone();
        chunks.sort_by_key(|c| c.2);
        let size = chunks[0].1;
        let mut xml = Vec::new();
        for (_, total, offset, bytes) in &chunks {
            if *total != size || *offset != xml.len() {
                return fail("Extended XMP 分片缺失、重叠或长度冲突");
            }
            xml.extend_from_slice(bytes);
        }
        if xml.len() != size {
            return fail("Extended XMP 分片不完整");
        }
        if md5_upper(&xml) != *guid {
            return fail("Extended XMP 校验失败");
        }
        let mut fragments: Vec<Segment> = chunks.iter().map(|c| c.0.clone()).collect();
        fragments.sort_by_key(|f| f.start);
        let (text, root, entries, items) = document(&xml)?;
        out.push(Packet {
            segment: fragments[0].clone(),
            guid: Some(guid.clone()),
            fragments,
            xml: text,
            root,
            entries,
            items,
        });
    }
    for p in out.iter().filter(|p| p.guid.is_none()) {
        for g in p.values(NOTE, "HasExtendedXMP") {
            if !groups.contains_key(&g.to_uppercase()) {
                return fail(format!("XMP 引用的扩展数据缺失：{g}"));
            }
        }
    }
    Ok(out)
}

/// Swift `Int(String)`: optional sign then ASCII digits, 64-bit.
pub fn swift_int(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let digits = if b.first().is_some_and(|c| *c == b'+' || *c == b'-') {
        &b[1..]
    } else {
        b
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    s.parse::<i64>().ok()
}

fn unique(values: Vec<&str>, err: &str, conflict: String) -> Result<Option<i64>> {
    let mut nums = Vec::new();
    for v in values {
        match swift_int(v.trim()) {
            Some(n) => nums.push(n),
            None => return fail(err.to_string()),
        }
    }
    let first = nums.first().copied();
    if nums.iter().any(|n| Some(*n) != first) {
        return fail(conflict);
    }
    Ok(first)
}

pub fn integer(packets: &[Packet], name: &str) -> Result<Option<i64>> {
    let v = packets
        .iter()
        .flat_map(|p| p.values(CAMERA, name))
        .collect();
    unique(
        v,
        &format!("XMP 时间或偏移数值无效：{name}"),
        format!("XMP 字段冲突：{name}"),
    )
}

pub fn vendor_integer(packets: &[Packet], ns: &str, name: &str) -> Result<Option<i64>> {
    let v = packets.iter().flat_map(|p| p.values(ns, name)).collect();
    unique(
        v,
        &format!("厂商 XMP 数值无效：{name}"),
        format!("厂商 XMP 字段冲突：{name}"),
    )
}
