//! In-process content search engine (PLAN §3: instant fsearch on mac, in-process ripgrep everywhere).

use std::path::{Path, PathBuf};
use std::time::Instant;

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;

/// A single matched line within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentMatch {
    pub path: PathBuf,
    pub line_number: usize,
    pub line_text: String,
}

/// The result of a content search query.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentSearchResult {
    pub query: String,
    pub matches: Vec<ContentMatch>,
    pub searched_files: usize,
    pub duration_ms: u64,
}

struct MatchSink<'a> {
    path: &'a Path,
    matches: &'a mut Vec<ContentMatch>,
    max_matches: usize,
}

impl<'a> Sink for MatchSink<'a> {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let line_number = mat.line_number().unwrap_or(1) as usize;
        let line_text = String::from_utf8_lossy(mat.bytes()).trim().to_string();
        self.matches.push(ContentMatch {
            path: self.path.to_path_buf(),
            line_number,
            line_text,
        });
        Ok(self.matches.len() < self.max_matches)
    }
}

/// Perform in-process ripgrep search over `root` directory matching `query`.
pub fn ripgrep_search(root: &Path, query: &str, limit: usize) -> ContentSearchResult {
    let start = Instant::now();
    let q = query.trim();
    if q.is_empty() || !root.exists() {
        return ContentSearchResult {
            query: query.to_string(),
            matches: Vec::new(),
            searched_files: 0,
            duration_ms: 0,
        };
    }

    let Ok(matcher) = RegexMatcherBuilder::new()
        .case_smart(true)
        .build_literals(&[q]) else {
        return ContentSearchResult {
            query: query.to_string(),
            matches: Vec::new(),
            searched_files: 0,
            duration_ms: start.elapsed().as_millis() as u64,
        };
    };

    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();

    let walker = WalkBuilder::new(root)
        .hidden(true) // ignore hidden files by default for snappiness
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .max_filesize(Some(2 * 1024 * 1024)) // 2 MB file guard
        .build();

    let mut matches = Vec::new();
    let mut searched_files = 0;

    for result in walker {
        if matches.len() >= limit {
            break;
        }
        let Ok(entry) = result else {
            continue;
        };
        if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            searched_files += 1;
            let path = entry.path();
            let mut sink = MatchSink {
                path,
                matches: &mut matches,
                max_matches: limit,
            };
            let _ = searcher.search_path(&matcher, path, &mut sink);
        }
    }

    ContentSearchResult {
        query: query.to_string(),
        matches,
        searched_files,
        duration_ms: start.elapsed().as_millis() as u64,
    }
}

/// Unified entry point for content search.
/// Dispatches to fsearch indexed grep where available on macOS, falling back to ripgrep.
pub fn search_content(root: &Path, query: &str, limit: usize) -> ContentSearchResult {
    #[cfg(all(target_os = "macos", feature = "fsearch-macos"))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty()
            && let Ok(grep) = fsearch::Grep::new(query.trim(), fsearch::GrepMode::Literal)
            && let Ok(q) = fsearch::Query::parse(query.trim(), &home)
        {
            let dir = fsearch::default_dir(&home);
            if let Ok(engine) = fsearch::Engine::start(fsearch::Options {
                dir,
                home,
                skip: None,
            }) {
                    let start = Instant::now();
                    if let Ok((res, _)) = engine.grep(&q, &grep) {
                        let mut matches = Vec::new();
                        for file in res.files {
                            let path_str = String::from_utf8_lossy(&file.path).to_string();
                            let path = PathBuf::from(path_str);
                            if path.starts_with(root) {
                                for (line_num, line_str) in file.lines {
                                    matches.push(ContentMatch {
                                        path: path.clone(),
                                        line_number: line_num,
                                        line_text: line_str.trim().to_string(),
                                    });
                                    if matches.len() >= limit {
                                        break;
                                    }
                                }
                            }
                            if matches.len() >= limit {
                                break;
                            }
                        }
                        if !matches.is_empty() {
                            return ContentSearchResult {
                                query: query.to_string(),
                                matches,
                                searched_files: res.read,
                                duration_ms: start.elapsed().as_millis() as u64,
                            };
                        }
                    }
                }
            }
        }

    // Portable in-process ripgrep fallback
    ripgrep_search(root, query, limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_ripgrep_search_finds_literal_matches() {
        let temp_dir = std::env::temp_dir().join(format!("ff_test_rg_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let file_a = temp_dir.join("a.rs");
        let file_b = temp_dir.join("b.txt");
        let file_bin = temp_dir.join("binary.bin");

        fs::write(&file_a, "fn main() {\n    println!(\"hello world\");\n}\n").unwrap();
        fs::write(&file_b, "first line\nhello from text file\nthird line\n").unwrap();
        fs::write(&file_bin, b"hello\x00world binary").unwrap();

        let res = ripgrep_search(&temp_dir, "hello", 10);
        assert_eq!(res.matches.len(), 2);
        assert!(res.matches.iter().any(|m| m.path == file_a && m.line_number == 2));
        assert!(res.matches.iter().any(|m| m.path == file_b && m.line_number == 2));
        // Binary was skipped
        assert!(!res.matches.iter().any(|m| m.path == file_bin));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
