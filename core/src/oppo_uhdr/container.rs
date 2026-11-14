//! HEIF graph composition. Source item IDs and coding properties stay stable;
//! the software-encoded gain graph is appended under unused IDs.
use super::Source;
use crate::{
    binary::be,
    fail, heif,
    mp4::{boxes, bx, full, BoxRef},
    Result,
};
use std::collections::BTreeMap;

type References = Vec<([u8; 4], usize, Vec<usize>)>;
struct Properties {
    raw: Vec<Vec<u8>>,
    associations: BTreeMap<usize, Vec<(bool, usize)>>,
}

fn one<'a>(bs: &'a [BoxRef], ty: &[u8; 4]) -> Result<&'a BoxRef> {
    let found: Vec<_> = bs.iter().filter(|b| b.is(ty)).collect();
    if found.len() != 1 {
        return fail("OPPO HEIF 图结构缺少唯一必需 box");
    }
    Ok(found[0])
}

fn primary(d: &[u8], layout: &heif::Layout) -> Result<usize> {
    let b = one(&layout.children, b"pitm")?;
    if b.end < b.payload + 6 {
        return fail("HEIF pitm 被截断");
    }
    let v = d[b.payload];
    let w = if v == 0 {
        2
    } else if v == 1 {
        4
    } else {
        return fail("HEIF pitm 版本无效");
    };
    if b.end != b.payload + 4 + w {
        return fail("HEIF pitm 长度无效");
    }
    let id = be(d, b.payload + 4, w)?;
    if !layout
        .items
        .iter()
        .any(|i| i.id == id && ["hvc1", "grid", "jpeg"].contains(&i.ty.as_str()))
    {
        return fail("HEIF 主图编码不受支持");
    }
    Ok(id)
}

fn properties(d: &[u8], layout: &heif::Layout) -> Result<Properties> {
    let p = one(&layout.children, b"iprp")?;
    let cs = boxes(d, p.payload, p.end)?;
    let co = one(&cs, b"ipco")?;
    let ma = one(&cs, b"ipma")?;
    if cs.len() != 2 || ma.end < ma.payload + 8 {
        return fail("HEIF 属性关联不受支持");
    }
    let raw: Vec<_> = boxes(d, co.payload, co.end)?
        .iter()
        .map(|p| d[p.start..p.end].to_vec())
        .collect();
    if raw.len() > 32000 {
        return fail("HEIF 属性过多");
    }
    let mut at = ma.payload;
    let version = d[at];
    let flags = be(d, at + 1, 3)?;
    if version > 1 || flags & !1 != 0 {
        return fail("HEIF ipma 版本或标志不受支持");
    }
    let count = be(d, at + 4, 4)?;
    at += 8;
    if count > 4096 {
        return fail("HEIF ipma 条目过多");
    }
    let mut associations = BTreeMap::new();
    let iw = if version == 0 { 2 } else { 4 };
    let pw = if flags & 1 == 0 { 1 } else { 2 };
    let essential = if pw == 1 { 0x80 } else { 0x8000 };
    for _ in 0..count {
        if at + iw + 1 > ma.end {
            return fail("HEIF ipma 被截断");
        }
        let id = be(d, at, iw)?;
        at += iw;
        let n = d[at] as usize;
        at += 1;
        if at + n * pw > ma.end
            || associations.contains_key(&id)
            || !layout.items.iter().any(|i| i.id == id)
        {
            return fail("HEIF ipma 项目无效");
        }
        let mut row = Vec::new();
        for _ in 0..n {
            let v = be(d, at, pw)?;
            at += pw;
            let index = v & (essential - 1);
            if index > raw.len() {
                return fail("HEIF 属性索引越界");
            }
            if index != 0 {
                row.push((v & essential != 0, index));
            }
        }
        associations.insert(id, row);
    }
    if at != ma.end {
        return fail("HEIF ipma 尾部无效");
    }
    Ok(Properties { raw, associations })
}

fn references(d: &[u8], layout: &heif::Layout) -> Result<References> {
    let bs: Vec<_> = layout.children.iter().filter(|b| b.is(b"iref")).collect();
    if bs.is_empty() {
        return Ok(vec![]);
    }
    if bs.len() != 1 {
        return fail("HEIF iref 不唯一");
    }
    let b = bs[0];
    if b.end < b.payload + 4 || d[b.payload] > 1 {
        return fail("HEIF iref 无效");
    }
    let w = if d[b.payload] == 0 { 2 } else { 4 };
    let mut result = Vec::new();
    for r in boxes(d, b.payload + 4, b.end)? {
        if r.end < r.payload + w + 2 {
            return fail("HEIF 引用被截断");
        }
        let from = be(d, r.payload, w)?;
        let n = be(d, r.payload + w, 2)?;
        if n == 0 || n > 4096 || r.end != r.payload + w + 2 + n * w {
            return fail("HEIF 引用长度无效");
        }
        let targets: Vec<_> = (0..n)
            .map(|i| be(d, r.payload + w + 2 + i * w, w))
            .collect::<Result<_>>()?;
        if std::iter::once(&from)
            .chain(targets.iter())
            .any(|id| !layout.items.iter().any(|i| i.id == *id))
        {
            return fail("HEIF 引用指向不存在的项目");
        }
        result.push((r.ty, from, targets));
    }
    Ok(result)
}

fn dimension(props: &Properties, id: usize) -> Result<(usize, usize)> {
    let rows = props
        .associations
        .get(&id)
        .ok_or_else(|| crate::Error::new("HEIF 主图缺少属性"))?;
    let sizes: Vec<_> = rows
        .iter()
        .map(|(_, i)| &props.raw[i - 1])
        .filter(|p| &p[4..8] == b"ispe")
        .collect();
    if sizes.len() != 1 || sizes[0].len() != 20 {
        return fail("HEIF 主图尺寸属性无效");
    }
    let w = be(sizes[0], 12, 4)?;
    let h = be(sizes[0], 16, 4)?;
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return fail("HEIF 主图尺寸超出范围");
    }
    Ok((w, h))
}

fn infos(
    d: &[u8],
    layout: &heif::Layout,
    ids: &BTreeMap<usize, usize>,
    hidden: bool,
) -> Result<Vec<Vec<u8>>> {
    let b = one(&layout.children, b"iinf")?;
    let width = if d[b.payload] == 0 { 2 } else { 4 };
    let mut out = Vec::new();
    for e in boxes(d, b.payload + 4 + width, b.end)? {
        let p = e.payload;
        let w = if d[p] == 2 { 2 } else { 4 };
        let old = be(d, p + 4, w)?;
        let id = *ids
            .get(&old)
            .ok_or_else(|| crate::Error::new("HEIF 项目 ID 缺失"))?;
        let flags = be(d, p + 1, 3)? as u32 | u32::from(hidden);
        let mut body = (id as u32).to_be_bytes().to_vec();
        body.extend_from_slice(&d[p + 4 + w..e.end]);
        out.push(full(b"infe", 3, flags, &body));
    }
    Ok(out)
}

pub(super) fn attach(source: &Source<'_>, donor: &[u8]) -> Result<Vec<u8>> {
    let base = source.base;
    let a = heif::layout(base)?;
    let b = heif::layout(donor)?;
    if a.children.iter().any(|c| {
        ![
            &b"hdlr"[..],
            b"pitm",
            b"iinf",
            b"iloc",
            b"iref",
            b"iprp",
            b"idat",
        ]
        .contains(&&c.ty[..])
    }) || b
        .items
        .iter()
        .any(|i| !["hvc1", "grid"].contains(&i.ty.as_str()))
        || a.items.len() + b.items.len() + 1 > 4096
    {
        return fail("OPPO 或增益图 HEIF 图结构尚不支持");
    }
    let base_id = primary(base, &a)?;
    if !a
        .items
        .iter()
        .any(|i| i.id == base_id && ["hvc1", "grid"].contains(&i.ty.as_str()))
    {
        return fail("OPPO 原主图必须为 HEVC/grid");
    }
    let gain_id = primary(donor, &b)?;
    let mut pa = properties(base, &a)?;
    let pb = properties(donor, &b)?;
    let (width, height) = dimension(&pa, base_id)?;
    if dimension(&pb, gain_id)? != (source.gain_width, source.gain_height)
        || width * source.gain_height != height * source.gain_width
        || source.gain_width > width
        || source.gain_height > height
    {
        return fail("OPPO 主图与增益图比例或编码尺寸不匹配");
    }
    // Transform behaviour on tmap inputs needs separate evidence. Do not
    // silently apply an orientation twice or change source property values.
    for p in &pa.raw {
        if (&p[4..8] == b"irot" && (p.len() != 9 || p[8] != 0))
            || &p[4..8] == b"imir"
            || &p[4..8] == b"clap"
            || &p[4..8] == b"auxC"
            || (&p[4..8] == b"hvcC"
                && (p.len() < 27 || p[24] & 3 == 0 || p[25] & 7 != 0 || p[26] & 7 != 0))
        {
            return fail("OPPO UHDR 首版不处理旋转、镜像、裁切或高位深主图");
        }
    }
    let colours: Vec<_> = pa
        .raw
        .iter()
        .enumerate()
        .filter(|(_, p)| &p[4..8] == b"colr")
        .map(|(i, _)| i + 1)
        .collect();
    if colours.len() != 1 {
        return fail("OPPO UHDR 需要唯一明确的主图色彩声明");
    }
    let has_colour = |id: usize| {
        pa.associations
            .get(&id)
            .is_some_and(|row| row.iter().any(|(_, i)| *i == colours[0]))
    };
    if !has_colour(base_id)
        && a.items
            .iter()
            .filter(|i| i.ty == "hvc1")
            .any(|i| !has_colour(i.id))
    {
        return fail("OPPO 主图色彩属性关联不完整");
    }
    let ids_a: BTreeMap<_, _> = a.items.iter().map(|i| (i.id, i.id)).collect();
    let max = a.items.iter().map(|i| i.id).max().unwrap();
    if max > u32::MAX as usize - b.items.len() - 2 {
        return fail("HEIF 项目 ID 超限");
    }
    let ids_b: BTreeMap<_, _> = b
        .items
        .iter()
        .enumerate()
        .map(|(n, i)| (i.id, max + n + 1))
        .collect();
    let tone_id = max + b.items.len() + 1;
    let gain_id = ids_b[&gain_id];
    let mut items: Vec<_> = a
        .items
        .iter()
        .map(|i| (i.id, heif::item_bytes(base, i)))
        .collect();
    items.extend(
        b.items
            .iter()
            .map(|i| (ids_b[&i.id], heif::item_bytes(donor, i))),
    );
    items.push((tone_id, super::tone_map(source.boost)));
    let mut infe = infos(base, &a, &ids_a, false)?;
    infe.extend(infos(donor, &b, &ids_b, true)?);
    let mut tone_info = (tone_id as u32).to_be_bytes().to_vec();
    tone_info.extend(b"\0\0tmapISO HDR\0");
    infe.push(full(b"infe", 3, 0, &tone_info));
    let mut iinf = (infe.len() as u16).to_be_bytes().to_vec();
    for entry in infe {
        iinf.extend(entry);
    }

    let property_offset = pa.raw.len();
    pa.raw.extend(pb.raw);
    for (id, row) in pb.associations {
        pa.associations.insert(
            ids_b[&id],
            row.into_iter()
                .map(|(e, i)| (e, i + property_offset))
                .collect(),
        );
    }
    let mut ispe = vec![0; 4];
    ispe.extend((width as u32).to_be_bytes());
    ispe.extend((height as u32).to_be_bytes());
    pa.raw.push(bx(b"ispe", &ispe));
    let size_prop = pa.raw.len();
    // This describes the derived HDR rendition, not its unchanged SDR input.
    pa.raw.push(full(b"pixi", 0, 0, &[3, 10, 10, 10]));
    let pixel_prop = pa.raw.len();
    // H.273: BT.2020 primaries, HLG transfer, BT.2020 non-constant matrix.
    pa.raw.push(bx(b"colr", b"nclx\0\x09\0\x12\0\x09\x80"));
    let hdr_colour = pa.raw.len();
    if pa.raw.len() > 32767 {
        return fail("HEIF 属性索引超限");
    }
    pa.associations.insert(
        tone_id,
        vec![(false, size_prop), (false, pixel_prop), (false, hdr_colour)],
    );
    let mut ipma = (pa.associations.len() as u32).to_be_bytes().to_vec();
    for (id, row) in &pa.associations {
        ipma.extend((*id as u32).to_be_bytes());
        ipma.push(row.len() as u8);
        for &(essential, index) in row {
            ipma.extend(((index as u16) | if essential { 0x8000 } else { 0 }).to_be_bytes());
        }
    }
    let mut iprp = bx(b"ipco", &pa.raw.concat());
    iprp.extend(full(b"ipma", 1, 1, &ipma));
    let mut refs = references(base, &a)?;
    refs.extend(
        references(donor, &b)?
            .into_iter()
            .map(|(ty, from, targets)| {
                (
                    ty,
                    ids_b[&from],
                    targets.into_iter().map(|id| ids_b[&id]).collect(),
                )
            }),
    );
    refs.push((*b"dimg", tone_id, vec![base_id, gain_id]));
    let mut iref = Vec::new();
    for (ty, from, targets) in refs {
        let mut body = (from as u32).to_be_bytes().to_vec();
        body.extend((targets.len() as u16).to_be_bytes());
        for id in targets {
            body.extend((id as u32).to_be_bytes());
        }
        iref.extend(bx(&ty, &body));
    }
    // Alternative grouping keeps SDR primary usable by older ImageIO while
    // advertising the tone-map rendition to Adaptive HDR readers.
    // Entity group IDs share the item ID namespace.
    let mut altr = ((tone_id + 1) as u32).to_be_bytes().to_vec();
    altr.extend(2u32.to_be_bytes());
    altr.extend((tone_id as u32).to_be_bytes());
    altr.extend((base_id as u32).to_be_bytes());
    let grpl = bx(b"grpl", &full(b"altr", 0, 0, &altr));
    let mut prefix = Vec::new();
    for top in &a.boxes {
        if top.is(b"ftyp") {
            let brands = &base[top.payload..top.end];
            if brands.len() < 8 || !(brands.len() - 8).is_multiple_of(4) {
                return fail("HEIF ftyp 品牌声明无效");
            }
            // Range-extension RGB 4:4:4 gain tiles require heix. The original
            // heic/MIAF conformance brands cannot describe this new graph.
            prefix.extend(bx(b"ftyp", b"heix\0\0\0\0mif1heixtmap"));
        } else if top.is(b"free") || top.is(b"skip") {
            prefix.extend_from_slice(&base[top.start..top.end]);
        }
    }
    let mut mdat = Vec::new();
    let mut iloc = vec![0x88, 0];
    iloc.extend((items.len() as u32).to_be_bytes());
    for (id, bytes) in items {
        let offset = prefix.len() + 8 + mdat.len();
        iloc.extend((id as u32).to_be_bytes());
        iloc.extend([0, 0, 0, 0, 0, 1]);
        iloc.extend((offset as u64).to_be_bytes());
        iloc.extend((bytes.len() as u64).to_be_bytes());
        mdat.extend(bytes);
    }
    let mut meta = Vec::new();
    for c in &a.children {
        if c.is(b"hdlr") || c.is(b"pitm") {
            meta.extend_from_slice(&base[c.start..c.end]);
        }
    }
    meta.extend(full(b"iinf", 0, 0, &iinf));
    meta.extend(full(b"iloc", 2, 0, &iloc));
    meta.extend(full(b"iref", 1, 0, &iref));
    meta.extend(bx(b"iprp", &iprp));
    meta.extend(grpl);
    prefix.extend(bx(b"mdat", &mdat));
    prefix.extend(full(b"meta", 0, 0, &meta));
    verify_preserved(base, &prefix)?;
    Ok(prefix)
}

pub(super) fn verify_preserved(base: &[u8], output: &[u8]) -> Result<()> {
    let a = heif::layout(base)?;
    let b = heif::layout(output)?;
    if primary(base, &a)? != primary(output, &b)? {
        return fail("OPPO 主图 ID 发生变化");
    }
    for item in &a.items {
        let Some(other) = b.items.iter().find(|i| i.id == item.id) else {
            return fail("OPPO 原图资源丢失");
        };
        if item.ty != other.ty
            || item.mime != other.mime
            || heif::item_bytes(base, item) != heif::item_bytes(output, other)
        {
            return fail("OPPO 原图编码或元数据被改变");
        }
    }
    let pa = properties(base, &a)?;
    let pb = properties(output, &b)?;
    for item in &a.items {
        let values = |p: &Properties| {
            p.associations
                .get(&item.id)
                .into_iter()
                .flatten()
                .map(|(e, i)| (*e, p.raw[i - 1].clone()))
                .collect::<Vec<_>>()
        };
        if values(&pa) != values(&pb) {
            return fail("OPPO 原图色彩、方向或解码属性被改变");
        }
    }
    let rb = references(output, &b)?;
    if references(base, &a)?.iter().any(|r| !rb.contains(r)) {
        return fail("OPPO 原图引用丢失");
    }
    Ok(())
}
