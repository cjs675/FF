//! File system operations: trash, rename, create file, create dir.
//! Cross-platform with OS-correct trash and recovery.

use std::fs;
use std::path::{Path, PathBuf};

/// Trash a file or directory safely.
/// Returns the resulting path in the Trash if available (e.g. on macOS via NSFileManager),
/// allowing instant programmatic undo/restore.
pub fn trash(path: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let file_mgr = NSFileManager::defaultManager();
        let path_str = path.to_str().ok_or_else(|| "Path is not valid UTF-8".to_string())?;
        let ns_path = NSString::from_str(path_str);
        let url = NSURL::fileURLWithPath(&ns_path);

        let mut resulting_url: Option<objc2::rc::Retained<NSURL>> = None;
        let res = file_mgr.trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting_url));
        match res {
            Ok(()) => {
                let trash_path = resulting_url.and_then(|u| {
                    u.path().map(|p| PathBuf::from(p.to_string()))
                });
                Ok(trash_path)
            }
            Err(_err) => {
                // If NSFileManager fails, fall back to the trash crate
                trash::delete(path).map_err(|e| format!("Failed to move to trash: {e}"))?;
                Ok(None)
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(|e| format!("Failed to move to trash: {e}"))?;
        Ok(None)
    }
}

/// Rename or move a file/directory.
pub fn rename(from: &Path, to: &Path) -> Result<(), String> {
    if !from.exists() {
        return Err(format!("Source does not exist: {}", from.display()));
    }
    if to.exists() {
        return Err(format!("Destination already exists: {}", to.display()));
    }
    fs::rename(from, to).map_err(|e| format!("Failed to rename: {e}"))
}

/// Create a new empty file. Fails if the file already exists.
pub fn create_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("File already exists: {}", path.display()));
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
        .map_err(|e| format!("Failed to create file: {e}"))
}

/// Create a new directory. Fails if directory already exists.
pub fn create_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("Directory already exists: {}", path.display()));
    }
    fs::create_dir(path).map_err(|e| format!("Failed to create directory: {e}"))
}

/// Restore a previously trashed file/directory by moving it back from Trash.
pub fn restore_trash(trash_path: &Path, original_path: &Path) -> Result<(), String> {
    if !trash_path.exists() {
        return Err(format!("Trashed item no longer exists at: {}", trash_path.display()));
    }
    if original_path.exists() {
        return Err(format!(
            "Cannot restore: item already exists at original location: {}",
            original_path.display()
        ));
    }
    fs::rename(trash_path, original_path).map_err(|e| format!("Failed to restore from trash: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_rename_trash_restore_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!("ff_test_ops_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let file_a = temp_dir.join("test_lifecycle.txt");
        let file_b = temp_dir.join("test_lifecycle_renamed.txt");

        // 1. Create file
        assert!(create_file(&file_a).is_ok());
        assert!(file_a.exists());
        // Fails to create existing
        assert!(create_file(&file_a).is_err());

        // 2. Rename file
        assert!(rename(&file_a, &file_b).is_ok());
        assert!(!file_a.exists());
        assert!(file_b.exists());

        // 3. Trash file
        let trash_res = trash(&file_b);
        assert!(trash_res.is_ok());
        assert!(!file_b.exists());

        // 4. If trash_path was returned (macOS), restore it
        if let Ok(Some(trash_path)) = trash_res {
            assert!(trash_path.exists());
            assert!(restore_trash(&trash_path, &file_b).is_ok());
            assert!(file_b.exists());
            assert!(!trash_path.exists());
            // Clean up
            let _ = fs::remove_file(&file_b);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_create_dir_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!("ff_test_ops_dir_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let dir_a = temp_dir.join("new_folder");
        assert!(create_dir(&dir_a).is_ok());
        assert!(dir_a.is_dir());
        // Duplicate fails
        assert!(create_dir(&dir_a).is_err());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
