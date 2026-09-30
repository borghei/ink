use serde::Deserialize;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Config {
    pub theme: Option<String>,
    pub width: Option<u16>,
    pub spacing: Option<String>,
    pub toc: Option<bool>,
    pub frontmatter: Option<bool>,
    pub behavior: Option<BehaviorConfig>,
    pub keybindings: Option<KeybindingsConfig>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct BehaviorConfig {
    /// When true, closing a file via q/Esc returns to the browser instead of exiting.
    pub browser_loop: Option<bool>,
    /// When false, ink does not grab the mouse, so the terminal's own
    /// click-to-open (OSC 8) and text selection keep working. Default true
    /// (mouse wheel scrolls the document).
    pub mouse_capture: Option<bool>,
    /// How copies reach the clipboard: "auto" | "osc52" | "native" | "off".
    pub clipboard: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct KeybindingsConfig {
    /// Built-in preset name: "default" | "vim" | "emacs".
    pub preset: Option<String>,
    /// Per-action overrides applied on top of the preset.
    /// Key is the action ID (e.g. "scroll_down"), value is the list of key strings.
    pub bindings: Option<HashMap<String, Vec<String>>>,
}

/// The ink config directory: `$XDG_CONFIG_HOME/ink` when that variable is
/// set to an absolute path (on every platform), otherwise `<platform>/ink`
/// (`~/.config` on Linux, `~/Library/Application Support` on macOS,
/// `%APPDATA%` on Windows).
pub fn config_dir() -> Option<PathBuf> {
    resolve_config_dir(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        dirs::config_dir(),
    )
}

/// Pure form of [`config_dir`], so the rule is testable without touching the
/// process environment. Per the XDG Base Directory spec, an empty or relative
/// `$XDG_CONFIG_HOME` is ignored.
fn resolve_config_dir(xdg: Option<&OsStr>, platform: Option<PathBuf>) -> Option<PathBuf> {
    let base = xdg
        .map(Path::new)
        .filter(|p| p.is_absolute())
        .map(Path::to_path_buf)
        .or(platform)?;
    Some(base.join("ink"))
}

/// Directory holding user theme files (`<config dir>/themes`).
pub fn themes_dir() -> Option<PathBuf> {
    Some(config_dir()?.join("themes"))
}

/// Resolved path of the active config file, if a config dir can be found.
pub fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("config.toml"))
}

/// Human-readable string for `ink config path`.
pub fn config_path_display() -> String {
    config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<no config dir on this platform>".to_string())
}

/// Load config from [`config_path`].
pub fn load_config() -> Option<Config> {
    let path = config_path()?;
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

/// Persist the chosen theme to the config file, preserving existing content
/// and comments. Creates the file (and parent dir) if absent.
pub fn set_theme(name: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let path = config_path().context("no config directory on this platform")?;
    let mut doc = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| c.parse::<toml_edit::DocumentMut>().ok())
        .unwrap_or_default();
    doc["theme"] = toml_edit::value(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, doc.to_string())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform() -> Option<PathBuf> {
        Some(PathBuf::from("/platform/config"))
    }

    #[test]
    fn xdg_set_is_used() {
        assert_eq!(
            resolve_config_dir(Some(OsStr::new("/xdg")), platform()),
            Some(PathBuf::from("/xdg/ink"))
        );
    }

    #[test]
    fn xdg_unset_falls_back_to_platform() {
        assert_eq!(
            resolve_config_dir(None, platform()),
            Some(PathBuf::from("/platform/config/ink"))
        );
    }

    #[test]
    fn xdg_empty_falls_back_to_platform() {
        assert_eq!(
            resolve_config_dir(Some(OsStr::new("")), platform()),
            Some(PathBuf::from("/platform/config/ink"))
        );
    }

    #[test]
    fn xdg_relative_falls_back_to_platform() {
        assert_eq!(
            resolve_config_dir(Some(OsStr::new("relative/dir")), platform()),
            Some(PathBuf::from("/platform/config/ink"))
        );
    }

    #[test]
    fn xdg_used_without_platform_dir() {
        assert_eq!(
            resolve_config_dir(Some(OsStr::new("/xdg")), None),
            Some(PathBuf::from("/xdg/ink"))
        );
        assert_eq!(resolve_config_dir(None, None), None);
    }
}
