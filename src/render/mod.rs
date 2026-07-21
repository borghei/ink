pub mod plain;

use crate::layout::StyledLine;
use crate::search::SearchState;
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
pub fn render_document_with_search(
    frame: &mut Frame,
    area: Rect,
    lines: &[Line<'static>],
    scroll_offset: u16,
    _total_lines: usize,
    search: &SearchState,
    t: &theme::Theme,
) {
    let offset = (scroll_offset as usize).min(lines.len());
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

    // The slice already starts at the viewport top, so no Paragraph scroll.
    frame.render_widget(Paragraph::new(rendered), area);
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
    let mut result_spans: Vec<Span<'static>> = Vec::new();

    for span in &line.spans {
        let text = span.content.to_string();
        let text_lower = text.to_lowercase();
        let original_style = span.style;

        let mut pos = 0;
        loop {
            match text_lower[pos..].find(&query_lower) {
                Some(found) => {
                    let abs_start = pos + found;
                    let abs_end = abs_start + query.len();

                    // Text before match
                    if abs_start > pos {
                        result_spans.push(Span::styled(
                            text[pos..abs_start].to_string(),
                            original_style,
                        ));
                    }

                    // The matched text — color + underline + bold, keep original bg
                    let mut hi_style = original_style
                        .fg(hi_color)
                        .add_modifier(Modifier::UNDERLINED)
                        .add_modifier(Modifier::BOLD);
                    if is_current {
                        hi_style = hi_style.add_modifier(Modifier::REVERSED);
                    }
                    result_spans.push(Span::styled(text[abs_start..abs_end].to_string(), hi_style));

                    pos = abs_end;
                }
                None => {
                    // Remaining text after last match
                    if pos < text.len() {
                        result_spans.push(Span::styled(text[pos..].to_string(), original_style));
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

/// Render the link-hint overlay: a centered popup listing labeled links.
pub fn render_link_hints(
    frame: &mut Frame,
    area: Rect,
    hints: &[(char, String)],
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
        .title(" Follow link — press a letter, Esc to cancel ")
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
