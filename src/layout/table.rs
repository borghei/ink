use super::{LayoutContext, SpanStyle, StyledLine, StyledSpan};
use crate::theme::Theme;
use comrak::nodes::{AstNode, NodeValue, TableAlignment};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Minimum readable width for a shrunk table column.
const MIN_COL: usize = 5;

/// A table cell's content: styled spans built with the same inline rules as a
/// paragraph (links, emphasis, inline code). A `\n` in a span's text is a hard
/// break (`<br>`, trailing backslash) that starts a new line in the cell. Wrapped
/// cell lines use the same type, minus the breaks.
type Cell = Vec<StyledSpan>;

/// Layout a markdown table with word-wrapped cells that fit within max_width.
pub(super) fn layout_table<'a>(
    node: &'a AstNode<'a>,
    ctx: &LayoutContext,
    lines: &mut Vec<StyledLine>,
) {
    let theme = ctx.theme;
    let max_width = ctx.width;
    let margin_str = " ".repeat(ctx.margin);
    let (headers, rows) = extract_table_data(node, ctx);
    if headers.is_empty() {
        return;
    }

    let num_cols = headers.len();
    let alignments: Vec<TableAlignment> = match &node.data.borrow().value {
        NodeValue::Table(t) => t.alignments.clone(),
        _ => Vec::new(),
    };

    // Check if table fits in max_width
    let overhead = num_cols + 1 + num_cols * 2; // borders + padding
    let available = max_width.saturating_sub(overhead);

    // Borders and padding alone can be wider than a very narrow view.
    let grid = if overhead > max_width {
        None
    } else {
        column_widths(&headers, &rows, available)
    };
    let col_widths = match grid {
        Some(widths) => widths,
        // Even each column's narrowest readable width doesn't fit side by
        // side: lay the rows out as key/value records (like `psql -x`),
        // which fit any width.
        None if !rows.is_empty() => {
            render_transposed(&headers, &rows, theme, max_width, &margin_str, lines);
            return;
        }
        // A header-only table has no records to stack; squeeze it.
        None => squeeze_widths(&ideal_widths(&headers, &rows), available),
    };

    let border_color = &theme.colors.table_border;
    let header_color = &theme.colors.table_header;

    let g = crate::glyphs::current();
    // Top border: ╭───┬───╮
    lines.push(border_line(
        &col_widths,
        g.tl,
        g.tee_down,
        g.tr,
        border_color,
        &margin_str,
    ));

    // Header row (wrapped)
    let header_wrapped = wrap_row(&headers, &col_widths);
    render_row_lines(
        &header_wrapped,
        &col_widths,
        &alignments,
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
        g.tee_right,
        g.cross,
        g.tee_left,
        border_color,
        &margin_str,
    ));

    // Data rows (wrapped)
    for (i, row) in rows.iter().enumerate() {
        let row_wrapped = wrap_row(row, &col_widths);
        render_row_lines(
            &row_wrapped,
            &col_widths,
            &alignments,
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
        g.bl,
        g.tee_up,
        g.br,
        border_color,
        &margin_str,
    ));
    lines.push(StyledLine::empty());
}

/// The longest word a column must show unbroken. Longer words (URLs, paths,
/// identifiers) break inside anyway, so they don't make the whole table
/// switch to the stacked layout.
const WORD_CAP: usize = 16;

/// Share of a column's non-empty body cells that must be numbers for it to
/// count as numeric, and so never wrap.
const NUMERIC_SHARE: f64 = 0.8;

/// Each column's natural width: its widest hard-broken line, header included.
fn ideal_widths(headers: &[Cell], rows: &[Vec<Cell>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|h| cell_width(h)).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(cell_width(cell));
            }
        }
    }
    widths
}

/// The narrowest a column can be and still read well: its longest word
/// (capped at [`WORD_CAP`]), or, for a numeric column, its widest number,
/// so `1,204.50` is never split into what look like two numbers.
fn min_widths(headers: &[Cell], rows: &[Vec<Cell>], ideal: &[usize]) -> Vec<usize> {
    (0..headers.len())
        .map(|i| {
            let body = || rows.iter().filter_map(move |r| r.get(i));
            let mut min = std::iter::once(&headers[i])
                .chain(body())
                .map(|c| longest_word(c).min(WORD_CAP))
                .max()
                .unwrap_or(0);
            if is_numeric_column(body()) {
                // Only the numbers stay whole; a text note in the column
                // ("n/a, refunded") still wraps.
                let widest_number = body()
                    .filter(|c| is_number(cell_text(c).trim()))
                    .map(|c| cell_width(c))
                    .max()
                    .unwrap_or(0);
                min = min.max(widest_number);
            }
            // An all-empty column has an ideal width of 0, and its minimum
            // must not exceed that.
            min.min(ideal[i])
        })
        .collect()
}

/// Column widths that sum to at most `available`: natural widths when they
/// fit, otherwise every column gets its minimum and the room left over goes
/// to the columns that want more, in proportion to how much more. `None`
/// when even the minimums don't fit side by side.
fn column_widths(headers: &[Cell], rows: &[Vec<Cell>], available: usize) -> Option<Vec<usize>> {
    let ideal = ideal_widths(headers, rows);
    if ideal.iter().sum::<usize>() <= available {
        return Some(ideal);
    }
    let min = min_widths(headers, rows, &ideal);
    let min_total: usize = min.iter().sum();
    if min_total > available {
        return None;
    }
    let slack = available - min_total;
    let wants: Vec<usize> = ideal.iter().zip(&min).map(|(i, m)| i - m).collect();
    let total_want: usize = wants.iter().sum();
    // `slack < total_want` here (the natural widths didn't fit), so no
    // column is offered more than it wants. Shares are floored, and the
    // columns with the largest remainders take the columns left over, so
    // the widths fill `available` exactly.
    let mut widths = min;
    let mut remainders = Vec::with_capacity(widths.len());
    let mut given = 0;
    for (i, want) in wants.iter().enumerate() {
        let exact = slack * want;
        widths[i] += exact / total_want;
        given += exact / total_want;
        remainders.push((exact % total_want, i));
    }
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for &(_, i) in remainders.iter().take(slack - given) {
        widths[i] += 1;
    }
    Some(widths)
}

/// Shrink `widths` proportionally into `available`, keeping [`MIN_COL`] per
/// column. Only for header-only tables, which have no stacked layout.
fn squeeze_widths(widths: &[usize], available: usize) -> Vec<usize> {
    let total: usize = widths.iter().sum();
    if total <= available || total == 0 {
        return widths.to_vec();
    }
    let scale = available as f64 / total as f64;
    let mut out: Vec<usize> = widths
        .iter()
        .map(|w| (((*w as f64) * scale).floor() as usize).max(MIN_COL))
        .collect();
    let mut sum: usize = out.iter().sum();
    while sum > available {
        match out.iter().enumerate().max_by_key(|(_, w)| **w) {
            Some((idx, w)) if *w > MIN_COL => {
                out[idx] -= 1;
                sum -= 1;
            }
            _ => break,
        }
    }
    out
}

/// Display width of the longest whitespace-separated word in a cell.
fn longest_word(cell: &[StyledSpan]) -> usize {
    split_lines(cell)
        .iter()
        .flat_map(|line| words(line))
        .map(|(_, w)| spans_width(&w))
        .max()
        .unwrap_or(0)
}

/// Whether most non-empty cells hold a single number: `42`, `-3.5`,
/// `1,204.50`, `$12`, `€3,000`, `15%`.
fn is_numeric_column<'c>(cells: impl Iterator<Item = &'c Cell>) -> bool {
    let (mut filled, mut numeric) = (0usize, 0usize);
    for cell in cells {
        let text = cell_text(cell);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        filled += 1;
        if is_number(text) {
            numeric += 1;
        }
    }
    filled > 0 && numeric as f64 >= filled as f64 * NUMERIC_SHARE
}

/// A cell's text with its styles dropped.
fn cell_text(cell: &[StyledSpan]) -> String {
    cell.iter().map(|s| s.text.as_str()).collect()
}

fn is_number(s: &str) -> bool {
    let s = s.strip_prefix(['-', '+', '\u{2212}']).unwrap_or(s);
    let s = s.strip_prefix(['$', '€', '£', '¥']).unwrap_or(s);
    let s = s.strip_suffix('%').unwrap_or(s);
    !s.is_empty()
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
}

/// Render a table too wide for the view as a stacked key/value layout: each
/// row becomes a record of `Header  value` lines, values wrapped, records
/// separated by a thin rule. Always fits the width.
fn render_transposed(
    headers: &[Cell],
    rows: &[Vec<Cell>],
    theme: &Theme,
    max_width: usize,
    margin: &str,
    lines: &mut Vec<StyledLine>,
) {
    const INDENT: usize = 2;
    const GAP: usize = 1;
    let header_color = &theme.colors.table_header;
    let border_color = &theme.colors.table_border;
    let label_style = SpanStyle {
        fg: Some(header_color.clone()),
        bold: true,
        ..Default::default()
    };

    // Label column: the widest header, capped so values keep a usable width.
    // A hard break in a header has no second line to go to in the label
    // column; it reads as a space there.
    let headers: Vec<Cell> = headers
        .iter()
        .map(|h| {
            h.iter()
                .map(|s| StyledSpan {
                    text: s.text.replace('\n', " "),
                    style: s.style.clone(),
                })
                .collect()
        })
        .collect();
    let max_hdr = headers.iter().map(|h| spans_width(h)).max().unwrap_or(0);
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

    let empty = Cell::new();
    for (ri, row) in rows.iter().enumerate() {
        for (ci, header) in headers.iter().enumerate() {
            let value = row.get(ci).unwrap_or(&empty);
            let value_lines = wrap_spans(value, value_w);
            for (li, vline) in value_lines.iter().enumerate() {
                let mut line = StyledLine::new();
                push_prefix(&mut line);
                if li == 0 {
                    let mut label = fit_spans(header, label_w);
                    push_span(&mut label, &" ".repeat(GAP), &SpanStyle::default());
                    line.spans.extend(with_base(&label, &label_style));
                } else {
                    line.push(StyledSpan {
                        text: " ".repeat(label_w + GAP),
                        style: SpanStyle::default(),
                    });
                }
                line.spans.extend(with_base(vline, &SpanStyle::default()));
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
                text: crate::glyphs::current().dashed.repeat(max_width),
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
pub(super) fn fit_label(s: &str, w: usize) -> String {
    fit_spans(&plain(s), w)
        .iter()
        .map(|span| span.text.as_str())
        .collect()
}

/// `fit_label` over styled spans: each kept piece keeps its style, the
/// ellipsis takes the style of the last kept piece, padding is unstyled.
fn fit_spans(spans: &[StyledSpan], w: usize) -> Cell {
    let mut out = Cell::new();
    if w == 0 {
        return out;
    }
    let sw = spans_width(spans);
    if sw <= w {
        for span in spans {
            push_span(&mut out, &span.text, &span.style);
        }
        push_span(&mut out, &" ".repeat(w - sw), &SpanStyle::default());
        return out;
    }
    let ellipsis = crate::glyphs::current().ellipsis;
    // The ellipsis only fits when the column is wider than it.
    let ellipsis = if ellipsis.width() < w { ellipsis } else { "" };
    let mut acc = 0;
    'fill: for span in spans {
        for g in span.text.graphemes(true) {
            let gw = g.width();
            if acc + gw > w - ellipsis.width() {
                break 'fill;
            }
            push_span(&mut out, g, &span.style);
            acc += gw;
        }
    }
    // The ellipsis continues the last text shown, so it never links to text
    // that was cut away entirely.
    let cut_style = out.last().map(|s| s.style.clone()).unwrap_or_default();
    push_span(&mut out, ellipsis, &cut_style);
    acc += ellipsis.width();
    if acc < w {
        push_span(&mut out, &" ".repeat(w - acc), &SpanStyle::default());
    }
    out
}

/// Word-wrap each cell in a row to fit its column width.
/// Returns a Vec of columns, each column being a Vec of wrapped lines.
fn wrap_row(cells: &[Cell], widths: &[usize]) -> Vec<Vec<Cell>> {
    let empty = Cell::new();
    widths
        .iter()
        .enumerate()
        .map(|(i, width)| wrap_spans(cells.get(i).unwrap_or(&empty), *width))
        .collect()
}

/// Display width of a cell: its widest hard-broken line.
fn cell_width(cell: &[StyledSpan]) -> usize {
    split_lines(cell)
        .iter()
        .map(|l| spans_width(l))
        .max()
        .unwrap_or(0)
}

/// Display width of a run of spans. Only the text counts; styles (and a
/// link's destination) take no columns.
fn spans_width(spans: &[StyledSpan]) -> usize {
    spans.iter().map(|s| s.text.width()).sum()
}

/// `text` as a single unstyled span.
fn plain(text: &str) -> Cell {
    vec![StyledSpan {
        text: text.to_string(),
        style: SpanStyle::default(),
    }]
}

/// Append `text` in `style`, extending the last span when it has the same
/// style, so an unstyled cell stays one span (and one escape sequence).
fn push_span(out: &mut Cell, text: &str, style: &SpanStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.style == *style {
            last.text.push_str(text);
            return;
        }
    }
    out.push(StyledSpan {
        text: text.to_string(),
        style: style.clone(),
    });
}

/// Split a cell on its hard breaks into lines (none of them containing `\n`).
fn split_lines(cell: &[StyledSpan]) -> Vec<Cell> {
    let mut lines = vec![Cell::new()];
    for span in cell {
        for (i, seg) in span.text.split('\n').enumerate() {
            if i > 0 {
                lines.push(Cell::new());
            }
            if let Some(line) = lines.last_mut() {
                push_span(line, seg, &span.style);
            }
        }
    }
    lines
}

/// Strip leading and trailing whitespace from a line of spans.
fn trim_spans(mut line: Cell) -> Cell {
    while line.first().is_some_and(|s| s.text.trim_start().is_empty()) {
        line.remove(0);
    }
    if let Some(first) = line.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    while line.last().is_some_and(|s| s.text.trim_end().is_empty()) {
        line.pop();
    }
    if let Some(last) = line.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    line
}

/// The cell's spans restyled on top of the cell's own `base` style (header
/// colour and bold, the alternate-row background): what the inline markup
/// sets wins, and the base fills in the rest.
fn with_base(spans: &[StyledSpan], base: &SpanStyle) -> Cell {
    let mut out = Cell::new();
    for span in spans {
        let s = &span.style;
        let style = SpanStyle {
            fg: s.fg.clone().or_else(|| base.fg.clone()),
            bg: s.bg.clone().or_else(|| base.bg.clone()),
            bold: s.bold || base.bold,
            italic: s.italic || base.italic,
            underline: s.underline || base.underline,
            strikethrough: s.strikethrough || base.strikethrough,
            dim: s.dim || base.dim,
            link_url: s.link_url.clone().or_else(|| base.link_url.clone()),
        };
        push_span(&mut out, &span.text, &style);
    }
    out
}

/// Word-wrap text to fit within max_width characters. A `\n` (hard break in
/// the cell) always starts a new line.
pub(super) fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    wrap_spans(&plain(text), max_width)
        .iter()
        .map(|line| line.iter().map(|s| s.text.as_str()).collect())
        .collect()
}

/// `wrap_text` over styled spans. Every piece keeps its span's style, so a
/// link broken across lines carries its destination on each fragment.
fn wrap_spans(cell: &[StyledSpan], max_width: usize) -> Vec<Cell> {
    if cell.iter().any(|s| s.text.contains('\n')) {
        return split_lines(cell)
            .into_iter()
            .flat_map(|line| wrap_spans(&trim_spans(line), max_width))
            .collect();
    }
    if max_width == 0 || spans_width(cell) <= max_width {
        let mut line = Cell::new();
        for span in cell {
            push_span(&mut line, &span.text, &span.style);
        }
        return vec![line];
    }

    let mut lines = Vec::new();
    let mut current = Cell::new();
    let mut current_width = 0;

    for (space_style, word) in words(cell) {
        let word_width = spans_width(&word);

        if !current.is_empty() && current_width + 1 + word_width <= max_width {
            // Fits on current line
            push_span(&mut current, " ", &space_style);
            for span in &word {
                push_span(&mut current, &span.text, &span.style);
            }
            current_width += 1 + word_width;
            continue;
        }
        if !current.is_empty() {
            // Start new line
            lines.push(std::mem::take(&mut current));
        }
        if word_width > max_width {
            // Too long for any line: force-break it, the last piece stays
            // open for the words that follow.
            let mut broken = force_break(&word, max_width);
            current = broken.pop().unwrap_or_default();
            lines.extend(broken);
        } else {
            current = word;
        }
        current_width = spans_width(&current);
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(Cell::new());
    }

    lines
}

/// Split a line of spans into whitespace-separated words. A word may span
/// several styles (`**bold**tail`); each comes with the style of the space in
/// front of it, so a space inside a link stays part of the link when joined.
fn words(line: &[StyledSpan]) -> Vec<(SpanStyle, Cell)> {
    let mut out = Vec::new();
    let mut word = Cell::new();
    let mut word_space = SpanStyle::default();
    let mut space: Option<SpanStyle> = None;
    for span in line {
        let mut start = None;
        for (i, ch) in span.text.char_indices() {
            if ch.is_whitespace() {
                if let Some(s) = start.take() {
                    push_span(&mut word, &span.text[s..i], &span.style);
                }
                if !word.is_empty() {
                    out.push((std::mem::take(&mut word_space), std::mem::take(&mut word)));
                    space = None;
                }
                space.get_or_insert_with(|| span.style.clone());
            } else if start.is_none() {
                start = Some(i);
                if word.is_empty() {
                    word_space = space.take().unwrap_or_default();
                }
            }
        }
        if let Some(s) = start {
            push_span(&mut word, &span.text[s..], &span.style);
        }
    }
    if !word.is_empty() {
        out.push((word_space, word));
    }
    out
}

/// Force-break a single long word into chunks of max_width, breaking only
/// between grapheme clusters so the chunk widths match the rendered widths.
/// A URL or path breaks after a `/ ? & = _ - .` when one sits in the second
/// half of the chunk, so `grant_type=client_credentials` splits at the `=`
/// rather than mid-word.
fn force_break(word: &[StyledSpan], max_width: usize) -> Vec<Cell> {
    let clusters: Vec<(&str, &SpanStyle)> = word
        .iter()
        .flat_map(|span| span.text.graphemes(true).map(move |g| (g, &span.style)))
        .collect();
    let mut parts = Vec::new();
    let mut start = 0;
    while start < clusters.len() {
        // As many clusters as fit, and always at least one.
        let mut end = start;
        let mut width = 0;
        while end < clusters.len() && (end == start || width + clusters[end].0.width() <= max_width)
        {
            width += clusters[end].0.width();
            end += 1;
        }
        if end < clusters.len() {
            let mut used = 0;
            let mut cut = None;
            for (k, (g, _)) in clusters[start..end].iter().enumerate() {
                used += g.width();
                if is_break_after(g) && k + 1 < end - start && used * 2 >= max_width {
                    cut = Some(start + k + 1);
                }
            }
            if let Some(cut) = cut {
                end = cut;
            }
        }
        let mut part = Cell::new();
        for (g, style) in &clusters[start..end] {
            push_span(&mut part, g, style);
        }
        parts.push(part);
        start = end;
    }
    parts
}

/// Characters a long URL, path, host name or identifier may break after.
fn is_break_after(g: &str) -> bool {
    matches!(g, "/" | "?" | "&" | "=" | "_" | "-" | ".")
}

/// Render a multi-line table row (each cell may have multiple wrapped lines).
#[allow(clippy::too_many_arguments)]
fn render_row_lines(
    wrapped_cols: &[Vec<Cell>],
    widths: &[usize],
    alignments: &[TableAlignment],
    text_color: Option<&String>,
    border_color: &str,
    alt_row: bool,
    theme: &Theme,
    margin: &str,
    lines: &mut Vec<StyledLine>,
) {
    // Find the tallest cell in this row
    let max_lines = wrapped_cols.iter().map(|col| col.len()).max().unwrap_or(1);
    let empty = Cell::new();

    for row_line in 0..max_lines {
        let mut line = StyledLine::new();
        if !margin.is_empty() {
            line.push(StyledSpan {
                text: margin.to_string(),
                style: SpanStyle::default(),
            });
        }
        line.push(StyledSpan {
            text: crate::glyphs::current().v.to_string(),
            style: SpanStyle {
                fg: Some(border_color.to_string()),
                ..Default::default()
            },
        });

        for (col_idx, width) in widths.iter().enumerate() {
            let cell_line = wrapped_cols
                .get(col_idx)
                .and_then(|col| col.get(row_line))
                .unwrap_or(&empty);

            let (pad_left, pad_right) = align_padding(
                alignments
                    .get(col_idx)
                    .copied()
                    .unwrap_or(TableAlignment::None),
                *width,
                spans_width(cell_line),
            );

            let mut style = SpanStyle::default();
            if let Some(c) = text_color {
                style.fg = Some(c.clone());
                style.bold = true;
            }
            if alt_row {
                style.bg = Some(theme.colors.code_block_bg.clone());
            }

            // Padding in the cell's base style, the text in its own styles
            // laid over that base; same-styled runs merge into one span.
            let mut cell = Cell::new();
            push_span(&mut cell, &format!(" {}", " ".repeat(pad_left)), &style);
            for span in with_base(cell_line, &style) {
                push_span(&mut cell, &span.text, &span.style);
            }
            push_span(&mut cell, &format!("{} ", " ".repeat(pad_right)), &style);
            line.spans.extend(cell);
            line.push(StyledSpan {
                text: crate::glyphs::current().v.to_string(),
                style: SpanStyle {
                    fg: Some(border_color.to_string()),
                    ..Default::default()
                },
            });
        }

        lines.push(line);
    }
}

/// Left and right padding that places `text_width` columns of cell text in
/// a `width`-column slot per the column's `:--`/`:-:`/`--:` alignment. Each
/// wrapped line of a cell is placed on its own, so a right-aligned
/// multi-line cell is ragged left. Centering leans left on an odd remainder
/// (as GitHub's HTML tables do).
fn align_padding(align: TableAlignment, width: usize, text_width: usize) -> (usize, usize) {
    let free = width.saturating_sub(text_width);
    match align {
        TableAlignment::Right => (free, 0),
        TableAlignment::Center => (free / 2, free - free / 2),
        TableAlignment::Left | TableAlignment::None => (0, free),
    }
}

fn border_line(
    widths: &[usize],
    left: &str,
    mid: &str,
    right: &str,
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
    let h = crate::glyphs::current().h;
    parts.push_str(left);
    for (i, w) in widths.iter().enumerate() {
        parts.push_str(&h.repeat(w + 2));
        if i < widths.len() - 1 {
            parts.push_str(mid);
        }
    }
    parts.push_str(right);
    line.push(StyledSpan {
        text: parts,
        style: SpanStyle {
            fg: Some(color.to_string()),
            ..Default::default()
        },
    });
    line
}

fn extract_table_data<'a>(
    node: &'a AstNode<'a>,
    ctx: &LayoutContext,
) -> (Vec<Cell>, Vec<Vec<Cell>>) {
    let mut headers = Vec::new();
    let mut rows = Vec::new();

    for child in node.children() {
        let data = child.data.borrow();
        match &data.value {
            NodeValue::TableRow(is_header) => {
                let header = *is_header;
                drop(data);
                let cells: Vec<Cell> = child
                    .children()
                    .map(|cell| collect_cell(cell, ctx))
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

/// A cell's inline content, styled as a paragraph styles it: links (with
/// their destination, sanitized with everything else by `sanitize_lines`),
/// emphasis, inline code without its backticks, math. A soft break is a
/// space, a hard break or `<br>` a `\n` that `wrap_spans` splits on.
fn collect_cell<'a>(node: &'a AstNode<'a>, ctx: &LayoutContext) -> Cell {
    let mut cell = Cell::new();
    for span in super::collect_inline_spans(node, ctx) {
        push_span(&mut cell, &span.text, &span.style);
    }
    if cell.iter().any(|s| s.text.contains('\n')) {
        // A break opens a new line only between content: a trailing or
        // leading `<br>`, or `<br><br>`, must not add an empty row line
        // (paragraphs drop a trailing break the same way).
        let kept: Vec<Cell> = split_lines(&cell)
            .into_iter()
            .filter(|l| !l.iter().all(|s| s.text.trim().is_empty()))
            .collect();
        cell = Cell::new();
        for (i, line) in kept.iter().enumerate() {
            if i > 0 {
                push_span(&mut cell, "\n", &SpanStyle::default());
            }
            for span in line {
                push_span(&mut cell, &span.text, &span.style);
            }
        }
    }
    cell
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parsed first table in `src`, handed to `f` with a layout context.
    fn with_table<R>(src: &str, f: impl for<'a> FnOnce(&'a AstNode<'a>, &LayoutContext) -> R) -> R {
        let arena = comrak::Arena::new();
        let root = comrak::parse_document(&arena, src, &crate::parser::options());
        let table = root
            .descendants()
            .find(|n| matches!(n.data.borrow().value, NodeValue::Table(_)))
            .unwrap();
        super::super::with_test_ctx(|ctx| f(table, ctx))
    }

    /// The body cells of the first table in `src`.
    fn body_cells(src: &str) -> Vec<Vec<Cell>> {
        with_table(src, |table, ctx| extract_table_data(table, ctx).1)
    }

    fn text(spans: &[StyledSpan]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn inline_code_in_a_cell_drops_its_backticks() {
        let cells = body_cells("| a | b |\n|---|---|\n| `GET /sync` | x `y` z |\n");
        assert_eq!(text(&cells[0][0]), "GET /sync");
        assert_eq!(text(&cells[0][1]), "x y z");
    }

    #[test]
    fn breaks_at_a_cell_edge_or_doubled_add_no_empty_lines() {
        let head = "| a | b |\n|---|---|\n";
        for (row, want) in [
            ("| step one<br>step two<br> | ok |", "step one\nstep two"),
            ("| <br>step one<br>step two | ok |", "step one\nstep two"),
            ("| one<br><br>two | ok |", "one\ntwo"),
            ("| one<br> <br/><BR>two<br><br> | ok |", "one\ntwo"),
            ("| <br> | ok |", ""),
            ("| one<br>two | ok |", "one\ntwo"),
        ] {
            let cells = body_cells(&format!("{head}{row}\n"));
            assert_eq!(text(&cells[0][0]), want, "{row}");
            // The row is exactly as tall as its content.
            let lines = &wrap_row(&cells[0], &[20, 4])[0];
            assert_eq!(lines.len(), want.split('\n').count().max(1), "{row}");
        }
    }

    #[test]
    fn inline_styles_in_a_cell_carry_their_style() {
        with_table(
            "| a |\n|---|\n| **bold** *it* `code` ~~gone~~ [ln](https://e.com/x) |\n",
            |table, ctx| {
                let cell = &extract_table_data(table, ctx).1[0][0];
                assert_eq!(text(cell), "bold it code gone ln");
                let find = |t: &str| cell.iter().find(|s| s.text == t).unwrap().style.clone();
                assert!(find("bold").bold);
                assert!(find("it").italic);
                assert_eq!(find("code").bg.as_ref(), Some(&ctx.theme.colors.code_bg));
                assert!(find("gone").strikethrough);
                assert_eq!(find("ln").link_url.as_deref(), Some("https://e.com/x"));
            },
        );
    }

    #[test]
    fn a_wrapped_link_keeps_its_destination_on_every_fragment() {
        let cells = body_cells("| a |\n|---|\n| see [alpha beta gamma](https://e.com/abc) |\n");
        let lines = wrap_spans(&cells[0][0], 8);
        assert_eq!(
            lines.iter().map(|l| text(l)).collect::<Vec<_>>(),
            vec!["see", "alpha", "beta", "gamma"]
        );
        for line in &lines[1..] {
            for span in line {
                assert_eq!(span.style.link_url.as_deref(), Some("https://e.com/abc"));
            }
        }
        // A word longer than the column is force-broken; every piece links.
        let cells = body_cells("| a |\n|---|\n| [abcdefghijkl](https://e.com/w) |\n");
        let lines = wrap_spans(&cells[0][0], 5);
        assert_eq!(lines.len(), 3, "{lines:#?}");
        for span in lines.iter().flatten() {
            assert_eq!(span.style.link_url.as_deref(), Some("https://e.com/w"));
        }
    }

    // A cell without markup wraps exactly as plain text did before cells
    // became styled spans: one span per line, same breaks.
    #[test]
    fn plain_cells_wrap_like_plain_text() {
        let s = "the quick brown fox jumps over a supercalifragilistic dog";
        for w in [0, 3, 5, 9, 14, 80] {
            let lines = wrap_spans(&plain(s), w);
            assert!(lines.iter().all(|l| l.len() <= 1), "{lines:#?}");
        }
        assert_eq!(
            wrap_text(s, 14),
            vec![
                "the quick",
                "brown fox",
                "jumps over a",
                "supercalifragi",
                "listic dog"
            ]
        );
    }

    // The space inside a link joins the link; the one before it does not.
    #[test]
    fn a_space_takes_the_style_of_the_text_around_it() {
        let cells = body_cells("| a |\n|---|\n| see [alpha beta](https://e.com/s) and more |\n");
        let lines = wrap_spans(&cells[0][0], 16);
        let texts: Vec<_> = lines[0].iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["see ", "alpha beta"]);
        assert_eq!(lines[0][0].style.link_url, None);
        assert_eq!(
            lines[0][1].style.link_url.as_deref(),
            Some("https://e.com/s")
        );
        assert_eq!(text(&lines[1]), "and more");
    }

    // The cell's base style (header colour and bold, alternate-row
    // background) fills in what the inline style leaves unset.
    #[test]
    fn inline_style_is_laid_over_the_cell_base_style() {
        let base = SpanStyle {
            fg: Some("#header".into()),
            bg: Some("#altrow".into()),
            bold: true,
            ..Default::default()
        };
        let link = StyledSpan {
            text: "ln".into(),
            style: SpanStyle {
                fg: Some("#link".into()),
                underline: true,
                link_url: Some("https://e.com".into()),
                ..Default::default()
            },
        };
        let out = with_base(&[plain("x ")[0].clone(), link], &base);
        assert_eq!(out[0].style, base);
        let s = &out[1].style;
        assert_eq!(s.fg.as_deref(), Some("#link"));
        assert_eq!(s.bg.as_deref(), Some("#altrow"));
        assert!(s.bold && s.underline);
        assert_eq!(s.link_url.as_deref(), Some("https://e.com"));
    }

    #[test]
    fn a_fitted_styled_label_is_exact_width_and_never_links_cut_text() {
        let linked = SpanStyle {
            link_url: Some("https://e.com".into()),
            ..Default::default()
        };
        let label = vec![
            plain("Header ")[0].clone(),
            StyledSpan {
                text: "linked".into(),
                style: linked.clone(),
            },
        ];
        for w in 1..16 {
            assert_eq!(spans_width(&fit_spans(&label, w)), w, "w={w}");
        }
        // Cut before the link: nothing (not even the ellipsis) links.
        assert!(fit_spans(&label, 7)
            .iter()
            .all(|s| s.style.link_url.is_none()));
        // Cut inside it: the shown part and its ellipsis link.
        let cut = fit_spans(&label, 10);
        assert_eq!(cut.last().unwrap().style, linked);
    }

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
        for part in force_break(&plain(&s), 4) {
            let w = spans_width(&part);
            assert!(w <= 4, "chunk {part:?} is {w} cols");
        }
    }
    /// Column widths for the first table in `src` with `available` content
    /// columns, `None` when it would be stacked.
    fn widths(src: &str, available: usize) -> Option<Vec<usize>> {
        with_table(src, |table, ctx| {
            let (headers, rows) = extract_table_data(table, ctx);
            column_widths(&headers, &rows, available)
        })
    }

    const KEY_DESC: &str = "| Key | Description |\n|---|---|\n\
        | timeout | The number of milliseconds to wait before the request is aborted. |\n\
        | retries | How many times to retry. |\n";

    #[test]
    fn a_short_column_keeps_its_width_while_a_long_one_wraps() {
        for available in [30, 50, 70] {
            let w = widths(KEY_DESC, available).unwrap();
            assert_eq!(w[0], "timeout".len(), "at {available}");
            assert_eq!(w.iter().sum::<usize>(), available, "at {available}");
        }
    }

    #[test]
    fn natural_widths_are_used_when_they_fit() {
        let w = widths("| a | bb |\n|---|---|\n| ccc | d |\n", 40).unwrap();
        assert_eq!(w, vec![3, 2]);
    }

    #[test]
    fn a_numeric_column_is_never_split() {
        let src = "| Item | Amount |\n|---|--:|\n\
            | A fairly long description of the item | 1,204.50 |\n\
            | Another one | -998.00 |\n";
        let w = widths(src, 20).unwrap();
        assert_eq!(w[1], "1,204.50".len());
        assert_eq!(w.iter().sum::<usize>(), 20);
    }

    #[test]
    fn numbers_are_recognised() {
        for n in [
            "42",
            "-3.5",
            "1,204.50",
            "$12",
            "€3,000",
            "15%",
            "+7",
            "\u{2212}4",
        ] {
            assert!(is_number(n), "{n}");
        }
        for n in ["", "-", "v1.2a", "1/2", "12 apples", "$", "."] {
            assert!(!is_number(n), "{n}");
        }
    }

    #[test]
    fn a_column_of_mostly_text_is_not_numeric() {
        let cells: Vec<Cell> = ["1", "2", "n/a", "x", "3"]
            .iter()
            .map(|t| plain(t))
            .collect();
        assert!(!is_numeric_column(cells.iter()));
        let cells: Vec<Cell> = ["1", "2", "", "4", "5", "?"]
            .iter()
            .map(|t| plain(t))
            .collect();
        assert!(is_numeric_column(cells.iter()));
    }

    #[test]
    fn the_table_is_stacked_exactly_when_its_words_cannot_fit() {
        let src = "| Name | Region | Status |\n|---|---|---|\n| Alpha | EMEA | active |\n";
        // Longest words: Alpha/Name 5, Region 6, Status/active 6 = 17.
        assert!(widths(src, 17).is_some());
        assert!(widths(src, 16).is_none());
    }

    #[test]
    fn a_long_url_does_not_force_the_stacked_layout() {
        let src = "| Service | Endpoint |\n|---|---|\n\
            | auth | https://auth.example.com/v2/oauth/token?grant_type=client_credentials |\n";
        let w = widths(src, 30).unwrap();
        assert!(w[1] >= WORD_CAP);
        assert_eq!(w.iter().sum::<usize>(), 30);
    }

    #[test]
    fn widths_always_fill_the_space_exactly() {
        let src = "| a | b | c |\n|---|---|---|\n\
            | one two three four five | six seven | eight nine ten eleven twelve thirteen |\n";
        for available in 20..60 {
            if let Some(w) = widths(src, available) {
                assert_eq!(w.iter().sum::<usize>(), available, "at {available}");
            }
        }
    }

    #[test]
    fn a_wide_character_column_is_measured_in_display_columns() {
        let src = "| 名前 | Note |\n|---|---|\n| 日本語サービス | some longer text that wraps |\n";
        let w = widths(src, 24).unwrap();
        assert_eq!(w[0], "日本語サービス".width());
    }

    #[test]
    fn a_long_url_breaks_after_a_separator() {
        let url = "https://auth.example.com/v2/oauth/token?grant_type=client";
        let parts: Vec<String> = force_break(&plain(url), 20)
            .iter()
            .map(|p| text(p))
            .collect();
        assert_eq!(parts.concat(), url);
        for p in &parts[..parts.len() - 1] {
            assert!(p.width() <= 20, "{p}");
            assert!(
                is_break_after(&p[p.len() - 1..]),
                "{p:?} doesn't end at a separator"
            );
        }
    }

    #[test]
    fn a_word_without_separators_still_breaks_at_the_width() {
        let parts: Vec<String> = force_break(&plain(&"x".repeat(25)), 10)
            .iter()
            .map(|p| text(p))
            .collect();
        assert_eq!(parts, vec!["x".repeat(10), "x".repeat(10), "x".repeat(5)]);
    }

    #[test]
    fn every_line_of_a_wrapped_header_is_bold() {
        let src = "| First line<br>second line | b |\n|---|---|\n| x | y |\n";
        let lines = with_table(src, |table, ctx| {
            let mut out = Vec::new();
            layout_table(table, ctx, &mut out);
            out
        });
        let header: Vec<_> = lines[1..]
            .iter()
            .take_while(|l| {
                !l.spans
                    .iter()
                    .any(|s| s.text.contains('┼') || s.text.contains('+'))
            })
            .collect();
        assert!(header.len() > 1, "header should wrap: {header:?}");
        for line in header {
            let text_spans: Vec<_> = line
                .spans
                .iter()
                .filter(|s| s.text.trim().chars().any(char::is_alphanumeric))
                .collect();
            assert!(text_spans.iter().all(|s| s.style.bold), "{line:?}");
        }
    }
}
