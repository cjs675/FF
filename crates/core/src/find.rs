//! Search backends behind one trait (PLAN §3).
//!
//! v0 floor: the portable filter — case-insensitive substring over the
//! loaded directory. SIMD fuzzy scoring and the fsearch provider ride
//! behind the same `FileFinder` trait later; nothing above this line may
//! know which provider answered.

use crate::model::Entry;

pub trait FileFinder {
    fn filter(&self, entries: &[Entry], query: &str) -> Vec<Entry>;
}

/// Portable everywhere by construction: pure, allocation-light, no indexes.
/// Never a second-class provider — it is the floor (PLAN §3, "keep rg" logic).
#[derive(Debug, Default, Clone, Copy)]
pub struct PortableFilter;

impl FileFinder for PortableFilter {
    fn filter(&self, entries: &[Entry], query: &str) -> Vec<Entry> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return entries.to_vec();
        }
        entries
            .iter()
            .filter(|e| e.name.to_lowercase().contains(&query))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryKind;
    use std::path::PathBuf;

    fn e(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            path: PathBuf::from(name),
            kind: EntryKind::File,
        }
    }

    #[test]
    fn empty_query_returns_everything() {
        let src = vec![e("a.rs"), e("b.md")];
        assert_eq!(PortableFilter.filter(&src, "   ").len(), 2);
    }

    #[test]
    fn filter_is_case_insensitive() {
        let src = vec![e("README.md"), e("notes.txt")];
        let hits = PortableFilter.filter(&src, "readme");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "README.md");
    }
}
