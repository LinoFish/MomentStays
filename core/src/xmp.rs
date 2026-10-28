//! XMP packets (standard + Extended XMP) and namespace-aware lookups (MLXMP).
use crate::binary::be;
use crate::jpeg::{segments, Segment};
use crate::{fail, Error, Result};
use md5::{Digest, Md5};
use std::collections::BTreeMap;

pub const HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
pub const EXTENDED: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
pub const CAMERA: &str = "http://ns.google.com/photos/1.0/camera/";
pub const ITEM: &str = "http://ns.google.com/photos/1.0/container/item/";
pub const HDR: &str = "http://ns.adobe.com/hdr-gain-map/1.0/";
pub const NOTE: &str = "http://ns.adobe.com/xmp/note/";

#[derive(Clone, Debug)]
struct Entry { ns: String, name: String, value: String }

/// Parsed facts of one XMP document, flattened so lookups need no tree.
#[derive(Clone, Debug)]
pub struct Packet {
    pub segment: Segment,
    pub guid: Option<String>,
    pub fragments: Vec<Segment>,
    pub xml: String,
    entries: Vec<Entry>,
    /// Per element: (Item:Semantic, Item:Length) as `property()` reads them.
    items: Vec<(Option<String>, Option<String>)>,
}

fn text_of(n: roxmltree::Node) -> String {
    n.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect()
}

fn property(n: roxmltree::Node, ns: &str, name: &str) -> Option<String> {
    if let Some(a) = n.attributes().find(|a| a.name() == name && a.namespace() == Some(ns)) {
        return Some(a.value().to_string());
    }
    n.children()
        .find(|c| c.is_element() && c.tag_name().name() == name && c.tag_name().namespace() == Some(ns))
        .map(text_of)
}

fn document(bytes: &[u8]) -> Result<(String, Vec<Entry>, Vec<(Option<String>, Option<String>)>)> {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == 0 { end -= 1; }
    let xml = std::str::from_utf8(&bytes[..end]).map_err(|_| Error::new("XMP 不是有效的 UTF-8"))?.to_string();
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(&xml, opts).map_err(|e| Error::new(format!("XMP 解析失败：{e}")))?;
    let mut entries = Vec::new();
    let mut items = Vec::new();
    for n in doc.root().descendants().filter(|n| n.is_element()) {
        let tag = n.tag_name();
        if let Some(ns) = tag.namespace() {
            entries.push(Entry { ns: ns.into(), name: tag.name().into(), value: text_of(n).trim().to_string() });
        }
        for a in n.attributes() {
            if let Some(ns) = a.namespace() {
                entries.push(Entry { ns: ns.into(), name: a.name().into(), value: a.value().into() });
            }
        }
        items.push((property(n, ITEM, "Semantic"), property(n, ITEM, "Length")));
    }
    Ok((xml, entries, items))
}

impl Packet {
    pub fn values(&self, ns: &str, name: &str) -> Vec<&str> {
        self.entries.iter().filter(|e| e.ns == ns && e.name == name).map(|e| e.value.as_str()).collect()
    }
    pub fn items(&self) -> &[(Option<String>, Option<String>)] { &self.items }
}

pub fn md5_upper(b: &[u8]) -> String {
    Md5::digest(b).iter().map(|x| format!("{x:02X}")).collect()
}

pub fn packets(data: &[u8]) -> Result<Vec<Packet>> {
    let mut out = Vec::new();
    let mut groups: BTreeMap<String, Vec<(Segment, usize, usize, Vec<u8>)>> = BTreeMap::new();
    for s in segments(data)? {
        if s.marker != 225 { continue; }
        let pl = s.payload(data);
        if pl.starts_with(EXTENDED) {
            let p = EXTENDED.len();
            if pl.len() < p + 40 { return fail("Extended XMP 数据头被截断"); }
            let guid = String::from_utf8_lossy(&pl[p..p + 32]).to_uppercase();
            if guid.chars().count() != 32 || !guid.chars().all(|c| c.is_ascii_hexdigit()) { return fail("Extended XMP 标识无效"); }
            let size = be(pl, p + 32, 4)?;
            let offset = be(pl, p + 36, 4)?;
            let bytes = pl[p + 40..].to_vec();
            if size == 0 || size > data.len() || offset > size || bytes.len() > size - offset { return fail("Extended XMP 分片偏移无效"); }
            groups.entry(guid).or_default().push((s, size, offset, bytes));
        } else if pl.starts_with(HEADER) {
            let (xml, entries, items) = document(&pl[HEADER.len()..])?;
            out.push(Packet { segment: s, guid: None, fragments: vec![], xml, entries, items });
        }
    }
    for (guid, chunks) in groups.iter() {
        let mut chunks = chunks.clone();
        chunks.sort_by_key(|c| c.2);
        let size = chunks[0].1;
        let mut xml = Vec::new();
        for (_, total, offset, bytes) in &chunks {
            if *total != size || *offset != xml.len() { return fail("Extended XMP 分片缺失、重叠或长度冲突"); }
            xml.extend_from_slice(bytes);
        }
        if xml.len() != size { return fail("Extended XMP 分片不完整"); }
        if md5_upper(&xml) != *guid { return fail("Extended XMP 校验失败"); }
        let mut fragments: Vec<Segment> = chunks.iter().map(|c| c.0.clone()).collect();
        fragments.sort_by_key(|f| f.start);
        let (text, entries, items) = document(&xml)?;
        out.push(Packet { segment: fragments[0].clone(), guid: Some(guid.clone()), fragments, xml: text, entries, items });
    }
    for p in out.iter().filter(|p| p.guid.is_none()) {
        for g in p.values(NOTE, "HasExtendedXMP") {
            if !groups.contains_key(&g.to_uppercase()) { return fail(format!("XMP 引用的扩展数据缺失：{g}")); }
        }
    }
    Ok(out)
}

/// Swift `Int(String)`: optional sign then ASCII digits, 64-bit.
pub fn swift_int(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let digits = if b.first().map_or(false, |c| *c == b'+' || *c == b'-') { &b[1..] } else { b };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) { return None; }
    s.parse::<i64>().ok()
}

fn unique(values: Vec<&str>, err: &str, conflict: String) -> Result<Option<i64>> {
    let mut nums = Vec::new();
    for v in values {
        match swift_int(v.trim()) { Some(n) => nums.push(n), None => return fail(err.to_string()) }
    }
    let first = nums.first().copied();
    if nums.iter().any(|n| Some(*n) != first) { return fail(conflict); }
    Ok(first)
}

pub fn integer(packets: &[Packet], name: &str) -> Result<Option<i64>> {
    let v = packets.iter().flat_map(|p| p.values(CAMERA, name)).collect();
    unique(v, &format!("XMP 时间或偏移数值无效：{name}"), format!("XMP 字段冲突：{name}"))
}

pub fn vendor_integer(packets: &[Packet], ns: &str, name: &str) -> Result<Option<i64>> {
    let v = packets.iter().flat_map(|p| p.values(ns, name)).collect();
    unique(v, &format!("厂商 XMP 数值无效：{name}"), format!("厂商 XMP 字段冲突：{name}"))
}
