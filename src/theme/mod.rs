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
    "terminal",
];

/// All available theme names: built-ins plus any `*.toml` in the user themes
/// directory (`themes/` under the ink config directory: `$XDG_CONFIG_HOME/ink`
/// or the platform default).
pub fn available_themes() -> Vec<String> {
    let mut names: Vec<String> = BUILTIN_THEMES.iter().map(|s| s.to_string()).collect();
    if let Some(themes_dir) = crate::config::themes_dir() {
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
    /// Background of selected text. Optional so themes written before
    /// selection existed still deserialize; see [`ThemeColors::selection`].
    #[serde(default)]
    pub selection_bg: Option<String>,
    #[serde(default)]
    pub selection_fg: Option<String>,
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

impl ThemeColors {
    /// Selection colors as `(bg, fg)`, filling in for a theme that predates
    /// them: the search-match color already had to be legible against this
    /// theme's background, and the background itself is the safest foreground
    /// to print on top of it.
    pub fn selection(&self) -> (String, String) {
        let bg = self
            .selection_bg
            .clone()
            .unwrap_or_else(|| self.search_match.clone());
        let fg = self
            .selection_fg
            .clone()
            .or_else(|| self.bg.clone())
            .unwrap_or_else(|| self.fg.clone());
        (bg, fg)
    }
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
        "terminal" => builtin::terminal(),
        _ => {
            // Try loading from the user config directory. Note the warning is
            // emitted on every path out of here, including "this platform has
            // no config dir" — a mistyped theme must never fail silently.
            let theme_path = crate::config::themes_dir()
                .map(|d| d.join(format!("{name}.toml")))
                .filter(|p| p.exists());
            match theme_path {
                Some(path) => match std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|c| toml::from_str(&c).map_err(|e| e.to_string()))
                {
                    Ok(theme) => return theme,
                    Err(e) => warn_theme_fallback_once(name, &e),
                },
                None => warn_theme_fallback_once(name, "no such builtin or theme file"),
            }
            // Fallback to dark
            builtin::dark()
        }
    }
}

/// A theme colour as written: a 24-bit hex value, one of the terminal's 16
/// palette slots, or the terminal's own default foreground/background.
///
/// Palette slots and `default` let a theme follow the user's terminal scheme
/// instead of fixing exact colours (the `terminal` theme is built from
/// nothing else). They are spelled `ansi:red`, `ansi:bright-blue`, `ansi:0`
/// to `ansi:15`, and `default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpec {
    Rgb(u8, u8, u8),
    /// ANSI palette index 0–15.
    Ansi(u8),
    /// The terminal's default colour (SGR 39 / 49).
    Default,
}

const ANSI_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// Parse a theme colour. Anything that is not a palette name or `default`
/// is read as hex, so invalid input degrades exactly as it always has.
pub fn parse_color(s: &str) -> ColorSpec {
    let s = s.trim();
    if s.eq_ignore_ascii_case("default") {
        return ColorSpec::Default;
    }
    if let Some(name) = s
        .get(..5)
        .filter(|p| p.eq_ignore_ascii_case("ansi:"))
        .map(|_| s[5..].to_ascii_lowercase())
    {
        if let Ok(i) = name.parse::<u8>() {
            if i < 16 {
                return ColorSpec::Ansi(i);
            }
        }
        let (base, bright) = match name.strip_prefix("bright-") {
            Some(rest) => (rest, true),
            None => (name.as_str(), false),
        };
        if let Some(i) = ANSI_NAMES.iter().position(|n| *n == base) {
            return ColorSpec::Ansi(i as u8 + if bright { 8 } else { 0 });
        }
    }
    let (r, g, b) = parse_hex(s);
    ColorSpec::Rgb(r, g, b)
}

/// Parse a theme colour to RGB. Palette slots map to xterm's default values
/// (the terminal's actual palette is unknown); `default` and invalid input
/// give (200,200,200).
pub fn hex_to_rgb(color: &str) -> (u8, u8, u8) {
    match parse_color(color) {
        ColorSpec::Rgb(r, g, b) => (r, g, b),
        ColorSpec::Ansi(i) => caps::XTERM_16[i as usize],
        ColorSpec::Default => (200, 200, 200),
    }
}

/// The SGR escape selecting `color` as foreground (`fg`) or background at
/// `level`. Palette slots and `default` are emitted as themselves at every
/// level, so the terminal's scheme applies.
pub fn sgr_color(color: &str, fg: bool, level: caps::ColorLevel) -> String {
    let params = match (parse_color(color), level) {
        (_, caps::ColorLevel::None) => String::new(),
        (ColorSpec::Rgb(r, g, b), _) => caps::sgr_params((r, g, b), fg, level),
        (ColorSpec::Ansi(i), _) => caps::ansi16_sgr(i, fg).to_string(),
        (ColorSpec::Default, _) => if fg { "39" } else { "49" }.to_string(),
    };
    format!("\x1b[{params}m")
}

fn parse_hex(hex: &str) -> (u8, u8, u8) {
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

/// Convert a hex string to a ratatui color for the reader, adapted to the
/// terminal: 24-bit, the nearest 256-palette or named ANSI colour, or the
/// terminal's default colour ([`Color::Reset`](ratatui::style::Color::Reset))
/// when colour is off (`--color=never`, `NO_COLOR`), where the reader relies
/// on bold, underline and reverse video instead.
pub fn hex_to_color(color: &str) -> ratatui::style::Color {
    let level = caps::tui_level();
    match parse_color(color) {
        ColorSpec::Rgb(r, g, b) => caps::rgb_at_level(r, g, b, level),
        ColorSpec::Ansi(_) if level == caps::ColorLevel::None => ratatui::style::Color::Reset,
        ColorSpec::Ansi(i) => caps::ansi16_color(i),
        ColorSpec::Default => ratatui::style::Color::Reset,
    }
}

/// Is the reader drawing without colour? Highlights that would otherwise be
/// a background colour (selection, hint labels) use reverse video then.
pub fn colorless() -> bool {
    caps::tui_level() == caps::ColorLevel::None
}

/// A theme file written before selection colors existed must still load, and
/// must still produce a legible selection. Every user theme in the wild is one
/// of these.
#[test]
#[cfg(test)]
fn user_theme_without_selection_keys() {
    const LEGACY: &str = r##"name = "mytheme"

[colors]
bg = "#1a1b26"
fg = "#c0caf5"
heading1 = "#7aa2f7"
heading2 = "#7dcfff"
heading3 = "#bb9af7"
heading4 = "#9ece6a"
heading5 = "#e0af68"
heading6 = "#f7768e"
bold = "#e6e8f0"
italic = "#c0caf5"
strikethrough = "#565f89"
code_fg = "#a9b1d6"
code_bg = "#24283b"
code_block_bg = "#24283b"
link = "#7aa2f7"
link_url = "#565f89"
blockquote_bar = "#565f89"
blockquote_text = "#a9b1d6"
list_bullet = "#7aa2f7"
list_number = "#7aa2f7"
table_border = "#3b4261"
table_header = "#7dcfff"
hr = "#3b4261"
task_done = "#9ece6a"
task_pending = "#565f89"
search_match = "#e0af68"
search_current = "#ff9e64"
status_bar_bg = "#16161e"
status_bar_fg = "#a9b1d6"
toc_active = "#7aa2f7"
toc_inactive = "#565f89"
admonition_note = "#7aa2f7"
admonition_warning = "#e0af68"
admonition_tip = "#9ece6a"
admonition_important = "#bb9af7"
admonition_caution = "#f7768e"
"##;
    let theme: Theme = toml::from_str(LEGACY).expect("legacy theme must still deserialize");
    assert!(theme.colors.selection_bg.is_none());
    let (bg, fg) = theme.colors.selection();
    // Falls back to the search-match color, which this theme already had to
    // make legible against its own background.
    assert_eq!(bg, "#e0af68");
    assert_eq!(fg, "#1a1b26");
}

#[cfg(test)]
mod hex_tests {
    use super::*;

    #[test]
    fn parses_valid_hex() {
        assert_eq!(hex_to_rgb("#ff0000"), (255, 0, 0));
        assert_eq!(hex_to_rgb("00ff80"), (0, 255, 128));
    }

    #[test]
    fn palette_names_and_default_parse() {
        assert_eq!(parse_color("default"), ColorSpec::Default);
        assert_eq!(parse_color("ansi:red"), ColorSpec::Ansi(1));
        assert_eq!(parse_color("ANSI:Bright-Blue"), ColorSpec::Ansi(12));
        assert_eq!(parse_color("ansi:bright-black"), ColorSpec::Ansi(8));
        assert_eq!(parse_color("ansi:15"), ColorSpec::Ansi(15));
        // Out of range or unknown names fall through to hex, i.e. the old
        // invalid-colour fallback.
        assert_eq!(parse_color("ansi:16"), ColorSpec::Rgb(200, 200, 200));
        assert_eq!(parse_color("ansi:orange"), ColorSpec::Rgb(200, 200, 200));
        assert_eq!(parse_color("ansi:é"), ColorSpec::Rgb(200, 200, 200));
        assert_eq!(parse_color("#ff0000"), ColorSpec::Rgb(255, 0, 0));
    }

    // Palette colours are emitted as palette escapes at every depth, so the
    // terminal's scheme applies; with colour off nothing is emitted.
    #[test]
    fn palette_colours_survive_every_depth() {
        use caps::ColorLevel::*;
        for level in [TrueColor, Ansi256, Ansi16] {
            assert_eq!(sgr_color("ansi:red", true, level), "\x1b[31m");
            assert_eq!(sgr_color("ansi:bright-cyan", false, level), "\x1b[106m");
            assert_eq!(sgr_color("default", true, level), "\x1b[39m");
            assert_eq!(sgr_color("default", false, level), "\x1b[49m");
        }
        assert_eq!(sgr_color("#ff0000", true, TrueColor), "\x1b[38;2;255;0;0m");
        assert_eq!(sgr_color("ansi:red", true, None), "\x1b[m");
    }

    #[test]
    fn terminal_theme_uses_only_palette_colours() {
        let t = resolve_theme("terminal");
        assert_eq!(t.name, "terminal");
        assert!(t.colors.bg.is_none());
        let c = &t.colors;
        for color in [
            &c.fg,
            &c.heading1,
            &c.heading6,
            &c.code_fg,
            &c.code_bg,
            &c.link,
            &c.table_border,
            &c.status_bar_bg,
            &c.status_bar_fg,
            &c.search_match,
        ] {
            assert!(!matches!(parse_color(color), ColorSpec::Rgb(..)), "{color}");
        }
        assert!(crate::highlight::theme_set()
            .themes
            .contains_key(&t.code_theme));
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
