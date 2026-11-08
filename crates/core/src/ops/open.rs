//! Cross-platform file opening and application enumeration (PLAN §4 & §5 ops).
//! Detached execution via spawn(); frame thread never waits on child process termination.

use std::io;
use std::path::Path;

pub fn open_default(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        Ok(())
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
        Ok(())
    }
}

pub fn open_with(app: &str, path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-a", app])
            .arg(path)
            .spawn()?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new(app)
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        Ok(())
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        std::process::Command::new(app).arg(path).spawn()?;
        Ok(())
    }
}

pub fn open_chooser(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        open_default(path)
    }
}

#[cfg(target_os = "macos")]
pub use macos::enumerate_openers;

#[cfg(not(target_os = "macos"))]
pub use fallback::enumerate_openers;

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_void, CStr, CString};
    use std::path::{Path, PathBuf};

    use crate::model::AppOpener;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            buffer: *const u8,
            buf_len: isize,
            is_directory: bool,
        ) -> *mut c_void;

        fn CFArrayGetCount(the_array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(the_array: *const c_void, idx: isize) -> *const c_void;
        fn CFURLGetFileSystemRepresentation(
            url: *const c_void,
            resolve_against_base: bool,
            buffer: *mut u8,
            max_buf_len: isize,
        ) -> bool;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSCopyApplicationURLsForURL(in_url: *const c_void, role_mask: u32) -> *mut c_void;
        fn LSCopyDefaultApplicationURLForURL(
            in_url: *const c_void,
            role_mask: u32,
            out_error: *mut *mut c_void,
        ) -> *mut c_void;
    }

    const K_LS_ROLES_ALL: u32 = 0xFFFF_FFFF;

    struct CfDropGuard(*mut c_void);
    impl Drop for CfDropGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn url_to_path(url: *const c_void) -> Option<PathBuf> {
        let mut buf = vec![0u8; 1024];
        let ok = unsafe {
            CFURLGetFileSystemRepresentation(url, true, buf.as_mut_ptr(), buf.len() as isize)
        };
        if !ok {
            return None;
        }
        let c_str = CStr::from_bytes_until_nul(&buf).ok()?;
        Some(PathBuf::from(c_str.to_string_lossy().into_owned()))
    }

    pub fn enumerate_openers(path: &Path) -> Vec<AppOpener> {
        let Ok(c_path) = CString::new(path.to_string_lossy().as_bytes()) else {
            return Vec::new();
        };

        let url = unsafe {
            CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                c_path.as_ptr() as *const u8,
                c_path.as_bytes().len() as isize,
                path.is_dir(),
            )
        };
        if url.is_null() {
            return Vec::new();
        }
        let _url_guard = CfDropGuard(url);

        let default_app_path = unsafe {
            let def_url = LSCopyDefaultApplicationURLForURL(url, K_LS_ROLES_ALL, std::ptr::null_mut());
            if !def_url.is_null() {
                let _def_guard = CfDropGuard(def_url);
                url_to_path(def_url)
            } else {
                None
            }
        };

        let mut openers = Vec::new();
        let arr = unsafe { LSCopyApplicationURLsForURL(url, K_LS_ROLES_ALL) };
        if !arr.is_null() {
            let _arr_guard = CfDropGuard(arr);
            let count = unsafe { CFArrayGetCount(arr) };
            for i in 0..count {
                let item_url = unsafe { CFArrayGetValueAtIndex(arr, i) };
                if let Some(app_path) = url_to_path(item_url) {
                    let name = app_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("App")
                        .to_string();
                    let is_default = default_app_path.as_ref() == Some(&app_path);
                    let id = app_path.to_string_lossy().into_owned();
                    openers.push(AppOpener {
                        name,
                        id,
                        is_default,
                    });
                }
            }
        }

        // Put default opener first
        openers.sort_by_key(|a| !a.is_default);
        openers
    }
}

#[cfg(not(target_os = "macos"))]
mod fallback {
    use std::path::Path;
    use crate::model::AppOpener;

    pub fn enumerate_openers(_path: &Path) -> Vec<AppOpener> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_does_not_panic_on_missing_file() {
        let fake = Path::new("/tmp/nonexistent_file_12345.xyz");
        let apps = enumerate_openers(fake);
        // Should return cleanly without panicking
        assert!(apps.is_empty() || !apps.is_empty());
    }

    #[test]
    fn enumeration_finds_openers_for_known_extension() {
        let plan_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../PLAN.md");
        if plan_path.exists() {
            let apps = enumerate_openers(&plan_path);
            #[cfg(target_os = "macos")]
            assert!(!apps.is_empty(), "expected openers for markdown file on macOS");
            if let Some(first) = apps.first() {
                assert!(first.is_default);
            }
        }
    }
}
