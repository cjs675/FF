//! Entry types. Columns (size, mtime) stat lazily later (PLAN §2); the
//! base model stays name+kind so the no-stat listing contract is type-level
//! enforced from day one.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    /// Listed via dirents; never followed at listing time (that would be
    /// a stat). Descending into one is attempted and failures surface.
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, EntryKind::Dir)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: String,
    pub is_dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileGitStatus {
    Modified,
    Staged,
    Untracked,
    Deleted,
    Renamed,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppOpener {
    pub name: String,
    pub id: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntryStats {
    pub size: u64,
    pub mtime_secs: i64,
}

impl EntryStats {
    pub fn format_size(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;
        const GB: u64 = 1024 * MB;
        if self.size >= GB {
            format!("{:.1} GB", self.size as f64 / GB as f64)
        } else if self.size >= MB {
            format!("{:.1} MB", self.size as f64 / MB as f64)
        } else if self.size >= KB {
            format!("{:.1} KB", self.size as f64 / KB as f64)
        } else {
            format!("{} B", self.size)
        }
    }

    pub fn format_mtime(&self) -> String {
        if self.mtime_secs <= 0 {
            return "-".to_string();
        }
        format_unix_timestamp(self.mtime_secs)
    }
}

/// Formats unix timestamp as YYYY-MM-DD HH:MM via civil date calculation (no chrono dependency).
fn format_unix_timestamp(secs: i64) -> String {
    if secs <= 0 {
        return "-".to_string();
    }
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;

    // Civil day algorithm (Howard Hinnant formula)
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1024 + doe / 1461 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_month = if mp < 10 { mp + 3 } else { mp - 9 };
    let y_year = if m_month <= 2 { y + 1 } else { y };

    format!("{y_year:04}-{m_month:02}-{d:02} {h:02}:{m:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_units() {
        assert_eq!(EntryStats { size: 500, mtime_secs: 0 }.format_size(), "500 B");
        assert_eq!(EntryStats { size: 1024, mtime_secs: 0 }.format_size(), "1.0 KB");
        assert_eq!(EntryStats { size: 1536, mtime_secs: 0 }.format_size(), "1.5 KB");
        assert_eq!(EntryStats { size: 1048576, mtime_secs: 0 }.format_size(), "1.0 MB");
        assert_eq!(EntryStats { size: 1073741824, mtime_secs: 0 }.format_size(), "1.0 GB");
    }

    #[test]
    fn format_mtime_epoch_and_recent() {
        assert_eq!(EntryStats { size: 0, mtime_secs: 0 }.format_mtime(), "-");
        // 1700000000 = 2023-11-14 22:13:20 UTC
        assert_eq!(EntryStats { size: 0, mtime_secs: 1700000000 }.format_mtime(), "2023-11-14 22:13");
    }
}
