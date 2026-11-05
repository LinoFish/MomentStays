//! Source file identity, independent of scan rows and conversion workflows.
use std::path::Path;

/// Batch-local identity of a file: device, inode, size, modification and
/// change times (Swift `SourceVersion`, from `lstat`). Never persisted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceVersion {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified_ns: i128,
    pub changed_ns: i128,
}

impl SourceVersion {
    fn read_kind(path: &Path, dir: bool) -> Option<Self> {
        let m = std::fs::symlink_metadata(path).ok()?;
        if (dir && !m.is_dir()) || (!dir && !m.is_file()) {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(SourceVersion {
                device: m.dev(),
                inode: m.ino(),
                size: m.size(),
                modified_ns: m.mtime() as i128 * 1_000_000_000 + m.mtime_nsec() as i128,
                changed_ns: m.ctime() as i128 * 1_000_000_000 + m.ctime_nsec() as i128,
            })
        }
        #[cfg(not(unix))]
        {
            let ns = |t: std::io::Result<std::time::SystemTime>| {
                t.ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos() as i128)
            };
            Some(SourceVersion {
                device: 0,
                inode: 0,
                size: m.len(),
                modified_ns: ns(m.modified()),
                changed_ns: ns(m.created()),
            })
        }
    }
    pub fn read(path: &Path) -> Option<Self> {
        Self::read_kind(path, false)
    }
    pub fn read_directory(path: &Path) -> Option<Self> {
        Self::read_kind(path, true)
    }
}
