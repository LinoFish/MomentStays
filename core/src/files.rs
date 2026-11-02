//! Input side of the command-line tools, shared by `momentstays-core` and
//! `momentstays-convert`: memory-mapped inputs, one directory walk, and an
//! order-preserving parallel map.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Read-only map of a file (no whole-file copy). Empty files map to an
/// empty slice.
pub struct Mapped {
    map: Option<memmap2::Mmap>,
}

impl Mapped {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let f = File::open(path)?;
        if f.metadata()?.len() == 0 {
            return Ok(Mapped { map: None });
        }
        // SAFETY: read-only map. If another process truncates the file while
        // it is mapped, reading the lost pages raises SIGBUS; that is an
        // accepted risk for a CLI over user files (outputs never replace an
        // input, see `output::ensure_distinct`).
        Ok(Mapped {
            map: Some(unsafe { memmap2::Mmap::map(&f)? }),
        })
    }

    pub fn bytes(&self) -> &[u8] {
        self.map.as_deref().unwrap_or(&[])
    }
}

/// JPEG or HEIF by file extension (case-insensitive).
pub fn is_media(p: &Path) -> bool {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| matches!(e.as_str(), "jpg" | "jpeg" | "heic" | "heif"))
}

/// One input found by [`walk`]: a file, or a path that could not be read
/// (unreadable directory or entry), reported instead of aborting the walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    File(PathBuf),
    Unreadable(PathBuf, String),
}

impl Found {
    pub fn path(&self) -> &Path {
        match self {
            Found::File(p) | Found::Unreadable(p, _) => p,
        }
    }
}

/// Every JPEG/HEIF file under `dir`, recursively, sorted by path components.
/// Symbolic links are skipped (no cycles, no escaping the folder). A
/// directory or entry that cannot be read becomes [`Found::Unreadable`] so
/// one bad folder does not hide the rest.
pub fn walk(dir: &Path) -> Vec<Found> {
    fn go(dir: &Path, out: &mut Vec<Found>) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                out.push(Found::Unreadable(dir.to_path_buf(), e.to_string()));
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    out.push(Found::Unreadable(dir.to_path_buf(), e.to_string()));
                    continue;
                }
            };
            let p = entry.path();
            match fs::symlink_metadata(&p) {
                Err(e) => out.push(Found::Unreadable(p, e.to_string())),
                Ok(md) if md.file_type().is_symlink() => {}
                Ok(md) if md.is_dir() => go(&p, out),
                Ok(md) if md.is_file() && is_media(&p) => out.push(Found::File(p)),
                Ok(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    go(dir, &mut out);
    // `Path` orders by components, so "a/b" sorts before "a-b/c".
    out.sort_by(|a, b| a.path().cmp(b.path()));
    out
}

/// Worker count when none is given: the number of CPUs.
pub fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// `f(0..n)` on up to `jobs` threads; results come back in index order.
pub fn parallel_map<T: Send>(n: usize, jobs: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    if jobs <= 1 || n <= 1 {
        return (0..n).map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<T>>> = Mutex::new((0..n).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..jobs.min(n) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let r = f(i);
                slots.lock().unwrap_or_else(|p| p.into_inner())[i] = Some(r);
            });
        }
    });
    slots
        .into_inner()
        .unwrap_or_else(|p| p.into_inner())
        .into_iter()
        .map(|o| o.expect("every index is filled"))
        .collect()
}
