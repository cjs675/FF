//! Search backends behind one trait (PLAN §3).
//!
//! v0 floor: the portable filter — case-insensitive substring over the
//! loaded directory. SIMD fuzzy scoring and the fsearch provider ride
//! behind the same `FileFinder` trait (v0.5 it18); nothing above this line may
//! know which provider answered.

pub mod content;
pub use content::{search_content, ContentMatch, ContentSearchResult};

use crate::model::Entry;

pub trait FileFinder: Send + Sync {
    fn filter(&self, entries: &[Entry], query: &str) -> Vec<Entry>;
    fn name(&self) -> &'static str {
        "portable"
    }
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

    fn name(&self) -> &'static str {
        "portable"
    }
}

/// macOS-accelerated finder using fsearch:
/// - In-memory directory filter: SIMD fuzzy scoring with ranking and substring fallback.
/// - Whole-disk / deep tree capability: optional in-process fsearch engine.
/// - Portable fallback: on non-macOS or when feature is absent, delegates cleanly to PortableFilter.
#[derive(Clone, Default)]
pub struct FsearchFinder {
    #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
    engine: Option<std::sync::Arc<fsearch::Engine>>,
}

impl std::fmt::Debug for FsearchFinder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FsearchFinder").finish()
    }
}

impl FsearchFinder {
    pub fn new() -> Self {
        #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
        {
            let home = std::env::var("HOME").unwrap_or_default();
            let engine = if !home.is_empty() {
                let dir = fsearch::default_dir(&home);
                fsearch::Engine::start(fsearch::Options {
                    dir,
                    home,
                    skip: None,
                })
                .ok()
                .map(std::sync::Arc::new)
            } else {
                None
            };
            Self { engine }
        }
        #[cfg(not(all(target_os = "macos", feature = "fsearch-macos")))]
        {
            Self {}
        }
    }

    #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
    pub fn engine(&self) -> Option<&std::sync::Arc<fsearch::Engine>> {
        self.engine.as_ref()
    }
}

impl FileFinder for FsearchFinder {
    fn filter(&self, entries: &[Entry], query: &str) -> Vec<Entry> {
        let q_trimmed = query.trim();
        if q_trimmed.is_empty() {
            return entries.to_vec();
        }

        #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
        {
            let q_lower = q_trimmed.to_lowercase();
            let q_bytes = q_lower.as_bytes();
            let mut scored: Vec<(Entry, i32)> = Vec::new();

            for entry in entries {
                let name_bytes = entry.name.as_bytes();
                if let Some(score) = fsearch::query::fuzzy_score(name_bytes, q_bytes) {
                    scored.push((entry.clone(), score));
                } else if entry.name.to_lowercase().contains(&q_lower) {
                    scored.push((entry.clone(), 0));
                }
            }

            // Rank highest score first, stable tie-break by name
            scored.sort_by(|(e_a, s_a), (e_b, s_b)| {
                s_b.cmp(s_a).then_with(|| e_a.name.cmp(&e_b.name))
            });

            scored.into_iter().map(|(e, _)| e).collect()
        }

        #[cfg(not(all(target_os = "macos", feature = "fsearch-macos")))]
        {
            PortableFilter.filter(entries, query)
        }
    }

    fn name(&self) -> &'static str {
        #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
        {
            "fsearch"
        }
        #[cfg(not(all(target_os = "macos", feature = "fsearch-macos")))]
        {
            "portable"
        }
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
        assert_eq!(FsearchFinder::default().filter(&src, "   ").len(), 2);
    }

    #[test]
    fn filter_is_case_insensitive() {
        let src = vec![e("README.md"), e("notes.txt")];
        let hits = PortableFilter.filter(&src, "readme");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "README.md");

        let fhits = FsearchFinder::default().filter(&src, "readme");
        assert_eq!(fhits.len(), 1);
        assert_eq!(fhits[0].name, "README.md");
    }

    #[test]
    fn fsearch_matches_fuzzy_acronyms_and_substrings() {
        let src = vec![
            e("Cargo.toml"),
            e("Cargo.lock"),
            e("package-dmg.sh"),
            e("AppIcon.icns"),
            e("shell.rs"),
        ];

        let finder = FsearchFinder::default();
        let hits = finder.filter(&src, "dmg");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "package-dmg.sh");

        let cargo_hits = finder.filter(&src, "cargo");
        assert_eq!(cargo_hits.len(), 2);
    }

    #[test]
    fn fsearch_ranks_exact_prefix_matches_ahead_of_fuzzy() {
        let src = vec![
            e("application_support.rs"),
            e("app.rs"),
            e("apple.rs"),
        ];

        let finder = FsearchFinder::default();
        let hits = finder.filter(&src, "app");
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].name, "app.rs");
    }

    #[test]
    fn provider_names() {
        assert_eq!(PortableFilter.name(), "portable");
        #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
        assert_eq!(FsearchFinder::default().name(), "fsearch");
        #[cfg(not(all(target_os = "macos", feature = "fsearch-macos")))]
        assert_eq!(FsearchFinder::default().name(), "portable");
    }
}
