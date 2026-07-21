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
    /// before entering the alternate screen. Never panics; returns a
    /// half-block capability on any failure or when `choice` is `HalfBlocks`.
    pub fn detect(choice: ProtocolChoice) -> Self {
        if choice == ProtocolChoice::HalfBlocks {
            return Self::halfblocks();
        }
        // Querying can fail on pipes, unsupported terminals, or timeout — any
        // error means "no graphics", i.e. half-blocks.
        let Ok(mut picker) = std::panic::catch_unwind(Picker::from_query_stdio)
            .unwrap_or_else(|_| Err(ratatui_image::errors::Errors::NoFontSize))
        else {
            return Self::halfblocks();
        };

        // Honor an explicit protocol request if the user forced one.
        let forced = match choice {
            ProtocolChoice::Kitty => Some(ProtocolType::Kitty),
            ProtocolChoice::Iterm2 => Some(ProtocolType::Iterm2),
            ProtocolChoice::Sixel => Some(ProtocolType::Sixel),
            _ => None,
        };
        if let Some(pt) = forced {
            picker.set_protocol_type(pt);
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

    #[test]
    fn halfblocks_capability_is_not_graphical() {
        let g = Graphics::halfblocks();
        assert!(!g.is_graphical());
        assert_eq!(g.font_size(), None);
    }
}
