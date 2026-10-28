//! Minimal mutable XML tree with ElementTree-compatible serialization.
//!
//! The Python CLI (`motionlive`) uses `xml.etree.ElementTree` for XMP: it
//! deletes attributes/children, rewrites Extended XMP GUIDs and re-serializes
//! documents. This module reproduces just enough of ElementTree semantics for
//! those documents: namespace-aware tags (`{uri}local`), attribute order,
//! text/tail preservation, and the exact `tostring` output shape (prefix
//! assignment, ` />` empty elements, attribute/text escaping).
//!
//! It is deliberately not a general XML library: it accepts the well-formed
//! XMP documents the Python code accepts and rejects the rest.

use crate::{fail, Error, Result};
use std::collections::HashMap;

/// One element. Tag and attribute names are (namespace URI, local name);
/// an empty namespace means no namespace, exactly like ElementTree.
#[derive(Clone, Debug)]
pub struct Element {
    pub ns: String,
    pub local: String,
    pub attrib: Vec<(String, String, String)>,
    pub children: Vec<Element>,
    pub text: Option<String>,
    pub tail: Option<String>,
}

impl Element {
    pub fn new(ns: &str, local: &str) -> Self {
        Element { ns: ns.to_string(), local: local.to_string(), attrib: Vec::new(),
                  children: Vec::new(), text: None, tail: None }
    }
    pub fn get_attr(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrib.iter().find(|(n, l, _)| n == ns && l == local).map(|(_, _, v)| v.as_str())
    }
    pub fn set_attr(&mut self, ns: &str, local: &str, value: &str) {
        if let Some(slot) = self.attrib.iter_mut().find(|(n, l, _)| n == ns && l == local) {
            slot.2 = value.to_string();
        } else {
            self.attrib.push((ns.to_string(), local.to_string(), value.to_string()));
        }
    }
    pub fn del_attr(&mut self, ns: &str, local: &str) {
        self.attrib.retain(|(n, l, _)| !(n == ns && l == local));
    }
    /// First direct child with this tag, like Element.find.
    pub fn find_child(&self, ns: &str, local: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.ns == ns && c.local == local)
    }
    pub fn find_child_mut(&mut self, ns: &str, local: &str) -> Option<&mut Element> {
        self.children.iter_mut().find(|c| c.ns == ns && c.local == local)
    }
    /// Direct child's text, like Element.findtext (None when missing or textless).
    pub fn find_child_text(&self, ns: &str, local: &str) -> Option<&str> {
        self.find_child(ns, local).and_then(|c| c.text.as_deref())
    }
    /// Pre-order traversal including self, document order.
    pub fn preorder<'a>(&'a self, out: &mut Vec<&'a Element>) {
        out.push(self);
        for c in &self.children { c.preorder(out); }
    }
}

/// Remove every child matching `pred` from every element, with the same
/// outcome as Python's `for parent in list(root.iter()): for child in
/// list(parent): if pred(child): parent.remove(child)`.
///
/// Parents are snapshotted first; removals are applied deepest-first and, per
/// parent, highest-index-first, so index shifts never change the result.
/// Returns how many children were removed.
pub fn remove_children_where(root: &mut Element, pred: &dyn Fn(&Element) -> bool) -> usize {
    // Collect (parent_path, child_index) from the live tree in pre-order,
    // then apply deepest-first / highest-index-first.
    let mut removals: Vec<(Vec<usize>, usize)> = Vec::new();
    fn collect(e: &Element, cur: &mut Vec<usize>, pred: &dyn Fn(&Element) -> bool,
               out: &mut Vec<(Vec<usize>, usize)>) {
        for (i, c) in e.children.iter().enumerate() {
            if pred(c) { out.push((cur.clone(), i)); }
        }
        for (i, c) in e.children.iter().enumerate() {
            cur.push(i);
            collect(c, cur, pred, out);
            cur.pop();
        }
    }
    collect(root, &mut Vec::new(), pred, &mut removals);
    // Deepest parents first; within one parent, highest child index first.
    removals.sort_by(|a, b| {
        b.0.len().cmp(&a.0.len())
            .then_with(|| b.0.cmp(&a.0))
            .then_with(|| b.1.cmp(&a.1))
    });
    let mut applied = 0;
    for (path, idx) in removals {
        // Raw-pointer walk: the borrow checker cannot follow a &mut that is
        // re-seated in a loop. This is safe: only one pointer is live at a
        // time, the tree is not reallocated during the walk, and removals
        // were ordered so earlier removals cannot invalidate later paths.
        let mut ptr = root as *mut Element;
        let mut ok = true;
        for step in &path {
            unsafe {
                let cur = &mut *ptr;
                match cur.children.get_mut(*step) {
                    Some(next) => ptr = next as *mut Element,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if ok {
            unsafe {
                let cur = &mut *ptr;
                if idx < cur.children.len() {
                    cur.children.remove(idx);
                    applied += 1;
                }
            }
        }
    }
    applied
}

/// ElementTree `values()`: attribute values first (document order), then
/// stripped element texts (document order).
pub fn values(root: &Element, ns: &str, name: &str) -> Vec<String> {
    let mut elems = Vec::new();
    root.preorder(&mut elems);
    let mut out: Vec<String> = elems.iter()
        .flat_map(|e| e.attrib.iter().filter(|(n, l, _)| n == ns && l == name).map(|(_, _, v)| v.clone()))
        .collect();
    out.extend(elems.iter()
        .filter(|e| e.ns == ns && e.local == name)
        .filter_map(|e| e.text.as_ref().map(|t| t.trim().to_string())));
    out
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Scope {
    prefixes: Vec<(String, String)>,
    default: Option<String>,
}

struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
    scopes: Vec<Scope>,
}

fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b':' || c >= 0x80
}
fn is_name_char(c: u8) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == b'-' || c == b'.'
}

fn normalize_line_ends(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') { chars.next(); }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

fn decode_entities(raw: &str, attr: bool) -> Result<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        let semi = rest.find(';').ok_or_else(|| Error::new("Malformed XMP: unterminated entity"))?;
        let name = &rest[..semi];
        rest = &rest[semi + 1..];
        match name {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ if name.starts_with("#x") || name.starts_with("#X") => {
                let n = u32::from_str_radix(&name[2..], 16)
                    .map_err(|_| Error::new("Malformed XMP: bad character reference"))?;
                out.push(char::from_u32(n).ok_or_else(|| Error::new("Malformed XMP: bad character reference"))?);
            }
            _ if name.starts_with('#') => {
                let n: u32 = name[1..].parse()
                    .map_err(|_| Error::new("Malformed XMP: bad character reference"))?;
                out.push(char::from_u32(n).ok_or_else(|| Error::new("Malformed XMP: bad character reference"))?);
            }
            _ => return fail(format!("Malformed XMP: undefined entity &{name};")),
        }
    }
    out.push_str(rest);
    if attr {
        // Attribute-value normalization: literal whitespace becomes a space.
        Ok(out.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect())
    } else {
        Ok(out)
    }
}

impl<'a> Parser<'a> {
    fn err<T>(&self, msg: &str) -> Result<T> {
        fail(format!("Malformed XMP: {msg}"))
    }
    fn peek(&self) -> Option<u8> { self.b.get(self.pos).copied() }
    fn starts_with(&self, s: &[u8]) -> bool {
        self.b.len() - self.pos.min(self.b.len()) >= s.len() && &self.b[self.pos..self.pos + s.len()] == s
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) { self.pos += 1; }
    }
    fn skip_misc(&mut self) -> Result<()> {
        loop {
            self.skip_ws();
            if self.starts_with(b"<?") {
                match self.find(b"?>") {
                    Some(end) => self.pos = end + 2,
                    None => return self.err("unterminated processing instruction"),
                }
            } else if self.starts_with(b"<!--") {
                match self.find(b"-->") {
                    Some(end) => self.pos = end + 3,
                    None => return self.err("unterminated comment"),
                }
            } else if self.starts_with(b"<!DOCTYPE") {
                self.pos += 9;
                // Skip to '>' honouring an internal subset [ ... ].
                let mut depth = 0;
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    if c == b'[' { depth += 1; }
                    else if c == b']' { depth -= 1; }
                    else if c == b'>' && depth <= 0 { break; }
                }
            } else {
                return Ok(());
            }
        }
    }
    fn find(&self, needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || self.pos > self.b.len() { return None; }
        self.b[self.pos..].windows(needle.len()).position(|w| w == needle).map(|i| self.pos + i)
    }
    fn name(&mut self) -> Result<String> {
        let start = self.pos;
        match self.peek() {
            Some(c) if is_name_start(c) => { self.pos += 1; }
            _ => return self.err("expected a name"),
        }
        while matches!(self.peek(), Some(c) if is_name_char(c)) { self.pos += 1; }
        Ok(String::from_utf8_lossy(&self.b[start..self.pos]).into_owned())
    }
    fn lookup(&self, prefix: &str) -> Option<String> {
        if prefix == "xml" {
            return Some("http://www.w3.org/XML/1998/namespace".to_string());
        }
        for scope in self.scopes.iter().rev() {
            for (p, uri) in &scope.prefixes {
                if p == prefix { return Some(uri.clone()); }
            }
        }
        None
    }
    fn expand(&self, qname: &str) -> Result<(String, String)> {
        match qname.split_once(':') {
            Some((prefix, local)) => {
                if prefix == "xmlns" {
                    return self.err("xmlns cannot be used as an element prefix");
                }
                match self.lookup(prefix) {
                    Some(uri) => Ok((uri, local.to_string())),
                    None => self.err("unbound namespace prefix"),
                }
            }
            None => {
                let default = self.scopes.iter().rev().find_map(|s| s.default.clone());
                Ok((default.unwrap_or_default(), qname.to_string()))
            }
        }
    }

    fn element(&mut self) -> Result<Element> {
        // self.pos is at '<' (not "</", "<?", "<!").
        self.pos += 1;
        let qname = self.name()?;
        let mut scope = Scope { prefixes: Vec::new(), default: self.scopes.last().and_then(|s| s.default.clone()) };
        let mut elem = Element::new("", "");
        let mut attrs: Vec<(String, String, String)> = Vec::new();
        let mut empty = false;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'/') => {
                    self.pos += 1;
                    if self.peek() != Some(b'>') { return self.err("expected '>' after '/'"); }
                    self.pos += 1;
                    empty = true;
                    break;
                }
                Some(b'>') => {
                    self.pos += 1;
                    break;
                }
                Some(_) => {
                    let aname = self.name()?;
                    self.skip_ws();
                    if self.peek() != Some(b'=') { return self.err("expected '=' after attribute name"); }
                    self.pos += 1;
                    self.skip_ws();
                    let quote = self.peek().ok_or_else(|| Error::new("Malformed XMP: truncated attribute"))?;
                    if quote != b'"' && quote != b'\'' { return self.err("expected quoted attribute value"); }
                    self.pos += 1;
                    let start = self.pos;
                    while let Some(c) = self.peek() {
                        if c == quote { break; }
                        if c == b'<' { return self.err("'<' in attribute value"); }
                        self.pos += 1;
                    }
                    if self.peek().is_none() { return self.err("unterminated attribute value"); }
                    let raw = String::from_utf8_lossy(&self.b[start..self.pos]).into_owned();
                    self.pos += 1;
                    let value = decode_entities(&raw, true)?;
                    if aname == "xmlns" {
                        scope.default = Some(value);
                    } else if let Some(prefix) = aname.strip_prefix("xmlns:") {
                        if prefix == "xmlns" { return self.err("invalid namespace declaration"); }
                        scope.prefixes.push((prefix.to_string(), value));
                    } else {
                        attrs.push((aname, value, String::new()));
                    }
                }
                None => return self.err("truncated element"),
            }
        }
        self.scopes.push(scope);
        // Resolve the element name now that its own declarations are in scope.
        let (ns, local) = self.expand(&qname)?;
        elem.ns = ns;
        elem.local = local;
        // Resolve attribute names (attributes without a prefix have no namespace).
        for (aname, value, _) in attrs {
            let (ans, alocal) = match aname.split_once(':') {
                Some((prefix, l)) => {
                    if prefix == "xmlns" { continue; }
                    match self.lookup(prefix) {
                        Some(uri) => (uri, l.to_string()),
                        None => return self.err("unbound namespace prefix"),
                    }
                }
                None => (String::new(), aname),
            };
            if elem.attrib.iter().any(|(n, l, _)| n == &ans && l == &alocal) {
                return self.err("duplicate attribute");
            }
            elem.attrib.push((ans, alocal, value));
        }
        if empty {
            self.scopes.pop();
            return Ok(elem);
        }
        // Content: text, children, closing tag.
        let mut current_text = String::new();
        let mut seen_child = false;
        loop {
            if self.pos >= self.b.len() { return self.err("unclosed element"); }
            if self.starts_with(b"</") {
                self.pos += 2;
                let close = self.name()?;
                self.skip_ws();
                if self.peek() != Some(b'>') { return self.err("expected '>' in closing tag"); }
                self.pos += 1;
                if close != qname { return self.err("mismatched closing tag"); }
                break;
            }
            if self.starts_with(b"<!--") {
                match self.find(b"-->") {
                    Some(end) => { self.pos = end + 3; continue; }
                    None => return self.err("unterminated comment"),
                }
            }
            if self.starts_with(b"<![CDATA[") {
                self.pos += 9;
                match self.find(b"]]>") {
                    Some(end) => {
                        let raw = String::from_utf8_lossy(&self.b[self.pos..end]).into_owned();
                        let text = normalize_line_ends(&raw);
                        if seen_child {
                            let last = elem.children.last_mut().unwrap();
                            last.tail = Some(match last.tail.take() {
                                Some(t) => t + &text,
                                None => text,
                            });
                        } else {
                            current_text.push_str(&text);
                        }
                        self.pos = end + 3;
                        continue;
                    }
                    None => return self.err("unterminated CDATA section"),
                }
            }
            if self.starts_with(b"<?") {
                match self.find(b"?>") {
                    Some(end) => { self.pos = end + 2; continue; }
                    None => return self.err("unterminated processing instruction"),
                }
            }
            if self.peek() == Some(b'<') {
                if seen_child {
                    // Text already attached as tail; start a fresh child.
                } else if !current_text.is_empty() {
                    elem.text = Some(std::mem::take(&mut current_text));
                }
                let child = self.element()?;
                elem.children.push(child);
                seen_child = true;
                continue;
            }
            // Text run.
            let start = self.pos;
            while let Some(c) = self.peek() {
                if c == b'<' { break; }
                self.pos += 1;
            }
            let raw = String::from_utf8_lossy(&self.b[start..self.pos]).into_owned();
            let text = normalize_line_ends(&decode_entities(&raw, false)?);
            if seen_child {
                let last = elem.children.last_mut().unwrap();
                last.tail = Some(match last.tail.take() {
                    Some(t) => t + &text,
                    None => text,
                });
            } else {
                current_text.push_str(&text);
            }
        }
        if !seen_child && !current_text.is_empty() {
            elem.text = Some(current_text);
        }
        self.scopes.pop();
        Ok(elem)
    }
}

/// Parse one XML document (ElementTree.fromstring semantics for XMP).
pub fn parse(data: &[u8]) -> Result<Element> {
    let mut bytes = data;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { bytes = &bytes[3..]; }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::new("Malformed XMP: not valid UTF-8"))?;
    let mut p = Parser { b: text.as_bytes(), pos: 0, scopes: Vec::new() };
    p.skip_misc()?;
    if p.peek() != Some(b'<') || p.starts_with(b"</") {
        return fail("Malformed XMP: no root element");
    }
    let root = p.element()?;
    p.skip_misc()?;
    if p.pos != p.b.len() {
        return fail("Malformed XMP: junk after document element");
    }
    Ok(root)
}

// ---------------------------------------------------------------------------
// Serialization (ElementTree.tostring, encoding="utf-8")
// ---------------------------------------------------------------------------

const REGISTERED: &[(&str, &str)] = &[
    ("x", "adobe:ns:meta/"),
    ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
    ("GCamera", "http://ns.google.com/photos/1.0/camera/"),
    ("Container", "http://ns.google.com/photos/1.0/container/"),
    ("Item", "http://ns.google.com/photos/1.0/container/item/"),
    ("hdrgm", "http://ns.adobe.com/hdr-gain-map/1.0/"),
];

fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
}

struct NsMap {
    /// nsN number per URI, assigned in first-encounter order (document
    /// pre-order, tag before attributes). Registered URIs consume a number
    /// too, even though their final prefix is the registered one: this is
    /// why ElementTree output can skip an ns number (e.g. ns0, ns2, ns3).
    numbers: HashMap<String, usize>,
    counter: usize,
    /// (final prefix, uri) for the root element's xmlns declarations,
    /// sorted by prefix, exactly like ElementTree.tostring.
    declarations: Vec<(String, String)>,
}

impl NsMap {
    fn new() -> Self { NsMap { numbers: HashMap::new(), counter: 0, declarations: Vec::new() } }
    fn observe(&mut self, uri: &str) {
        if uri.is_empty() || uri == "http://www.w3.org/XML/1998/namespace" {
            return;
        }
        if !self.numbers.contains_key(uri) {
            self.numbers.insert(uri.to_string(), self.counter);
            self.counter += 1;
        }
    }
    fn collect(&mut self, e: &Element) {
        self.observe(&e.ns);
        for (ns, _, _) in &e.attrib {
            self.observe(ns);
        }
        for c in &e.children {
            self.collect(c);
        }
    }
    fn finish(&mut self) {
        let mut decls: Vec<(String, String)> = self
            .numbers
            .keys()
            .map(|uri| (self.prefix(uri), uri.clone()))
            .collect();
        decls.sort_by(|a, b| a.0.cmp(&b.0));
        self.declarations = decls;
    }
    fn prefix(&self, uri: &str) -> String {
        if uri.is_empty() {
            return String::new();
        }
        if uri == "http://www.w3.org/XML/1998/namespace" {
            return "xml".to_string();
        }
        if let Some((p, _)) = REGISTERED.iter().find(|(_, u)| *u == uri) {
            return p.to_string();
        }
        match self.numbers.get(uri) {
            Some(n) => format!("ns{n}"),
            None => String::new(),
        }
    }
}

fn qname(ns: &str, local: &str, map: &NsMap) -> String {
    if ns.is_empty() {
        local.to_string()
    } else {
        let p = map.prefix(ns);
        format!("{p}:{local}")
    }
}

fn write_element(e: &Element, map: &NsMap, out: &mut String, is_root: bool) {
    out.push('<');
    out.push_str(&qname(&e.ns, &e.local, map));
    if is_root {
        for (prefix, uri) in &map.declarations {
            out.push_str(&format!(" xmlns:{prefix}=\"{uri}\""));
        }
    }
    for (ns, local, value) in &e.attrib {
        out.push(' ');
        out.push_str(&qname(ns, local, map));
        out.push_str("=\"");
        escape_attr(value, out);
        out.push('"');
    }
    let has_text = e.text.as_ref().map_or(false, |t| !t.is_empty());
    if e.children.is_empty() && !has_text {
        out.push_str(" />");
    } else {
        out.push('>');
        if let Some(t) = &e.text { escape_text(t, out); }
        for c in &e.children { write_element(c, map, out, false); }
        out.push_str("</");
        out.push_str(&qname(&e.ns, &e.local, map));
        out.push('>');
    }
    if let Some(t) = &e.tail { escape_text(t, out); }
}

/// ElementTree.tostring(root, encoding="utf-8"): no XML declaration.
pub fn tostring(root: &Element) -> Vec<u8> {
    let mut map = NsMap::new();
    map.collect(root);
    map.finish();
    let mut out = String::new();
    write_element(root, &map, &mut out, true);
    out.into_bytes()
}
