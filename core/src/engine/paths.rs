//! Input selection and output placement (Swift `NativeBatch.files`,
//! `selection`, `relative`, `validateOutput`, `normalized`, `root`).

use crate::engine::failure;
use crate::Result;
use std::cmp::Ordering;
use std::path::{Component, Path, PathBuf};

pub const PHOTO_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "heic", "heif"];

/// Directory extensions Finder treats as packages; their contents are not
/// photos the user picked (Swift `.skipsPackageDescendants`).
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

pub(crate) fn extension(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

pub(crate) fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `deletingPathExtension().lastPathComponent`.
pub(crate) fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Absolute, standardized, symlinks resolved where the path exists
/// (`standardizedFileURL.resolvingSymlinksInPath()`).
pub fn normalized(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let abs = lexical(&abs);
    if let Ok(c) = std::fs::canonicalize(&abs) {
        return strip_verbatim(c);
    }
    // Resolve the deepest existing ancestor, keep the rest as written.
    let mut tail = Vec::new();
    let mut cur = abs.as_path();
    while let Some(parent) = cur.parent() {
        if let Some(name) = cur.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(c) = std::fs::canonicalize(parent) {
            let mut out = strip_verbatim(c);
            for n in tail.iter().rev() {
                out.push(n);
            }
            return out;
        }
        cur = parent;
    }
    abs
}

/// Windows `canonicalize` returns `\\?\C:\...`; keep the plain form.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

/// The folder itself, or a file's parent folder.
pub fn root(p: &Path) -> PathBuf {
    if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

/// `path` with a trailing separator, for prefix tests.
fn dir_prefix(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    if s.ends_with('/') || s.ends_with('\\') {
        s
    } else {
        format!("{s}{}", std::path::MAIN_SEPARATOR)
    }
}

/// Finder-style ordering (`localizedStandardCompare`): case-insensitive,
/// digit runs compared by value.
pub fn standard_compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut m = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    m.push(c);
                    x.next();
                }
                let mut n = String::new();
                while let Some(d) = y.peek().copied().filter(char::is_ascii_digit) {
                    n.push(d);
                    y.next();
                }
                let (mt, nt) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = mt.len().cmp(&nt.len()).then_with(|| mt.cmp(nt));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

fn sort_paths(v: &mut [PathBuf]) {
    v.sort_by(|a, b| standard_compare(&a.to_string_lossy(), &b.to_string_lossy()));
}

/// Photos under a file or folder (Swift `NativeBatch.files`): hidden
/// entries, symlinks and package contents skipped; an unreadable folder
/// fails the whole selection instead of silently shrinking it.
pub fn files(source: &Path) -> Result<Vec<PathBuf>> {
    let meta = match std::fs::metadata(source) {
        Ok(m) => m,
        Err(_) => return failure("输入不是可读取的文件或文件夹"),
    };
    if meta.is_file() {
        if !PHOTO_EXTENSIONS.contains(&extension(source).as_str()) {
            return failure("请选择 Android 动态照片 JPEG/HEIC 或照片文件夹");
        }
        return Ok(vec![source.to_path_buf()]);
    }
    if !meta.is_dir() {
        return failure("输入不是可读取的文件或文件夹");
    }
    let mut found = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut stack = vec![source.to_path_buf()];
    let mut first = true;
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                if first {
                    return failure("无法读取照片文件夹");
                }
                errors.push(e.to_string());
                continue;
            }
        };
        first = false;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(e.to_string());
                    continue;
                }
            };
            let path = entry.path();
            if file_name(&path).starts_with('.') {
                continue;
            }
            let Ok(ty) = entry.file_type() else { continue };
            if ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                if !PACKAGE_EXTENSIONS.contains(&extension(&path).as_str()) {
                    stack.push(path);
                }
            } else if ty.is_file() && PHOTO_EXTENSIONS.contains(&extension(&path).as_str()) {
                found.push(path);
            }
        }
    }
    if let Some(e) = errors.first() {
        return failure(format!("部分目录不可读取：{e}"));
    }
    sort_paths(&mut found);
    Ok(found)
}

/// The selected inputs expanded and deduplicated, with their common base
/// folder (Swift `NativeBatch.selection`).
pub fn selection(sources: &[PathBuf]) -> Result<(PathBuf, Vec<PathBuf>)> {
    let Some(first) = sources.first() else {
        return failure("请选择照片或文件夹");
    };
    let mut base = root(&normalized(first));
    let mut selected: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for source in sources.iter().map(|s| normalized(s)) {
        let parent = root(&source);
        while parent != base && !parent.to_string_lossy().starts_with(&dir_prefix(&base)) {
            match base.parent() {
                Some(next) if next != base => base = next.to_path_buf(),
                _ => break,
            }
        }
        for file in files(&source)? {
            let n = normalized(&file);
            if seen.insert(n.clone()) {
                selected.push(n);
            }
        }
    }
    sort_paths(&mut selected);
    Ok((base, selected))
}

pub fn relative(source: &Path, root: &Path) -> Result<String> {
    let s = normalized(source).to_string_lossy().into_owned();
    let r = normalized(root);
    let prefix = dir_prefix(&r);
    match s.strip_prefix(&prefix) {
        Some(rest) => Ok(rest.replace('\\', "/")),
        None => failure(format!(
            "输入文件位于所选目录之外：{s} / {}",
            r.to_string_lossy()
        )),
    }
}

pub fn validate_output(sources: &[PathBuf], output: &Path) -> Result<()> {
    let out = normalized(output);
    for source in sources {
        let input = normalized(&root(source));
        if out == input || out.to_string_lossy().starts_with(&dir_prefix(&input)) {
            return failure("请选择原片文件夹之外的输出位置");
        }
    }
    Ok(())
}
