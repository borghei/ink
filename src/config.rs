use serde::Deserialize;
use std::collections::HashMap;

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

/// Resolved path of the active config file, if the platform has a config dir.
pub fn config_path() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("ink").join("config.toml"))
}

/// Human-readable string for `ink config path`.
pub fn config_path_display() -> String {
    config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<no config dir on this platform>".to_string())
}

/// Load config from ~/.config/ink/config.toml
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
