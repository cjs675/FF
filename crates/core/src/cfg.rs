//! Ghostty-style flat `key = value` config (PLAN §2).
//!
//! Rules, encoded as tests below:
//! - empty file = fully usable app (all defaults)
//! - unknown keys warn, never fail
//! - comments: whole-line `# ...` and inline trailing `# ...`
//! - `keybind` is repeatable; value shape is `combo=action`

use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Launch {
    #[default]
    Session,
    Frequents,
    Home,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub theme: String,
    pub preview: bool,
    pub preview_syntax: bool,
    pub search: String,
    pub content: String,
    pub launch: Launch,
    pub show_hidden: bool,
    pub keybinds: Vec<(String, String)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "gruvbox-dark".into(),
            preview: true,
            preview_syntax: true,
            search: "auto".into(),
            content: "auto".into(),
            launch: Launch::Session,
            show_hidden: false,
            keybinds: Vec::new(),
        }
    }
}

/// Resolves standard config path for the platform:
/// - macOS / Linux: `$XDG_CONFIG_HOME/ff/config` or `~/.config/ff/config`
/// - Windows: `%APPDATA%\ff\config`
pub fn default_path() -> Option<PathBuf> {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;

    if cfg!(target_os = "windows") {
        let base = env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming"));
        return Some(base.join("ff").join("config"));
    }

    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    Some(base.join("ff").join("config"))
}

/// Parses config text; returns config + warnings. Never hard-fails.
pub fn parse(src: &str) -> (Config, Vec<String>) {
    let mut cfg = Config::default();
    let mut warnings = Vec::new();

    for (n, raw) in src.lines().enumerate() {
        let line_no = n + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = match line.split_once('#') {
            Some((code, _)) => code.trim(),
            None => line,
        };
        let Some((key, value)) = line.split_once('=') else {
            warnings.push(format!(
                "line {line_no}: expected `key = value`, got `{raw}`"
            ));
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        match key {
            "theme" => cfg.theme = value.to_string(),
            "preview" => match value {
                "true" | "peek" => cfg.preview = true,
                "false" | "off" => cfg.preview = false,
                _ => warnings.push(format!("line {line_no}: `preview` wants peek|off (or true|false)")),
            },
            "preview.syntax" => match value {
                "true" => cfg.preview_syntax = true,
                "false" => cfg.preview_syntax = false,
                _ => warnings.push(format!("line {line_no}: `preview.syntax` wants true|false")),
            },
            "search" => match value {
                "auto" | "portable" | "fsearch" => cfg.search = value.to_string(),
                _ => warnings.push(format!(
                    "line {line_no}: `search` wants auto|portable|fsearch, got `{value}`"
                )),
            },
            "content" => match value {
                "auto" | "rg" | "fsearch" | "off" => cfg.content = value.to_string(),
                _ => warnings.push(format!(
                    "line {line_no}: `content` wants auto|rg|fsearch|off, got `{value}`"
                )),
            },
            "launch" => match value {
                "session" => cfg.launch = Launch::Session,
                "frequents" => cfg.launch = Launch::Frequents,
                "home" => cfg.launch = Launch::Home,
                _ => warnings.push(format!(
                    "line {line_no}: `launch` wants session|frequents|home, got `{value}`"
                )),
            },
            "show_hidden" => match value {
                "true" => cfg.show_hidden = true,
                "false" => cfg.show_hidden = false,
                _ => warnings.push(format!("line {line_no}: `show_hidden` wants true|false, got `{value}`")),
            },
            "keybind" => match value.split_once('=') {
                Some((combo, action)) => cfg
                    .keybinds
                    .push((combo.trim().to_string(), action.trim().to_string())),
                None => warnings.push(format!(
                    "line {line_no}: `keybind` value must be `combo=action`, got `{value}`"
                )),
            },
            other => warnings.push(format!("line {line_no}: unknown key `{other}`")),
        }
    }

    (cfg, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_fully_usable_defaults() {
        let (cfg, warnings) = parse("");
        assert_eq!(cfg.theme, "gruvbox-dark");
        assert_eq!(cfg.search, "auto");
        assert!(cfg.keybinds.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_keys_warn_never_fail() {
        let (cfg, warnings) = parse("color_foo = red\n");
        assert_eq!(cfg.theme, "gruvbox-dark");
        assert!(warnings.iter().any(|w| w.contains("unknown key")));
    }

    #[test]
    fn keybind_is_repeatable_and_inline_comments_are_stripped() {
        let (cfg, warnings) = parse(
            "keybind = space=peek\n\
             keybind = cmd+f=find   # open the find layer\n",
        );
        assert_eq!(cfg.keybinds.len(), 2);
        assert_eq!(cfg.keybinds[0], ("space".into(), "peek".into()));
        assert_eq!(cfg.keybinds[1], ("cmd+f".into(), "find".into()));
        assert!(warnings.is_empty());
    }

    #[test]
    fn invalid_enum_values_keep_default_with_warning() {
        let (cfg, warnings) = parse("search = spotlight\n");
        assert_eq!(cfg.search, "auto");
        assert!(warnings.iter().any(|w| w.contains("`search`")));
    }

    #[test]
    fn parses_show_hidden() {
        let (cfg, warnings) = parse("show_hidden = true\n");
        assert!(cfg.show_hidden);
        assert!(warnings.is_empty());

        let (cfg, warnings) = parse("show_hidden = false\n");
        assert!(!cfg.show_hidden);
        assert!(warnings.is_empty());

        let (cfg, warnings) = parse("show_hidden = maybe\n");
        assert!(!cfg.show_hidden);
        assert!(warnings.iter().any(|w| w.contains("`show_hidden`")));
    }

    #[test]
    fn default_path_points_to_config_file() {
        if let Some(path) = default_path() {
            assert!(path.ends_with("ff/config") || path.ends_with("ff\\config"));
        }
    }
}
