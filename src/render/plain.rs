use crate::layout;
use crate::parser;
use crate::parser::frontmatter;
use crate::theme;
use crate::Args;
use anyhow::Result;
use comrak::{parse_document, Arena};

/// Render markdown to ANSI-styled plain text (no TUI, pipe-friendly).
pub fn render_plain(source: &str, args: &Args) -> Result<String> {
    let (_, content) = if args.frontmatter {
        (None, source.to_string())
    } else {
        frontmatter::strip_frontmatter(source)
    };

    // Pre-process wikilinks
    let content = crate::wikilink::process_wikilinks(&content);

    let arena = Arena::new();
    let options = parser::options();
    let root = parse_document(&arena, &content, &options);
    let t = theme::resolve_theme(&args.theme);
    // `width` is the total output width; reserve the left margin from it so the
    // rendered lines (margin + content) never exceed the requested width.
    let margin: usize = 2;
    let width = args.width.unwrap_or(80);
    let content_width = width.saturating_sub(margin as u16).max(8);
    let styled_lines = layout::layout_document(
        root,
        &t,
        content_width,
        args.spacing,
        margin,
        None,
        args.images,
    )
    .lines;

    let caps = theme::caps::caps();
    let mut output = String::new();
    for line in &styled_lines {
        for span in &line.spans {
            let mut codes = Vec::new();
            if span.style.bold {
                codes.push("1");
            }
            if span.style.italic {
                codes.push("3");
            }
            if span.style.underline {
                codes.push("4");
            }
            if span.style.strikethrough {
                codes.push("9");
            }
            if span.style.dim {
                codes.push("2");
            }
            // NO_COLOR: keep text attributes, drop color (per no-color.org).
            let color_on = !caps.no_color;
            if color_on {
                if let Some(ref fg) = span.style.fg {
                    output.push_str(&sgr_color(theme::hex_to_rgb(fg), true, caps.truecolor));
                }
                if let Some(ref bg) = span.style.bg {
                    output.push_str(&sgr_color(theme::hex_to_rgb(bg), false, caps.truecolor));
                }
            }
            if !codes.is_empty() {
                output.push_str(&format!("\x1b[{}m", codes.join(";")));
            }

            // OSC 8 hyperlink. Layout already sanitizes URLs; re-check here
            // so this sink stays safe even if a future code path skips it.
            let link_url = span
                .style
                .link_url
                .as_deref()
                .and_then(crate::sanitize::sanitize_url);
            if let Some(ref url) = link_url {
                output.push_str(&format!("\x1b]8;;{url}\x1b\\"));
            }

            output.push_str(&span.text);

            if link_url.is_some() {
                output.push_str("\x1b]8;;\x1b\\");
            }

            let emitted_color = color_on && (span.style.fg.is_some() || span.style.bg.is_some());
            if emitted_color
                || span.style.bold
                || span.style.italic
                || span.style.underline
                || span.style.strikethrough
                || span.style.dim
            {
                output.push_str("\x1b[0m");
            }
        }
        output.push('\n');
    }

    Ok(output)
}

/// Build an SGR color escape: 24-bit when `truecolor`, else the 256-color
/// cube. `fg` selects foreground (38) vs background (48).
fn sgr_color((r, g, b): (u8, u8, u8), fg: bool, truecolor: bool) -> String {
    let sel = if fg { 38 } else { 48 };
    if truecolor {
        format!("\x1b[{sel};2;{r};{g};{b}m")
    } else {
        let idx = theme::caps::rgb_to_256(r, g, b);
        format!("\x1b[{sel};5;{idx}m")
    }
}
