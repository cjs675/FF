//! Directory scanning.
//!
//! `DirScanner` is the single source of directory listings. The v0 production
//! implementation (no-stat listing, viewport-first population, neighbor
//! prefetch, `notify` refresh — PLAN §2) builds on this trait; until it
//! lands, `MemoryScanner` serves both tests and the v0 shell so the shell
//! never touches disk before the real engine exists.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::model::{Entry, EntryKind};

/// Returns the entries of one directory, sorted directories-first then
/// case-insensitive by name.
pub trait DirScanner {
    fn scan(&self, dir: &Path) -> Vec<Entry>;
}

/// In-memory fake. Paths are *not* resolved through the filesystem: passing
/// a relative root like `PathBuf::from("root")` guarantees the shell keeps
/// the "never touches disk" demo guarantee.
#[derive(Debug, Default, Clone)]
pub struct MemoryScanner {
    dirs: BTreeMap<PathBuf, Vec<Entry>>,
}

impl MemoryScanner {
    pub fn mkdir(&mut self, dir: &Path) {
        self.dirs.entry(dir.to_path_buf()).or_default();
    }

    pub fn put_dir(&mut self, dir: &Path, name: &str) {
        let path = dir.join(name);
        self.entry(dir, name, EntryKind::Dir, &path);
        self.mkdir(&path);
    }

    pub fn put_file(&mut self, dir: &Path, name: &str) {
        let path = dir.join(name);
        self.entry(dir, name, EntryKind::File, &path);
    }

    fn entry(&mut self, dir: &Path, name: &str, kind: EntryKind, path: &Path) {
        self.dirs.entry(dir.to_path_buf()).or_default().push(Entry {
            name: name.to_string(),
            path: path.to_path_buf(),
            kind,
        });
    }

    pub fn has(&self, dir: &Path) -> bool {
        self.dirs.contains_key(dir)
    }
}

impl DirScanner for MemoryScanner {
    fn scan(&self, dir: &Path) -> Vec<Entry> {
        match self.dirs.get(dir) {
            Some(entries) if !entries.is_empty() => {
                let mut sorted = entries.clone();
                sorted.sort_by_key(|e| (!e.is_dir(), e.name.to_lowercase()));
                sorted
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_sort_before_files_case_insensitive() {
        let mut s = MemoryScanner::default();
        let root = Path::new("root");
        s.put_file(root, "zebra.md");
        s.put_file(root, "Apple.rs");
        s.put_dir(root, "notes");
        s.put_file(root, "banana.txt");

        let scanned = s.scan(root);
        let names: Vec<&str> = scanned.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["notes", "Apple.rs", "banana.txt", "zebra.md"]);
    }

    #[test]
    fn unknown_dir_is_empty_and_not_present() {
        let s = MemoryScanner::default();
        assert!(s.scan(Path::new("nowhere")).is_empty());
        assert!(!s.has(Path::new("nowhere")));
    }
}
