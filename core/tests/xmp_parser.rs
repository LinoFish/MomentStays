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
    // Keep this gate in debug and release builds: both must accept the cap
    // and reject a 9,000-level packet on a 1 MiB host worker stack.
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

#[test]
fn qualified_names_follow_the_namespaces_rules() {
    // `rdf:2escription` (a digit after the prefix), `a:b:c`, `:a`, `a:` are not
    // XML names; expat, the old reader and Python all reject them. Accepting
    // them let a damaged element name through as a clean one.
    for bad in ["rdf:2escription", "rdf:-x", "rdf:.x", "a:b:c", ":a", "rdf:"] {
        let xml =
            format!(r#"<x:xmpmeta {NS}><rdf:RDF><{bad} rdf:about=""/></rdf:RDF></x:xmpmeta>"#);
        assert!(xmp::packets(&jpeg(&xml)).is_err(), "element {bad}");
    }
    for bad in ["GCamera:2x", "GCamera:a:b", ":x", "2x", "-x"] {
        let xml = doc(&format!(r#"{bad}="1"/>"#));
        assert!(xmp::packets(&jpeg(&xml)).is_err(), "attribute {bad}");
    }
    // Valid names keep working, including non-ASCII and underscores.
    let xml = doc(r#"GCamera:MotionPhoto="1" dc:_x="a" dc:é="b" dc:a-b.c1="c"/>"#);
    assert_eq!(motion_photo(&xml), Some(1));
}

#[test]
fn attributes_need_whitespace_and_content_cannot_hold_cdata_end() {
    // `"1"Item:Length=` (no space between attributes) and a bare `]]>` in
    // text are not well-formed XML; expat rejects them and so must we.
    let xml = doc(r#"GCamera:MotionPhoto="1"dc:x="a"/>"#);
    assert!(xmp::packets(&jpeg(&xml)).is_err());
    let xml = doc("GCamera:MotionPhoto=\"1\"><dc:x>a]]>b</dc:x></rdf:Description>");
    assert!(xmp::packets(&jpeg(&xml)).is_err());
    // Both are fine where XML allows them.
    let xml = doc("GCamera:MotionPhoto=\"a]]>b\" dc:x='1'\n\tdc:y=\"2\" ><dc:z><![CDATA[x]]></dc:z></rdf:Description>");
    let packets = xmp::packets(&jpeg(&xml)).unwrap();
    assert_eq!(packets.len(), 1);
}

#[test]
fn xmlns_prefix_with_colon_is_rejected() {
    let doc = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:s2:Aux="urn:a"></x:xmpmeta>"#;
    assert!(momentstays_core::pyxml::parse(doc).is_err());
}
