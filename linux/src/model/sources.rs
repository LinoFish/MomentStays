//! Source folders (folders only; there is no photo-library source on Linux).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFolder {
    pub path: PathBuf,
    /// Rescan automatically when files in the folder change.
    pub monitoring: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddOutcome {
    Added,
    /// The folder replaced this many folders it contains.
    Merged(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddError {
    /// Already listed, or inside a listed folder (scans are recursive).
    AlreadyCovered(PathBuf),
}

#[derive(Default, Debug)]
pub struct Sources {
    folders: Vec<SourceFolder>,
}

fn inside(path: &Path, folder: &Path) -> bool {
    path.starts_with(folder)
}

impl Sources {
    pub fn folders(&self) -> &[SourceFolder] {
        &self.folders
    }
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty()
    }
    pub fn paths(&self) -> Vec<PathBuf> {
        self.folders.iter().map(|f| f.path.clone()).collect()
    }

    pub fn add(&mut self, path: PathBuf) -> Result<AddOutcome, AddError> {
        if let Some(cover) = self.folders.iter().find(|f| inside(&path, &f.path)) {
            return Err(AddError::AlreadyCovered(cover.path.clone()));
        }
        let before = self.folders.len();
        self.folders.retain(|f| !inside(&f.path, &path));
        let merged = before - self.folders.len();
        self.folders.push(SourceFolder {
            path,
            monitoring: false,
        });
        Ok(if merged > 0 {
            AddOutcome::Merged(merged)
        } else {
            AddOutcome::Added
        })
    }

    pub fn remove(&mut self, path: &Path) -> Option<SourceFolder> {
        let i = self.folders.iter().position(|f| f.path == path)?;
        Some(self.folders.remove(i))
    }

    pub fn set_monitoring(&mut self, path: &Path, on: bool) -> bool {
        match self.folders.iter_mut().find(|f| f.path == path) {
            Some(f) if f.monitoring != on => {
                f.monitoring = on;
                true
            }
            _ => false,
        }
    }

    /// The output folder must be outside every source folder (the core
    /// refuses it too: "请选择原片文件夹之外的输出位置"); checked early for the UI.
    pub fn conflicts_with_output(&self, output: &Path) -> bool {
        self.folders.iter().any(|f| inside(output, &f.path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_and_dedupe() {
        let mut s = Sources::default();
        assert_eq!(s.add("/photos/a".into()), Ok(AddOutcome::Added));
        assert_eq!(s.add("/photos/b".into()), Ok(AddOutcome::Added));
        assert_eq!(
            s.add("/photos/a".into()),
            Err(AddError::AlreadyCovered("/photos/a".into()))
        );
        assert_eq!(
            s.add("/photos/a/sub".into()),
            Err(AddError::AlreadyCovered("/photos/a".into()))
        );
        // A sibling whose name only shares a prefix is not nested.
        assert_eq!(s.add("/photos/ab".into()), Ok(AddOutcome::Added));
        assert_eq!(s.add("/photos".into()), Ok(AddOutcome::Merged(3)));
        assert_eq!(s.paths(), vec![PathBuf::from("/photos")]);
        assert!(s.remove(Path::new("/photos")).is_some());
        assert!(s.is_empty());
        assert!(s.remove(Path::new("/photos")).is_none());
    }

    #[test]
    fn monitoring_toggle() {
        let mut s = Sources::default();
        s.add("/a".into()).ok();
        assert!(!s.folders()[0].monitoring);
        assert!(s.set_monitoring(Path::new("/a"), true));
        assert!(!s.set_monitoring(Path::new("/a"), true));
        assert!(s.folders()[0].monitoring);
        assert!(!s.set_monitoring(Path::new("/missing"), true));
    }

    #[test]
    fn output_inside_source_conflicts() {
        let mut s = Sources::default();
        s.add("/photos".into()).ok();
        assert!(s.conflicts_with_output(Path::new("/photos")));
        assert!(s.conflicts_with_output(Path::new("/photos/out")));
        assert!(!s.conflicts_with_output(Path::new("/photos-out")));
        assert!(!s.conflicts_with_output(Path::new("/export")));
    }
}
