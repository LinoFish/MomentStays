//! Synthetic protocol/transaction tests; no upstream photo or converter code.
mod engine_support;
use engine_support::Dir;
use momentstays_core::{
    engine::{
        self, native::Native, options::ConversionOptions, output, static_hdr, NoNative, StopToken,
    },
    heif, mp4, oppo_uhdr,
};
use serde_json::json;

fn bx(ty: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend(ty);
    out.extend(payload);
    out
}
fn full(ty: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![version];
    out.extend(&flags.to_be_bytes()[1..]);
    out.extend(payload);
    bx(ty, &out)
}

fn heic(width: u32, height: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = bx(b"ftyp", b"heic\0\0\0\0heicmif1");
    let offset = out.len() + 8;
    out.extend(bx(b"mdat", payload));
    let mut handler = vec![0; 8];
    handler.extend(b"pict");
    handler.extend([0; 4]);
    let mut meta = bx(b"hdlr", &handler);
    meta.extend(full(b"pitm", 0, 0, &1u16.to_be_bytes()));
    let mut info = 1u16.to_be_bytes().to_vec();
    info.extend(full(b"infe", 2, 0, b"\0\x01\0\0hvc1camera\0"));
    meta.extend(full(b"iinf", 0, 0, &info));
    let mut loc = vec![0x44, 0];
    loc.extend(1u16.to_be_bytes());
    loc.extend(1u16.to_be_bytes());
    loc.extend([0, 0, 0, 1]);
    loc.extend((offset as u32).to_be_bytes());
    loc.extend((payload.len() as u32).to_be_bytes());
    meta.extend(full(b"iloc", 0, 0, &loc));
    let mut size = width.to_be_bytes().to_vec();
    size.extend(height.to_be_bytes());
    let mut props = full(b"ispe", 0, 0, &size);
    props.extend(bx(b"colr", b"nclx\0\x0c\0\x0d\0\0\x80"));
    let mut iprp = bx(b"ipco", &props);
    iprp.extend(full(b"ipma", 0, 0, &[0, 0, 0, 1, 0, 1, 2, 1, 2]));
    meta.extend(bx(b"iprp", &iprp));
    out.extend(full(b"meta", 0, 0, &meta));
    out
}

fn gain() -> Vec<u8> {
    // Independently generated 2x2 RGB test image, not camera media.
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode("/9j/4AAQSkZJRgABAQAASABIAAD/4QBARXhpZgAATU0AKgAAAAgAAYdpAAQAAAABAAAAGgAAAAAAAqACAAQAAAABAAAAAqADAAQAAAABAAAAAgAAAAD/7QA4UGhvdG9zaG9wIDMuMAA4QklNBAQAAAAAAAA4QklNBCUAAAAAABDUHYzZjwCyBOmACZjs+EJ+/8AAEQgAAgACAwERAAIRAQMRAf/EAB8AAAEFAQEBAQEBAAAAAAAAAAABAgMEBQYHCAkKC//EALUQAAIBAwMCBAMFBQQEAAABfQECAwAEEQUSITFBBhNRYQcicRQygZGhCCNCscEVUtHwJDNicoIJChYXGBkaJSYnKCkqNDU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6g4SFhoeIiYqSk5SVlpeYmZqio6Slpqeoqaqys7S1tre4ubrCw8TFxsfIycrS09TV1tfY2drh4uPk5ebn6Onq8fLz9PX29/j5+v/EAB8BAAMBAQEBAQEBAQEAAAAAAAABAgMEBQYHCAkKC//EALURAAIBAgQEAwQHBQQEAAECdwABAgMRBAUhMQYSQVEHYXETIjKBCBRCkaGxwQkjM1LwFWJy0QoWJDThJfEXGBkaJicoKSo1Njc4OTpDREVGR0hJSlNUVVZXWFlaY2RlZmdoaWpzdHV2d3h5eoKDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uLj5OXm5+jp6vLz9PX29/j5+v/bAEMAAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAf/bAEMBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAf/dAAQAAf/aAAwDAQACEQMRAD8A+yPA/wAEPgt/whfhD/i0Pwv/AORX8P8A/Mg+Ff8AoE2n/UJr7g+PP//Z").unwrap()
}
fn params() -> [f32; 20] {
    [
        1., 1., 1., 1., 4., 4., 4., 1., 1., 1., 0., 0., 0., 0., 0., 0., 1., 4., 4., 0.,
    ]
}
fn photo_with(
    mut values: [f32; 20],
    edit: impl FnOnce(&mut serde_json::Value),
    gain_bytes: &[u8],
) -> Vec<u8> {
    let mut d = heic(4, 4, b"original-primary");
    d.extend(bx(b"QTI ", &[0; 16]));
    let gain_at = d.len();
    d.extend(gain_bytes);
    let info_at = d.len();
    for f in &mut values {
        d.extend(f.to_le_bytes());
    }
    let start = d.len();
    let mut entries = json!([
        {"name":"local.uhdr.gainmap.data","offset":start-gain_at,"length":gain_bytes.len(),"version":1},
        {"name":"local.uhdr.gainmap.info","offset":start-info_at,"length":80,"version":1}
    ]);
    edit(&mut entries);
    let bytes = serde_json::to_vec(&entries).unwrap();
    let length = bytes.len() + 9;
    d.extend(bytes);
    d.extend(b"\0jxrs");
    d.extend((length as u32).to_le_bytes());
    d
}
fn photo() -> Vec<u8> {
    photo_with(params(), |_| {}, &gain())
}
struct Codec;
impl Native for Codec {
    fn image_decodes(&self, _: &[u8]) -> Option<bool> {
        Some(true)
    }
}

#[test]
fn eof_directory_ranges_and_narrow_parameter_profile_are_enforced() {
    let good = photo();
    let parsed = oppo_uhdr::read(&good).unwrap().unwrap();
    assert_eq!(parsed.boost, 4.0);
    assert_eq!(parsed.gain, gain());
    assert!(oppo_uhdr::read(&heic(4, 4, b"sdr")).unwrap().is_none());
    for index in [0, 3, 7, 9, 10, 13, 16, 17, 18, 19] {
        let mut f = params();
        f[index] += 0.25;
        assert!(
            oppo_uhdr::read(&photo_with(f, |_| {}, &gain())).is_err(),
            "field {index}"
        );
    }
    for value in [f32::NAN, f32::INFINITY, 0., -1., 17.] {
        let mut f = params();
        f[4] = value;
        assert!(oppo_uhdr::read(&photo_with(f, |_| {}, &gain())).is_err());
    }
    let broken = photo_with(params(), |e| e[1]["offset"] = json!(usize::MAX), &gain());
    assert!(oppo_uhdr::read(&broken).is_err());
    let overlap = photo_with(
        params(),
        |e| e[1]["offset"] = e[0]["offset"].clone(),
        &gain(),
    );
    assert!(oppo_uhdr::read(&overlap).is_err());
    let duplicate = photo_with(params(), |e| e[1]["name"] = e[0]["name"].clone(), &gain());
    assert!(oppo_uhdr::read(&duplicate).is_err());
    let version = photo_with(params(), |e| e[1]["version"] = json!(2), &gain());
    assert!(oppo_uhdr::read(&version).is_err());
    let unknown = photo_with(params(), |e| e[1]["name"] = json!("video.mp4"), &gain());
    assert!(oppo_uhdr::read(&unknown).is_err());
    let mut extra_gain = gain();
    extra_gain.extend(gain());
    assert!(oppo_uhdr::read(&photo_with(params(), |_| {}, &extra_gain)).is_err());
    let mut length = good.clone();
    let n = length.len();
    length[n - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(oppo_uhdr::read(&length).is_err());
    // Real cameras use 64-bit mdat lengths even for small files.
    let layout = heif::layout(parsed.base).unwrap();
    let mdat = layout.boxes.iter().find(|b| b.is(b"mdat")).unwrap();
    let mut extended = good.clone();
    for item in &layout.items {
        let offset = (item.ranges[0].start + 8) as u32;
        extended[item.offset_at..item.offset_at + 4].copy_from_slice(&offset.to_be_bytes());
    }
    extended[mdat.start..mdat.start + 4].copy_from_slice(&1u32.to_be_bytes());
    extended.splice(
        mdat.payload..mdat.payload,
        ((mdat.end - mdat.start + 8) as u64).to_be_bytes(),
    );
    assert!(oppo_uhdr::read(&extended).unwrap().is_some());
    // Bounded parser must refuse arbitrary truncations without panic.
    for end in 0..good.len() {
        let _ = oppo_uhdr::read(&good[..end]);
    }
}

#[test]
fn output_keeps_original_items_and_builds_an_alternative_tone_map_graph() {
    let bytes = photo();
    let input = oppo_uhdr::read(&bytes).unwrap().unwrap();
    let result = oppo_uhdr::convert(&Codec, &input).unwrap();
    oppo_uhdr::verify_preserved(input.base, &result).unwrap();
    assert!(oppo_uhdr::read(&result).unwrap().is_none());
    let layout = heif::layout(&result).unwrap();
    assert_eq!(layout.items.len(), 4);
    assert!(!layout.items.iter().any(|i| i.ty == "jpeg"));
    assert_eq!(layout.items.iter().filter(|i| i.ty == "hvc1").count(), 2);
    let tone = layout.items.iter().find(|i| i.ty == "tmap").unwrap();
    let metadata = heif::item_bytes(&result, tone);
    assert_eq!(metadata.len(), 62);
    assert_eq!(&metadata[..6], &[0, 0, 0, 0, 0, 0x40]);
    // Expected log2 headroom = 2; independent binary read, not writer roundtrip.
    assert_eq!(
        u32::from_be_bytes(metadata[14..18].try_into().unwrap()),
        2_000_000
    );
    assert!(layout.children.iter().any(|b| b.is(b"grpl")));
    let brands = layout.boxes.iter().find(|b| b.is(b"ftyp")).unwrap();
    assert_eq!(&result[brands.payload..brands.payload + 4], b"heix");
    assert!(result[brands.payload + 8..brands.end]
        .as_chunks::<4>()
        .0
        .iter()
        .any(|b| b == b"tmap"));
    let grpl = layout.children.iter().find(|b| b.is(b"grpl")).unwrap();
    let groups = mp4::boxes(&result, grpl.payload, grpl.end).unwrap();
    let group_id = u32::from_be_bytes(
        result[groups[0].payload + 4..groups[0].payload + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    assert!(!layout.items.iter().any(|i| i.id == group_id));
    let iprp = layout.children.iter().find(|b| b.is(b"iprp")).unwrap();
    let props = mp4::boxes(&result, iprp.payload, iprp.end).unwrap();
    let ipco = props.iter().find(|b| b.is(b"ipco")).unwrap();
    let raw = mp4::boxes(&result, ipco.payload, ipco.end).unwrap();
    let ipma = props.iter().find(|b| b.is(b"ipma")).unwrap();
    let mut at = ipma.payload + 8;
    let mut hdr_props = vec![];
    while at < ipma.end {
        let id = u32::from_be_bytes(result[at..at + 4].try_into().unwrap()) as usize;
        let n = result[at + 4] as usize;
        at += 5;
        for _ in 0..n {
            let index =
                (u16::from_be_bytes(result[at..at + 2].try_into().unwrap()) & 0x7fff) as usize;
            if id == tone.id {
                hdr_props.push(&raw[index - 1]);
            }
            at += 2;
        }
    }
    let pixi = hdr_props.iter().find(|b| b.is(b"pixi")).unwrap();
    assert_eq!(&result[pixi.payload + 4..pixi.end], &[3, 10, 10, 10]);
    let colr = hdr_props.iter().find(|b| b.is(b"colr")).unwrap();
    assert_eq!(
        &result[colr.payload..colr.end],
        b"nclx\0\x09\0\x12\0\x09\x80"
    );
    let refs = layout.children.iter().find(|b| b.is(b"iref")).unwrap();
    let dimg = mp4::boxes(&result, refs.payload + 4, refs.end).unwrap();
    assert_eq!(dimg.len(), 2);
    assert!(dimg[0].is(b"dimg"));
    assert_eq!(
        u16::from_be_bytes(
            result[dimg[1].payload + 4..dimg[1].payload + 6]
                .try_into()
                .unwrap()
        ),
        2
    );
    let original = layout.items.iter().find(|i| i.id == 1).unwrap();
    let mut tampered = result.clone();
    tampered[original.ranges[0].start] ^= 1;
    assert!(oppo_uhdr::verify_preserved(input.base, &tampered).is_err());
    assert!(oppo_uhdr::convert(&NoNative, &input).is_err());
}

#[test]
fn only_apple_direction_can_publish_and_resume_experimental_uhdr() {
    let dir = Dir::new("oppo-uhdr");
    let path = dir.0.join("photo.heic");
    let bytes = photo();
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        static_hdr::inspect(&NoNative, &bytes).unwrap(),
        Some("OPPO UHDR（实验）")
    );
    let mut options = ConversionOptions {
        direction: "to-android".into(),
        ..ConversionOptions::default()
    };
    let target = dir.0.join("android");
    assert!(static_hdr::convert(
        &Codec,
        &path,
        &target,
        &options,
        &StopToken::new(),
        false,
        &Default::default()
    )
    .is_err());
    assert!(!target.exists());
    options.direction = "to-apple".into();
    options.legacy_hdr = false;
    let target = dir.0.join("apple");
    let disabled = static_hdr::convert(
        &Codec,
        &path,
        &target,
        &options,
        &StopToken::new(),
        false,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(disabled["status"], "skipped");
    assert!(!target.exists());
    options.legacy_hdr = true;
    for expected in ["converted", "resumed"] {
        let r = static_hdr::convert(
            &Codec,
            &path,
            &target,
            &options,
            &StopToken::new(),
            false,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(r["status"], expected);
        assert_eq!(r["hdr"]["profile"], oppo_uhdr::PROFILE);
        output::verify(&target).unwrap();
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let row = engine::scanrow::scan_file(&Codec, &path, "photo.heic", None);
    assert!(row.is_static_hdr());
}

#[test]
#[ignore = "requires separately licensed local OPPO fixtures and Apple ImageIO"]
fn real_uhdr_preserves_camera_items_and_decodes_on_apple() {
    let fixture_dir = std::path::PathBuf::from(
        std::env::var_os("OPPO_UHDR_FIXTURE_DIR").expect("OPPO_UHDR_FIXTURE_DIR"),
    );
    let dir = Dir::new("oppo-real");
    for name in ["uhdr-master-v1-01.heic", "uhdr-master-v2-01.heic"] {
        let path = fixture_dir.join(name);
        let bytes = std::fs::read(&path).unwrap();
        let hash = momentstays_core::sha256_hex(&bytes);
        let source = oppo_uhdr::read(&bytes).unwrap().unwrap();
        let opts = ConversionOptions {
            direction: "to-apple".into(),
            legacy_hdr: true,
            ..Default::default()
        };
        let destination = dir.0.join(name);
        let sources = [path.clone()];
        let outcome = engine::batch::run(
            &momentstays_core::platform::SystemNative,
            &engine::batch::RunRequest {
                sources: &sources,
                output: &destination,
                options: &opts,
                scan_rows: None,
                concurrency: Some(1),
            },
            &StopToken::new(),
            None,
            |_| {},
        )
        .unwrap();
        assert_eq!(outcome.counts["converted"], 1, "{}", outcome.report_data);
        assert_eq!(outcome.counts["failed"], 0);
        let result = &outcome.report_data["files"][0];
        let target = std::path::PathBuf::from(result["output"].as_str().unwrap());
        assert_eq!(result["hdr"]["status"], "converted-experimental");
        let file = result["output_files"]["static"].as_str().unwrap();
        let out = std::fs::read(target.join(file)).unwrap();
        oppo_uhdr::verify_preserved(source.base, &out).unwrap();
        let layout = heif::layout(&out).unwrap();
        assert!(!layout.items.iter().any(|i| i.ty == "jpeg"));
        assert_eq!(result["hdr"]["gain_encoding"], "hevc-444-pcm");
        if let Some(output_dir) = std::env::var_os("OPPO_UHDR_OUTPUT_DIR") {
            let output_dir = std::path::PathBuf::from(output_dir);
            std::fs::create_dir_all(&output_dir).unwrap();
            std::fs::write(output_dir.join(name), &out).unwrap();
        }
        let report = momentstays_core::apple_hdr::read(&out).unwrap();
        assert!(report.heic.unwrap().iso_tmap);
        assert_eq!(
            momentstays_core::platform::SystemNative.image_decodes(&out),
            Some(true)
        );
        assert_eq!(
            momentstays_core::sha256_hex(&std::fs::read(&path).unwrap()),
            hash
        );
        output::verify(&target).unwrap();
        eprintln!(
            "{name}: source={} output={}, camera items retained; HDR display not tested",
            bytes.len(),
            out.len()
        );
    }
}
