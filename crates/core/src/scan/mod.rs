//! Directory scanning. PLAN §2: no-stat listing, viewport-first population,
//! neighbor prefetch; the `notify` watcher + selection-anchored refresh land
//! in the following milestone. The contract for any implementation of
//! [`DirSource`]: `poll` must never block the UI thread on filesystem
//! syscalls — work happens off-thread, results arrive as events.

mod fs;

use std::path::{Path, PathBuf};

use crate::model::Entry;

pub use fs::FsEngine;

/// What the shell renders per frame for some directory. `complete` marks an
/// authoritative listing (cache or fully read+sorted); when false the
/// listing is still loading/streaming and may be partial or empty.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub complete: bool,
}

/// Poll-based, non-blocking source of directory listings.
pub trait DirSource {
    fn poll(&mut self, dir: &Path) -> Listing;
}

/// Canonical order: directories first, then case-insensitive by name.
pub(crate) fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by_key(|e| (!e.is_dir(), e.name.to_lowercase()));
}

/// In-memory fake. Paths are *not* resolved through the filesystem: passing
/// a relative root like `PathBuf::from("root")` guarantees listings never
/// touch disk. Used by tests and anything needing a deterministic tree.
#[derive(Debug, Default, Clone)]
pub struct MemoryScanner {
    dirs: std::collections::BTreeMap<PathBuf, Vec<Entry>>,
}

impl MemoryScanner {
    pub fn mkdir(&mut self, dir: &Path) {
        self.dirs.entry(dir.to_path_buf()).or_default();
    }

    pub fn put_dir(&mut self, dir: &Path, name: &str) {
        let path = dir.join(name);
        self.entry(dir, name, crate::model::EntryKind::Dir, &path);
        self.mkdir(&path);
    }

    pub fn put_file(&mut self, dir: &Path, name: &str) {
        let path = dir.join(name);
        self.entry(dir, name, crate::model::EntryKind::File, &path);
    }

    fn entry(&mut self, dir: &Path, name: &str, kind: crate::model::EntryKind, path: &Path) {
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

impl DirSource for MemoryScanner {
    fn poll(&mut self, dir: &Path) -> Listing {
        match self.dirs.get(dir) {
            Some(entries) if !entries.is_empty() => {
                let mut sorted = entries.clone();
                sort_entries(&mut sorted);
                Listing {
                    entries: sorted,
                    complete: true,
                }
            }
            _ => Listing::default(),
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

        let scanned = s.poll(root);
        let names: Vec<&str> = scanned.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["notes", "Apple.rs", "banana.txt", "zebra.md"]);
        assert!(scanned.complete);
    }

    #[test]
    fn unknown_dir_is_empty_and_not_present() {
        let mut s = MemoryScanner::default();
        assert!(s.poll(Path::new("nowhere")).entries.is_empty());
        assert!(!s.has(Path::new("nowhere")));
    }
}
