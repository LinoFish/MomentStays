//! Bounded HEIF item parsing; no pixel decode (PhotoHEIF).
use crate::binary::be;
use crate::mp4::{boxes, BoxRef};
use crate::{fail, hex, jpeg, tiff::Tiff, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct Item {
    pub id: usize, pub ty: String, pub mime: String, pub method: usize,
    pub offset_at: usize, pub length_at: usize, pub offset_width: usize, pub length_width: usize,
    pub base: usize, pub ranges: Vec<Range<usize>>,
}

pub struct Layout { pub boxes: Vec<BoxRef>, pub children: Vec<BoxRef>, pub items: Vec<Item> }

pub fn matches(d: &[u8]) -> bool {
    d.len() >= 16 && &d[4..8] == b"ftyp" && [&b"heic"[..], b"heix", b"mif1"].contains(&&d[8..12])
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool { a.start < b.end && b.start < a.end }

pub fn layout(d: &[u8]) -> Result<Layout> {
    let top = boxes(d, 0, d.len())?;
    if top.first().map_or(true, |b| !b.is(b"ftyp")) || top.iter().any(|b| b.is(b"moov") || b.is(b"mpvd"))
        || top.iter().filter(|b| b.is(b"meta")).count() != 1 {
        return fail("HEIC 静态容器结构不受支持或仍包含视频");
    }
    let meta = top.iter().find(|b| b.is(b"meta")).unwrap().clone();
    if meta.end - meta.payload < 4 || d[meta.payload] != 0 { return fail("HEIC meta 版本不受支持"); }
    let children = boxes(d, meta.payload + 4, meta.end)?;
    let one = |t: &[u8; 4]| -> Result<BoxRef> {
        let f: Vec<&BoxRef> = children.iter().filter(|b| b.is(t)).collect();
        if f.len() != 1 { return fail(format!("HEIC 缺少唯一的 {}", String::from_utf8_lossy(t))); }
        Ok(f[0].clone())
    };
    let handler = one(b"hdlr")?;
    if handler.end - handler.payload < 12 || &d[handler.payload + 8..handler.payload + 12] != b"pict" { return fail("HEIC 不是静态图像容器"); }
    let info = one(b"iinf")?;
    let loc = one(b"iloc")?;
    let mut types: HashMap<usize, (String, String)> = HashMap::new();
    if info.end - info.payload < 6 { return fail("HEIC iinf 被截断"); }
    let iv = d[info.payload];
    if iv > 1 { return fail("HEIC iinf 版本不受支持"); }
    let cw = if iv == 0 { 2 } else { 4 };
    let entries = boxes(d, info.payload + 4 + cw, info.end)?;
    if entries.len() > 4096 || be(d, info.payload + 4, cw)? != entries.len() { return fail("HEIC 项目数量无效"); }
    for e in &entries {
        if !e.is(b"infe") || e.end - e.payload < 13 { return fail("HEIC infe 无效"); }
        let p = e.payload;
        let v = d[p];
        if v != 2 && v != 3 { return fail("HEIC infe 版本不受支持"); }
        let w = if v == 2 { 2 } else { 4 };
        let id = be(d, p + 4, w)?;
        let type_at = p + 6 + w;
        if !(type_at + 4 < e.end) || be(d, p + 4 + w, 2)? != 0 { return fail("HEIC 加密项目不受支持"); }
        let ty = String::from_utf8_lossy(&d[type_at..type_at + 4]).into_owned();
        if !["hvc1", "grid", "Exif", "mime", "tmap", "iden", "jpeg"].contains(&ty.as_str()) || types.contains_key(&id) {
            return fail("HEIC 含未知或重复项目，未推测处理");
        }
        let mut mime = String::new();
        if ty == "mime" {
            let rest = &d[type_at + 4..e.end];
            let z = rest.iter().position(|b| *b == 0);
            let z2 = z.and_then(|z| rest[z + 1..].iter().position(|b| *b == 0).map(|e2| (z, z + 1 + e2)));
            let (z, e2) = match z2 { Some(v) => v, None => return fail("HEIC MIME 项目被截断") };
            mime = String::from_utf8_lossy(&rest[z + 1..e2]).into_owned();
            if mime != "application/rdf+xml" { return fail("HEIC 未知 MIME 项目不受支持"); }
        }
        types.insert(id, (ty, mime));
    }
    let mut p = loc.payload;
    let le_ = loc.end;
    macro_rules! take { ($w:expr) => {{ let w: usize = $w;
        if w > 8 || p > le_ || w > le_ - p { return fail("HEIC iloc 字段被截断"); }
        if w == 0 { 0usize } else { let v = be(d, p, w)?; p += w; v } }}; }
    let version = take!(1);
    take!(3);
    if version > 2 { return fail("HEIC iloc 版本不受支持"); }
    let sizes = take!(1);
    let bases = take!(1);
    let (ow, lw, bw) = (sizes >> 4, sizes & 15, bases >> 4);
    let iw = if version == 0 { 0 } else { bases & 15 };
    if ![4, 8].contains(&ow) || ![4, 8].contains(&lw) || ![0, 4, 8].contains(&bw) || ![0, 4, 8].contains(&iw) {
        return fail("HEIC iloc 字段宽度不受支持");
    }
    let count = take!(if version == 2 { 4 } else { 2 });
    if count != types.len() { return fail("HEIC 项目描述与位置表数量不同"); }
    let mut items: Vec<Item> = Vec::new();
    let mut owned: Vec<Range<usize>> = Vec::new();
    let idat: Vec<BoxRef> = children.iter().filter(|b| b.is(b"idat")).cloned().collect();
    let mdat: Vec<BoxRef> = top.iter().filter(|b| b.is(b"mdat")).cloned().collect();
    for _ in 0..count {
        let id = take!(if version == 2 { 4 } else { 2 });
        let method = if version == 0 { 0 } else { take!(2) };
        let dri = take!(2);
        let ty = match types.get(&id) {
            Some(t) if dri == 0 && (method == 0 || method == 1) && !items.iter().any(|i| i.id == id) => t.clone(),
            _ => return fail("HEIC 项目引用或存储方式不受支持"),
        };
        let base = take!(bw);
        let ec = take!(2);
        if ec == 0 || ec > 1024 { return fail("HEIC extent 数量无效"); }
        let (mut ranges, mut offset_at, mut length_at) = (Vec::new(), 0, 0);
        for _ in 0..ec {
            if take!(iw) != 0 { return fail("HEIC extent 索引不受支持"); }
            offset_at = p;
            let offset = take!(ow);
            length_at = p;
            let length = take!(lw);
            let storage = if method == 1 { &idat } else { &mdat };
            if method != 0 && storage.len() != 1 { return fail("HEIC idat 不唯一"); }
            let origin = if method == 1 { storage[0].payload } else { 0 };
            if base > d.len() || offset > d.len() - base || origin > d.len() - base - offset { return fail("HEIC extent 偏移越界"); }
            let start = origin + base + offset;
            if length == 0 || length > d.len() - start || !storage.iter().any(|s| start >= s.payload && start + length <= s.end) {
                return fail("HEIC extent 不在图像数据范围");
            }
            let r = start..start + length;
            if owned.iter().any(|o| overlaps(o, &r)) { return fail("HEIC 项目数据重叠"); }
            owned.push(r.clone());
            ranges.push(r);
        }
        items.push(Item { id, ty: ty.0, mime: ty.1, method, offset_at, length_at, offset_width: ow, length_width: lw, base, ranges });
    }
    if p != loc.end { return fail("HEIC iloc 尾部数据无效"); }
    Ok(Layout { boxes: top, children, items })
}

pub fn item_bytes(d: &[u8], item: &Item) -> Vec<u8> {
    item.ranges.iter().flat_map(|r| d[r.clone()].iter().copied()).collect()
}

/// `Exif\0\0` + TIFF of the single Exif item, shaped like a JPEG APP1 payload.
pub fn exif(d: &[u8]) -> Result<Vec<u8>> {
    let l = layout(d)?;
    let items: Vec<&Item> = l.items.iter().filter(|i| i.ty == "Exif").collect();
    if items.len() != 1 { return fail("HEIC 缺少唯一 EXIF 项目，暂不新建项目"); }
    let raw = item_bytes(d, items[0]);
    if raw.len() < 12 { return fail("HEIC EXIF 被截断"); }
    let off = be(&raw, 0, 4)?;
    if off > raw.len() - 12 { return fail("HEIC EXIF TIFF 偏移越界"); }
    let tiff = &raw[4 + off..];
    Tiff::new(tiff)?;
    let mut out = jpeg::EXIF.to_vec();
    out.extend_from_slice(tiff);
    Ok(out)
}

pub fn coding_hash(d: &[u8]) -> Result<String> {
    let l = layout(d)?;
    let mut h = Sha256::new();
    let mut items: Vec<&Item> = l.items.iter().collect();
    items.sort_by_key(|i| i.id);
    for item in items.into_iter().filter(|i| i.ty != "Exif" && i.ty != "mime") {
        let count: usize = item.ranges.iter().map(|r| r.len()).sum();
        h.update(crate::binary::bytes(item.id as u64, 4, false));
        h.update(item.ty.as_bytes());
        h.update(crate::binary::bytes(count as u64, 8, false));
        for r in &item.ranges { h.update(&d[r.clone()]); }
    }
    for b in l.children.iter().filter(|b| b.is(b"iprp") || b.is(b"iref") || b.is(b"pitm")) {
        if b.is(b"iref") {
            if b.end - b.payload < 4 || d[b.payload] > 1 { return fail("HEIC iref 版本不受支持"); }
            let refs: Vec<BoxRef> = boxes(d, b.payload + 4, b.end)?.into_iter().filter(|r| !r.is(b"cdsc")).collect();
            if !refs.is_empty() {
                h.update(&d[b.payload..b.payload + 4]);
                for r in &refs { h.update(&d[r.payload - 8..r.end]); }
            }
        } else {
            h.update(&d[b.payload..b.end]);
        }
    }
    Ok(hex(&h.finalize()))
}
