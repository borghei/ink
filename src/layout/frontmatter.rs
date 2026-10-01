//! The `--frontmatter` metadata box: the document's frontmatter, which
//! `parser::frontmatter::prepare` turned into a fenced block, drawn as a
//! bordered key/value list titled "frontmatter".
//!
//! ```text
//! ╭─ frontmatter ───────────╮
//! │ title   Hello           │
//! │ tags    rust, terminal  │
//! ╰─────────────────────────╯
//! ```
//!
//! Keys and values are document text: they reach the terminal only through
//! the layout's final sanitizer pass, like everything else.

use super::table::{fit_label, wrap_text};
use super::{add_spacing, CodeBlockSpec, LayoutContext, SpanStyle, StyledLine, StyledSpan};
use crate::parser::frontmatter::{entries, Format};
use unicode_width::UnicodeWidthStr;

const LABEL: &str = " frontmatter ";
/// Columns between the key and value columns.
const GAP: usize = 2;

/// Lay out the metadata box for a fenced block whose info string is
/// `ink-frontmatter <format>` and whose literal is the frontmatter body.
pub(super) fn layout_frontmatter(
    format_name: &str,
    literal: &str,
    ctx: &LayoutContext,
    lines: &mut Vec<StyledLine>,
) {
    let start_line = lines.len();
    let format = Format::from_name(format_name).unwrap_or(Format::Yaml);
    let mut rows = entries(format, literal);
    if rows.is_empty() {
        // Nothing readable as keys: show the block's lines as they are.
        rows = literal
            .lines()
            .map(|l| (String::new(), vec![l.to_string()]))
            .collect();
    }

    let g = crate::glyphs::current();
    let inner_max = ctx.width.max(LABEL.width() + 6).saturating_sub(4);
    let max_key = rows.iter().map(|(k, _)| k.width()).max().unwrap_or(0);
    let key_w = max_key.min(inner_max / 3);
    let gap = if key_w == 0 { 0 } else { GAP };
    let max_value = rows
        .iter()
        .flat_map(|(_, v)| v.iter().map(|l| l.width()))
        .max()
        .unwrap_or(0);
    let inner = (key_w + gap + max_value)
        .min(inner_max)
        .max(LABEL.width() + 1);
    let value_w = inner.saturating_sub(key_w + gap).max(1);
    let border_width = inner + 4;

    let border = SpanStyle {
        fg: Some(ctx.theme.colors.table_border.clone()),
        ..Default::default()
    };
    let key_style = SpanStyle {
        fg: Some(ctx.theme.colors.table_header.clone()),
        bold: true,
        ..Default::default()
    };

    // ╭─ frontmatter ───╮
    let mut top = StyledLine::new();
    ctx.add_margin(&mut top);
    top.push(StyledSpan {
        text: format!("{}{}", g.tl, g.h),
        style: border.clone(),
    });
    top.push(StyledSpan {
        text: LABEL.to_string(),
        style: SpanStyle {
            fg: Some(ctx.theme.colors.heading3.clone()),
            ..Default::default()
        },
    });
    top.push(StyledSpan {
        text: format!(
            "{}{}",
            g.h.repeat(border_width.saturating_sub(LABEL.width() + 3)),
            g.tr
        ),
        style: border.clone(),
    });
    lines.push(top);

    for (key, values) in &rows {
        let mut visual: Vec<String> = Vec::new();
        let empty = [String::new()];
        let values = if values.is_empty() {
            &empty[..]
        } else {
            values
        };
        for value in values {
            // Raw nested lines keep their indentation.
            let lead = value.len() - value.trim_start_matches(' ').len();
            let lead = lead.min(value_w.saturating_sub(1));
            for part in wrap_text(value.trim_start_matches(' '), value_w - lead) {
                visual.push(format!("{}{part}", " ".repeat(lead)));
            }
        }
        for (i, text) in visual.iter().enumerate() {
            let mut line = StyledLine::new();
            ctx.add_margin(&mut line);
            line.push(StyledSpan {
                text: format!("{} ", g.v),
                style: border.clone(),
            });
            if key_w > 0 {
                let label = if i == 0 {
                    fit_label(key, key_w)
                } else {
                    " ".repeat(key_w)
                };
                line.push(StyledSpan {
                    text: format!("{label}{}", " ".repeat(gap)),
                    style: key_style.clone(),
                });
            }
            let pad = value_w.saturating_sub(text.width());
            line.push(StyledSpan {
                text: text.clone(),
                style: SpanStyle::default(),
            });
            line.push(StyledSpan {
                text: format!("{} {}", " ".repeat(pad), g.v),
                style: border.clone(),
            });
            lines.push(line);
        }
    }

    let mut bottom = StyledLine::new();
    ctx.add_margin(&mut bottom);
    bottom.push(StyledSpan {
        text: format!(
            "{}{}{}",
            g.bl,
            g.h.repeat(border_width.saturating_sub(2)),
            g.br
        ),
        style: border,
    });
    lines.push(bottom);

    // `c` copies the frontmatter as written, like any code block.
    if ctx.record_headings {
        ctx.code_blocks.borrow_mut().push(CodeBlockSpec {
            line_index: start_line,
            rows: lines.len() - start_line,
            lang: "frontmatter".to_string(),
            source: literal.trim_end_matches('\n').to_string(),
        });
    }
    add_spacing(ctx, lines);
}
