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

pub fn plain_xmp() -> Vec<u8> {
    b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\"/></rdf:RDF></x:xmpmeta>".to_vec()
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
