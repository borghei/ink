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
    let width = args.width.unwrap_or(80);
    let styled_lines =
        layout::layout_document(root, &t, width, args.spacing, 2, None, args.images).lines;

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
            if let Some(ref fg) = span.style.fg {
                let (r, g, b) = theme::hex_to_rgb(fg);
                output.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
            }
            if let Some(ref bg) = span.style.bg {
                let (r, g, b) = theme::hex_to_rgb(bg);
                output.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
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

            if span.style.fg.is_some()
                || span.style.bg.is_some()
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
