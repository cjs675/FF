//! Session persistence (PLAN §5 launch): last dir, selection, scroll.
//!
//! Flat `key = value` in the cfg.rs house style, but machine-written only:
//! `cwd` and `selection` (the selected filename; the anchor machinery re-seats
//! it on relaunch and `scroll_to_me` restores its visibility). Values escape
//! literal backslashes and newlines — both are legal filename bytes on unix.
//! Writes are atomic (tmp + rename) and never on the UI thread: the scan
//! worker debounces them during the session, and app exit writes once after
//! the render loop is gone (not mid-frame, so the invariant in PLAN §1 holds).

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionState {
    pub cwd: PathBuf,
    pub selection: Option<String>,
}

pub fn default_state() -> SessionState {
    SessionState {
        cwd: home_dir(),
        selection: None,
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
        out
    }
}

/// Parses stored state; `None` = corrupt/absent, the caller falls back to
/// defaults. Empty `cwd` is rejected (an exit-frame placeholder must never
/// be restored). Unknown keys are silently tolerated.
pub fn parse(text: &str) -> Option<SessionState> {
    let mut cwd: Option<PathBuf> = None;
    let mut selection: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unesc(value.trim());
        match key.trim() {
            "cwd" => cwd = Some(PathBuf::from(value)),
            "selection"
                if !value.is_empty() => {
                    selection = Some(value);
                }
            _ => {}
        }
    }
    let cwd = cwd.filter(|p| !p.as_os_str().is_empty())?;
    Some(SessionState { cwd, selection })
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
        };
        assert_eq!(parse(&state.encode()), Some(state));
    }

    #[test]
    fn missing_selection_stays_absent_and_empty_cwd_rejected() {
        let bare = SessionState {
            cwd: PathBuf::from("/x"),
            selection: None,
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
}
