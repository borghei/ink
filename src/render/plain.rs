use crate::layout;
use crate::parser;
use crate::parser::frontmatter;
use crate::theme;
use crate::Args;
use anyhow::Result;
use comrak::{parse_document, Arena};

/// Render markdown to ANSI-styled plain text (no TUI, pipe-friendly).
///
/// Styled unless `NO_COLOR` is set; the CLI decides with
/// [`render_plain_with_color`] instead, which also honors `--color` and
/// whether stdout is a terminal.
pub fn render_plain(source: &str, args: &Args) -> Result<String> {
    render_plain_with_color(source, args, !theme::caps::caps().no_color)
}

/// Render markdown to plain text. With `color` false the output contains no
/// escape sequences at all — no SGR color or attributes, no OSC 8 links —
/// so it is safe for files, pipes, and `git` textconv.
pub fn render_plain_with_color(source: &str, args: &Args, color: bool) -> Result<String> {
    // Frontmatter is stripped, or with `--frontmatter` turned into a block
    // the layout draws as a metadata box.
    let content = frontmatter::prepare(source, args.frontmatter);

    // Pre-process wikilinks
    let content = crate::wikilink::process_wikilinks(&content);

    let arena = Arena::new();
    let options = parser::options_for(&content);
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
        None, // plain output never uses graphics protocols
    )
    .lines;

    let depth = theme::caps::caps().depth;
    let mut output = String::new();
    for line in &styled_lines {
        if !color {
            for span in &line.spans {
                output.push_str(&span.text);
            }
            output.push('\n');
            continue;
        }
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
            if span.style.fg.is_some() || span.style.bg.is_some() {
                if let Some(ref fg) = span.style.fg {
                    output.push_str(&sgr_color(theme::hex_to_rgb(fg), true, depth));
                }
                if let Some(ref bg) = span.style.bg {
                    output.push_str(&sgr_color(theme::hex_to_rgb(bg), false, depth));
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

            let emitted_color = span.style.fg.is_some() || span.style.bg.is_some();
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

/// Build an SGR color escape at the terminal's depth: 24-bit, the 256-color
/// cube, or one of the 16 ANSI colours. `fg` selects foreground vs
/// background.
fn sgr_color(rgb: (u8, u8, u8), fg: bool, depth: theme::caps::ColorLevel) -> String {
    format!("\x1b[{}m", theme::caps::sgr_params(rgb, fg, depth))
}
