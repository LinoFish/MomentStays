//! One XML parser for reading and writing XMP (`pyxml`). These pin the
//! cases where the former roxmltree reader and the writer disagreed, the
//! XMP property forms real files use, and the nesting cap.

use momentstays_core::{format, pyxml, scan, xmp};

fn jpeg(xml: &str) -> Vec<u8> {
    let mut p = xmp::HEADER.to_vec();
    p.extend_from_slice(xml.as_bytes());
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE1];
    v.extend(((p.len() + 2) as u16).to_be_bytes());
    v.extend(p);
    v.extend([0xFF, 0xDA, 0, 2, 1, 2, 3, 0xFF, 0xD9]);
    v
}

const NS: &str = r#"xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:GCamera="http://ns.google.com/photos/1.0/camera/" xmlns:Container="http://ns.google.com/photos/1.0/container/" xmlns:Item="http://ns.google.com/photos/1.0/container/item/" xmlns:dc="http://purl.org/dc/elements/1.1/""#;

fn doc(inner: &str) -> String {
    format!(
        r#"<x:xmpmeta {NS}><rdf:RDF><rdf:Description rdf:about="" {inner}</rdf:RDF></x:xmpmeta>"#
    )
}

fn motion_photo(xml: &str) -> Option<i64> {
    let packets = xmp::packets(&jpeg(xml)).expect("packets");
    xmp::integer(&packets, "MotionPhoto").expect("integer")
}

#[test]
fn attribute_element_and_cdata_forms_read_the_same() {
    assert_eq!(motion_photo(&doc(r#"GCamera:MotionPhoto="1"/>"#)), Some(1));
    assert_eq!(
        motion_photo(&doc(
            "><GCamera:MotionPhoto> 1 </GCamera:MotionPhoto></rdf:Description>"
        )),
        Some(1)
    );
    assert_eq!(
        motion_photo(&doc(
            "><GCamera:MotionPhoto><![CDATA[1]]></GCamera:MotionPhoto></rdf:Description>"
        )),
        Some(1)
    );
}

#[test]
fn directory_items_in_element_form_are_found() {
    let xml = doc(
        r#"GCamera:MotionPhoto="1"><Container:Directory><rdf:Seq><rdf:li><Container:Item><Item:Semantic>MotionPhoto</Item:Semantic><Item:Length>55</Item:Length></Container:Item></rdf:li></rdf:Seq></Container:Directory></rdf:Description>"#,
    );
    let packets = xmp::packets(&jpeg(&xml)).unwrap();
    let items: Vec<_> = packets[0]
        .items()
        .iter()
        .filter(|i| i.0.is_some())
        .cloned()
        .collect();
    assert_eq!(
        items,
        vec![(Some("MotionPhoto".to_string()), Some("55".to_string()))]
    );
}

#[test]
fn xml_lang_entities_padding_and_nul_are_accepted() {
    let xml = format!(
        "{}{}\0\0",
        doc(
            r#"GCamera:MotionPhoto="1"><dc:title><rdf:Alt><rdf:li xml:lang="x-default">a &amp; &#x4E2D;</rdf:li></rdf:Alt></dc:title></rdf:Description>"#
        ),
        " ".repeat(2048)
    );
    assert_eq!(motion_photo(&xml), Some(1));
}

#[test]
fn dtd_entities_are_rejected_by_reader_like_the_writer() {
    // roxmltree expanded `&one;`, so a file could scan as a Motion Photo and
    // then fail to convert. Now reading fails the same way writing does.
    let xml = format!(
        "<!DOCTYPE x [<!ENTITY one \"1\">]>{}",
        doc(r#"GCamera:MotionPhoto="&one;"/>"#)
    );
    let err = xmp::packets(&jpeg(&xml)).unwrap_err();
    assert!(err.0.contains("undefined entity"), "{}", err.0);
    assert!(pyxml::parse(xml.as_bytes()).is_err());
}

fn nested(depth: usize) -> String {
    let mut x = String::from(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">"#);
    x.push_str(&"<a>".repeat(depth));
    x.push_str(&"</a>".repeat(depth));
    x.push_str("</x:xmpmeta>");
    x
}

#[test]
fn deep_nesting_is_an_error_not_a_stack_overflow() {
    // A few thousand nested tags used to overflow the stack and abort the
    // process (a whole folder scan included). Parsing costs ~1.2 KB of stack
    // per level: ~320 KB at the cap, ~10 MB for the 9,000 levels below.
    // A 1 MB thread (half of Rust's default) fits the cap and would crash
    // if it regressed.
    let handle = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let ok = jpeg(&nested(pyxml::MAX_DEPTH - 1));
            assert!(xmp::packets(&ok).is_ok());
            let deep = jpeg(&nested(9_000)); // fits one 64 KiB APP1
            let err = format::parse(&deep).unwrap_err();
            assert!(err.0.contains("nested too deeply"), "{}", err.0);
            let row = scan::scan_row(&deep);
            assert!(!row.is_motion);
        })
        .unwrap();
    handle.join().expect("no stack overflow");
}

#[test]
fn xml_illegal_characters_are_rejected_like_the_old_reader_and_python() {
    // Control characters (NUL, ESC, ...) are not XML 1.0 characters. The
    // parser the app shipped with rejected them, as does Python's; accepting
    // them would let a damaged packet through as a clean one.
    for bad in ["\u{0}", "\u{1b}", "\u{b}", "\u{1f}", "\u{FFFE}", "\u{FFFF}"] {
        let xml = doc(&format!(r#"GCamera:MotionPhoto="1" dc:x="a{bad}b"/>"#));
        let err = xmp::packets(&jpeg(&xml)).unwrap_err();
        assert!(err.0.contains("invalid character"), "{bad:?}: {}", err.0);
        // Also in text content and in comments.
        let xml = doc(&format!(
            "><GCamera:MotionPhoto>1{bad}</GCamera:MotionPhoto></rdf:Description>"
        ));
        assert!(xmp::packets(&jpeg(&xml)).is_err(), "{bad:?} in text");
    }
    // ...and through character references.
    for r in [
        "&#0;",
        "&#1;",
        "&#x1B;",
        "&#xFFFE;",
        "&#xD800;",
        "&#x110000;",
    ] {
        let xml = doc(&format!(r#"GCamera:MotionPhoto="1" dc:x="{r}"/>"#));
        assert!(xmp::packets(&jpeg(&xml)).is_err(), "{r}");
    }
}

#[test]
fn tab_newline_cr_and_unicode_remain_valid() {
    let xml = doc("GCamera:MotionPhoto=\"1\" dc:x=\"a\tb\nc\r\nd 中 😀 &#9;&#xA;&#x1F600;\"/>");
    assert_eq!(motion_photo(&xml), Some(1));
}
