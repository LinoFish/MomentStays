//! Which photos a selection of files and folders stands for, and whether an
//! output folder may be used for it, as the Apple apps decide it (formerly
//! Swift `NativeBatch.files` / `selection` / `validateOutput` and
//! `AndroidExport.selection`, apple/shared). Request modes `input-files`,
//! `input-selection`, `validate-output`, `normalized-paths`,
//! `standard-order` and `standard-compare`.
//!
//! The rules live here; the file-system facts and the Foundation path
//! semantics they rest on come from an [`Fs`]:
//!
//! * Apple ([`system`] on macOS / iOS): the facts Foundation itself reads —
//!   CoreFoundation resource values, the CFURL directory enumerator
//!   (hidden entries and package contents skipped exactly as
//!   `FileManager.enumerator(options: [.skipsHiddenFiles,
//!   .skipsPackageDescendants])`), Finder ordering
//!   (`localizedStandardCompare`, i.e. `CFStringCompareWithOptionsAndLocale`
//!   with case-, width-insensitive numeric forced ordering in the current
//!   locale), and `standardizedFileURL.resolvingSymlinksInPath()` (string
//!   standardization, `getattrlist(ATTR_CMN_FULLPATH)`, the automount
//!   `/private` strip).
//! * Elsewhere: `std::fs` facts, dot-file hiding, the package extension
//!   list and the Finder-like comparison of `engine::paths`.
//!
//! A URL is modelled as its path plus whether it is a directory URL
//! (`hasDirectoryPath`, the trailing slash), because Swift `URL` equality —
//! and with it the base-folder ascent and the output check — depends on it.
use crate::engine::failure;
use crate::Result;
use serde_json::{json, Value};
use std::cmp::Ordering;

/// A file URL: `path` as `URL.path` returns it (no trailing slash except
/// for "/"), `dir` = `URL.hasDirectoryPath`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Url {
    pub path: String,
    pub dir: bool,
}

impl Url {
    pub fn new(path: impl Into<String>, dir: bool) -> Url {
        let path = path.into();
        // A trailing slash makes a directory URL; `.path` drops it.
        let dir = dir || (path.len() > 1 && path.ends_with('/'));
        Url {
            path: dropping_trailing_slashes(&path),
            dir,
        }
    }
    pub fn from_json(v: &Value) -> Option<Url> {
        match v {
            Value::String(s) => Some(Url::new(s.clone(), false)),
            Value::Object(o) => Some(Url::new(
                o.get("path")?.as_str()?.to_string(),
                o.get("dir").and_then(Value::as_bool).unwrap_or(false),
            )),
            _ => None,
        }
    }
    pub fn json(&self) -> Value {
        json!({"path": self.path, "dir": self.dir})
    }
}

/// Resource facts of one URL (`isDirectory`, `isRegularFile`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub is_dir: bool,
    pub is_regular: bool,
}

/// One enumerated entry with the facts the selection reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub is_regular: bool,
    pub is_symlink: bool,
}

/// A recursive listing: entries in enumeration order (an entry whose facts
/// could not be read carries the error's message) and the errors the
/// enumerator reported on the way.
#[derive(Clone, Debug, Default)]
pub struct Walk {
    pub entries: Vec<std::result::Result<Entry, String>>,
    pub errors: Vec<String>,
}

/// The platform facts and path semantics the selection rules rest on.
pub trait Fs {
    /// `URL.resourceValues(forKeys: [.isDirectoryKey, .isRegularFileKey])`;
    /// Err carries the thrown error's `localizedDescription`.
    fn facts(&self, path: &str) -> std::result::Result<Facts, String>;
    /// Recursive listing of a folder, hidden entries and package contents
    /// skipped; None when no enumerator could be made.
    fn walk(&self, root: &Url) -> Option<Walk>;
    /// Finder ordering (`localizedStandardCompare`).
    fn compare(&self, a: &str, b: &str) -> Ordering;
    /// URLs in [`Fs::compare`] order of their paths.
    fn sort(&self, v: &mut Vec<Url>) {
        v.sort_by(|a, b| self.compare(&a.path, &b.path));
    }
    /// `standardizedFileURL.resolvingSymlinksInPath()`.
    fn normalized(&self, url: &Url) -> Url;
    /// Whether `c` joins the grapheme cluster before it (Swift `Character`
    /// boundaries for `String.hasPrefix`).
    fn extends_cluster(&self, c: char) -> bool {
        is_extender(c)
    }
}

pub const PHOTO_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "heic", "heif"];
const MOVIE_EXTENSIONS: [&str; 2] = ["mov", "mp4"];

/// Swift `URL.pathExtension` (swift-foundation `String.pathExtension`).
pub fn path_extension(path: &str) -> String {
    let name = last_path_component(path);
    let b = name.as_bytes();
    if b.last() == Some(&b'.') {
        return String::new();
    }
    let Some(dot) = name.rfind('.') else {
        return String::new();
    };
    if dot == 0 {
        return String::new();
    }
    let stem = &name[..dot];
    if (stem.len() == 1 || stem.len() == 2) && stem.bytes().all(|c| c == b'.') {
        return String::new();
    }
    let ext = &name[dot + 1..];
    if !valid_extension(ext) {
        return String::new();
    }
    ext.to_string()
}

fn valid_extension(ext: &str) -> bool {
    const INVALID: [char; 14] = [
        ' ', '/', '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}',
        '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
    ];
    if ext.ends_with('.') {
        return false;
    }
    match ext.rfind('.') {
        Some(d) => !ext[..d].contains('/') && !ext[d + 1..].chars().any(|c| INVALID.contains(&c)),
        None => !ext.chars().any(|c| INVALID.contains(&c)),
    }
}

/// `pathExtension.lowercased()` (Swift lowercases per scalar, without
/// the final-sigma context rule).
fn lower_extension(path: &str) -> String {
    path_extension(path)
        .chars()
        .flat_map(char::to_lowercase)
        .collect()
}

/// `s` up to and including the character that starts at byte `i`.
fn through(s: &str, i: usize) -> &str {
    &s[..i + s[i..].chars().next().map_or(0, char::len_utf8)]
}

/// swift-foundation `String.lastPathComponent`.
pub fn last_path_component(s: &str) -> &str {
    if s.len() <= 1 {
        return s;
    }
    let Some(last) = s.rfind('/') else { return s };
    if last != s.len() - 1 {
        return &s[last + 1..];
    }
    let Some(non_slash) = s[..last].rfind(|c| c != '/') else {
        return "/";
    };
    let upto = through(s, non_slash);
    match s[..non_slash].rfind('/') {
        Some(prev) => &upto[prev + 1..],
        None => upto,
    }
}

pub fn dropping_trailing_slashes(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    match s.rfind(|c| c != '/') {
        Some(i) => through(s, i).to_string(),
        None => "/".into(),
    }
}

/// `URL.deletingLastPathComponent()` of an absolute file URL: always a
/// directory URL.
pub fn deleting_last_path_component(url: &Url) -> Url {
    let s = if url.dir && url.path != "/" {
        format!("{}/", url.path)
    } else {
        url.path.clone()
    };
    let parent = deleting_last(&s);
    Url::new(
        if parent.is_empty() {
            "/".into()
        } else {
            parent
        },
        true,
    )
}

/// swift-foundation `String.deletingLastPathComponent()`.
fn deleting_last(s: &str) -> String {
    let Some(last) = s.rfind('/') else {
        return String::new();
    };
    let Some(non_slash) = s[..last].rfind(|c| c != '/') else {
        return "/".into();
    };
    if last != s.len() - 1 {
        return through(s, non_slash).to_string();
    }
    let Some(prev) = s[..non_slash].rfind('/') else {
        return String::new();
    };
    match s[..prev].rfind(|c| c != '/') {
        Some(p) => through(s, p).to_string(),
        None => "/".into(),
    }
}

/// Swift `String.hasPrefix` for a prefix that ends with "/": a literal
/// prefix that is also a run of whole `Character`s, i.e. the scalar after
/// it does not join the prefix's last grapheme cluster.
pub fn has_prefix(fs: &dyn Fs, s: &str, prefix: &str) -> bool {
    if !s.starts_with(prefix) {
        return false;
    }
    match s[prefix.len()..].chars().next() {
        None => true,
        Some(c) => prefix.is_empty() || !fs.extends_cluster(c),
    }
}

/// Grapheme-extending scalars (Grapheme_Extend, SpacingMark, ZWJ) by block;
/// the Apple [`Fs`] asks CoreFoundation's non-base set instead.
pub fn is_extender(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x0610..=0x061A | 0x064B..=0x065F
        | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x0900..=0x0903 | 0x093A..=0x094F
        | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF
        | 0x200C | 0x200D | 0x20D0..=0x20FF | 0x302A..=0x302F | 0x3099 | 0x309A
        | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F
        | 0xE0100..=0xE01EF)
}

/// `NativeBatch.root`: the folder itself, or a file's parent folder.
pub fn root(fs: &dyn Fs, url: &Url) -> Url {
    if fs.facts(&url.path).map(|f| f.is_dir).unwrap_or(false) {
        url.clone()
    } else {
        deleting_last_path_component(url)
    }
}

fn dir_prefix(path: &str) -> String {
    if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    }
}

fn sorted(fs: &dyn Fs, mut v: Vec<Url>) -> Vec<Url> {
    fs.sort(&mut v);
    v
}

/// Photos under a file or folder: hidden entries, symlinks and package
/// contents skipped; an unreadable folder fails the whole selection
/// instead of silently shrinking it.
pub fn files(fs: &dyn Fs, source: &Url) -> Result<Vec<Url>> {
    let facts = match fs.facts(&source.path) {
        Ok(f) => f,
        Err(e) => return failure(e),
    };
    if facts.is_regular {
        if !PHOTO_EXTENSIONS.contains(&lower_extension(&source.path).as_str()) {
            return failure("请选择 Android 动态照片 JPEG/HEIC 或照片文件夹");
        }
        return Ok(vec![source.clone()]);
    }
    if !facts.is_dir {
        return failure("输入不是可读取的文件或文件夹");
    }
    let Some(walk) = fs.walk(source) else {
        return failure("无法读取照片文件夹");
    };
    let mut found = Vec::new();
    for entry in walk.entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => return failure(e),
        };
        if entry.is_regular
            && !entry.is_symlink
            && PHOTO_EXTENSIONS.contains(&lower_extension(&entry.path).as_str())
        {
            found.push(Url::new(entry.path, false));
        }
    }
    if let Some(e) = walk.errors.first() {
        return failure(format!("部分目录不可读取：{e}"));
    }
    Ok(sorted(fs, found))
}

/// The selected inputs expanded and deduplicated, with their common base
/// folder.
pub fn selection(fs: &dyn Fs, sources: &[Url]) -> Result<(Url, Vec<Url>)> {
    let Some(first) = sources.first() else {
        return failure("请选择照片或文件夹");
    };
    let mut base = root(fs, &fs.normalized(first));
    let mut seen = std::collections::HashSet::new();
    let mut selected = Vec::new();
    for source in sources.iter().map(|s| fs.normalized(s)) {
        let parent = root(fs, &source);
        loop {
            let prefix = if base.path == "/" {
                "/".to_string()
            } else {
                format!("{}/", base.path)
            };
            if parent == base || has_prefix(fs, &parent.path, &prefix) {
                break;
            }
            let next = deleting_last_path_component(&base);
            if next == base {
                break;
            }
            base = next;
        }
        for file in files(fs, &source)? {
            let n = fs.normalized(&file);
            if seen.insert(n.clone()) {
                selected.push(n);
            }
        }
    }
    Ok((base, sorted(fs, selected)))
}

/// Android export's selection: a chosen movie stands for the one still
/// whose Apple content identifier it carries.
pub fn android_selection(fs: &dyn Fs, sources: &[Url]) -> Result<(Url, Vec<Url>)> {
    let mut images = Vec::new();
    for source in sources {
        if MOVIE_EXTENSIONS.contains(&lower_extension(&source.path).as_str()) {
            let still = super::live_selection(std::path::Path::new(&source.path))?;
            images.push(Url::new(still.to_string_lossy().into_owned(), false));
        } else {
            images.extend(files(fs, source)?);
        }
    }
    if images.is_empty() {
        return failure("未找到完整实况照片");
    }
    selection(fs, &images)
}

/// The output folder must not be, or lie inside, a source's folder.
pub fn validate_output(fs: &dyn Fs, sources: &[Url], output: &Url) -> Result<()> {
    for source in sources {
        let input = fs.normalized(&root(fs, source));
        let out = fs.normalized(output);
        let prefix = dir_prefix(&input.path);
        if out == input || has_prefix(fs, &out.path, &prefix) {
            return failure("请选择原片文件夹之外的输出位置");
        }
    }
    Ok(())
}

// MARK: Foundation path strings (swift-foundation String+Path)

fn compress_slashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut slash = false;
    for c in s.chars() {
        if c == '/' {
            if !slash {
                out.push(c);
            }
            slash = true;
        } else {
            out.push(c);
            slash = false;
        }
    }
    out
}

pub(crate) fn standardizing_slashes(s: &str) -> String {
    dropping_trailing_slashes(&compress_slashes(s))
}

pub(crate) fn has_dot_dot_component(s: &str) -> bool {
    if s.len() < 2 {
        return false;
    }
    #[derive(PartialEq)]
    enum S {
        Initial,
        Dot,
        DotDot,
        Looking,
    }
    let mut st = S::Initial;
    for &v in s.as_bytes() {
        st = match st {
            S::Initial => match v {
                b'.' => S::Dot,
                b'/' => S::Initial,
                _ => S::Looking,
            },
            S::Dot => match v {
                b'.' => S::DotDot,
                b'/' => S::Initial,
                _ => S::Looking,
            },
            S::DotDot => {
                if v == b'/' {
                    return true;
                }
                S::Looking
            }
            S::Looking => {
                if v == b'/' {
                    S::Initial
                } else {
                    S::Looking
                }
            }
        };
    }
    st == S::DotDot
}

/// swift-foundation `resolveDotSegmentsInPlace` (RFC 3986, not RFC 1808),
/// then `_removingDotSegments`'s "." → "".
pub(crate) fn removing_dot_segments(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    #[derive(Clone, Copy)]
    enum S {
        Initial,
        Dot,
        DotDot,
        Slash,
        SlashDot,
        SlashDotDot,
        Append,
        SkipSlashes,
    }
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut st = S::Initial;
    for &v in s.as_bytes() {
        match st {
            S::Initial => {
                if v == b'.' {
                    st = S::Dot
                } else if v == b'/' {
                    st = S::Slash
                } else {
                    out.push(v);
                    st = S::Append
                }
            }
            S::Dot => {
                if v == b'.' {
                    st = S::DotDot
                } else if v == b'/' {
                    st = S::Initial
                } else {
                    out.extend([b'.', v]);
                    st = S::Append
                }
            }
            S::DotDot => {
                if v == b'/' {
                    st = S::Initial
                } else {
                    out.extend([b'.', b'.', v]);
                    st = S::Append
                }
            }
            S::Slash => {
                if v == b'.' {
                    st = S::SlashDot
                } else if v == b'/' {
                    out.push(b'/')
                } else {
                    out.extend([b'/', v]);
                    st = S::Append
                }
            }
            S::SlashDot => {
                if v == b'.' {
                    st = S::SlashDotDot
                } else if v == b'/' {
                    st = S::Slash
                } else {
                    out.extend([b'/', b'.', v]);
                    st = S::Append
                }
            }
            S::SlashDotDot => {
                if v == b'/' {
                    if out.is_empty() {
                        st = S::Slash
                    } else if let Some(last) = out.iter().rposition(|&c| c == b'/') {
                        out.truncate(last);
                        st = S::Slash
                    } else {
                        out.clear();
                        st = S::SkipSlashes
                    }
                } else {
                    out.extend([b'/', b'.', b'.', v]);
                    st = S::Append
                }
            }
            S::Append => {
                if v == b'/' {
                    st = S::Slash
                } else {
                    out.push(v)
                }
            }
            S::SkipSlashes => {
                if v == b'/' {
                } else if v == b'.' {
                    st = S::Dot
                } else {
                    out.push(v);
                    st = S::Append
                }
            }
        }
    }
    match st {
        S::Slash | S::SlashDot => out.push(b'/'),
        S::SlashDotDot => {
            if out.is_empty() {
                out.push(b'/')
            } else if let Some(prev) = out.iter().rposition(|&c| c == b'/') {
                out.truncate(prev + 1)
            } else {
                out.clear()
            }
        }
        _ => {}
    }
    if out.is_empty() || out == b"." {
        return String::new();
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Whether `s` is `prefix` or starts with `prefix` + "/".
fn has_path_prefix(s: &str, prefix: &str) -> bool {
    s.starts_with(prefix) && (s.len() == prefix.len() || s.as_bytes()[prefix.len()] == b'/')
}

/// swift-foundation `_standardizingPath` (no tilde expansion: callers pass
/// absolute paths), with the platform's symlink resolution and existence
/// test.
pub(crate) fn standardizing_path(
    s: &str,
    resolve: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
) -> String {
    let mut result = standardizing_slashes(s);
    if has_dot_dot_component(&result) {
        if let Some(r) = resolve(&result) {
            result = r;
        }
    }
    result = removing_dot_segments(&result);
    const PREFIXES: [&str; 3] = ["/private/var/automount/", "/var/automount/", "/private/"];
    const EXCLUDED: [&str; 12] = [
        "/Applications",
        "/Library",
        "/System",
        "/Users",
        "/Volumes",
        "/bin",
        "/cores",
        "/dev",
        "/opt",
        "/private",
        "/sbin",
        "/usr",
    ];
    for prefix in PREFIXES {
        if result.starts_with(prefix) {
            let new_path = &result[prefix.len() - 1..];
            let excluded = EXCLUDED.iter().any(|e| has_path_prefix(new_path, e));
            if !excluded && exists(new_path) {
                result = new_path.to_string();
            }
            break;
        }
    }
    result
}

/// `URL.standardizedFileURL.resolvingSymlinksInPath()` given the
/// platform's symlink resolution (`_resolvingSymlinksInPath`) and
/// `FileManager.fileExists(atPath:)`.
///
/// Foundation rebuilds the URL from the file system: the result is a
/// directory URL exactly when its path names an existing directory,
/// whatever the input URL said.
pub(crate) fn foundation_normalized(
    url: &Url,
    resolve: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
    is_dir: &dyn Fn(&str) -> bool,
) -> Url {
    let with_hint = |p: String, dir: bool| -> Url {
        if p.is_empty() {
            return url.clone();
        }
        Url::new(p, dir)
    };
    if url.path.is_empty() {
        return url.clone();
    }
    // standardizedFileURL
    let standardized = with_hint(standardizing_path(&url.path, resolve, exists), url.dir);
    if standardized.path.is_empty() {
        return standardized;
    }
    // resolvingSymlinksInPath
    let mut result = standardized.path.clone();
    if let Some(r) = resolve(&result) {
        result = r;
    }
    let resolved = with_hint(
        standardizing_path(&result, resolve, exists),
        standardized.dir,
    );
    let dir = is_dir(&resolved.path);
    Url {
        path: resolved.path,
        dir,
    }
}

// MARK: Portable facts

/// `std::fs` facts for hosts without Foundation: dot files hidden, the
/// package extension list of `engine::paths`, `engine::paths` ordering and
/// symlink resolution.
pub struct PortableFs;

const PACKAGE_EXTENSIONS: [&str; 12] = [
    "app",
    "bundle",
    "framework",
    "plugin",
    "kext",
    "photoslibrary",
    "photolibrary",
    "aplibrary",
    "migratedphotolibrary",
    "imovielibrary",
    "fcpbundle",
    "xcodeproj",
];

impl Fs for PortableFs {
    fn facts(&self, path: &str) -> std::result::Result<Facts, String> {
        match std::fs::metadata(path) {
            Ok(m) => Ok(Facts {
                is_dir: m.is_dir(),
                is_regular: m.is_file(),
            }),
            Err(_) => Ok(Facts::default()),
        }
    }
    fn walk(&self, root: &Url) -> Option<Walk> {
        let mut walk = Walk::default();
        let mut stack = vec![root.path.clone()];
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) => {
                    walk.errors.push(e.to_string());
                    continue;
                }
            };
            let mut names: Vec<_> = entries.flatten().collect();
            names.sort_by_key(|e| e.file_name());
            for entry in names {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let path = if dir.ends_with('/') {
                    format!("{dir}{name}")
                } else {
                    format!("{dir}/{name}")
                };
                let Ok(ty) = entry.file_type() else { continue };
                if ty.is_dir() && !PACKAGE_EXTENSIONS.contains(&lower_extension(&path).as_str()) {
                    stack.push(path.clone());
                }
                walk.entries.push(Ok(Entry {
                    path,
                    is_regular: ty.is_file(),
                    is_symlink: ty.is_symlink(),
                }));
            }
        }
        Some(walk)
    }
    fn compare(&self, a: &str, b: &str) -> Ordering {
        crate::engine::paths::standard_compare(a, b)
    }
    fn normalized(&self, url: &Url) -> Url {
        let resolve = |p: &str| {
            std::fs::canonicalize(p)
                .ok()
                .map(|c| c.to_string_lossy().into_owned())
        };
        let exists = |p: &str| std::path::Path::new(p).exists();
        let is_dir = |p: &str| std::path::Path::new(p).is_dir();
        foundation_normalized(url, &resolve, &exists, &is_dir)
    }
}

/// The host's [`Fs`]: Foundation's facts on Apple platforms.
pub fn system() -> &'static dyn Fs {
    #[cfg(target_vendor = "apple")]
    {
        &crate::platform::apple_fs::AppleFs
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        &PortableFs
    }
}

// MARK: Requests

fn urls(v: &Value) -> Vec<Url> {
    v.as_array()
        .map(|a| a.iter().filter_map(Url::from_json).collect())
        .unwrap_or_default()
}

fn missing<T>(key: &str) -> Result<T> {
    failure(format!("Missing {key}"))
}

/// `input-files`: {"input": url} → {"files": [path]}.
pub fn files_request(fs: &dyn Fs, req: &Value) -> Result<Value> {
    let Some(input) = Url::from_json(&req["input"]) else {
        return missing("input");
    };
    let files = files(fs, &input)?;
    Ok(json!({"files": files.iter().map(|u| u.path.clone()).collect::<Vec<_>>()}))
}

/// `input-selection`: {"inputs": [url], "android": bool} →
/// {"base": url, "entries": [path]}.
pub fn selection_request(fs: &dyn Fs, req: &Value) -> Result<Value> {
    let inputs = urls(&req["inputs"]);
    let (base, entries) = if req["android"].as_bool().unwrap_or(false) {
        android_selection(fs, &inputs)?
    } else {
        selection(fs, &inputs)?
    };
    Ok(json!({
        "base": base.json(),
        "entries": entries.iter().map(|u| u.path.clone()).collect::<Vec<_>>(),
    }))
}

/// `validate-output`: {"inputs": [url], "output": url} → {} or the error.
pub fn validate_request(fs: &dyn Fs, req: &Value) -> Result<Value> {
    let Some(output) = Url::from_json(&req["output"]) else {
        return missing("output");
    };
    validate_output(fs, &urls(&req["inputs"]), &output)?;
    Ok(json!({}))
}

/// `normalized-paths`: {"inputs": [url]} → {"paths": [url]}.
pub fn normalized_request(fs: &dyn Fs, req: &Value) -> Result<Value> {
    let out: Vec<Value> = urls(&req["inputs"])
        .iter()
        .map(|u| fs.normalized(u).json())
        .collect();
    Ok(json!({ "paths": out }))
}

/// `standard-order`: {"names": [string]} → {"names": sorted}.
pub fn order_request(fs: &dyn Fs, req: &Value) -> Result<Value> {
    let mut names: Vec<String> = req["names"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    names.sort_by(|a, b| fs.compare(a, b));
    Ok(json!({ "names": names }))
}

/// `standard-compare`: {"pairs": [[a, b]]} → {"order": [-1 | 0 | 1]}, the
/// comparison of each pair in its own argument order.
pub fn compare_request(fs: &dyn Fs, req: &Value) -> Result<Value> {
    let order: Vec<i64> = req["pairs"]
        .as_array()
        .map(|pairs| {
            pairs
                .iter()
                .map(|p| {
                    let a = p[0].as_str().unwrap_or("");
                    let b = p[1].as_str().unwrap_or("");
                    match fs.compare(a, b) {
                        Ordering::Less => -1,
                        Ordering::Equal => 0,
                        Ordering::Greater => 1,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "order": order }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An in-memory tree: path → (facts, children).
    #[derive(Default)]
    struct FakeFs {
        nodes: HashMap<String, Facts>,
        unreadable: Vec<String>,
    }
    impl FakeFs {
        fn dir(mut self, p: &str) -> Self {
            self.nodes.insert(
                p.into(),
                Facts {
                    is_dir: true,
                    is_regular: false,
                },
            );
            self
        }
        fn file(mut self, p: &str) -> Self {
            self.nodes.insert(
                p.into(),
                Facts {
                    is_dir: false,
                    is_regular: true,
                },
            );
            self
        }
    }
    impl Fs for FakeFs {
        fn facts(&self, path: &str) -> std::result::Result<Facts, String> {
            self.nodes
                .get(path)
                .copied()
                .ok_or_else(|| format!("no such file {path}"))
        }
        fn walk(&self, root: &Url) -> Option<Walk> {
            let prefix = format!("{}/", root.path);
            let mut walk = Walk::default();
            let mut keys: Vec<_> = self
                .nodes
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .collect();
            keys.sort();
            for k in keys {
                if self
                    .unreadable
                    .iter()
                    .any(|u| k.starts_with(&format!("{u}/")))
                {
                    continue;
                }
                if last_path_component(k).starts_with('.') {
                    continue;
                }
                let f = self.nodes[k];
                walk.entries.push(Ok(Entry {
                    path: k.clone(),
                    is_regular: f.is_regular,
                    is_symlink: false,
                }));
            }
            for u in &self.unreadable {
                if u.starts_with(&prefix) || *u == root.path {
                    walk.errors.push(format!("cannot open {u}"));
                }
            }
            Some(walk)
        }
        fn compare(&self, a: &str, b: &str) -> Ordering {
            crate::engine::paths::standard_compare(a, b)
        }
        fn normalized(&self, url: &Url) -> Url {
            let is_dir = |p: &str| self.nodes.get(p).is_some_and(|f| f.is_dir);
            foundation_normalized(url, &|_| None, &|_| false, &is_dir)
        }
    }

    fn fake() -> FakeFs {
        FakeFs::default()
            .dir("/a")
            .dir("/a/b")
            .dir("/a/c")
            .file("/a/b/IMG_10.jpg")
            .file("/a/b/IMG_2.JPG")
            .file("/a/b/.h.jpg")
            .file("/a/b/x.png")
            .file("/a/c/y.heic")
            .file("/a/b/..jpg")
            .dir("/a/b/d.jpg")
    }

    #[test]
    fn extensions_follow_foundation() {
        for (p, e) in [
            ("/a/x.jpg", "jpg"),
            ("/a/..jpg", ""),
            ("/a/...jpg", ""),
            ("/a/a..jpg", "jpg"),
            ("/a/.jpg", ""),
            ("/a/x.jpg.", ""),
            ("/a/x.jp g", ""),
            ("/a/x.JPG", "JPG"),
            ("/a/x.tar.gz", "gz"),
            ("/a/x.jpg/", "jpg"),
            ("x", ""),
            ("/a/\u{301}lead.jpg", "jpg"),
            ("/a/e\u{301}/", ""),
            ("/a/照片.HEIC", "HEIC"),
            ("/a/x.jpg\u{200e}", ""),
        ] {
            assert_eq!(path_extension(p), e, "{p}");
        }
    }

    #[test]
    fn files_and_selection() {
        let fs = fake();
        let got = files(&fs, &Url::new("/a/b", true)).unwrap();
        let paths: Vec<_> = got.iter().map(|u| u.path.as_str()).collect();
        assert_eq!(paths, ["/a/b/IMG_2.JPG", "/a/b/IMG_10.jpg"]);
        assert!(files(&fs, &Url::new("/a/b/x.png", false)).is_err());
        assert!(files(&fs, &Url::new("/nope", false)).is_err());
        let (base, entries) = selection(
            &fs,
            &[Url::new("/a/b/IMG_10.jpg", false), Url::new("/a/c", true)],
        )
        .unwrap();
        assert_eq!(base, Url::new("/a", true));
        assert_eq!(entries.len(), 2);
        // Normalizing rebuilds a URL from the file system, so a folder given
        // without its trailing slash still meets the base folder.
        for inputs in [
            [Url::new("/a/b/IMG_10.jpg", false), Url::new("/a/b", false)],
            [Url::new("/a/b", false), Url::new("/a/b/IMG_10.jpg", false)],
            [Url::new("/a/b", true), Url::new("/a/b/IMG_10.jpg", false)],
        ] {
            let (base, _) = selection(&fs, &inputs).unwrap();
            assert_eq!(base, Url::new("/a/b", true));
        }
    }

    #[test]
    fn unreadable_folder_fails() {
        let mut fs = fake();
        fs.unreadable.push("/a/c".into());
        let e = files(&fs, &Url::new("/a", true)).unwrap_err().to_string();
        assert!(e.starts_with("部分目录不可读取："), "{e}");
    }

    #[test]
    fn output_validation() {
        let fs = fake();
        let s = [Url::new("/a/b/IMG_10.jpg", false)];
        assert!(validate_output(&fs, &s, &Url::new("/a/b", true)).is_err());
        assert!(validate_output(&fs, &s, &Url::new("/a/b/out", true)).is_err());
        assert!(validate_output(&fs, &s, &Url::new("/a/bb", true)).is_ok());
        assert!(validate_output(&fs, &s, &Url::new("/a", true)).is_ok());
        assert!(validate_output(&fs, &s, &Url::new("/a/b", false)).is_err());
        // A name starting with a combining mark is not inside the prefix.
        assert!(validate_output(&fs, &s, &Url::new("/a/b/\u{301}x", true)).is_ok());
    }

    #[test]
    fn dot_segments() {
        for (i, o) in [
            ("/a/./b", "/a/b"),
            ("/a/../b", "/b"),
            ("/..", "/"),
            ("/a/b/..", "/a/"),
            ("/a/b/.", "/a/b/"),
            ("/a/..b", "/a/..b"),
            (".", ""),
        ] {
            assert_eq!(removing_dot_segments(i), o, "{i}");
        }
        assert_eq!(
            deleting_last_path_component(&Url::new("/a/b", false)),
            Url::new("/a", true)
        );
        assert_eq!(
            deleting_last_path_component(&Url::new("/a", true)),
            Url::new("/", true)
        );
        assert_eq!(
            deleting_last_path_component(&Url::new("/a\u{301}/b\u{301}", true)),
            Url::new("/a\u{301}", true)
        );
        assert_eq!(
            deleting_last_path_component(&Url::new("/照片/e\u{301}.jpg", false)),
            Url::new("/照片", true)
        );
        assert_eq!(dropping_trailing_slashes("/é//"), "/é");
        assert_eq!(last_path_component("/a/é/"), "é");
    }

    // `PortableFs` is the non-Apple backend and speaks '/'-separated paths. On
    // Apple it would also apply Foundation's "/private" stripping, which
    // `engine::paths` does not, and Windows paths are not its domain.
    #[cfg(all(unix, not(target_vendor = "apple")))]
    #[test]
    fn portable_matches_engine_paths() {
        let root = std::env::temp_dir().join(format!("ms-selection-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("x/y")).unwrap();
        // The engine resolves symlinks (macOS: /var -> /private/var); start from the resolved path.
        let root = std::fs::canonicalize(&root).unwrap();
        for n in ["x/a.jpg", "x/B.JPG", "x/y/c.heic", "x/.d.jpg", "x/e.txt"] {
            std::fs::write(root.join(n), b"").unwrap();
        }
        let fs = PortableFs;
        let input = root.join("x");
        let (_, ours) =
            selection(&fs, &[Url::new(input.to_string_lossy().into_owned(), true)]).unwrap();
        let (_, theirs) = crate::engine::paths::selection(&[input]).unwrap();
        let theirs: Vec<String> = theirs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let ours: Vec<String> = ours.into_iter().map(|u| u.path).collect();
        assert_eq!(ours, theirs);
        let _ = std::fs::remove_dir_all(&root);
    }
}
