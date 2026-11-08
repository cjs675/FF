//! Session persistence (PLAN §5 launch): last dir, selection, scroll, frequents.
//!
//! Flat `key = value` in the cfg.rs house style, but machine-written only:
//! `cwd`, `selection`, and `frequent` entries (path, count, last_visited).
//! Values escape literal backslashes and newlines — both are legal filename
//! bytes on unix. Writes are atomic (tmp + rename) and never on the UI thread:
//! the scan worker debounces them during the session, and app exit writes once
//! after the render loop is gone (not mid-frame, so the invariant in PLAN §1 holds).

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrequentEntry {
    pub path: PathBuf,
    pub count: u64,
    pub last_visited: u64,
}

impl FrequentEntry {
    pub fn frecency_score(&self, now: u64) -> f64 {
        let age_secs = now.saturating_sub(self.last_visited);
        let weight = if age_secs < 4 * 3600 {
            4.0
        } else if age_secs < 24 * 3600 {
            2.0
        } else if age_secs < 7 * 86400 {
            1.0
        } else if age_secs < 30 * 86400 {
            0.5
        } else {
            0.25
        };
        (self.count as f64) * weight
    }
}

pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn record_visit(entries: &mut Vec<FrequentEntry>, path: &Path, now: u64) {
    if path.as_os_str().is_empty() {
        return;
    }
    if let Some(existing) = entries.iter_mut().find(|e| e.path == path) {
        existing.count = existing.count.saturating_add(1);
        existing.last_visited = now;
    } else {
        entries.push(FrequentEntry {
            path: path.to_path_buf(),
            count: 1,
            last_visited: now,
        });
    }

    if entries.len() > 128 {
        entries.sort_by(|a, b| {
            b.frecency_score(now)
                .partial_cmp(&a.frecency_score(now))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries.truncate(128);
    }
}

pub fn ranked_frequents(entries: &[FrequentEntry], now: u64) -> Vec<FrequentEntry> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| {
        b.frecency_score(now)
            .partial_cmp(&a.frecency_score(now))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.last_visited.cmp(&a.last_visited))
            .then_with(|| a.path.cmp(&b.path))
    });
    sorted
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionState {
    pub cwd: PathBuf,
    pub selection: Option<String>,
    pub frequents: Vec<FrequentEntry>,
}

pub fn default_state() -> SessionState {
    SessionState {
        cwd: home_dir(),
        selection: None,
        frequents: Vec::new(),
    }
}

pub fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Where last-session state lives, per OS convention: macOS
/// `~/Library/Application Support/ff/session`, Windows `%APPDATA%\ff`,
/// unix the XDG state dir (`~/.local/state/ff/session`).
pub fn default_path() -> Option<PathBuf> {
    let home = home_dir();
    if cfg!(target_os = "macos") {
        return Some(
            home.join("Library")
                .join("Application Support")
                .join("ff")
                .join("session"),
        );
    }
    if cfg!(target_os = "windows") {
        let base = env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming"));
        return Some(base.join("ff").join("session"));
    }
    let base = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("state"));
    Some(base.join("ff").join("session"))
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl SessionState {
    pub fn encode(&self) -> String {
        let mut out = String::new();
        out.push_str("cwd = ");
        out.push_str(&esc(&self.cwd.to_string_lossy()));
        out.push('\n');
        if let Some(selection) = &self.selection {
            out.push_str("selection = ");
            out.push_str(&esc(selection));
            out.push('\n');
        }
        for freq in &self.frequents {
            out.push_str("frequent = ");
            out.push_str(&format!(
                "{}:{}:{}\n",
                freq.last_visited,
                freq.count,
                esc(&freq.path.to_string_lossy())
            ));
        }
        out
    }
}

/// Parses stored state; `None` = corrupt/absent, the caller falls back to
/// defaults. Empty `cwd` is rejected (an exit-frame placeholder must never
/// be restored). Unknown keys are silently tolerated.
pub fn parse(text: &str) -> Option<SessionState> {
    let mut cwd: Option<PathBuf> = None;
    let mut selection: Option<String> = None;
    let mut frequents: Vec<FrequentEntry> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let raw_val = value.trim();
        match key.trim() {
            "cwd" => {
                let value = unesc(raw_val);
                cwd = Some(PathBuf::from(value));
            }
            "selection" => {
                let value = unesc(raw_val);
                if !value.is_empty() {
                    selection = Some(value);
                }
            }
            "frequent" => {
                if let Some((ts_str, rest)) = raw_val.split_once(':')
                    && let Some((cnt_str, path_esc)) = rest.split_once(':')
                    && let Ok(last_visited) = ts_str.parse::<u64>()
                    && let Ok(count) = cnt_str.parse::<u64>()
                {
                    let path_str = unesc(path_esc);
                    if !path_str.is_empty() {
                        frequents.push(FrequentEntry {
                            path: PathBuf::from(path_str),
                            count,
                            last_visited,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    let cwd = cwd.filter(|p| !p.as_os_str().is_empty())?;
    Some(SessionState {
        cwd,
        selection,
        frequents,
    })
}

/// Missing or corrupt file means "no stored state": `load` maps every error
/// to `None` rather than failing the launch.
pub fn load(path: &Path) -> Option<SessionState> {
    let text = fs::read_to_string(path).ok()?;
    parse(&text)
}

/// Atomic write: tmp file in the same directory, then rename over the target.
pub fn save(path: &Path, state: &SessionState) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if parent.is_file() {
            let _ = fs::remove_file(parent);
        }
        let _ = fs::create_dir_all(parent);
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, state.encode())?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_selection_with_special_chars() {
        let state = SessionState {
            cwd: PathBuf::from("/tmp/we ird"),
            selection: Some("a=b\\x.txt\nline2.md".to_string()),
            frequents: vec![
                FrequentEntry {
                    path: PathBuf::from("/tmp/docs"),
                    count: 5,
                    last_visited: 1700000000,
                },
            ],
        };
        assert_eq!(parse(&state.encode()), Some(state));
    }

    #[test]
    fn missing_selection_stays_absent_and_empty_cwd_rejected() {
        let bare = SessionState {
            cwd: PathBuf::from("/x"),
            selection: None,
            frequents: Vec::new(),
        };
        assert_eq!(parse(&bare.encode()), Some(bare));
        assert_eq!(parse("selection = only\n"), None); // cwd is mandatory
        assert_eq!(parse("cwd = \n"), None); // and must be non-empty
    }

    #[test]
    fn save_then_load_roundtrip_atomically() {
        let dir = std::env::temp_dir().join(format!(
            "ff-session-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("session");
        let state = SessionState {
            cwd: dir.clone(),
            selection: Some("one.txt".into()),
            frequents: vec![FrequentEntry {
                path: dir.clone(),
                count: 3,
                last_visited: 1234567,
            }],
        };
        save(&path, &state).unwrap();
        assert_eq!(load(&path), Some(state));
        assert!(!path.with_extension("tmp").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_path_targets_platform_file() {
        if let Some(p) = default_path() {
            assert_eq!(p.file_name().and_then(|n| n.to_str()), Some("session"));
            assert!(
                p.ancestors()
                    .any(|a| a.file_name().and_then(|n| n.to_str()) == Some("ff"))
            );
        }
    }

    #[test]
    fn frecency_decay_and_ranking() {
        let now = 1000000;
        let mut entries = Vec::new();
        // Visit older path 10 times (10 days ago)
        entries.push(FrequentEntry {
            path: PathBuf::from("/older"),
            count: 10,
            last_visited: now - (10 * 86400),
        });
        // Visit recent path 4 times (1 hour ago)
        entries.push(FrequentEntry {
            path: PathBuf::from("/recent"),
            count: 4,
            last_visited: now - 3600,
        });

        // Recent: 4 * 4.0 = 16.0
        // Older: 10 * 0.5 = 5.0
        let ranked = ranked_frequents(&entries, now);
        assert_eq!(ranked[0].path, PathBuf::from("/recent"));
        assert_eq!(ranked[1].path, PathBuf::from("/older"));

        // Record a new visit to /older right now -> count 11, age 0 -> 11 * 4.0 = 44.0
        record_visit(&mut entries, Path::new("/older"), now);
        let updated_ranked = ranked_frequents(&entries, now);
        assert_eq!(updated_ranked[0].path, PathBuf::from("/older"));
        assert_eq!(updated_ranked[0].count, 11);
    }
}
