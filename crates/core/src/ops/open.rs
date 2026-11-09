//! Cross-platform file opening and application enumeration (PLAN §4 & §5 ops).
//! Detached execution via spawn(); frame thread never waits on child process termination.

use std::io;
use std::path::{Path, PathBuf};

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

/// Checks if an application identifier refers to a CLI terminal editor.
pub fn is_terminal_editor(app: &str) -> bool {
    let lower = app.trim().to_lowercase();
    let name = Path::new(&lower)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&lower);
    matches!(
        name,
        "nvim" | "neovim" | "vim" | "vi" | "nano" | "emacs" | "helix" | "hx" | "micro" | "kak"
    )
}

/// Resolves the absolute path to an editor binary if found on the system.
pub fn resolve_editor_binary(editor: &str) -> String {
    if editor.contains('/') && Path::new(editor).exists() {
        return editor.to_string();
    }
    let candidates = [
        format!("/opt/homebrew/bin/{editor}"),
        format!("/usr/local/bin/{editor}"),
        format!("/usr/bin/{editor}"),
        format!("/bin/{editor}"),
    ];
    for cand in &candidates {
        if Path::new(cand).exists() {
            return cand.clone();
        }
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let p = dir.join(editor);
            if p.is_file() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    editor.to_string()
}

/// Opens a file in a terminal editor (e.g. nvim) inside a terminal window (Ghostty on macOS, Terminal fallback).
pub fn open_in_terminal_editor(editor: &str, path: &Path) -> io::Result<()> {
    let resolved_editor = resolve_editor_binary(editor);
    #[cfg(target_os = "macos")]
    {
        let ghostty_app = Path::new("/Applications/Ghostty.app");
        let user_ghostty = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Applications/Ghostty.app"));
        let ghostty_exists = ghostty_app.exists()
            || user_ghostty.as_ref().is_some_and(|p| p.exists());

        if ghostty_exists {
            std::process::Command::new("open")
                .args(["-na", "Ghostty.app", "--args", "-e", &resolved_editor])
                .arg(path)
                .spawn()?;
            return Ok(());
        }

        // Fallback to Terminal.app via osascript
        let script = format!(
            "tell application \"Terminal\" to do script \"{} \\\"{}\\\"\"\ntell application \"Terminal\" to activate",
            resolved_editor,
            path.display()
        );
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn()?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = std::process::Command::new("cmd")
            .args(["/C", "start", "wt", &resolved_editor])
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        if status.is_err() {
            std::process::Command::new("cmd")
                .args(["/C", "start", "cmd", "/k", &resolved_editor])
                .arg(path)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()?;
        }
        Ok(())
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        for term in &["ghostty", "alacritty", "kitty", "wezterm", "gnome-terminal", "x-terminal-emulator", "xterm"] {
            if std::process::Command::new("which").arg(term).output().map(|o| o.status.success()).unwrap_or(false) {
                let mut cmd = std::process::Command::new(term);
                if *term == "gnome-terminal" {
                    cmd.arg("--");
                } else {
                    cmd.arg("-e");
                }
                cmd.arg(&resolved_editor).arg(path);
                if cmd.spawn().is_ok() {
                    return Ok(());
                }
            }
        }
        std::process::Command::new(&resolved_editor).arg(path).spawn()?;
        Ok(())
    }
}

/// Routes application opening to either terminal emulator or GUI app launcher.
pub fn open_app(app: &str, path: &Path) -> io::Result<()> {
    if is_terminal_editor(app) {
        open_in_terminal_editor(app, path)
    } else {
        open_with(app, path)
    }
}

/// Determines whether a file is source code, script, configuration, or plain text.
pub fn is_text_or_code(path: &Path, ext: &str, name: &str) -> bool {
    const CODE_EXTS: &[&str] = &[
        "sh", "bash", "zsh", "fish", "ksh", "csh", "awk", "sed",
        "rs", "py", "pyw", "rb", "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx",
        "go", "zig", "swift", "kt", "kts", "scala", "cs", "fs", "lua", "php", "pl", "pm", "tcl", "r", "jl",
        "ex", "exs", "erl", "hrl", "clj", "cljs", "edn", "hs", "lhs", "ml", "mli", "lisp", "el", "scm", "rkt",
        "nim", "d", "pas", "v", "dart", "html", "htm", "css", "scss", "sass", "less", "vue", "svelte", "astro",
        "json", "json5", "jsonc", "yaml", "yml", "toml", "xml", "ini", "cfg", "conf", "env", "properties",
        "sql", "graphql", "gql", "proto", "csv", "tsv", "md", "markdown", "rst", "adoc", "txt", "text",
        "log", "diff", "patch", "tex", "bib", "cmake", "justfile",
    ];

    if CODE_EXTS.contains(&ext) {
        return true;
    }

    const TEXT_NAMES: &[&str] = &[
        "makefile", "dockerfile", "containerfile", "gemfile", "rakefile", "vagrantfile", "brewfile",
        "cmakelists.txt", "license", "licence", "readme", "contributing", "changelog",
    ];

    if TEXT_NAMES.contains(&name) || name.starts_with('.') {
        return true;
    }

    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        let mut buf = [0u8; 512];
        if let Ok(n) = f.read(&mut buf)
            && n > 0
            && !buf[..n].contains(&0)
            && std::str::from_utf8(&buf[..n]).is_ok()
        {
            return true;
        }
    }

    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartOpenerTarget {
    Custom(String),
    IntelliJ,
    CLion,
    Preview,
    Editor(String),
    Default,
}

/// Resolves the intended application or viewer for a given file path.
pub fn resolve_smart_opener(path: &Path, custom_openers: &[(String, String)]) -> SmartOpenerTarget {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    // 1. Custom opener rules take precedence
    for (rule_target, app) in custom_openers {
        let rule_target = rule_target.to_lowercase();
        if rule_target == ext || rule_target == name {
            return SmartOpenerTarget::Custom(app.clone());
        }
    }

    // 2. Built-in smart rules
    // Java -> IntelliJ IDEA
    if ext == "java" {
        return SmartOpenerTarget::IntelliJ;
    }

    // C/C++ -> CLion
    if matches!(
        ext.as_str(),
        "cpp" | "c" | "h" | "hpp" | "cc" | "cxx" | "c++" | "h++" | "hh" | "hxx" | "tpp" | "inl"
    ) {
        return SmartOpenerTarget::CLion;
    }

    // PDF & Word -> Preview
    if matches!(ext.as_str(), "pdf" | "docx" | "doc") {
        return SmartOpenerTarget::Preview;
    }

    // Scripts, code, configs, text -> nvim
    if is_text_or_code(path, &ext, &name) {
        return SmartOpenerTarget::Editor("nvim".to_string());
    }

    // Fallback to OS default opener
    SmartOpenerTarget::Default
}

/// Opens a file using smart per-extension rules and user custom configuration.
pub fn open_smart(path: &Path, custom_openers: &[(String, String)]) -> io::Result<()> {
    match resolve_smart_opener(path, custom_openers) {
        SmartOpenerTarget::Custom(app) => open_app(&app, path),
        SmartOpenerTarget::IntelliJ => {
            if open_app("IntelliJ IDEA", path).is_ok() || open_app("nvim", path).is_ok() {
                Ok(())
            } else {
                open_default(path)
            }
        }
        SmartOpenerTarget::CLion => {
            if open_app("CLion", path).is_ok() || open_app("nvim", path).is_ok() {
                Ok(())
            } else {
                open_default(path)
            }
        }
        SmartOpenerTarget::Preview => {
            if open_app("Preview", path).is_ok() {
                Ok(())
            } else {
                open_default(path)
            }
        }
        SmartOpenerTarget::Editor(editor) => {
            if open_app(&editor, path).is_ok() {
                Ok(())
            } else {
                open_default(path)
            }
        }
        SmartOpenerTarget::Default => open_default(path),
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
        let md_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
        if md_path.exists() {
            let apps = enumerate_openers(&md_path);
            #[cfg(target_os = "macos")]
            assert!(!apps.is_empty(), "expected openers for markdown file on macOS");
            if let Some(first) = apps.first() {
                assert!(first.is_default);
            }
        }
    }

    #[test]
    fn terminal_editor_detection() {
        assert!(is_terminal_editor("nvim"));
        assert!(is_terminal_editor("neovim"));
        assert!(is_terminal_editor("vim"));
        assert!(is_terminal_editor("nano"));
        assert!(is_terminal_editor("/opt/homebrew/bin/nvim"));
        assert!(!is_terminal_editor("IntelliJ IDEA"));
        assert!(!is_terminal_editor("CLion"));
        assert!(!is_terminal_editor("Preview"));
    }

    #[test]
    fn smart_opener_resolution_builtins() {
        let custom = Vec::new();
        assert_eq!(
            resolve_smart_opener(Path::new("App.java"), &custom),
            SmartOpenerTarget::IntelliJ
        );
        assert_eq!(
            resolve_smart_opener(Path::new("main.cpp"), &custom),
            SmartOpenerTarget::CLion
        );
        assert_eq!(
            resolve_smart_opener(Path::new("header.hpp"), &custom),
            SmartOpenerTarget::CLion
        );
        assert_eq!(
            resolve_smart_opener(Path::new("doc.pdf"), &custom),
            SmartOpenerTarget::Preview
        );
        assert_eq!(
            resolve_smart_opener(Path::new("report.docx"), &custom),
            SmartOpenerTarget::Preview
        );
        assert_eq!(
            resolve_smart_opener(Path::new("deploy.sh"), &custom),
            SmartOpenerTarget::Editor("nvim".to_string())
        );
        assert_eq!(
            resolve_smart_opener(Path::new("lib.rs"), &custom),
            SmartOpenerTarget::Editor("nvim".to_string())
        );
        assert_eq!(
            resolve_smart_opener(Path::new("Makefile"), &custom),
            SmartOpenerTarget::Editor("nvim".to_string())
        );
        assert_eq!(
            resolve_smart_opener(Path::new("photo.png"), &custom),
            SmartOpenerTarget::Default
        );
    }

    #[test]
    fn smart_opener_resolution_custom_overrides() {
        let custom = vec![
            ("java".to_string(), "nvim".to_string()),
            ("pdf".to_string(), "Skim".to_string()),
        ];
        assert_eq!(
            resolve_smart_opener(Path::new("App.java"), &custom),
            SmartOpenerTarget::Custom("nvim".to_string())
        );
        assert_eq!(
            resolve_smart_opener(Path::new("doc.pdf"), &custom),
            SmartOpenerTarget::Custom("Skim".to_string())
        );
        // Unoverridden fallback to builtins
        assert_eq!(
            resolve_smart_opener(Path::new("main.cpp"), &custom),
            SmartOpenerTarget::CLion
        );
    }
}
