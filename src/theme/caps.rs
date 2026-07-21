//! Terminal color capabilities, detected once from the environment.
//!
//! Honors the `NO_COLOR` convention (https://no-color.org/) and downgrades
//! 24-bit RGB to the 256-color cube when the terminal doesn't advertise
//! truecolor, so themes stay legible on 256-color terminals.

use ratatui::style::Color;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy)]
pub struct TermCaps {
    /// `NO_COLOR` was set (any value) — strip color, keep text attributes.
    pub no_color: bool,
    /// Terminal advertises 24-bit color (`COLORTERM=truecolor|24bit`, or a
    /// known-truecolor `TERM`).
    pub truecolor: bool,
}

impl TermCaps {
    fn detect() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();
        let truecolor = colorterm.eq_ignore_ascii_case("truecolor")
            || colorterm.eq_ignore_ascii_case("24bit")
            || term.contains("truecolor")
            || term.contains("direct")
            || term.contains("kitty")
            || term.contains("alacritty");
        Self {
            no_color,
            truecolor,
        }
    }
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

    #[test]
    fn quantize_stays_in_palette() {
        for (r, g, b) in [(0, 0, 0), (255, 255, 255), (128, 64, 200), (10, 10, 10)] {
            let idx = rgb_to_256(r, g, b);
            assert!(idx >= 16, "index {idx} below color range");
        }
    }
}
