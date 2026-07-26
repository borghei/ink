use super::{SpanStyle, StyledLine, StyledSpan};
use crate::theme::Theme;
use comrak::nodes::{AstNode, NodeValue};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Minimum readable width for a shrunk table column.
const MIN_COL: usize = 5;

/// Layout a markdown table with word-wrapped cells that fit within max_width.
pub fn layout_table<'a>(
    node: &'a AstNode<'a>,
    theme: &Theme,
    max_width: usize,
    margin: usize,
    lines: &mut Vec<StyledLine>,
) {
    let margin_str = " ".repeat(margin);
    let (headers, rows) = extract_table_data(node);
    if headers.is_empty() {
        return;
    }

    let num_cols = headers.len();

    // Calculate ideal column widths from content
    let mut col_widths: Vec<usize> = headers.iter().map(|h| h.width()).collect();
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max(cell.width());
            }
        }
    }

    // Check if table fits in max_width
    let overhead = num_cols + 1 + num_cols * 2; // borders + padding
    let total_content: usize = col_widths.iter().sum();
    let total = total_content + overhead;

    // If the grid can't fit even at the minimum column width, the columns can't
    // sit side by side readably — fall back to a transposed key/value layout
    // (like `psql -x`) that fits any width.
    if num_cols * MIN_COL + overhead > max_width && !rows.is_empty() {
        render_transposed(&headers, &rows, theme, max_width, &margin_str, lines);
        return;
    }

    if total > max_width {
        // Shrink columns proportionally to fit. A small per-column floor keeps
        // narrow tables inside the width by wrapping cells hard rather than
        // overflowing (only reached when the table is wider than the view).
        let available = max_width.saturating_sub(overhead);
        if total_content > 0 {
            let scale = available as f64 / total_content as f64;
            for w in &mut col_widths {
                let new_w = ((*w as f64) * scale).floor() as usize;
                *w = new_w.max(MIN_COL);
            }
            // Trim excess from widest columns
            let mut sum: usize = col_widths.iter().sum();
            while sum > available {
                if let Some((idx, _)) = col_widths.iter().enumerate().max_by_key(|(_, w)| *w) {
                    if col_widths[idx] > MIN_COL {
                        col_widths[idx] -= 1;
                        sum -= 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }

    let border_color = &theme.colors.table_border;
    let header_color = &theme.colors.table_header;

    // Top border: ╭───┬───╮
    lines.push(border_line(
        &col_widths,
        '╭',
        '┬',
        '╮',
        border_color,
        &margin_str,
    ));

    // Header row (wrapped)
    let header_wrapped = wrap_row(&headers, &col_widths);
    render_row_lines(
        &header_wrapped,
        &col_widths,
        Some(header_color),
        border_color,
        false,
        theme,
        &margin_str,
        lines,
    );

    // Separator: ├───┼───┤
    lines.push(border_line(
        &col_widths,
        '├',
        '┼',
        '┤',
        border_color,
        &margin_str,
    ));

    // Data rows (wrapped)
    for (i, row) in rows.iter().enumerate() {
        let row_wrapped = wrap_row(row, &col_widths);
        render_row_lines(
            &row_wrapped,
            &col_widths,
            None,
            border_color,
            i % 2 == 1,
            theme,
            &margin_str,
            lines,
        );
    }

    // Bottom border: ╰───┴───╯
    lines.push(border_line(
        &col_widths,
        '╰',
        '┴',
        '╯',
        border_color,
        &margin_str,
    ));
    lines.push(StyledLine::empty());
}

/// Render a table too wide for the view as a stacked key/value layout: each
/// row becomes a record of `Header  value` lines, values wrapped, records
/// separated by a thin rule. Always fits the width.
fn render_transposed(
    headers: &[String],
    rows: &[Vec<String>],
    theme: &Theme,
    max_width: usize,
    margin: &str,
    lines: &mut Vec<StyledLine>,
) {
    const INDENT: usize = 2;
    const GAP: usize = 1;
    let header_color = &theme.colors.table_header;
    let border_color = &theme.colors.table_border;

    // Label column: the widest header, capped so values keep a usable width.
    let max_hdr = headers.iter().map(|h| h.width()).max().unwrap_or(0);
    let label_w = max_hdr
        .min(max_width.saturating_sub(INDENT + GAP + 8))
        .max(1);
    let value_w = max_width.saturating_sub(INDENT + label_w + GAP).max(1);

    let push_prefix = |line: &mut StyledLine| {
        if !margin.is_empty() {
            line.push(StyledSpan {
                text: margin.to_string(),
                style: SpanStyle::default(),
            });
        }
        line.push(StyledSpan {
            text: " ".repeat(INDENT),
            style: SpanStyle::default(),
        });
    };

    for (ri, row) in rows.iter().enumerate() {
        for (ci, header) in headers.iter().enumerate() {
            let value = row.get(ci).map(|s| s.as_str()).unwrap_or("");
            let value_lines = wrap_text(value, value_w);
            for (li, vline) in value_lines.iter().enumerate() {
                let mut line = StyledLine::new();
                push_prefix(&mut line);
                if li == 0 {
                    line.push(StyledSpan {
                        text: format!("{}{}", fit_label(header, label_w), " ".repeat(GAP)),
                        style: SpanStyle {
                            fg: Some(header_color.clone()),
                            bold: true,
                            ..Default::default()
                        },
                    });
                } else {
                    line.push(StyledSpan {
                        text: " ".repeat(label_w + GAP),
                        style: SpanStyle::default(),
                    });
                }
                line.push(StyledSpan {
                    text: vline.clone(),
                    style: SpanStyle::default(),
                });
                lines.push(line);
            }
        }
        // Thin separator between records (not after the last).
        if ri + 1 < rows.len() {
            let mut sep = StyledLine::new();
            if !margin.is_empty() {
                sep.push(StyledSpan {
                    text: margin.to_string(),
                    style: SpanStyle::default(),
                });
            }
            sep.push(StyledSpan {
                text: "╌".repeat(max_width),
                style: SpanStyle {
                    fg: Some(border_color.clone()),
                    ..Default::default()
                },
            });
            lines.push(sep);
        }
    }
    lines.push(StyledLine::empty());
}

/// Pad or truncate (with an ellipsis) `s` to exactly `w` display columns.
/// Measures grapheme clusters, never splitting inside one: per-char sums
/// disagree with the rendered width for emoji (VS16, ZWJ sequences), which
/// used to return labels wider than the column.
fn fit_label(s: &str, w: usize) -> String {
    if w == 0 {
        return String::new();
    }
    let sw = s.width();
    if sw == w {
        return s.to_string();
    }
    if sw < w {
        return format!("{s}{}", " ".repeat(w - sw));
    }
    let mut out = String::new();
    let mut acc = 0;
    for g in s.graphemes(true) {
        let gw = g.width();
        if acc + gw > w - 1 {
            break;
        }
        out.push_str(g);
        acc += gw;
    }
    out.push('…');
    acc += 1;
    if acc < w {
        out.push_str(&" ".repeat(w - acc));
    }
    out
}

/// Word-wrap each cell in a row to fit its column width.
/// Returns a Vec of columns, each column being a Vec of wrapped lines.
fn wrap_row(cells: &[String], widths: &[usize]) -> Vec<Vec<String>> {
    let mut wrapped_cols: Vec<Vec<String>> = Vec::new();

    for (i, width) in widths.iter().enumerate() {
        let cell = cells.get(i).map(|s| s.as_str()).unwrap_or("");
        let lines = wrap_text(cell, *width);
        wrapped_cols.push(lines);
    }

    wrapped_cols
}

/// Word-wrap text to fit within max_width characters.
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    if text.width() <= max_width {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = word.width();

        if current_width == 0 {
            // First word on line — if it's too long, force-break it
            if word_width > max_width {
                let broken = force_break(word, max_width);
                for (j, part) in broken.iter().enumerate() {
                    if j < broken.len() - 1 {
                        lines.push(part.clone());
                    } else {
                        current = part.clone();
                        current_width = part.width();
                    }
                }
            } else {
                current = word.to_string();
                current_width = word_width;
            }
        } else if current_width + 1 + word_width <= max_width {
            // Fits on current line
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            // Start new line
            lines.push(std::mem::take(&mut current));
            current_width = 0;
            if word_width > max_width {
                let broken = force_break(word, max_width);
                for (j, part) in broken.iter().enumerate() {
                    if j < broken.len() - 1 {
                        lines.push(part.clone());
                    } else {
                        current = part.clone();
                        current_width = part.width();
                    }
                }
            } else {
                current = word.to_string();
                current_width = word_width;
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Force-break a single long word into chunks of max_width, breaking only
/// between grapheme clusters so the chunk widths match the rendered widths.
fn force_break(word: &str, max_width: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for g in word.graphemes(true) {
        let gw = g.width();
        if current_width + gw > max_width && !current.is_empty() {
            parts.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push_str(g);
        current_width += gw;
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Render a multi-line table row (each cell may have multiple wrapped lines).
#[allow(clippy::too_many_arguments)]
fn render_row_lines(
    wrapped_cols: &[Vec<String>],
    widths: &[usize],
    text_color: Option<&String>,
    border_color: &str,
    alt_row: bool,
    theme: &Theme,
    margin: &str,
    lines: &mut Vec<StyledLine>,
) {
    // Find the tallest cell in this row
    let max_lines = wrapped_cols.iter().map(|col| col.len()).max().unwrap_or(1);

    for row_line in 0..max_lines {
        let mut line = StyledLine::new();
        if !margin.is_empty() {
            line.push(StyledSpan {
                text: margin.to_string(),
                style: SpanStyle::default(),
            });
        }
        line.push(StyledSpan {
            text: "│".to_string(),
            style: SpanStyle {
                fg: Some(border_color.to_string()),
                ..Default::default()
            },
        });

        for (col_idx, width) in widths.iter().enumerate() {
            let cell_text = wrapped_cols
                .get(col_idx)
                .and_then(|col| col.get(row_line))
                .map(|s| s.as_str())
                .unwrap_or("");

            let cell_width = cell_text.width();
            let padding = width.saturating_sub(cell_width);

            let mut style = SpanStyle::default();
            if let Some(c) = text_color {
                style.fg = Some(c.clone());
                style.bold = row_line == 0; // Only bold the first line of header
            }
            if alt_row {
                style.bg = Some(theme.colors.code_block_bg.clone());
            }

            line.push(StyledSpan {
                text: format!(" {cell_text}{} ", " ".repeat(padding)),
                style,
            });
            line.push(StyledSpan {
                text: "│".to_string(),
                style: SpanStyle {
                    fg: Some(border_color.to_string()),
                    ..Default::default()
                },
            });
        }

        lines.push(line);
    }
}

fn border_line(
    widths: &[usize],
    left: char,
    mid: char,
    right: char,
    color: &str,
    margin: &str,
) -> StyledLine {
    let mut line = StyledLine::new();
    if !margin.is_empty() {
        line.push(StyledSpan {
            text: margin.to_string(),
            style: SpanStyle::default(),
        });
    }
    let mut parts = String::new();
    parts.push(left);
    for (i, w) in widths.iter().enumerate() {
        parts.push_str(&"─".repeat(w + 2));
        if i < widths.len() - 1 {
            parts.push(mid);
        }
    }
    parts.push(right);
    line.push(StyledSpan {
        text: parts,
        style: SpanStyle {
            fg: Some(color.to_string()),
            ..Default::default()
        },
    });
    line
}

fn extract_table_data<'a>(node: &'a AstNode<'a>) -> (Vec<String>, Vec<Vec<String>>) {
    let mut headers = Vec::new();
    let mut rows = Vec::new();

    for child in node.children() {
        let data = child.data.borrow();
        match &data.value {
            NodeValue::TableRow(is_header) => {
                let header = *is_header;
                drop(data);
                let cells: Vec<String> = child
                    .children()
                    .map(|cell| collect_cell_text(cell))
                    .collect();
                if header {
                    headers = cells;
                } else {
                    rows.push(cells);
                }
            }
            _ => {
                drop(data);
            }
        }
    }

    (headers, rows)
}

fn collect_cell_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    collect_cell_text_inner(node, &mut text);
    text
}

fn collect_cell_text_inner<'a>(node: &'a AstNode<'a>, buf: &mut String) {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Text(t) => buf.push_str(t),
        NodeValue::Code(c) => {
            buf.push('`');
            buf.push_str(&c.literal);
            buf.push('`');
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => buf.push(' '),
        _ => {}
    }
    drop(data);
    for child in node.children() {
        collect_cell_text_inner(child, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `fit_label(s, w)` must return exactly `w` display columns for every
    // input — the transposed layout aligns values on that guarantee. The
    // emoji inputs are the regression: per-char width accounting returned a
    // 41-column label for a 27-column slot.
    #[test]
    fn fit_label_is_always_exact_width() {
        let inputs = [
            "plain header",
            "short",
            "⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️⚠️",
            "👨‍👩‍👧‍👦👨‍👩‍👧‍👦👨‍👩‍👧‍👦 family emoji header",
            "日本語のとても長いヘッダー",
            "",
        ];
        for s in inputs {
            for w in 1..30 {
                let fitted = fit_label(s, w);
                assert_eq!(
                    fitted.width(),
                    w,
                    "fit_label({s:?}, {w}) returned {:?} ({} cols)",
                    fitted,
                    fitted.width()
                );
            }
        }
        assert_eq!(fit_label("anything", 0), "");
    }

    // Truncation must never split inside a grapheme cluster: a ZWJ family
    // emoji is all-or-nothing.
    #[test]
    fn fit_label_never_splits_a_cluster() {
        let family = "👨‍👩‍👧‍👦"; // width 2, many chars
        let s = format!("{family}{family}{family}");
        let fitted = fit_label(&s, 3);
        assert_eq!(fitted.width(), 3);
        // Either a whole cluster survived or none did; no bare ZWJ fragments.
        assert!(!fitted.contains('\u{200D}') || fitted.contains(family));
    }

    #[test]
    fn force_break_measures_clusters() {
        let s = "⚠️".repeat(10); // 20 columns as rendered
        for part in force_break(&s, 4) {
            assert!(part.width() <= 4, "chunk {part:?} is {} cols", part.width());
        }
    }
}
