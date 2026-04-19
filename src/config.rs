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

/// Load config from ~/.config/ink/config.toml
pub fn load_config() -> Option<Config> {
    let config_dir = dirs::config_dir()?;
    let config_path = config_dir.join("ink").join("config.toml");
    let content = std::fs::read_to_string(config_path).ok()?;
    toml::from_str(&content).ok()
}
