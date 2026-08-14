pub mod plain;

use crate::layout::StyledLine;
use crate::search::SearchState;
use crate::selection::Selection;
use crate::theme;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Convert our StyledLine list into ratatui Lines for display.
pub fn styled_lines_to_ratatui(lines: &[StyledLine], theme_name: &str) -> Vec<Line<'static>> {
    let t = theme::resolve_theme(theme_name);
    lines
        .iter()
        .map(|line| {
            let spans: Vec<Span<'static>> =
                line.spans.iter().map(|s| span_to_ratatui(s, &t)).collect();
            Line::from(spans)
        })
        .collect()
}

fn span_to_ratatui(span: &crate::layout::StyledSpan, t: &theme::Theme) -> Span<'static> {
    let mut style = Style::default();

    // Always set fg from span or theme default
    let fg = span.style.fg.as_ref().unwrap_or(&t.colors.fg);
    style = style.fg(theme::hex_to_color(fg));

    // Always set bg from span or theme default
    if let Some(ref bg) = span.style.bg {
        style = style.bg(theme::hex_to_color(bg));
    } else if let Some(ref theme_bg) = t.colors.bg {
        style = style.bg(theme::hex_to_color(theme_bg));
    }

    if span.style.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if span.style.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if span.style.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if span.style.strikethrough {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    // No DIM — broken on light themes, inconsistent across terminals
    Span::styled(span.text.clone(), style)
}

/// Top bar: progress bar only (thin colored line)
#[allow(clippy::too_many_arguments)]
pub fn render_top_bar(
    frame: &mut Frame,
    area: Rect,
    _filename: &str,
    scroll_offset: usize,
    total_lines: usize,
    viewport_height: usize,
    t: &theme::Theme,
    _tab_info: Option<(usize, usize)>,
) {
    let accent = theme::hex_to_color(&t.colors.heading2);
    let bg = theme::hex_to_color(&t.colors.status_bar_bg);
    let width = area.width as usize;

    if total_lines == 0 || total_lines <= viewport_height {
        let line = Line::from(Span::styled(
            "▔".repeat(width),
            Style::default().fg(accent).bg(bg),
        ));
        frame.render_widget(Paragraph::new(vec![line]), area);
        return;
    }

    let progress = (scroll_offset as f64 / (total_lines - viewport_height) as f64).min(1.0);
    let filled = ((progress * width as f64) as usize).max(1);
    let empty = width.saturating_sub(filled);

    let line = Line::from(vec![
        Span::styled("▔".repeat(filled), Style::default().fg(accent).bg(bg)),
        Span::styled("▔".repeat(empty), Style::default().fg(bg).bg(bg)),
    ]);
    frame.render_widget(Paragraph::new(vec![line]), area);
}

/// Bottom bar: keybindings (left) + filename + stats (right)
#[allow(clippy::too_many_arguments)]
pub fn render_bottom_bar(
    frame: &mut Frame,
    area: Rect,
    t: &theme::Theme,
    filename: &str,
    word_count: usize,
    reading_time: usize,
    multi_tab: bool,
    tab_info: Option<(usize, usize)>,
    file_missing: bool,
) {
    let bg = theme::hex_to_color(&t.colors.status_bar_bg);
    let fg = theme::hex_to_color(&t.colors.status_bar_fg);
    let dim_fg = theme::hex_to_color(&t.colors.link_url);

    let mut keys: Vec<(&str, &str)> = vec![
        ("↑↓/jk", "scroll"),
        ("/", "search"),
        ("f", "links"),
        ("t", "toc"),
        ("?", "help"),
    ];
    if multi_tab {
        keys.push(("Tab", "next"));
    }
    keys.push(("q", "quit"));

    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, (key, desc)) in keys.iter().enumerate() {
        spans.push(Span::styled(
            format!(" {key} "),
            Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            desc.to_string(),
            Style::default().fg(dim_fg).bg(bg),
        ));
        if i < keys.len() - 1 {
            spans.push(Span::styled(" · ", Style::default().fg(dim_fg).bg(bg)));
        }
    }

    // Right side: filename + stats
    let short_name = std::path::Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(filename);

    let tab_part = if let Some((current, total)) = tab_info {
        format!(" [{}/{}]", current + 1, total)
    } else {
        String::new()
    };

    let right_name = format!("{short_name}{tab_part}");
    let right_stats = format!("  {word_count} words · ~{reading_time} min  ");
    let missing_label = if file_missing { " [file missing] " } else { "" };
    let right_total_len = unicode_width::UnicodeWidthStr::width(right_name.as_str())
        + unicode_width::UnicodeWidthStr::width(right_stats.as_str())
        + unicode_width::UnicodeWidthStr::width(missing_label);

    let left_len: usize = spans
        .iter()
        .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    let total_width = area.width as usize;
    let padding_len = total_width.saturating_sub(left_len + right_total_len);
    let bar_style = Style::default().fg(fg).bg(bg);

    spans.push(Span::styled(" ".repeat(padding_len), bar_style));
    if file_missing {
        let warn_fg = theme::hex_to_color(&t.colors.heading2);
        spans.push(Span::styled(
            missing_label,
            Style::default()
                .fg(warn_fg)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        right_name,
        bar_style.add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        right_stats,
        Style::default().fg(dim_fg).bg(bg),
    ));

    frame.render_widget(Paragraph::new(vec![Line::from(spans)]), area);
}

/// Render document with search match highlighting (inline text only, not full lines).
///
/// Only the lines currently on screen are materialized: layout produces one
/// display line per `Line` (the document `Paragraph` never wraps), so the
/// slice `[scroll_offset .. scroll_offset + height]` is exactly the viewport.
/// This bounds per-frame work to the viewport instead of the whole document.
#[allow(clippy::too_many_arguments)]
pub fn render_document_with_search(
    frame: &mut Frame,
    area: Rect,
    lines: &[Line<'static>],
    scroll_offset: usize,
    _total_lines: usize,
    search: &SearchState,
    selection: Option<&Selection>,
    plain: &[String],
    t: &theme::Theme,
) {
    let offset = scroll_offset.min(lines.len());
    let end = (offset + area.height as usize).min(lines.len());
    let visible = &lines[offset..end];

    let searching = !search.query.is_empty() && !search.matches.is_empty();
    let rendered: Vec<Line<'static>> = if searching {
        let match_color = theme::hex_to_color(&t.colors.search_match);
        let current_color = theme::hex_to_color(&t.colors.search_current);
        visible
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let abs = offset + i;
                let is_current = search.is_current_match_line(abs);
                let is_match = search.is_match_line(abs);
                if !is_match && !is_current {
                    return line.clone();
                }
                let hi_color = if is_current {
                    current_color
                } else {
                    match_color
                };
                highlight_query_in_line(line, &search.query, hi_color, is_current)
            })
            .collect()
    } else {
        visible.to_vec()
    };

    // Selection paints last: it is the thing the user is actively pointing at,
    // so it wins over a search highlight underneath it.
    let rendered = match selection {
        Some(sel) => rendered
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                let abs = offset + i;
                let width = plain.get(abs).map(|p| crate::selection::line_width(p));
                match (width, sel.highlight_span(abs, width.unwrap_or(0))) {
                    (Some(_), Some((from, to))) => {
                        let (bg, fg) = t.colors.selection();
                        let style = Style::default()
                            .bg(theme::hex_to_color(&bg))
                            .fg(theme::hex_to_color(&fg));
                        restyle_columns(&line, from, to, style)
                    }
                    _ => line,
                }
            })
            .collect(),
        None => rendered,
    };

    // The slice already starts at the viewport top, so no Paragraph scroll.
    frame.render_widget(Paragraph::new(rendered), area);
}

/// Re-style the part of `line` covering display columns `[from, to)`.
///
/// Works in terminal cells rather than bytes, so a wide glyph is restyled whole
/// or not at all — a selection edge can never split one in half.
fn restyle_columns(line: &Line<'static>, from: usize, to: usize, style: Style) -> Line<'static> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let mut out: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 2);
    let mut col = 0usize;
    for span in &line.spans {
        let text = span.content.to_string();
        // Fast path: the span lies entirely inside or entirely outside.
        let span_w = text.width();
        if col >= to || col + span_w <= from {
            out.push(Span::styled(text, span.style));
            col += span_w;
            continue;
        }

        let mut pending = String::new();
        let mut pending_selected = false;
        for g in text.graphemes(true) {
            let w = g.width().max(1);
            let selected = col + w > from && col < to;
            if !pending.is_empty() && selected != pending_selected {
                let st = if pending_selected {
                    span.style.patch(style)
                } else {
                    span.style
                };
                out.push(Span::styled(std::mem::take(&mut pending), st));
            }
            pending_selected = selected;
            pending.push_str(g);
            col += w;
        }
        if !pending.is_empty() {
            let st = if pending_selected {
                span.style.patch(style)
            } else {
                span.style
            };
            out.push(Span::styled(pending, st));
        }
    }

    // A selection that runs past the end of the text (the newline a multi-line
    // selection swallows) shows as one trailing cell, so the user can see the
    // line break is included.
    if to > col {
        let pad = (to - col).min(1);
        out.push(Span::styled(" ".repeat(pad), style));
    }
    Line::from(out)
}

/// Lowercase `s`, and map every byte of the result back to the byte offset in
/// `s` it came from.
///
/// Case folding is not length-preserving — `İ` (U+0130) is two bytes but
/// lowercases to three — so an offset found in the lowered copy cannot be used
/// to slice the original. `offsets[i]` is the byte offset in `s` of the
/// character that produced byte `i` of the lowered string, plus one trailing
/// entry equal to `s.len()` so a match's end offset always resolves.
fn lowercase_with_offsets(s: &str) -> (String, Vec<usize>) {
    let mut lowered = String::with_capacity(s.len());
    let mut offsets = Vec::with_capacity(s.len() + 1);
    for (idx, ch) in s.char_indices() {
        for lc in ch.to_lowercase() {
            lowered.push(lc);
        }
        // Every byte this character expanded into maps back to its offset.
        offsets.resize(lowered.len(), idx);
    }
    offsets.push(s.len());
    (lowered, offsets)
}

/// Split spans in a line to highlight only the matched query text.
/// Uses underline + color (not background) so text stays readable.
fn highlight_query_in_line(
    line: &Line<'static>,
    query: &str,
    hi_color: Color,
    is_current: bool,
) -> Line<'static> {
    let query_lower = query.to_lowercase();
    // An empty query matches at every position — it would never advance.
    if query_lower.is_empty() {
        return line.clone();
    }
    let mut result_spans: Vec<Span<'static>> = Vec::new();

    for span in &line.spans {
        let text = span.content.to_string();
        let (text_lower, offsets) = lowercase_with_offsets(&text);
        let original_style = span.style;

        // `lpos` walks the lowercased copy (where matches are found); `opos`
        // is the corresponding position in `text` (where slices are taken).
        // Every slice of `text` goes through `offsets`, never through a
        // lowercase index, so a multi-byte case mapping can't split a char.
        let mut lpos = 0;
        let mut opos = 0;
        loop {
            match text_lower[lpos..].find(&query_lower) {
                Some(found) => {
                    let lstart = lpos + found;
                    let lend = lstart + query_lower.len();
                    let ostart = offsets[lstart];
                    let oend = offsets[lend];

                    // Text before match
                    if ostart > opos {
                        result_spans
                            .push(Span::styled(text[opos..ostart].to_string(), original_style));
                    }

                    // The matched text — color + underline + bold, keep original bg
                    let mut hi_style = original_style
                        .fg(hi_color)
                        .add_modifier(Modifier::UNDERLINED)
                        .add_modifier(Modifier::BOLD);
                    if is_current {
                        hi_style = hi_style.add_modifier(Modifier::REVERSED);
                    }
                    // A match landing inside one source character's expansion
                    // (searching "i" against "İ") covers no original bytes.
                    if oend > ostart {
                        result_spans.push(Span::styled(text[ostart..oend].to_string(), hi_style));
                    }

                    lpos = lend;
                    opos = oend;
                }
                None => {
                    // Remaining text after last match
                    if opos < text.len() {
                        result_spans.push(Span::styled(text[opos..].to_string(), original_style));
                    }
                    break;
                }
            }
        }
    }

    Line::from(result_spans)
}

/// Render the search input bar.
pub fn render_search_bar(frame: &mut Frame, area: Rect, search: &SearchState, t: &theme::Theme) {
    let bg = theme::hex_to_color(&t.colors.status_bar_bg);
    let fg = theme::hex_to_color(&t.colors.status_bar_fg);
    let accent = theme::hex_to_color(&t.colors.heading2);
    let dim_fg = theme::hex_to_color(&t.colors.link_url);

    let match_info = if search.query.is_empty() {
        String::new()
    } else if search.matches.is_empty() {
        " (no matches)".to_string()
    } else {
        format!(" [{}/{}]", search.current_match + 1, search.match_count())
    };

    let line = Line::from(vec![
        Span::styled(
            "  / ",
            Style::default()
                .fg(accent)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(search.query.clone(), Style::default().fg(fg).bg(bg)),
        Span::styled("█", Style::default().fg(accent).bg(bg)),
        Span::styled(match_info, Style::default().fg(dim_fg).bg(bg)),
        Span::styled(" ".repeat(area.width as usize), Style::default().bg(bg)),
    ]);

    frame.render_widget(Paragraph::new(vec![line]), area);
}

/// Render the table of contents sidebar.
pub fn render_toc(
    frame: &mut Frame,
    area: Rect,
    entries: &[crate::toc::TocEntry],
    selected: usize,
    t: &theme::Theme,
) {
    let active_color = theme::hex_to_color(&t.colors.toc_active);
    let inactive_color = theme::hex_to_color(&t.colors.toc_inactive);
    let border_color = theme::hex_to_color(&t.colors.table_border);
    let toc_bg = t.colors.bg.as_ref().map(|bg| theme::hex_to_color(bg));

    let lines: Vec<Line<'static>> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let indent = "  ".repeat((entry.level as usize).saturating_sub(1));
            let marker = if i == selected { "▸ " } else { "  " };
            let text = format!("{indent}{marker}{}", entry.text);
            let color = if i == selected {
                active_color
            } else {
                inactive_color
            };
            let mut style = Style::default().fg(color);
            if let Some(bg) = toc_bg {
                style = style.bg(bg);
            }
            if i == selected {
                style = style.add_modifier(Modifier::BOLD);
            }
            Line::from(Span::styled(text, style))
        })
        .collect();

    let mut block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border_color))
        .title(" Contents ")
        .title_style(
            Style::default()
                .fg(active_color)
                .add_modifier(Modifier::BOLD),
        );
    if let Some(bg) = toc_bg {
        block = block.style(Style::default().bg(bg));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render the help overlay: a centered popup of keybindings.
pub fn render_help(frame: &mut Frame, area: Rect, t: &theme::Theme) {
    let entries = crate::input::keymap_summary();
    let title_color = theme::hex_to_color(&t.colors.heading1);
    let key_color = theme::hex_to_color(&t.colors.heading2);
    let desc_color = theme::hex_to_color(&t.colors.status_bar_fg);
    let bg = theme::hex_to_color(&t.colors.code_block_bg);
    let border_color = theme::hex_to_color(&t.colors.table_border);

    let label_w = entries.iter().map(|(l, _)| l.len()).max().unwrap_or(10);
    let lines: Vec<Line<'static>> = entries
        .iter()
        .map(|(label, keys)| {
            Line::from(vec![
                Span::styled(
                    format!("  {label:label_w$}  "),
                    Style::default().fg(desc_color).bg(bg),
                ),
                Span::styled(keys.join(", "), Style::default().fg(key_color).bg(bg)),
            ])
        })
        .collect();

    let popup_width = 48u16.min(area.width.saturating_sub(2));
    let popup_height = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(" Keys — press any key to close ")
        .title_style(
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(bg));
    frame.render_widget(Paragraph::new(lines).block(block), popup_area);
}

/// Paint code-block hint labels directly onto the blocks' top borders.
///
/// Unlike the link overlay, these are not listed in a popup: a code block *is*
/// its own label site, and putting `[a]` on the border keeps the block itself
/// visible while you choose.
pub fn render_code_hints(
    frame: &mut Frame,
    area: Rect,
    hints: &[(char, u16, u16)],
    t: &theme::Theme,
) {
    let bg = theme::hex_to_color(&t.colors.search_current);
    let fg = t
        .colors
        .bg
        .as_ref()
        .map(|b| theme::hex_to_color(b))
        .unwrap_or_else(|| theme::hex_to_color(&t.colors.fg));
    let style = Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD);

    for (label, row, col) in hints {
        if *row >= area.height {
            continue;
        }
        let text = format!("[{label}]");
        let x = (area.x + col).min(area.x + area.width.saturating_sub(text.len() as u16));
        frame.buffer_mut().set_string(x, area.y + row, &text, style);
    }
}

/// Render a transient message (a copy result) in place of the bottom bar.
pub fn render_flash_bar(frame: &mut Frame, area: Rect, message: &str, t: &theme::Theme) {
    let bg = theme::hex_to_color(&t.colors.status_bar_bg);
    let accent = theme::hex_to_color(&t.colors.heading2);
    let line = Line::from(vec![
        Span::styled(
            " ✓ ",
            Style::default()
                .fg(accent)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(message.to_string(), Style::default().fg(accent).bg(bg)),
    ]);
    frame.render_widget(
        Paragraph::new(vec![line]).style(Style::default().bg(bg)),
        area,
    );
}

/// Render the link-hint overlay: a centered popup listing labeled links.
pub fn render_link_hints(
    frame: &mut Frame,
    area: Rect,
    hints: &[(char, String)],
    title: &str,
    t: &theme::Theme,
) {
    let title_color = theme::hex_to_color(&t.colors.heading1);
    let label_color = theme::hex_to_color(&t.colors.heading2);
    let url_color = theme::hex_to_color(&t.colors.link_url);
    let bg = theme::hex_to_color(&t.colors.code_block_bg);
    let border_color = theme::hex_to_color(&t.colors.table_border);

    let popup_width = 60u16.min(area.width.saturating_sub(2));
    let max_url = popup_width.saturating_sub(8) as usize;
    let lines: Vec<Line<'static>> = hints
        .iter()
        .map(|(label, url)| {
            let shown: String = if url.chars().count() > max_url {
                format!(
                    "{}…",
                    url.chars()
                        .take(max_url.saturating_sub(1))
                        .collect::<String>()
                )
            } else {
                url.clone()
            };
            Line::from(vec![
                Span::styled(
                    format!("  {label}  "),
                    Style::default()
                        .fg(label_color)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(shown, Style::default().fg(url_color).bg(bg)),
            ])
        })
        .collect();

    let popup_height = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(format!(" {title} "))
        .title_style(
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(bg));
    frame.render_widget(Paragraph::new(lines).block(block), popup_area);
}

/// Render the theme picker overlay.
pub fn render_theme_picker(
    frame: &mut Frame,
    area: Rect,
    themes: &[&str],
    selected: usize,
    current_theme: &str,
    t: &theme::Theme,
) {
    let active_color = theme::hex_to_color(&t.colors.heading1);
    let inactive_color = theme::hex_to_color(&t.colors.status_bar_fg);
    let bg = theme::hex_to_color(&t.colors.code_block_bg);
    let border_color = theme::hex_to_color(&t.colors.table_border);

    let popup_width = 26u16;
    let popup_height = (themes.len() as u16) + 4;
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let lines: Vec<Line<'static>> = themes
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let marker = if i == selected { " ▸ " } else { "   " };
            let check = if *name == current_theme { " ✓" } else { "" };
            let text = format!("{marker}{name}{check}");
            let color = if i == selected {
                active_color
            } else {
                inactive_color
            };
            let mut style = Style::default().fg(color).bg(bg);
            if i == selected {
                style = style.add_modifier(Modifier::BOLD);
            }
            Line::from(Span::styled(text, style))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(" Theme ")
        .title_style(
            Style::default()
                .fg(active_color)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(bg));

    frame.render_widget(Paragraph::new(lines).block(block), popup_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_of(line: &Line<'static>) -> Vec<String> {
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    fn highlight(text: &str, query: &str) -> Line<'static> {
        let line = Line::from(vec![Span::raw(text.to_string())]);
        highlight_query_in_line(&line, query, Color::Red, false)
    }

    #[test]
    fn offsets_map_lowercase_bytes_back_to_source() {
        // 'İ' is 2 bytes and lowercases to 3, so the map must not be identity.
        let (lowered, offsets) = lowercase_with_offsets("İa");
        assert_eq!(lowered, "i\u{307}a");
        // All 3 bytes of the expansion point at the source char (offset 0),
        // 'a' starts at source offset 2, and the trailing entry is the length.
        assert_eq!(offsets, vec![0, 0, 0, 2, 3]);
    }

    #[test]
    fn highlights_plain_ascii_match() {
        let out = highlight("Hello World", "world");
        assert_eq!(spans_of(&out), vec!["Hello ", "World"]);
    }

    #[test]
    fn highlights_every_occurrence() {
        let out = highlight("aXaXa", "x");
        assert_eq!(spans_of(&out), vec!["a", "X", "a", "X", "a"]);
    }

    /// Regression: case folding is not length-preserving, so byte offsets from
    /// the lowercased copy used to be applied to the original string — which
    /// panicked on a char boundary and killed the process inside raw mode.
    #[test]
    fn length_changing_case_folding_does_not_panic() {
        // Turkish dotted capital I: 2 bytes in, 3 bytes lowercased.
        assert!(!spans_of(&highlight("İstanbul", "i")).is_empty());
        // Capital sharp s: 3 bytes in, 2 bytes lowercased.
        assert!(!spans_of(&highlight("MASSE ẞ", "masse")).is_empty());
        // The whole document text is preserved even when a match maps to no
        // original bytes.
        assert_eq!(spans_of(&highlight("İstanbul", "i")).concat(), "İstanbul");
        assert_eq!(spans_of(&highlight("aİb", "i")).concat(), "aİb");
    }

    #[test]
    fn empty_query_returns_line_unchanged() {
        let out = highlight("anything", "");
        assert_eq!(spans_of(&out), vec!["anything"]);
    }
}

#[cfg(test)]
mod selection_paint {
    use super::*;

    fn text_of(line: &Line<'static>) -> Vec<(String, bool)> {
        line.spans
            .iter()
            .map(|s| (s.content.to_string(), s.style.bg.is_some()))
            .collect()
    }

    #[test]
    fn restyle_splits_a_span_at_the_selection_edges() {
        let line = Line::from(vec![Span::raw("hello world")]);
        let sel = Style::default().bg(Color::Blue);
        let out = restyle_columns(&line, 6, 11, sel);
        assert_eq!(
            text_of(&out),
            vec![("hello ".to_string(), false), ("world".to_string(), true)]
        );
    }

    #[test]
    fn restyle_spans_a_selection_across_several_spans() {
        let line = Line::from(vec![
            Span::raw("ab"),
            Span::styled("cd", Style::default().fg(Color::Red)),
            Span::raw("ef"),
        ]);
        let out = restyle_columns(&line, 1, 5, Style::default().bg(Color::Blue));
        let selected: String = out
            .spans
            .iter()
            .filter(|s| s.style.bg.is_some())
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(selected, "bcde");
    }

    #[test]
    fn restyle_never_splits_a_wide_glyph() {
        // 世 occupies columns 0-1; a selection starting at column 1 must take
        // the whole character rather than emit half of it.
        let line = Line::from(vec![Span::raw("世界")]);
        let out = restyle_columns(&line, 1, 4, Style::default().bg(Color::Blue));
        let selected: String = out
            .spans
            .iter()
            .filter(|s| s.style.bg.is_some())
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(selected, "世界");
    }

    #[test]
    fn restyle_marks_the_swallowed_newline_with_one_trailing_cell() {
        let line = Line::from(vec![Span::raw("short")]);
        let out = restyle_columns(&line, 0, 6, Style::default().bg(Color::Blue));
        let last = out.spans.last().unwrap();
        assert_eq!(last.content.as_ref(), " ");
        assert!(last.style.bg.is_some());
    }

    #[test]
    fn restyle_leaves_a_line_outside_the_range_untouched() {
        let line = Line::from(vec![Span::raw("hello")]);
        let out = restyle_columns(&line, 8, 12, Style::default().bg(Color::Blue));
        assert_eq!(text_of(&out)[0], ("hello".to_string(), false));
    }
}
