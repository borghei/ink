//! Terminal graphics-protocol detection and image-protocol construction.
//!
//! Wraps `ratatui-image`'s `Picker`, which probes the terminal for Kitty
//! graphics / iTerm2 inline / Sixel support and the cell (font) size. When a
//! graphics protocol is available we render real pixel images; otherwise we
//! fall back to the universal Unicode half-block renderer (`src/image.rs`).
//!
//! Detection queries the terminal over stdio, so it must run once at startup
//! on a real TTY. Any failure (pipe, unsupported terminal, timeout) degrades
//! silently to half-blocks — the graphics path never changes behavior on a
//! terminal that can't do graphics.

use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::sliced::SlicedProtocol;

/// How the user asked images to be rendered (`--image-protocol`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolChoice {
    /// Detect the best supported protocol; fall back to half-blocks.
    Auto,
    /// Force Unicode half-blocks (works anywhere with truecolor).
    HalfBlocks,
    Kitty,
    Iterm2,
    Sixel,
}

impl ProtocolChoice {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "halfblocks" | "half-blocks" | "blocks" | "unicode" => Some(Self::HalfBlocks),
            "kitty" => Some(Self::Kitty),
            "iterm2" | "iterm" => Some(Self::Iterm2),
            "sixel" => Some(Self::Sixel),
            _ => None,
        }
    }
}

/// Resolved graphics capability for this session.
pub struct Graphics {
    picker: Option<Picker>,
    graphical: bool,
}

impl Graphics {
    /// A half-block-only capability (no terminal query). Used for `--plain`,
    /// non-TTY output, and as the fallback.
    pub fn halfblocks() -> Self {
        Self {
            picker: None,
            graphical: false,
        }
    }

    /// Detect the terminal's image capability. Must be called on a real TTY,
    /// before entering the alternate screen. Returns a half-block capability
    /// on any failure or when `choice` is `HalfBlocks`.
    pub fn detect(choice: ProtocolChoice) -> Self {
        if choice == ProtocolChoice::HalfBlocks {
            return Self::halfblocks();
        }
        // Honor an explicit protocol request if the user forced one.
        let forced = match choice {
            ProtocolChoice::Kitty => Some(ProtocolType::Kitty),
            ProtocolChoice::Iterm2 => Some(ProtocolType::Iterm2),
            ProtocolChoice::Sixel => Some(ProtocolType::Sixel),
            _ => None,
        };

        // A terminal that never answers makes the query wait out its ~2 s
        // timeout at every start, so known non-graphics terminals skip it.
        // A forced protocol still applies (with an assumed cell size).
        let mut picker = if query_skip_reason().is_some() {
            if forced.is_none() {
                return Self::halfblocks();
            }
            Picker::halfblocks()
        } else {
            // Querying can fail on pipes, unsupported terminals, or timeout —
            // any error means "no graphics", i.e. half-blocks. (No
            // catch_unwind: release builds abort on panic, so it could never
            // catch anything that matters.)
            match Picker::from_query_stdio() {
                Ok(p) => p,
                Err(_) => return Self::halfblocks(),
            }
        };

        if let Some(pt) = forced {
            picker.set_protocol_type(pt);
        } else {
            picker.set_protocol_type(auto_protocol(picker.protocol_type(), is_iterm()));
        }

        let graphical = picker.protocol_type() != ProtocolType::Halfblocks;
        Self {
            picker: Some(if graphical {
                picker
            } else {
                return Self::halfblocks();
            }),
            graphical,
        }
    }

    /// True when a real graphics protocol (Kitty/iTerm2/Sixel) is active.
    pub fn is_graphical(&self) -> bool {
        self.graphical
    }

    /// Cell (font) size in pixels, if known. Used to reserve rows for an image.
    pub fn font_size(&self) -> Option<(u16, u16)> {
        self.picker
            .as_ref()
            .map(|p| (p.font_size().width, p.font_size().height))
    }

    /// Build a scroll-sliceable protocol for `image`, fitted into a
    /// `cols`×`rows` cell area. Returns `None` if not in graphics mode or on
    /// encode failure (caller keeps the reserved blank rows).
    pub fn build(&self, image: DynamicImage, cols: u16, rows: u16) -> Option<SlicedProtocol> {
        let picker = self.picker.as_ref()?;
        if !self.graphical {
            return None;
        }
        SlicedProtocol::new(picker, image, Some(Size::new(cols, rows))).ok()
    }
}

/// Why the graphics query is not worth running here, if it is not: the
/// terminal cannot do pixel graphics, or there is no terminal to ask.
pub fn query_skip_reason() -> Option<&'static str> {
    use std::io::IsTerminal;
    skip_reason_for(
        &std::env::var("TERM").unwrap_or_default(),
        &std::env::var("TERM_PROGRAM").unwrap_or_default(),
        std::io::stdout().is_terminal(),
    )
}

/// Pure form of [`query_skip_reason`].
fn skip_reason_for(term: &str, term_program: &str, stdout_tty: bool) -> Option<&'static str> {
    if !stdout_tty {
        return Some("stdout is not a terminal");
    }
    if term == "linux" || term == "dumb" {
        return Some("TERM has no graphics protocol");
    }
    // Terminal.app speaks none of kitty / iTerm2 / sixel and does not answer
    // the probe, so the query would only cost its full timeout.
    if term_program == "Apple_Terminal" {
        return Some("Terminal.app has no graphics protocol");
    }
    None
}

/// What the query said vs. what we should actually use, when the user hasn't
/// forced a protocol.
///
/// iTerm2 (3.5+) answers the kitty graphics query, so detection reports
/// kitty — but iTerm2's kitty implementation lacks the unicode-placeholder
/// mechanism the sliced kitty renderer depends on, so every image paints as
/// a silent blank block. Its native inline-image protocol works, so prefer
/// that whenever we're actually running under iTerm. Every other detection
/// result is passed through untouched.
fn auto_protocol(detected: ProtocolType, iterm: bool) -> ProtocolType {
    if detected == ProtocolType::Kitty && iterm {
        ProtocolType::Iterm2
    } else {
        detected
    }
}

/// Are we running under iTerm2 (locally or across ssh)? iTerm sets
/// `TERM_PROGRAM` locally and propagates `LC_TERMINAL` over ssh.
fn is_iterm() -> bool {
    is_iterm_env(
        std::env::var("TERM_PROGRAM").ok().as_deref(),
        std::env::var("LC_TERMINAL").ok().as_deref(),
    )
}

/// Pure form of [`is_iterm`], so the matching is testable without mutating
/// process-wide environment state.
fn is_iterm_env(term_program: Option<&str>, lc_terminal: Option<&str>) -> bool {
    term_program.is_some_and(|v| v.contains("iTerm"))
        || lc_terminal.is_some_and(|v| v.contains("iTerm"))
}

/// What `Auto` detection would resolve `detected` to on this system — the
/// `ink doctor` report shows both values so protocol overrides are visible.
pub fn auto_protocol_for_report(detected: ProtocolType) -> ProtocolType {
    auto_protocol(detected, is_iterm())
}

/// Given an image's pixel dimensions and the cell size, compute how many
/// terminal cells (cols × rows) it should occupy, fitted to `max_cols` and a
/// row cap, preserving aspect ratio.
pub fn cell_dimensions(
    img_w: u32,
    img_h: u32,
    font: (u16, u16),
    max_cols: u16,
    max_rows: u16,
) -> (u16, u16) {
    let (fw, fh) = (font.0.max(1) as u32, font.1.max(1) as u32);
    if img_w == 0 || img_h == 0 {
        return (1, 1);
    }
    // Natural size in cells, then clamp width to the content area.
    let nat_cols = (img_w / fw).max(1);
    let cols = nat_cols.min(max_cols as u32).max(1);
    // Height in pixels at that display width, converted to rows.
    let display_w_px = cols * fw;
    let display_h_px = display_w_px * img_h / img_w;
    let rows = display_h_px.div_ceil(fh).clamp(1, max_rows as u32);
    (cols as u16, rows as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_parsing() {
        assert_eq!(ProtocolChoice::parse("auto"), Some(ProtocolChoice::Auto));
        assert_eq!(ProtocolChoice::parse("Kitty"), Some(ProtocolChoice::Kitty));
        assert_eq!(
            ProtocolChoice::parse("half-blocks"),
            Some(ProtocolChoice::HalfBlocks)
        );
        assert_eq!(ProtocolChoice::parse("nope"), None);
    }

    #[test]
    fn cell_dims_preserve_aspect_and_clamp() {
        // 100x50 px image, 10x20 px cells → natural 10 cols x ~5 rows.
        let (c, r) = cell_dimensions(100, 50, (10, 20), 80, 30);
        assert_eq!(c, 10);
        assert!((2..=5).contains(&r), "rows {r}");
        // Clamp width to max_cols.
        let (c2, _) = cell_dimensions(2000, 1000, (10, 20), 40, 30);
        assert_eq!(c2, 40);
        // Zero-dimension guard.
        assert_eq!(cell_dimensions(0, 0, (10, 20), 80, 30), (1, 1));
    }

    // Regression: iTerm2 3.5+ answers the kitty query, and rendering kitty
    // there produced blank images. Auto-detection must swap to iTerm2's own
    // protocol on iTerm, and must not disturb any other outcome.
    #[test]
    fn auto_protocol_prefers_iterm2_over_kitty_on_iterm() {
        assert_eq!(
            auto_protocol(ProtocolType::Kitty, true),
            ProtocolType::Iterm2
        );
        // Real kitty (not iTerm) keeps kitty.
        assert_eq!(
            auto_protocol(ProtocolType::Kitty, false),
            ProtocolType::Kitty
        );
        // Any other detected protocol passes through, iTerm or not.
        for iterm in [true, false] {
            for pt in [
                ProtocolType::Sixel,
                ProtocolType::Iterm2,
                ProtocolType::Halfblocks,
            ] {
                assert_eq!(auto_protocol(pt, iterm), pt, "{pt:?} iterm={iterm}");
            }
        }
    }

    #[test]
    fn iterm_detected_from_either_env_var() {
        // Local iTerm sets TERM_PROGRAM; over ssh it propagates LC_TERMINAL.
        assert!(is_iterm_env(Some("iTerm.app"), None));
        assert!(is_iterm_env(None, Some("iTerm2")));
        assert!(is_iterm_env(Some("tmux"), Some("iTerm2")));
        // Other terminals are left alone.
        assert!(!is_iterm_env(Some("Apple_Terminal"), None));
        assert!(!is_iterm_env(Some("WezTerm"), Some("wezterm")));
        assert!(!is_iterm_env(None, None));
    }

    #[test]
    fn query_is_skipped_on_terminals_that_cannot_answer() {
        assert!(skip_reason_for("xterm-256color", "", false).is_some());
        assert!(skip_reason_for("linux", "", true).is_some());
        assert!(skip_reason_for("dumb", "", true).is_some());
        assert!(skip_reason_for("xterm-256color", "Apple_Terminal", true).is_some());
        assert!(skip_reason_for("xterm-256color", "iTerm.app", true).is_none());
        assert!(skip_reason_for("xterm-kitty", "", true).is_none());
        assert!(skip_reason_for("tmux-256color", "tmux", true).is_none());
    }

    #[test]
    fn halfblocks_capability_is_not_graphical() {
        let g = Graphics::halfblocks();
        assert!(!g.is_graphical());
        assert_eq!(g.font_size(), None);
    }
}
