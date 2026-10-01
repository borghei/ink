//! Terminal color capabilities, detected once from the environment.
//!
//! Honors the `NO_COLOR` convention (https://no-color.org/) and downgrades
//! 24-bit RGB to the 256-color cube when the terminal doesn't advertise
//! truecolor, so themes stay legible on 256-color terminals.
//!
//! [`color_enabled`] decides whether non-TUI output (`--plain`, `ink diff`)
//! carries escape sequences at all, with this precedence:
//!
//! 1. an explicit `--color always|never` beats everything;
//! 2. `NO_COLOR` (non-empty) turns color off;
//! 3. `CLICOLOR_FORCE` (non-empty, not `0`) or `FORCE_COLOR` (non-empty)
//!    turns it on;
//! 4. `TERM=dumb` turns it off;
//! 5. otherwise color is on only when stdout is a terminal.

use ratatui::style::Color;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy)]
pub struct TermCaps {
    /// `NO_COLOR` was set to a non-empty value — strip color, keep text
    /// attributes (TUI).
    pub no_color: bool,
    /// Terminal advertises 24-bit color (`COLORTERM=truecolor|24bit`, a
    /// known-truecolor `TERM`, Windows Terminal, or a known `TERM_PROGRAM`).
    pub truecolor: bool,
}

/// Non-empty value of an environment variable.
fn env_set(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl TermCaps {
    fn detect() -> Self {
        let no_color = env_set("NO_COLOR").is_some();
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();
        let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let truecolor = is_truecolor(
            &colorterm,
            &term,
            &term_program,
            std::env::var_os("WT_SESSION").is_some(),
        );
        Self {
            no_color,
            truecolor,
        }
    }
}

/// Pure truecolor detection. Windows Terminal (`WT_SESSION`) and several
/// common terminals support 24-bit color without setting `COLORTERM`.
fn is_truecolor(colorterm: &str, term: &str, term_program: &str, wt_session: bool) -> bool {
    colorterm.eq_ignore_ascii_case("truecolor")
        || colorterm.eq_ignore_ascii_case("24bit")
        || term.contains("truecolor")
        || term.contains("direct")
        || term.contains("kitty")
        || term.contains("alacritty")
        || wt_session
        || matches!(term_program, "vscode" | "ghostty" | "WezTerm" | "iTerm.app")
}

/// The `--color` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ColorChoice {
    /// Color when stdout is a terminal (and the environment allows it).
    #[default]
    Auto,
    /// Always emit color and hyperlinks.
    Always,
    /// Never emit escape sequences.
    Never,
}

/// Should non-TUI output carry escape sequences? See the module docs for
/// the precedence.
pub fn color_enabled(choice: ColorChoice, stdout_tty: bool) -> bool {
    resolve_color(
        choice,
        &env_set,
        std::env::var("TERM").unwrap_or_default().as_str(),
        stdout_tty,
    )
}

/// Pure form of [`color_enabled`]: `env` returns a variable's non-empty value.
fn resolve_color(
    choice: ColorChoice,
    env: &dyn Fn(&str) -> Option<String>,
    term: &str,
    stdout_tty: bool,
) -> bool {
    match choice {
        ColorChoice::Always => return true,
        ColorChoice::Never => return false,
        ColorChoice::Auto => {}
    }
    if env("NO_COLOR").is_some() {
        return false;
    }
    if env("CLICOLOR_FORCE").is_some_and(|v| v != "0") || env("FORCE_COLOR").is_some() {
        return true;
    }
    if term == "dumb" {
        return false;
    }
    stdout_tty
}

/// Cached capabilities for the current process.
pub fn caps() -> TermCaps {
    static CAPS: OnceLock<TermCaps> = OnceLock::new();
    *CAPS.get_or_init(TermCaps::detect)
}

/// Adapt an RGB color to the terminal's capabilities: drop it entirely under
/// `NO_COLOR`, quantize to the 256-color cube when truecolor is unavailable,
/// or pass it through unchanged.
pub fn adapt(color: Color) -> Option<Color> {
    let c = caps();
    if c.no_color {
        return None;
    }
    if c.truecolor {
        return Some(color);
    }
    match color {
        Color::Rgb(r, g, b) => Some(Color::Indexed(rgb_to_256(r, g, b))),
        other => Some(other),
    }
}

/// Map an RGB triple to the nearest xterm-256 palette index.
pub fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    // Grayscale ramp (232..=255) when the channels are close together.
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max - min < 8 {
        // 24 gray levels from 8 to 238.
        if r < 8 {
            return 16;
        }
        if r > 248 {
            return 231;
        }
        return 232 + ((r as u16 - 8) * 23 / 240) as u8;
    }
    // 6x6x6 color cube (16..=231).
    let comp = |v: u8| -> u16 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            ((v as u16 - 35) / 40).min(5)
        }
    };
    (16 + 36 * comp(r) + 6 * comp(g) + comp(b)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            vars.iter()
                .find(|(k, v)| *k == name && !v.is_empty())
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn color_precedence() {
        use ColorChoice::*;
        let none = env_of(&[]);
        // Auto follows the TTY.
        assert!(resolve_color(Auto, &none, "xterm", true));
        assert!(!resolve_color(Auto, &none, "xterm", false));
        // The explicit flag beats everything.
        let no_color = env_of(&[("NO_COLOR", "1")]);
        assert!(resolve_color(Always, &no_color, "dumb", false));
        assert!(!resolve_color(
            Never,
            &env_of(&[("FORCE_COLOR", "1")]),
            "xterm",
            true
        ));
        // NO_COLOR beats the force vars and the TTY; an empty one is unset.
        let both = env_of(&[("NO_COLOR", "1"), ("CLICOLOR_FORCE", "1")]);
        assert!(!resolve_color(Auto, &both, "xterm", true));
        assert!(resolve_color(
            Auto,
            &env_of(&[("NO_COLOR", "")]),
            "xterm",
            true
        ));
        // Force vars beat TERM=dumb and a pipe.
        assert!(resolve_color(
            Auto,
            &env_of(&[("CLICOLOR_FORCE", "1")]),
            "dumb",
            false
        ));
        assert!(resolve_color(
            Auto,
            &env_of(&[("FORCE_COLOR", "3")]),
            "xterm",
            false
        ));
        assert!(!resolve_color(
            Auto,
            &env_of(&[("CLICOLOR_FORCE", "0")]),
            "xterm",
            false
        ));
        // TERM=dumb is off even on a TTY.
        assert!(!resolve_color(Auto, &none, "dumb", true));
    }

    #[test]
    fn truecolor_detection_covers_terminals_without_colorterm() {
        assert!(is_truecolor("truecolor", "xterm-256color", "", false));
        assert!(
            is_truecolor("", "xterm-256color", "", true),
            "Windows Terminal"
        );
        for program in ["vscode", "ghostty", "WezTerm", "iTerm.app"] {
            assert!(
                is_truecolor("", "xterm-256color", program, false),
                "{program}"
            );
        }
        assert!(!is_truecolor("", "xterm-256color", "Apple_Terminal", false));
    }

    #[test]
    fn quantize_stays_in_palette() {
        for (r, g, b) in [(0, 0, 0), (255, 255, 255), (128, 64, 200), (10, 10, 10)] {
            let idx = rgb_to_256(r, g, b);
            assert!(idx >= 16, "index {idx} below color range");
        }
    }
}
