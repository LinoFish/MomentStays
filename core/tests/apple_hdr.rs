//! Apple HDR reading (`apple_hdr`): MakerNote scalars in every number type,
//! the legacy JPEG gain map, and HEIC structure (auxC / ipma / auxl / tmap /
//! 10-bit / PQ / HLG). The HEIC files here are synthetic: they carry the
//! boxes the reader looks at, with made-up payload bytes.

mod common;

use base64::Engine;
use common::{bx, full};
use momentstays_core::apple_hdr::{self, Status, GAIN_MAP_TYPE};
use momentstays_core::{jpeg, pyjpeg};
use std::collections::HashMap;

fn fixture(name: &str) -> Vec<u8> {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures-pycli")
        .join(format!("{name}.b64"));
    base64::engine::general_purpose::STANDARD
        .decode(std::fs::read_to_string(p).unwrap().trim_ascii())
        .unwrap()
}

// ---------------------------------------------------------------------------
// MakerNote / Exif builders
// ---------------------------------------------------------------------------

/// One MakerNote entry: tag, TIFF type, count, and how the value is stored.
enum Val {
    /// Value bytes live in the 4-byte field (already in note byte order).
    Inline([u8; 4]),
    /// Value bytes live after the entry table; the field holds their offset.
    Out(Vec<u8>),
    /// The field holds exactly this offset (to test bounds).
    At(u32),
}

/// A MakerNote entry: tag, TIFF type, count, value.
type Entry = (u16, u16, u32, Val);

fn u16b(little: bool, v: u16) -> [u8; 2] {
    if little {
        v.to_le_bytes()
    } else {
        v.to_be_bytes()
    }
}
fn u32b(little: bool, v: u32) -> [u8; 4] {
    if little {
        v.to_le_bytes()
    } else {
        v.to_be_bytes()
    }
}

fn note(little: bool, entries: &[Entry]) -> Vec<u8> {
    let mut out = b"Apple iOS\0\0\x01".to_vec();
    out.extend(if little { b"II" } else { b"MM" });
    out.extend(u16b(little, entries.len() as u16));
    let table_end = 16 + entries.len() * 12 + 4;
    let mut data: Vec<u8> = Vec::new();
    for (tag, ty, count, val) in entries {
        out.extend(u16b(little, *tag));
        out.extend(u16b(little, *ty));
        out.extend(u32b(little, *count));
        match val {
            Val::Inline(b) => out.extend(b),
            Val::Out(bytes) => {
                out.extend(u32b(little, (table_end + data.len()) as u32));
                data.extend(bytes);
            }
            Val::At(o) => out.extend(u32b(little, *o)),
        }
    }
    out.extend([0u8; 4]);
    out.extend(data);
    out
}

fn float(little: bool, v: f32) -> Val {
    Val::Inline(if little {
        v.to_le_bytes()
    } else {
        v.to_be_bytes()
    })
}
fn rational(little: bool, num: u32, den: u32) -> Val {
    Val::Out([u32b(little, num), u32b(little, den)].concat())
}
fn double(little: bool, v: f64) -> Val {
    Val::Out(if little {
        v.to_le_bytes().to_vec()
    } else {
        v.to_be_bytes().to_vec()
    })
}

/// A TIFF block: IFD0 -> Exif IFD -> MakerNote.
fn tiff_with_note(note: &[u8]) -> Vec<u8> {
    let mut t = b"MM\0*\0\0\0\x08".to_vec();
    // IFD0 at 8: one entry (ExifIFD pointer -> 26).
    t.extend([0, 1, 0x87, 0x69, 0, 4, 0, 0, 0, 1, 0, 0, 0, 26, 0, 0, 0, 0]);
    // Exif IFD at 26: one entry (MakerNote, type 7, count, offset 44).
    t.extend([0, 1, 0x92, 0x7C, 0, 7]);
    t.extend((note.len() as u32).to_be_bytes());
    t.extend(44u32.to_be_bytes());
    t.extend([0, 0, 0, 0]);
    t.extend(note);
    t
}

/// `legacy-mpf.jpg` with its Exif segment replaced (MPF offsets repaired).
fn legacy_jpeg_with_note(note: &[u8]) -> Vec<u8> {
    let d = fixture("legacy-mpf.jpg");
    let segs = jpeg::segments(&d).unwrap();
    let exif = segs
        .iter()
        .find(|s| s.marker == 0xE1 && s.payload(&d).starts_with(jpeg::EXIF))
        .expect("fixture has an Exif segment");
    let mut payload = jpeg::EXIF.to_vec();
    payload.extend(tiff_with_note(note));
    let mut replace = HashMap::new();
    replace.insert(exif.start, pyjpeg::app(0xE1, &payload).unwrap());
    pyjpeg::edit_header(&d, &replace, &[]).unwrap()
}

const A: f32 = 0.8; // tag 33 < 1
const B: f32 = 0.1; // tag 48 -> stops = -0.101 * 0.1 + 1.601 = 1.5909

fn expect_stops() -> f64 {
    // The same f32 -> f64 widening the reader does.
    let (a, b) = (A as f64, B as f64);
    assert!(a < 1.0 && b > 0.01);
    -0.101 * b + 1.601
}

// ---------------------------------------------------------------------------
// stops model
// ---------------------------------------------------------------------------

#[test]
fn headroom_model_branches_and_limits() {
    let cases: &[(f64, f64, Option<f64>)] = &[
        (0.5, 0.0, Some(1.8)),  // a < 1, b <= 0.01
        (0.5, 0.01, Some(1.6)), // boundary: still the first piece
        (0.5, 1.0, Some(1.5)),  // a < 1, b > 0.01
        (1.0, 0.0, Some(3.0)),  // a >= 1, b <= 0.01
        (2.0, 1.0, Some(2.0)),  // a >= 1, b > 0.01
        (0.5, 100.0, None),     // negative result clamps to 0: unusable
        (1.0, -5.0, None),      // -70 * -5 + 3 = 353 stops: out of range
        (f64::NAN, 0.1, None),  // not finite
        (0.5, f64::INFINITY, None),
    ];
    for (a, b, want) in cases {
        let got = apple_hdr::stops_from(*a, *b);
        match (got, want) {
            (Some(g), Some(w)) => assert!((g - w).abs() < 1e-9, "{a},{b}: {g} != {w}"),
            (g, w) => assert_eq!(g, *w, "{a},{b}"),
        }
    }
}

// ---------------------------------------------------------------------------
// JPEG
// ---------------------------------------------------------------------------

#[test]
fn legacy_fixture_is_read_with_swift_reasons() {
    let d = fixture("legacy-mpf.jpg");
    let r = apple_hdr::read(&d).unwrap();
    assert_eq!(r.container, "jpeg");
    assert_eq!(r.status, Status::Legacy);
    assert_eq!(r.reason, "检测到可恢复的旧 Apple HDR");
    assert!((r.stops.unwrap() - 1.5909).abs() < 1e-9);
    assert_eq!(r.stops, r.maker.stops);
    assert!(r.maker.present && r.maker.has_33 && r.maker.has_48);
    assert_eq!(
        r.gain.as_deref().unwrap(),
        fixture("expected/legacy-gain.bin")
    );
    // JSON shape the Swift side decodes.
    let j = r.to_json();
    assert_eq!(j["status"], "legacy");
    assert_eq!(j["container"], "jpeg");
    assert_eq!(j["maker"]["has_33"], true);
}

#[test]
fn files_without_a_legacy_map_say_why() {
    let cases = [
        ("static.jpg", "没有可恢复的 HDR 增益数据；SDR 保持原样"),
        ("motion.jpg", "没有可恢复的 HDR 增益数据；SDR 保持原样"),
        (
            "ultra.jpg",
            "已有辅助图，但缺少可验证的 HDR 亮度参数；原样保留，不推测增益",
        ),
    ];
    for (name, reason) in cases {
        let r = apple_hdr::read(&fixture(name)).unwrap();
        assert_eq!(r.status, Status::None, "{name}");
        assert_eq!(r.reason, reason, "{name}");
        assert!(r.gain.is_none() && r.stops.is_none(), "{name}");
    }
}

#[test]
fn every_number_type_and_byte_order_gives_the_same_stops() {
    for little in [false, true] {
        let variants: Vec<(&str, Vec<Entry>)> = vec![
            (
                "float",
                vec![(33, 11, 1, float(little, A)), (48, 11, 1, float(little, B))],
            ),
            (
                "rational",
                vec![
                    (33, 5, 1, rational(little, 4, 5)),
                    (48, 5, 1, rational(little, 1, 10)),
                ],
            ),
            (
                "srational",
                vec![
                    (33, 10, 1, rational(little, 4, 5)),
                    (48, 10, 1, rational(little, 1, 10)),
                ],
            ),
            (
                "double",
                vec![
                    (33, 12, 1, double(little, 0.8)),
                    (48, 12, 1, double(little, 0.1)),
                ],
            ),
            (
                "mixed, other tags around",
                vec![
                    (1, 4, 1, Val::Inline([0; 4])),
                    (33, 5, 1, rational(little, 8, 10)),
                    (17, 2, 4, Val::Inline(*b"id\0\0")),
                    (48, 11, 1, float(little, B)),
                ],
            ),
        ];
        for (name, entries) in variants {
            let d = legacy_jpeg_with_note(&note(little, &entries));
            let r = apple_hdr::read(&d).unwrap_or_else(|e| panic!("{name}/{little}: {e}"));
            assert_eq!(r.status, Status::Legacy, "{name}/{little}");
            let stops = r.stops.unwrap();
            // f32 values widen to slightly different f64s than exact ones.
            assert!(
                (stops - 1.5909).abs() < 1e-4,
                "{name}/{little}: {stops} vs {}",
                expect_stops()
            );
        }
    }
}

#[test]
fn unusable_scalars_keep_the_original_without_failing() {
    let incomplete = "旧增益图亮度参数不完整，已保留原始数据";
    let cases: Vec<(&str, Vec<u8>)> = vec![
        // Tag 48 missing.
        (
            "tag 48 missing",
            note(false, &[(33, 11, 1, float(false, A))]),
        ),
        // NaN / out-of-range results are "not usable", not errors.
        (
            "nan",
            note(
                false,
                &[
                    (33, 11, 1, float(false, f32::NAN)),
                    (48, 11, 1, float(false, B)),
                ],
            ),
        ),
        (
            "too many stops",
            note(
                false,
                &[
                    (33, 11, 1, float(false, 2.0)),
                    (48, 11, 1, float(false, -10.0)),
                ],
            ),
        ),
        // A count other than 1 is ignored.
        (
            "count 2",
            note(
                false,
                &[(33, 11, 2, float(false, A)), (48, 11, 1, float(false, B))],
            ),
        ),
        // A note too short to hold a directory is skipped, not an error.
        ("short note", b"Apple iOS\0\0\x01MM".to_vec()),
    ];
    for (name, n) in cases {
        let r =
            apple_hdr::read(&legacy_jpeg_with_note(&n)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r.status, Status::None, "{name}");
        assert_eq!(r.reason, incomplete, "{name}");
        assert!(r.gain.is_none(), "{name}");
    }
}

#[test]
fn damaged_scalars_are_errors() {
    let table_end = 16 + 2 * 12 + 4;
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "zero denominator",
            note(
                false,
                &[
                    (33, 5, 1, rational(false, 4, 0)),
                    (48, 11, 1, float(false, B)),
                ],
            ),
            "分母为零",
        ),
        (
            "offset inside the entry table",
            note(
                false,
                &[(33, 5, 1, Val::At(20)), (48, 11, 1, float(false, B))],
            ),
            "偏移无效",
        ),
        (
            "offset past the note",
            note(
                false,
                &[
                    (33, 5, 1, Val::At((table_end + 4000) as u32)),
                    (48, 11, 1, float(false, B)),
                ],
            ),
            "偏移无效",
        ),
        (
            "bad byte order",
            {
                let mut n = note(false, &[(33, 11, 1, float(false, A))]);
                n[12..14].copy_from_slice(b"XX");
                n
            },
            "字节序无效",
        ),
    ];
    for (name, n, want) in cases {
        let err = apple_hdr::read(&legacy_jpeg_with_note(&n)).unwrap_err();
        assert!(err.0.contains(want), "{name}: {}", err.0);
    }
}

// ---------------------------------------------------------------------------
// HEIC
// ---------------------------------------------------------------------------

/// A HEIC with: item 1 primary (hvc1), item 2 Exif, item 3 gain map (hvc1),
/// plus optional extras. Properties: 1 hvcC, 2 auxC, 3 colr.
#[derive(Clone)]
struct Heic {
    exif_note: Option<Vec<u8>>,
    /// Bit depth minus 8 in the hvcC (0 = 8-bit).
    depth: u8,
    /// colr nclx transfer characteristics, if a colr property is present.
    transfer: Option<u16>,
    /// auxC on item 3; the URN stored there.
    aux_urn: Option<&'static str>,
    /// `auxl` reference of item 3: which item it points at.
    auxl_to: Option<u16>,
    /// Extra items (id, type 4cc).
    extra_items: Vec<(u16, &'static str)>,
    with_iprp: bool,
    /// A second item with the same gain-map auxC.
    second_gain_map: bool,
}

impl Heic {
    fn plain() -> Heic {
        Heic {
            exif_note: Some(note(
                false,
                &[(33, 11, 1, float(false, A)), (48, 11, 1, float(false, B))],
            )),
            depth: 0,
            transfer: None,
            aux_urn: Some(GAIN_MAP_TYPE),
            auxl_to: Some(1),
            extra_items: vec![],
            with_iprp: true,
            second_gain_map: false,
        }
    }

    fn build(&self) -> Vec<u8> {
        let exif_item: Vec<u8> = match &self.exif_note {
            Some(n) => [vec![0, 0, 0, 0], tiff_with_note(n)].concat(),
            None => [vec![0, 0, 0, 0], b"MM\0*\0\0\0\x08\0\0\0\0\0\0".to_vec()].concat(),
        };
        let mut items: Vec<(u16, &str, Vec<u8>)> = vec![
            (1, "hvc1", b"primary-coded-data".to_vec()),
            (2, "Exif", exif_item),
            (3, "hvc1", b"gainmap-coded-data".to_vec()),
        ];
        if self.second_gain_map {
            items.push((4, "hvc1", b"second-gainmap".to_vec()));
        }
        for (id, ty) in &self.extra_items {
            items.push((*id, ty, b"extra-item-data".to_vec()));
        }
        let meta = |offsets: &[u32]| -> Vec<u8> {
            let mut infe = Vec::new();
            for (id, ty, _) in &items {
                let mut p = id.to_be_bytes().to_vec();
                p.extend([0, 0]);
                p.extend(ty.as_bytes());
                p.push(0);
                infe.extend(full("infe", 2, &p));
            }
            let iinf = full(
                "iinf",
                0,
                &[(items.len() as u16).to_be_bytes().to_vec(), infe].concat(),
            );
            let mut iloc = vec![0x44, 0x00];
            iloc.extend((items.len() as u16).to_be_bytes());
            for ((id, _, data), off) in items.iter().zip(offsets) {
                iloc.extend(id.to_be_bytes());
                iloc.extend([0, 0, 0, 1]);
                iloc.extend(off.to_be_bytes());
                iloc.extend((data.len() as u32).to_be_bytes());
            }
            let mut hdlr = vec![0u8; 4];
            hdlr.extend(b"pict");
            hdlr.extend([0u8; 13]);
            let mut children = vec![
                full("hdlr", 0, &hdlr),
                full("pitm", 0, &1u16.to_be_bytes()),
                iinf,
                full("iloc", 0, &iloc),
            ];
            if let Some(to) = self.auxl_to {
                let mut r = 3u16.to_be_bytes().to_vec();
                r.extend(1u16.to_be_bytes());
                r.extend(to.to_be_bytes());
                children.push(full("iref", 0, &bx("auxl", &r)));
            }
            if self.with_iprp {
                let mut hvcc = vec![0u8; 23];
                hvcc[17] = self.depth;
                let mut props = vec![bx("hvcC", &hvcc)];
                let mut aux = vec![0u8; 4];
                aux.extend(self.aux_urn.unwrap_or("urn:other:aux").as_bytes());
                aux.push(0);
                props.push(bx("auxC", &aux));
                if let Some(t) = self.transfer {
                    let mut colr = b"nclx".to_vec();
                    colr.extend(1u16.to_be_bytes());
                    colr.extend(t.to_be_bytes());
                    colr.extend(1u16.to_be_bytes());
                    colr.push(0);
                    props.push(bx("colr", &colr));
                }
                // ipma (version 0, 7-bit indices): item 1 -> hvcC (+colr),
                // item 3 (and 4) -> auxC.
                let mut entries: Vec<(u16, Vec<u8>)> = vec![(1, vec![1])];
                if let Some(c) = self.transfer.map(|_| 3u8) {
                    entries[0].1.push(c);
                }
                entries.push((3, vec![2]));
                if self.second_gain_map {
                    entries.push((4, vec![2]));
                }
                let mut ipma = (entries.len() as u32).to_be_bytes().to_vec();
                for (id, idx) in &entries {
                    ipma.extend(id.to_be_bytes());
                    ipma.push(idx.len() as u8);
                    ipma.extend(idx);
                }
                children.push(bx(
                    "iprp",
                    &[bx("ipco", &props.concat()), full("ipma", 0, &ipma)].concat(),
                ));
            }
            full("meta", 0, &children.concat())
        };
        let ftyp = bx("ftyp", b"heic\0\0\0\0mif1heic");
        let zero = vec![0u32; items.len()];
        let meta_len = meta(&zero).len();
        let mut offsets = Vec::new();
        let mut at = ftyp.len() + meta_len + 8;
        for (_, _, data) in &items {
            offsets.push(at as u32);
            at += data.len();
        }
        let mut out = ftyp;
        out.extend(meta(&offsets));
        out.extend(bx(
            "mdat",
            &items
                .iter()
                .flat_map(|(_, _, d)| d.clone())
                .collect::<Vec<_>>(),
        ));
        out
    }
}

#[test]
fn heic_gain_map_is_found_and_linked_to_the_primary() {
    let d = Heic::plain().build();
    let r = apple_hdr::read(&d).unwrap();
    assert_eq!(r.container, "heic");
    assert_eq!(r.status, Status::GainMap, "{}", r.reason);
    let h = r.heic.as_ref().unwrap();
    assert_eq!(h.primary_item, Some(1));
    assert_eq!(h.gain_map_item, Some(3));
    assert!(h.marker && !h.iso_tmap && !h.hdr_signal);
    assert!(r.maker.present && r.maker.has_33 && r.maker.has_48);
    assert!((r.stops.unwrap() - expect_stops()).abs() < 1e-4);
    assert!(r.gain.is_none(), "HEIC pixels are decoded by the OS");
    assert_eq!(apple_hdr::stops(&d).unwrap(), r.stops);
    let j = r.to_json();
    assert_eq!(j["heic"]["gain_map_item"], 3);
    assert_eq!(j["status"], "gainmap");
}

#[test]
fn heic_without_hdr_is_sdr() {
    let mut h = Heic::plain();
    h.aux_urn = None; // auxC with another URN
    h.auxl_to = None;
    let r = apple_hdr::read(&h.build()).unwrap();
    assert_eq!(r.status, Status::None, "{}", r.reason);
    assert_eq!(r.reason, "未检测到可恢复 HDR；不生成增益图");
    assert_eq!(r.heic.unwrap().gain_map_item, None);
}

#[test]
fn heic_hdr_signals_refuse_instead_of_assuming_sdr() {
    // 10-bit HEVC.
    let mut h = Heic::plain();
    h.aux_urn = None;
    h.depth = 2;
    let r = apple_hdr::read(&h.build()).unwrap();
    assert_eq!(r.status, Status::Refuse);
    assert!(r.heic.as_ref().unwrap().hdr_signal);
    // PQ and HLG transfer functions; sRGB (1) is fine.
    for (transfer, refuse) in [(16u16, true), (18, true), (1, false), (13, false)] {
        let mut h = Heic::plain();
        h.aux_urn = None;
        h.transfer = Some(transfer);
        let r = apple_hdr::read(&h.build()).unwrap();
        assert_eq!(r.status == Status::Refuse, refuse, "transfer {transfer}");
    }
    // An ISO tone-map item.
    let mut h = Heic::plain();
    h.aux_urn = None;
    h.extra_items = vec![(9, "tmap")];
    let r = apple_hdr::read(&h.build()).unwrap();
    assert_eq!(r.status, Status::Refuse);
    assert!(r.heic.unwrap().iso_tmap);
    // No property boxes to inspect at all: conservative.
    let mut h = Heic::plain();
    h.with_iprp = false;
    let r = apple_hdr::read(&h.build()).unwrap();
    assert_eq!(r.status, Status::Refuse);
}

#[test]
fn heic_gain_map_of_another_image_is_not_the_primarys() {
    let mut h = Heic::plain();
    h.auxl_to = Some(2); // auxl points at the Exif item, not the primary
    let r = apple_hdr::read(&h.build()).unwrap();
    assert_eq!(r.heic.as_ref().unwrap().gain_map_item, None);
    // The marker is still in the file, so the original is kept, not guessed.
    assert_eq!(r.status, Status::Refuse);
}

#[test]
fn heic_with_two_gain_maps_is_not_guessed() {
    let mut h = Heic::plain();
    h.second_gain_map = true;
    let r = apple_hdr::read(&h.build()).unwrap();
    assert_eq!(r.heic.as_ref().unwrap().gain_map_item, None);
    assert_eq!(r.status, Status::Refuse);
}

#[test]
fn heic_with_unknown_items_can_still_be_read() {
    // `heif::layout` (the writer's view) refuses unknown item types; reading
    // Apple HDR must not, or a camera file with extra items loses its map.
    let mut h = Heic::plain();
    h.extra_items = vec![(7, "av01"), (8, "uri ")];
    let d = h.build();
    assert!(momentstays_core::heif::layout(&d).is_err());
    let r = apple_hdr::read(&d).unwrap();
    assert_eq!(r.status, Status::GainMap);
}

#[test]
fn heic_without_exif_note_has_no_stops() {
    let mut h = Heic::plain();
    h.exif_note = None;
    let r = apple_hdr::read(&h.build()).unwrap();
    assert_eq!(r.status, Status::GainMap);
    assert_eq!(r.stops, None);
    assert!(!r.maker.present);
}

#[test]
fn truncated_property_boxes_are_errors_not_panics() {
    let d = Heic::plain().build();
    for cut in (0..d.len()).step_by(7) {
        let _ = apple_hdr::read(&d[..cut]); // must not panic
    }
    // Flip bytes inside the meta box.
    for i in 0..d.len().min(400) {
        let mut m = d.clone();
        m[i] ^= 0xFF;
        let _ = apple_hdr::read(&m);
    }
}
