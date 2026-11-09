//! Off-thread file and directory preview inspection for the Peek pane.
//!
//! Invariant: File system inspection and image decoding occur exclusively on the
//! background worker thread, delivering pure data payloads (`PeekPayload`) to the UI.

use std::fs;
use std::io::Read;
use std::path::Path;

/// Maximum text payload size to read into memory (128 KB).
pub const MAX_PEEK_TEXT_BYTES: usize = 128 * 1024;

/// Maximum file size eligible for full preview (10 MB). Files exceeding this
/// are guarded with `PeekPayload::TooLarge` to preserve memory and latency.
pub const MAX_PEEK_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum image byte size eligible for decode (5 MB).
pub const MAX_PEEK_IMAGE_SIZE: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeekPayload {
    Empty,
    Directory {
        count: usize,
        size: u64,
        mtime_secs: i64,
        preview_entries: Vec<crate::model::Entry>,
    },
    FileText {
        content: String,
        total_size: u64,
        truncated: bool,
        git_diff: Option<String>,
    },
    Image {
        width: u32,
        height: u32,
        total_size: u64,
        rgba: Vec<u8>,
    },
    Binary {
        size: u64,
    },
    TooLarge {
        size: u64,
    },
    Failed {
        error: String,
    },
}

/// Inspects `path` and builds a preview payload. Must be called on worker thread.
pub fn load_peek(path: &Path) -> PeekPayload {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(err) => return PeekPayload::Failed { error: err.to_string() },
    };

    if metadata.is_dir() {
        let mut count = 0;
        let mut size = 0;
        let mut preview_entries = Vec::new();
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                count += 1;
                let ft = entry.file_type().ok();
                let kind = match ft {
                    Some(t) if t.is_dir() => crate::model::EntryKind::Dir,
                    Some(t) if t.is_symlink() => crate::model::EntryKind::Symlink,
                    _ => crate::model::EntryKind::File,
                };
                if preview_entries.len() < 32 {
                    preview_entries.push(crate::model::Entry {
                        name: entry.file_name().to_string_lossy().into_owned(),
                        path: entry.path(),
                        kind,
                    });
                }
                if let Ok(meta) = entry.metadata()
                    && meta.is_file()
                {
                    size += meta.len();
                }
            }
        }
        crate::scan::sort_entries(&mut preview_entries);
        let mtime_secs = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        return PeekPayload::Directory {
            count,
            size,
            mtime_secs,
            preview_entries,
        };
    }

    let total_size = metadata.len();
    if total_size > MAX_PEEK_FILE_SIZE {
        return PeekPayload::TooLarge { size: total_size };
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Check if path is a known image format
    if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico"
    ) && total_size <= MAX_PEEK_IMAGE_SIZE
        && let Ok(bytes) = fs::read(path)
        && let Ok(img) = image::load_from_memory(&bytes)
    {
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        return PeekPayload::Image {
            width,
            height,
            total_size,
            rgba: rgba.into_raw(),
        };
    }

    // Read head of file up to MAX_PEEK_TEXT_BYTES
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(err) => return PeekPayload::Failed { error: err.to_string() },
    };

    let mut buffer = Vec::new();
    let mut handle = (&mut file).take(MAX_PEEK_TEXT_BYTES as u64);
    if let Err(err) = handle.read_to_end(&mut buffer) {
        return PeekPayload::Failed { error: err.to_string() };
    }

    // Check for null bytes indicating binary content (first 8 KB)
    let check_len = buffer.len().min(8192);
    if buffer[..check_len].contains(&0) {
        return PeekPayload::Binary { size: total_size };
    }

    let truncated = total_size > buffer.len() as u64;
    let content = String::from_utf8_lossy(&buffer).into_owned();
    let git_diff = crate::ops::git::get_file_diff(path);

    PeekPayload::FileText {
        content,
        total_size,
        truncated,
        git_diff,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_peek_text_file() {
        let temp_dir = std::env::temp_dir().join(format!("ff_test_peek_text_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("sample.rs");
        let mut f = fs::File::create(&file_path).unwrap();
        writeln!(f, "fn main() {{\n    println!(\"hello\");\n}}").unwrap();

        let payload = load_peek(&file_path);
        match payload {
            PeekPayload::FileText { content, total_size, truncated, .. } => {
                assert!(content.contains("fn main()"));
                assert!(!truncated);
                assert!(total_size > 0);
            }
            other => panic!("expected FileText, got {:?}", other),
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_peek_binary_file() {
        let temp_dir = std::env::temp_dir().join(format!("ff_test_peek_bin_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("sample.bin");
        let mut f = fs::File::create(&file_path).unwrap();
        f.write_all(&[0x7f, 0x45, 0x4c, 0x46, 0x00, 0x01, 0x01]).unwrap();

        let payload = load_peek(&file_path);
        match payload {
            PeekPayload::Binary { size } => {
                assert_eq!(size, 7);
            }
            other => panic!("expected Binary, got {:?}", other),
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_peek_directory() {
        let temp_dir = std::env::temp_dir().join(format!("ff_test_peek_dir_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let f1 = temp_dir.join("a.txt");
        let mut f = fs::File::create(&f1).unwrap();
        writeln!(f, "hello").unwrap();

        let payload = load_peek(&temp_dir);
        match payload {
            PeekPayload::Directory { count, size, preview_entries, .. } => {
                assert_eq!(count, 1);
                assert!(size > 0);
                assert_eq!(preview_entries.len(), 1);
            }
            other => panic!("expected Directory, got {:?}", other),
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
