//! Safe output files: write to a temp file next to the target, check what
//! actually landed on disk, then rename it over the target.
//!
//! A crash, a full disk or a failed check never leaves a half-written
//! output, and an existing file at the target is only replaced by a
//! complete, verified one. Files are not fsynced (see `write_then_verify`). Parts are written in order without being joined
//! first, so a `head ++ video ++ tail` output never exists in memory.

use crate::{fail, fail_as, Error, ErrorKind, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn io_err(what: &str, e: std::io::Error) -> Error {
    Error::with_kind(ErrorKind::Io, format!("{what}: {e}"))
}

/// `path` resolved to an absolute, symlink-free form. The file itself may
/// not exist yet; its parent directory must.
fn resolve(path: &Path) -> Option<PathBuf> {
    if let Ok(p) = fs::canonicalize(path) {
        return Some(p);
    }
    let name = path.file_name()?;
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    Some(fs::canonicalize(parent).ok()?.join(name))
}

/// Refuse an output path that names one of the inputs. Replacing an input
/// in place would lose the original if anything after that went wrong, and
/// on Windows the rename fails while the input is still memory-mapped.
pub fn ensure_distinct(inputs: &[&Path], output: &Path) -> Result<()> {
    let Some(out) = resolve(output) else {
        return fail(format!("输出路径无效: {}", output.display()));
    };
    for input in inputs {
        if resolve(input).is_some_and(|i| i == out) {
            return fail(format!("输出路径不能与输入相同: {}", output.display()));
        }
    }
    Ok(())
}

fn temp_path(target: &Path) -> Result<PathBuf> {
    let name = match target.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => return fail(format!("输出路径无效: {}", target.display())),
    };
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = format!(".{name}.momentstays-{}-{n}.tmp", std::process::id());
    Ok(match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(tmp),
        _ => PathBuf::from(tmp),
    })
}

/// Write `parts` in order to `target` atomically. `verify` receives the
/// bytes read back from the finished temp file (memory-mapped, not copied);
/// the target is only replaced when it returns `Ok`. Returns the byte count.
pub fn write_atomic(
    target: &Path,
    parts: &[&[u8]],
    verify: impl FnOnce(&[u8]) -> Result<()>,
) -> Result<u64> {
    let mut v = Some(verify);
    let lens = write_atomic_many(&[(target, parts)], |_, bytes| {
        (v.take().expect("one file"))(bytes)
    })?;
    Ok(lens[0])
}

/// [`write_atomic`] for files that belong together (a Live Photo still and
/// its movie): every file is written and verified (`verify(index, bytes)`)
/// before any target is replaced, so a failure leaves all targets as they
/// were. Only the final renames can fail part-way (e.g. the disk vanishes
/// between them), which no portable API can make atomic across files.
pub fn write_atomic_many(
    files: &[(&Path, &[&[u8]])],
    mut verify: impl FnMut(usize, &[u8]) -> Result<()>,
) -> Result<Vec<u64>> {
    let mut temps: Vec<PathBuf> = Vec::new();
    let result = (|| {
        let mut lens = Vec::new();
        for (i, (target, parts)) in files.iter().enumerate() {
            let tmp = temp_path(target)?;
            temps.push(tmp.clone());
            lens.push(write_then_verify(&tmp, parts, |b| verify(i, b))?);
        }
        for ((target, _), tmp) in files.iter().zip(&temps) {
            fs::rename(tmp, target).map_err(|e| io_err("写出失败", e))?;
        }
        Ok(lens)
    })();
    if result.is_err() {
        for tmp in &temps {
            let _ = fs::remove_file(tmp);
        }
    }
    result
}

fn write_then_verify(
    tmp: &Path,
    parts: &[&[u8]],
    verify: impl FnOnce(&[u8]) -> Result<()>,
) -> Result<u64> {
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp)
            .map_err(|e| io_err("写出失败", e))?;
        for p in parts {
            f.write_all(p).map_err(|e| io_err("写出失败", e))?;
        }
        // No fsync: it cost ~1.4 s of a 1.6 s 200 MB conversion in testing.
        // Inputs are never replaced (`ensure_distinct`) and a target is only
        // replaced by a complete, verified file, so a power cut can lose the
        // new output but never an original.
    }
    let expected: u64 = parts.iter().map(|p| p.len() as u64).sum();
    let f = File::open(tmp).map_err(|e| io_err("读回失败", e))?;
    let len = f.metadata().map_err(|e| io_err("读回失败", e))?.len();
    if len != expected {
        return fail_as(ErrorKind::VerifyFailed, "写出校验失败：文件长度不同");
    }
    if len == 0 {
        verify(&[])?;
    } else {
        // SAFETY: read-only map of our own temp file, which nothing else
        // knows the name of; the map is dropped before the rename.
        let map = unsafe { memmap2::Mmap::map(&f) }.map_err(|e| io_err("读回失败", e))?;
        verify(&map)?;
    }
    Ok(len)
}

/// Check used when there is no format-level read-back: the file must hold
/// exactly `parts`.
pub fn same_bytes<'a>(parts: &'a [&'a [u8]]) -> impl FnOnce(&[u8]) -> Result<()> + 'a {
    move |written| {
        let mut at = 0;
        for p in parts {
            if written.get(at..at + p.len()) != Some(*p) {
                return fail_as(ErrorKind::VerifyFailed, "写出校验失败：文件内容不同");
            }
            at += p.len();
        }
        if at != written.len() {
            return fail_as(ErrorKind::VerifyFailed, "写出校验失败：文件长度不同");
        }
        Ok(())
    }
}
