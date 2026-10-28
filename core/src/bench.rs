//! Synthetic workloads for `momentstays-core bench` (ROADMAP P1).
use crate::jpeg::app;
use crate::xmp::HEADER;

fn noise(n: usize, seed: &mut u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(n + n / 200);
    while v.len() < n {
        *seed ^= *seed << 13; *seed ^= *seed >> 7; *seed ^= *seed << 17;
        for b in seed.to_le_bytes() {
            v.push(b);
            if b == 0xff { v.push(0); } // entropy stuffing, like a real scan
        }
    }
    v
}

/// Google Motion Photo with a `still_mb` scan and a `video_mb` mdat.
pub fn google_motion_photo(still_mb: usize, video_mb: usize) -> Vec<u8> {
    let mut seed = 0x9E3779B97F4A7C15u64;
    let mut video = Vec::new();
    let mbox = |t: &[u8], p: &[u8]| { let mut v = ((p.len() + 8) as u32).to_be_bytes().to_vec(); v.extend(t); v.extend(p); v };
    video.extend(mbox(b"ftyp", b"isom\0\0\0\0isom"));
    video.extend(mbox(b"moov", &[0; 64]));
    video.extend(mbox(b"mdat", &noise(video_mb << 20, &mut seed)));
    let xml = format!(concat!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#,
        r#"<rdf:Description xmlns:GCamera="http://ns.google.com/photos/1.0/camera/" xmlns:Container="http://ns.google.com/photos/1.0/container/" xmlns:Item="http://ns.google.com/photos/1.0/container/item/" "#,
        r#"GCamera:MotionPhoto="1" GCamera:MotionPhotoPresentationTimestampUs="1420000"><Container:Directory><rdf:Seq>"#,
        r#"<rdf:li><Container:Item Item:Semantic="Primary" Item:Length="0"/></rdf:li>"#,
        r#"<rdf:li><Container:Item Item:Semantic="MotionPhoto" Item:Length="{}"/></rdf:li>"#,
        r#"</rdf:Seq></Container:Directory></rdf:Description></rdf:RDF></x:xmpmeta>"#), video.len());
    let mut x = HEADER.to_vec();
    x.extend(xml.as_bytes());
    let mut d = vec![0xff, 0xd8];
    d.extend(app(0xe1, &x).unwrap());
    d.extend(app(0xdb, &[0; 65]).unwrap());
    d.extend(app(0xc0, b"\x08\x10\0\x10\0\x01\x01\x11\0").unwrap());
    d.extend(app(0xda, b"\x01\x01\0\0\0\0").unwrap());
    d.extend(noise(still_mb << 20, &mut seed));
    d.extend([0xff, 0xd9]);
    d.extend(video);
    d
}
