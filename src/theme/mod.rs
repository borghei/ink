pub mod builtin;
pub mod caps;
pub mod detect;

use serde::Deserialize;

/// The built-in theme names, in display order.
pub const BUILTIN_THEMES: &[&str] = &[
    "dark",
    "light",
    "dracula",
    "catppuccin",
    "nord",
    "tokyo-night",
    "gruvbox",
    "solarized",
];

/// All available theme names: built-ins plus any `*.toml` in the user themes
/// directory (`~/.config/ink/themes/`).
pub fn available_themes() -> Vec<String> {
    let mut names: Vec<String> = BUILTIN_THEMES.iter().map(|s| s.to_string()).collect();
    if let Some(dir) = dirs::config_dir() {
        let themes_dir = dir.join("ink").join("themes");
        if let Ok(entries) = std::fs::read_dir(&themes_dir) {
            let mut user: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let path = e.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .filter(|n| !BUILTIN_THEMES.contains(&n.as_str()))
                .collect();
            user.sort();
            names.extend(user);
        }
    }
    names
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Theme {
    pub name: String,
    pub colors: ThemeColors,
    #[serde(default)]
    pub code_theme: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeColors {
    pub bg: Option<String>,
    pub fg: String,
    pub heading1: String,
    pub heading2: String,
    pub heading3: String,
    pub heading4: String,
    pub heading5: String,
    pub heading6: String,
    pub bold: String,
    #[allow(dead_code)]
    pub italic: String,
    pub strikethrough: String,
    pub code_fg: String,
    pub code_bg: String,
    pub code_block_bg: String,
    pub link: String,
    pub link_url: String,
    pub blockquote_bar: String,
    pub blockquote_text: String,
    pub list_bullet: String,
    pub list_number: String,
    pub table_border: String,
    pub table_header: String,
    pub hr: String,
    pub task_done: String,
    pub task_pending: String,
    pub search_match: String,
    pub search_current: String,
    pub status_bar_bg: String,
    pub status_bar_fg: String,
    pub toc_active: String,
    pub toc_inactive: String,
    pub admonition_note: String,
    pub admonition_warning: String,
    pub admonition_tip: String,
    pub admonition_important: String,
    pub admonition_caution: String,
}

/// Resolve a theme by name. Checks built-in themes first, then user config dir.
/// A broken or unknown theme falls back to `dark`, but silently doing so
/// left users debugging "why does my theme do nothing". Warn once per
/// process, on stderr, before the TUI owns the terminal (resolve_theme is
/// called on every draw, so once matters).
fn warn_theme_fallback_once(name: &str, reason: &str) {
    use std::sync::Once;
    static WARNED: Once = Once::new();
    WARNED.call_once(|| {
        eprintln!("ink: theme '{name}' could not be loaded ({reason}), falling back to 'dark'");
    });
}

pub fn resolve_theme(name: &str) -> Theme {
    if name == "auto" {
        let is_dark = detect::is_dark_background();
        return if is_dark {
            builtin::dark()
        } else {
            builtin::light()
        };
    }

    match name {
        "dark" => builtin::dark(),
        "light" => builtin::light(),
        "dracula" => builtin::dracula(),
        "catppuccin" => builtin::catppuccin(),
        "nord" => builtin::nord(),
        "tokyo-night" => builtin::tokyo_night(),
        "gruvbox" => builtin::gruvbox(),
        "solarized" => builtin::solarized(),
        _ => {
            // Try loading from user config directory
            if let Some(config_dir) = dirs::config_dir() {
                let theme_path = config_dir
                    .join("ink")
                    .join("themes")
                    .join(format!("{name}.toml"));
                if theme_path.exists() {
                    match std::fs::read_to_string(&theme_path)
                        .map_err(|e| e.to_string())
                        .and_then(|c| toml::from_str(&c).map_err(|e| e.to_string()))
                    {
                        Ok(theme) => return theme,
                        Err(e) => warn_theme_fallback_once(name, &e),
                    }
                } else {
                    warn_theme_fallback_once(name, "no such builtin or theme file");
                }
            }
            // Fallback to dark
            builtin::dark()
        }
    }
}

/// Parse a hex color string to RGB. Returns (200,200,200) for invalid input.
pub fn hex_to_rgb(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    // Byte length alone is not enough to make the slices below safe: a 6-byte
    // string can be two multi-byte characters, and slicing would split one.
    if hex.len() < 6 || !hex.is_ascii() {
        return (200, 200, 200);
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(200);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(200);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(200);
    (r, g, b)
}

/// Convert a hex string to a ratatui color, adapted to the terminal:
/// quantized to the 256-color cube when truecolor isn't available. (In the
/// TUI, `NO_COLOR` is not honored — an alternate-screen reader without color
/// is not useful; use `--plain` for the no-color path.)
pub fn hex_to_color(hex: &str) -> ratatui::style::Color {
    let (r, g, b) = hex_to_rgb(hex);
    let rgb = ratatui::style::Color::Rgb(r, g, b);
    if caps::caps().truecolor {
        rgb
    } else {
        ratatui::style::Color::Indexed(caps::rgb_to_256(r, g, b))
    }
}

#[cfg(test)]
mod hex_tests {
    use super::*;

    #[test]
    fn parses_valid_hex() {
        assert_eq!(hex_to_rgb("#ff0000"), (255, 0, 0));
        assert_eq!(hex_to_rgb("00ff80"), (0, 255, 128));
    }

    /// Regression: the length guard counts bytes, so a 6-byte string of
    /// multi-byte characters passed it and then split a char while slicing.
    #[test]
    fn non_ascii_hex_falls_back_instead_of_panicking() {
        assert_eq!(hex_to_rgb("€€"), (200, 200, 200)); // 6 bytes, 2 chars
        assert_eq!(hex_to_rgb("#ααα"), (200, 200, 200)); // 6 bytes, 3 chars
        assert_eq!(hex_to_rgb("fff"), (200, 200, 200)); // too short
    }
}
