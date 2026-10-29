//! Shared synthetic HEIC still builder for HEIC tests.
//!
//! No pixel decode anywhere in the core, so fake `hvc1` bytes are enough
//! for the container logic. The builder satisfies `heif::layout` and
//! `heif::exif`: ftyp + meta(hdlr, pitm, iinf, iloc[, iref]) + mdat.
//! Items: 1 = hvc1, 2 = Exif, 3 = mime XMP (when `with_mime`).
//! With `with_iref`, a cdsc reference links the mime item to the primary.

pub fn bx(t: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(t.as_bytes());
    v.extend(payload);
    v
}

pub fn full(t: &str, version: u8, body: &[u8]) -> Vec<u8> {
    let mut p = vec![version, 0, 0, 0];
    p.extend(body);
    bx(t, &p)
}

pub fn u16b(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

pub fn u32b(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// Minimal TIFF (MM, 0 entries) for the Exif item, as `heif::exif`
/// expects: 4-byte offset (0) + TIFF bytes.
pub fn exif_item() -> Vec<u8> {
    let mut v = vec![0u8, 0, 0, 0];
    v.extend(b"MM");
    v.extend([0u8, 42]);
    v.extend(u32b(8));
    v.extend(u16b(0));
    v.extend(u32b(0));
    v
}

/// Exif item whose TIFF carries an Apple MakerNote with tag 0x11
/// (ContentIdentifier, ASCII) and tag 0x17 (LivePhotoVideoIndex,
/// LONG8), mirroring the real iPhone MakerNote layout the JPEG
/// `maker_note` writer handles: MakerNote-internal offsets are
/// relative to the MakerNote start; the 4-byte item prefix is the
/// TIFF offset (0 here).
pub fn exif_item_with_makernote(old_uuid: &str, video_index: u64) -> Vec<u8> {
    let mut uuid = old_uuid.as_bytes().to_vec();
    uuid.push(0);
    // MakerNote: header (14) + count (2) + 2 entries (24) + next (4) =
    // 44 bytes before external values.
    let mut note = b"Apple iOS\0\x00\x01MM".to_vec();
    note.extend(2u16.to_be_bytes());
    // tag 0x11, ASCII, external value at 44.
    note.extend(0x11u16.to_be_bytes());
    note.extend(2u16.to_be_bytes());
    note.extend((uuid.len() as u32).to_be_bytes());
    note.extend(44u32.to_be_bytes());
    // tag 0x17, LONG8 (type 16), external value right after the uuid.
    let index_at = 44 + uuid.len();
    note.extend(0x17u16.to_be_bytes());
    note.extend(16u16.to_be_bytes());
    note.extend(1u32.to_be_bytes());
    note.extend((index_at as u32).to_be_bytes());
    note.extend([0u8; 4]);
    note.extend(&uuid);
    note.extend(video_index.to_be_bytes());
    // TIFF: MM, 42, IFD0 at 8; IFD0 has the Exif pointer (0x8769);
    // Exif IFD has the MakerNote (0x927C, undefined).
    let exif_ifd_at = 8 + 2 + 12 + 4;
    let note_at = exif_ifd_at + 2 + 12 + 4;
    let mut tiff = b"MM".to_vec();
    tiff.extend([0u8, 42]);
    tiff.extend(8u32.to_be_bytes());
    tiff.extend(1u16.to_be_bytes());
    tiff.extend(0x8769u16.to_be_bytes());
    tiff.extend(4u16.to_be_bytes());
    tiff.extend(1u32.to_be_bytes());
    tiff.extend((exif_ifd_at as u32).to_be_bytes());
    tiff.extend([0u8; 4]);
    tiff.extend(1u16.to_be_bytes());
    tiff.extend(0x927Cu16.to_be_bytes());
    tiff.extend(7u16.to_be_bytes());
    tiff.extend((note.len() as u32).to_be_bytes());
    tiff.extend((note_at as u32).to_be_bytes());
    tiff.extend([0u8; 4]);
    tiff.extend(&note);
    let mut item = vec![0u8, 0, 0, 0];
    item.extend(tiff);
    item
}

pub fn plain_xmp() -> Vec<u8> {
    b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\"/></rdf:RDF></x:xmpmeta>".to_vec()
}

/// Motion XMP for parse-level fixtures: GCamera MotionPhoto=1 plus a
/// presentation timestamp, under the GCamera namespace URI the parser
/// looks up (prefix itself is irrelevant).
pub fn motion_xmp(timestamp_us: i64) -> Vec<u8> {
    format!("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:GCamera=\"http://ns.google.com/photos/1.0/camera/\" GCamera:MotionPhoto=\"1\" GCamera:MotionPhotoPresentationTimestampUs=\"{timestamp_us}\"/></rdf:RDF></x:xmpmeta>").into_bytes()
}

fn be_n(v: usize, w: usize) -> Vec<u8> {
    v.to_be_bytes()[8 - w..].to_vec()
}

/// One HEIC item for the parameterized still builder.
pub struct HeicItem {
    pub id: usize,
    pub ty: &'static str,
    pub data: Vec<u8>,
    /// 0 = extent data lives in the top-level mdat, 1 = in the meta idat.
    pub method: usize,
    /// Split `data` into this many extents (>= 1).
    pub extents: usize,
}

pub struct HeicStillSpec {
    pub iinf_version: u8, // 0 or 1
    pub infe_version: u8, // 2 or 3
    pub iloc_version: u8, // 0, 1 or 2
    pub offset_width: usize, // 4 or 8
    pub length_width: usize, // 4 or 8
    pub base_width: usize,   // 0, 4 or 8
    pub base: usize,         // base_offset value (0 when base_width is 0)
    pub index_width: usize,  // 0, 4 or 8 (v1/v2 only; forced 0 for v0)
    pub with_iref: bool,
}

/// Parameterized HEIC still builder (ROADMAP item 3): same container
/// shape as `build_still` but with selectable iinf/infe/iloc versions,
/// offset/length/base/index widths, mdat vs idat storage (per item),
/// and multi-extent items. Two-pass like `build_still`: entry sizes
/// are value-independent, so a zero-offset pass measures the meta box
/// before real offsets are assigned.
pub fn build_still_spec(spec: &HeicStillSpec, items: &[HeicItem]) -> Vec<u8> {
    assert!(!items.is_empty());
    // iloc v0 has no method field: its entries are implicitly mdat.
    assert!(
        spec.iloc_version > 0 || items.iter().all(|i| i.method == 0),
        "iloc v0 cannot express idat (method 1); use v1+"
    );
    assert!(spec.offset_width == 4 || spec.offset_width == 8);
    assert!(spec.length_width == 4 || spec.length_width == 8);
    assert!(spec.base_width == 0 || spec.base_width == 4 || spec.base_width == 8);
    assert!(spec.iloc_version <= 2);
    let id_w = if spec.infe_version == 3 { 4 } else { 2 };
    let count_w = if spec.iinf_version == 1 { 4 } else { 2 };
    let iw = if spec.iloc_version == 0 { 0 } else { spec.index_width };
    // Chunk split per item (in item order, method groups kept separate).
    let mut chunks: Vec<(usize, Vec<u8>)> = Vec::new(); // (item index, chunk bytes)
    let mut mdat_len = 0usize;
    let mut idat_len = 0usize;
    for (idx, item) in items.iter().enumerate() {
        let n = item.extents.max(1);
        let step = item.data.len().div_ceil(n);
        let mut at = 0usize;
        while at < item.data.len() || (item.data.is_empty() && chunks.iter().filter(|(i, _)| *i == idx).count() == 0) {
            let end = (at + step).min(item.data.len());
            let chunk = item.data[at..end].to_vec();
            if item.method == 1 { idat_len += chunk.len(); } else { mdat_len += chunk.len(); }
            chunks.push((idx, chunk));
            at = end;
            if item.data.is_empty() { break; }
        }
    }
    let has_idat = items.iter().any(|i| i.method == 1);
    let build_meta = |stored: &[(usize, Vec<(usize, usize, usize)>) ]| -> (Vec<u8>, usize, usize) {
        // stored: per item, list of (offset_value, length, index) per extent.
        // Returns (meta box bytes, iloc box len, idat payload absolute offset base).
        let mut hdlr_body = vec![0u8; 4];
        hdlr_body.extend(b"pict");
        hdlr_body.extend([0u8; 4]);
        let hdlr = full("hdlr", 0, &hdlr_body);
        let pitm_ver = if id_w == 4 { 1u8 } else { 0u8 };
        let pitm_body = be_n(items[0].id, id_w);
        let pitm = full("pitm", pitm_ver, &pitm_body);
        let mut infe_boxes = Vec::new();
        for item in items {
            let mut body = be_n(item.id, id_w);
            body.extend([0u8, 0]);
            body.extend(item.ty.as_bytes());
            if item.ty == "mime" {
                body.extend(b"\0application/rdf+xml\0");
            } else {
                body.push(0);
            }
            infe_boxes.push(full("infe", spec.infe_version, &body));
        }
        let mut iinf_body = be_n(items.len(), count_w);
        for b in &infe_boxes {
            iinf_body.extend(b);
        }
        let iinf = full("iinf", spec.iinf_version, &iinf_body);
        let mut iloc_body = vec![((spec.offset_width << 4) | spec.length_width) as u8, ((spec.base_width << 4) | iw) as u8];
        if spec.iloc_version == 2 {
            iloc_body.extend(be_n(items.len(), 4));
        } else {
            iloc_body.extend(be_n(items.len(), 2));
        }
        for (i, item) in items.iter().enumerate() {
            if spec.iloc_version == 2 {
                iloc_body.extend(be_n(item.id, 4));
            } else {
                iloc_body.extend(be_n(item.id, 2));
            }
            if spec.iloc_version > 0 {
                iloc_body.extend([0u8, item.method as u8]);
            }
            iloc_body.extend([0u8, 0]); // data_reference_index = 0
            iloc_body.extend(be_n(spec.base, spec.base_width));
            let exts = &stored[i].1;
            iloc_body.extend(be_n(exts.len(), 2));
            for (off, len, _idx) in exts {
                if spec.iloc_version > 0 {
                    iloc_body.extend(be_n(0, iw));
                }
                iloc_body.extend(be_n(*off, spec.offset_width));
                iloc_body.extend(be_n(*len, spec.length_width));
            }
        }
        let iloc = full("iloc", spec.iloc_version, &iloc_body);
        let mut children = hdlr.clone();
        children.extend(&pitm);
        children.extend(&iinf);
        children.extend(&iloc);
        let mut iref_len = 0usize;
        if spec.with_iref {
            if let Some(mime) = items.iter().find(|i| i.ty == "mime") {
                let iref_ver = if id_w == 4 { 1u8 } else { 0u8 };
                let mut cdsc_body = be_n(mime.id, id_w);
                cdsc_body.extend([0u8, 1]);
                cdsc_body.extend(be_n(items[0].id, id_w));
                let cdsc = bx("cdsc", &cdsc_body);
                let iref = full("iref", iref_ver, &cdsc);
                iref_len = iref.len();
                children.extend(&iref);
            }
        }
        // idat goes last; its payload start is computed by the caller
        // from the children lengths (idat length is offset-independent).
        let meta_len_pre = 12 + children.len() + if has_idat { 8 + idat_len } else { 0 };
        let _ = meta_len_pre;
        (children, iloc.len(), iref_len)
    };
    let ftyp = bx("ftyp", b"heic\0\0\0\0heic");
    // Pass 1: zero offsets to measure.
    let zero: Vec<(usize, Vec<(usize, usize, usize)>)> = items.iter().map(|item| {
        let n = item.extents.max(1);
        (item.id, (0..n).map(|_| (0usize, 0usize, 0usize)).collect())
    }).collect();
    let (children1, iloc_len, iref_len) = build_meta(&zero);
    let meta_len = 12 + children1.len() + if has_idat { 8 + idat_len } else { 0 };
    let mdat_payload_start = ftyp.len() + meta_len + 8;
    let idat_payload_start = ftyp.len() + 12 + children1.len() + 8;
    let _ = iloc_len;
    let _ = iref_len;
    // Pass 2: real stored offsets (absolute - origin - base).
    let mut mdat_at = mdat_payload_start;
    let mut idat_at = idat_payload_start;
    let mut stored: Vec<(usize, Vec<(usize, usize, usize)>)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let mut exts = Vec::new();
        for (ci, chunk) in chunks.iter().filter(|(i, _)| *i == idx) {
            let _ = ci;
            if item.method == 1 {
                exts.push((idat_at.checked_sub(idat_payload_start + spec.base).expect("base exceeds idat position"), chunk.len(), 0usize));
                idat_at += chunk.len();
            } else {
                exts.push((mdat_at.checked_sub(spec.base).expect("base exceeds mdat position"), chunk.len(), 0usize));
                mdat_at += chunk.len();
            }
        }
        stored.push((item.id, exts));
    }
    let (children, _, _) = build_meta(&stored);
    let mut meta_children = children;
    let mut idat_body: Vec<u8> = Vec::new();
    if has_idat {
        for (idx, item) in items.iter().enumerate() {
            if item.method == 1 {
                for (_, chunk) in chunks.iter().filter(|(i, _)| *i == idx) {
                    idat_body.extend(chunk);
                }
            }
        }
        meta_children.extend(bx("idat", &idat_body));
    }
    let meta = full("meta", 0, &meta_children);
    debug_assert_eq!(meta.len(), meta_len);
    let mut mdat_body: Vec<u8> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        if item.method == 0 {
            for (_, chunk) in chunks.iter().filter(|(i, _)| *i == idx) {
                mdat_body.extend(chunk);
            }
        }
    }
    debug_assert_eq!(mdat_body.len(), mdat_len);
    let mut out = ftyp;
    out.extend(meta);
    if mdat_len > 0 || !has_idat {
        out.extend(bx("mdat", &mdat_body));
    }
    out
}

/// Samsung SEF tail for the mpvd form (mirrors `heifwrite::samsung_sef`):
/// MotionPhoto_Data record with an mpv2 pointer, then the SEFH
/// directory and the size + SEFT footer. `video_start` is the absolute
/// file offset of the video (still length + mpvd header).
pub fn sef_tail_mpv2(video_start: usize, video_len: usize) -> Vec<u8> {
    let mut record = vec![0u8, 0, 0x30, 0x0a];
    record.extend((16u32).to_le_bytes());
    record.extend(b"MotionPhoto_Data");
    record.extend(b"mpv2");
    record.extend((video_start as u32).to_be_bytes());
    record.extend((video_len as u32).to_be_bytes());
    let mut directory = b"SEFH".to_vec();
    directory.extend((107u32).to_le_bytes());
    directory.extend((1u32).to_le_bytes());
    directory.extend([0u8, 0, 0x30, 0x0a]);
    directory.extend((record.len() as u32).to_le_bytes());
    directory.extend((record.len() as u32).to_le_bytes());
    let mut out = record;
    out.extend(&directory);
    out.extend((directory.len() as u32).to_le_bytes());
    out.extend(b"SEFT");
    out
}

/// Samsung sefd box (S20 form, verified against the MIT sample in
/// `tests/SOURCES.md`): extra SEF records, then the MotionPhoto_Data
/// record whose payload IS the MP4 (no mpv2 indirection), then the
/// SEFH directory and footer. The whole box runs to EOF.
pub fn sefd_box(video: &[u8]) -> Vec<u8> {
    fn record(tag: [u8; 4], name: &[u8], value: &[u8]) -> Vec<u8> {
        let mut r = tag.to_vec();
        r.extend((name.len() as u32).to_le_bytes());
        r.extend(name);
        r.extend(value);
        r
    }
    let r1 = record([0, 0, 0x01, 0x0a], b"Image_UTC_Data", b"12345678901\0");
    let r2 = record([0, 0, 0xa1, 0x0a], b"MCC_Data", b"1\0\0");
    let mut r3 = vec![0u8, 0, 0x30, 0x0a];
    r3.extend((16u32).to_le_bytes());
    r3.extend(b"MotionPhoto_Data");
    r3.extend(video);
    let mut directory = b"SEFH".to_vec();
    directory.extend((106u32).to_le_bytes());
    directory.extend((3u32).to_le_bytes());
    // Entries must appear in record order; back = head - record_start.
    // Directory length is fixed (12 + 3*12 = 48), so record starts are
    // known once the payload prefix length is known: sefd payload =
    // r1 + r2 + r3 + directory + footer(8).
    let dir_len = 48usize;
    let footer_len = 8usize;
    let payload_len = r1.len() + r2.len() + r3.len() + dir_len + footer_len;
    // back values are measured from the directory head backwards.
    let mut backs = Vec::new();
    let mut at = 0usize;
    for r in [&r1, &r2, &r3] {
        // record start relative to payload start; directory head is at
        // payload_len - dir_len - footer_len.
        let dir_head = payload_len - dir_len - footer_len;
        backs.push(dir_head - at);
        at += r.len();
    }
    let tags = [[0u8, 0, 0x01, 0x0a], [0u8, 0, 0xa1, 0x0a], [0u8, 0, 0x30, 0x0a]];
    let lens = [r1.len(), r2.len(), r3.len()];
    for i in 0..3 {
        directory.extend(tags[i]);
        directory.extend((backs[i] as u32).to_le_bytes());
        directory.extend((lens[i] as u32).to_le_bytes());
    }
    let mut payload = r1;
    payload.extend(r2);
    payload.extend(r3);
    payload.extend(&directory);
    payload.extend((directory.len() as u32).to_le_bytes());
    payload.extend(b"SEFT");
    bx("sefd", &payload)
}

pub fn build_still(with_mime: bool, with_iref: bool) -> Vec<u8> {
    let hvc1 = b"fake-hvc1-coding-data-0123456789".to_vec();
    let exif = exif_item();
    let mut items: Vec<(u16, &str, Vec<u8>)> = vec![(1, "hvc1", hvc1), (2, "Exif", exif)];
    if with_mime {
        items.push((3, "mime", plain_xmp()));
    }
    let mut hdlr_body = vec![0u8; 4];
    hdlr_body.extend(b"pict");
    hdlr_body.extend([0u8; 4]);
    let hdlr = full("hdlr", 0, &hdlr_body);
    let pitm_body = u16b(1).to_vec();
    let pitm = full("pitm", 0, &pitm_body);
    let mut infe_boxes = Vec::new();
    for (id, ty, _) in &items {
        let mut body = u16b(*id).to_vec();
        body.extend([0u8, 0]);
        body.extend(ty.as_bytes());
        if *ty == "mime" {
            body.extend(b"\0application/rdf+xml\0");
        } else {
            body.push(0);
        }
        infe_boxes.push(full("infe", 2, &body));
    }
    let mut iinf_body = u16b(items.len() as u16).to_vec();
    for b in &infe_boxes {
        iinf_body.extend(b);
    }
    let iinf = full("iinf", 0, &iinf_body);
    // Two-pass iloc: build meta once with zero offsets to measure, then
    // rebuild with real absolute offsets (entry size is constant).
    let build_meta = |offsets: &[(usize, usize)]| -> Vec<u8> {
        let mut iloc_body = vec![0x44u8, 0x00];
        iloc_body.extend(u16b(items.len() as u16));
        for (i, (id, _, _)) in items.iter().enumerate() {
            let (off, len) = offsets[i];
            iloc_body.extend(u16b(*id));
            iloc_body.extend(u16b(0));
            iloc_body.extend(u16b(1));
            iloc_body.extend(u32b(off as u32));
            iloc_body.extend(u32b(len as u32));
        }
        let iloc = full("iloc", 0, &iloc_body);
        let mut children = hdlr.clone();
        children.extend(&pitm);
        children.extend(&iinf);
        children.extend(&iloc);
        if with_iref {
            let mut cdsc_body = u16b(3).to_vec();
            cdsc_body.extend(u16b(1));
            cdsc_body.extend(u16b(1));
            let cdsc = bx("cdsc", &cdsc_body);
            children.extend(full("iref", 0, &cdsc));
        }
        // `full` adds the version/flags word; children follow directly.
        full("meta", 0, &children)
    };
    let ftyp = bx("ftyp", b"heic\0\0\0\0heic");
    let zero: Vec<(usize, usize)> = items.iter().map(|_| (0, 0)).collect();
    let meta_len = build_meta(&zero).len();
    let mdat_start = ftyp.len() + meta_len + 8;
    let mut offsets = Vec::new();
    let mut at = mdat_start;
    for (_, _, bytes) in &items {
        offsets.push((at, bytes.len()));
        at += bytes.len();
    }
    let meta = build_meta(&offsets);
    let mut mdat_body = Vec::new();
    for (_, _, bytes) in &items {
        mdat_body.extend(bytes);
    }
    let mut out = ftyp;
    out.extend(meta);
    out.extend(bx("mdat", &mdat_body));
    out
}
