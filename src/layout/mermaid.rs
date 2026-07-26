use super::{SpanStyle, StyledLine, StyledSpan};
use crate::theme::Theme;
use unicode_width::UnicodeWidthStr;

/// Parse and render a mermaid diagram as styled terminal text.
/// This is a built-in ASCII renderer for common diagram types.
pub fn render_mermaid(source: &str, theme: &Theme, width: usize, margin: usize) -> Vec<StyledLine> {
    let trimmed = source.trim();
    let diagram_color = &theme.colors.heading2;
    let text_color = &theme.colors.code_fg;
    let border_color = &theme.colors.table_border;
    let arrow_color = &theme.colors.heading3;

    let margin_str = " ".repeat(margin);

    let chart_width = width.min(56);

    // Detect diagram type
    if trimmed.starts_with("graph") || trimmed.starts_with("flowchart") {
        render_flowchart(
            trimmed,
            diagram_color,
            text_color,
            border_color,
            arrow_color,
            &margin_str,
            chart_width,
        )
    } else if trimmed.starts_with("sequenceDiagram") {
        render_sequence(
            trimmed,
            diagram_color,
            text_color,
            border_color,
            arrow_color,
            &margin_str,
            chart_width,
        )
    } else if trimmed.starts_with("pie") {
        render_pie(trimmed, theme, &margin_str, chart_width)
    } else if trimmed.starts_with("gantt") {
        render_gantt(trimmed, theme, &margin_str, chart_width)
    } else {
        render_unknown(
            trimmed,
            border_color,
            text_color,
            diagram_color,
            &margin_str,
            chart_width,
        )
    }
}

fn render_flowchart(
    source: &str,
    diagram_color: &str,
    text_color: &str,
    border_color: &str,
    arrow_color: &str,
    margin: &str,
    width: usize,
) -> Vec<StyledLine> {
    let mut lines = Vec::new();
    let mut nodes: Vec<(String, String)> = Vec::new(); // (id, label)
    let mut edges: Vec<(String, String, String)> = Vec::new(); // (from, to, label)

    for raw_line in source.lines().skip(1) {
        let l = raw_line.trim();
        if l.is_empty() {
            continue;
        }

        // Parse edges: A --> B, A -->|text| B, A --- B
        if let Some((from_part, rest)) = split_arrow(l) {
            let from_id = extract_node_id(&from_part);
            let from_label = extract_node_label(&from_part);
            let (edge_label, to_part) = extract_edge_label(rest);
            let to_id = extract_node_id(&to_part);
            let to_label = extract_node_label(&to_part);

            // Register nodes
            if !from_id.is_empty() && !nodes.iter().any(|(id, _)| id == &from_id) {
                nodes.push((from_id.clone(), from_label));
            }
            if !to_id.is_empty() && !nodes.iter().any(|(id, _)| id == &to_id) {
                nodes.push((to_id.clone(), to_label));
            }
            edges.push((from_id, to_id, edge_label));
        } else if !l.starts_with("graph") && !l.starts_with("flowchart") {
            // Standalone node definition
            let id = extract_node_id(l);
            let label = extract_node_label(l);
            if !id.is_empty() && !nodes.iter().any(|(nid, _)| nid == &id) {
                nodes.push((id, label));
            }
        }
    }

    lines.push(make_header(
        "flowchart",
        border_color,
        diagram_color,
        margin,
        width,
    ));

    // Render nodes vertically with arrows between them
    for (i, edge) in edges.iter().enumerate() {
        let from_label = nodes
            .iter()
            .find(|(id, _)| id == &edge.0)
            .map(|(_, l)| l.as_str())
            .unwrap_or(&edge.0);
        let to_label = nodes
            .iter()
            .find(|(id, _)| id == &edge.1)
            .map(|(_, l)| l.as_str())
            .unwrap_or(&edge.1);

        // From node box (only render if first edge or different from previous)
        if i == 0 || (i > 0 && edges[i - 1].1 != edge.0) {
            lines.push(make_node_line(from_label, border_color, text_color, margin));
        }

        // Arrow
        let mut arrow_line = StyledLine::new();
        push_margin(&mut arrow_line, margin);
        arrow_line.push(StyledSpan {
            text: "│     ".to_string(),
            style: SpanStyle {
                fg: Some(border_color.to_string()),
                ..Default::default()
            },
        });

        let arrow_text = if edge.2.is_empty() {
            "  │".to_string()
        } else {
            format!("  │ {}", edge.2)
        };
        arrow_line.push(StyledSpan {
            text: arrow_text,
            style: SpanStyle {
                fg: Some(arrow_color.to_string()),
                ..Default::default()
            },
        });
        lines.push(arrow_line);

        let mut arrow_head = StyledLine::new();
        push_margin(&mut arrow_head, margin);
        arrow_head.push(StyledSpan {
            text: "│     ".to_string(),
            style: SpanStyle {
                fg: Some(border_color.to_string()),
                ..Default::default()
            },
        });
        arrow_head.push(StyledSpan {
            text: "  ▼".to_string(),
            style: SpanStyle {
                fg: Some(arrow_color.to_string()),
                ..Default::default()
            },
        });
        lines.push(arrow_head);

        // To node box
        lines.push(make_node_line(to_label, border_color, text_color, margin));
    }

    if edges.is_empty() {
        for (_, label) in &nodes {
            lines.push(make_node_line(label, border_color, text_color, margin));
        }
    }

    lines.push(make_footer(border_color, margin, width));
    lines.push(StyledLine::empty());

    lines
}

fn make_node_line(label: &str, border_color: &str, text_color: &str, margin: &str) -> StyledLine {
    // Single-line box representation: │     [ Label ]
    let mut result = StyledLine::new();
    push_margin(&mut result, margin);
    result.push(StyledSpan {
        text: "│     ".to_string(),
        style: SpanStyle {
            fg: Some(border_color.to_string()),
            ..Default::default()
        },
    });
    result.push(StyledSpan {
        text: format!("[ {} ]", label),
        style: SpanStyle {
            fg: Some(text_color.to_string()),
            bold: true,
            ..Default::default()
        },
    });
    result
}

fn render_sequence(
    source: &str,
    diagram_color: &str,
    text_color: &str,
    border_color: &str,
    arrow_color: &str,
    margin: &str,
    width: usize,
) -> Vec<StyledLine> {
    let mut lines = Vec::new();
    let mut participants: Vec<String> = Vec::new();

    lines.push(make_header(
        "sequence diagram",
        border_color,
        diagram_color,
        margin,
        width,
    ));

    for raw_line in source.lines().skip(1) {
        let l = raw_line.trim();
        if l.is_empty() {
            continue;
        }

        if l.starts_with("participant ") {
            let name = l.strip_prefix("participant ").unwrap_or("").trim();
            if !participants.contains(&name.to_string()) {
                participants.push(name.to_string());
            }
        } else if l.contains("->>") || l.contains("-->>") || l.contains("->") || l.contains("-->") {
            // Parse: Alice->>Bob: Hello
            let (arrow, from, to, msg) = parse_sequence_line(l);
            let arrow_sym = if arrow.contains(">>") {
                "──▶"
            } else {
                "───"
            };

            let mut line = StyledLine::new();
            push_margin(&mut line, margin);
            line.push(StyledSpan {
                text: "│  ".to_string(),
                style: SpanStyle {
                    fg: Some(border_color.to_string()),
                    ..Default::default()
                },
            });
            line.push(StyledSpan {
                text: format!("{from} "),
                style: SpanStyle {
                    fg: Some(text_color.to_string()),
                    bold: true,
                    ..Default::default()
                },
            });
            line.push(StyledSpan {
                text: format!("{arrow_sym} "),
                style: SpanStyle {
                    fg: Some(arrow_color.to_string()),
                    ..Default::default()
                },
            });
            line.push(StyledSpan {
                text: to.to_string(),
                style: SpanStyle {
                    fg: Some(text_color.to_string()),
                    bold: true,
                    ..Default::default()
                },
            });
            if !msg.is_empty() {
                line.push(StyledSpan {
                    text: format!(": {msg}"),
                    style: SpanStyle {
                        fg: Some(text_color.to_string()),
                        italic: true,
                        ..Default::default()
                    },
                });
            }
            lines.push(line);
        } else if l.starts_with("Note") {
            let mut line = StyledLine::new();
            push_margin(&mut line, margin);
            line.push(StyledSpan {
                text: "│  ".to_string(),
                style: SpanStyle {
                    fg: Some(border_color.to_string()),
                    ..Default::default()
                },
            });
            line.push(StyledSpan {
                text: format!("  📝 {l}"),
                style: SpanStyle {
                    fg: Some(text_color.to_string()),
                    italic: true,
                    ..Default::default()
                },
            });
            lines.push(line);
        }
    }

    lines.push(make_footer(border_color, margin, width));
    lines.push(StyledLine::empty());

    lines
}

fn render_pie(source: &str, theme: &Theme, margin: &str, width: usize) -> Vec<StyledLine> {
    let mut lines = Vec::new();
    let border_color = &theme.colors.table_border;
    let colors = [
        &theme.colors.heading1,
        &theme.colors.heading2,
        &theme.colors.heading3,
        &theme.colors.heading4,
        &theme.colors.heading5,
        &theme.colors.heading6,
        &theme.colors.admonition_note,
        &theme.colors.admonition_tip,
    ];

    let mut title = "Pie Chart".to_string();
    let mut slices: Vec<(String, f64)> = Vec::new();

    for raw_line in source.lines().skip(1) {
        let l = raw_line.trim();
        if l.starts_with("title ") {
            title = l.strip_prefix("title ").unwrap_or("").to_string();
        } else if l.contains(':') {
            let parts: Vec<&str> = l.splitn(2, ':').collect();
            if parts.len() == 2 {
                let label = parts[0].trim().trim_matches('"').to_string();
                if let Ok(val) = parts[1].trim().parse::<f64>() {
                    slices.push((label, val));
                }
            }
        }
    }

    let total: f64 = slices.iter().map(|(_, v)| v).sum();

    lines.push(make_header(
        &title,
        border_color,
        &theme.colors.heading2,
        margin,
        width,
    ));

    // Render as horizontal bar chart
    let max_bar = 30usize;
    for (i, (label, value)) in slices.iter().enumerate() {
        let pct = if total > 0.0 {
            value / total * 100.0
        } else {
            0.0
        };
        let bar_len = ((pct / 100.0) * max_bar as f64) as usize;
        let color = colors[i % colors.len()];

        let mut line = StyledLine::new();
        push_margin(&mut line, margin);
        line.push(StyledSpan {
            text: "│  ".to_string(),
            style: SpanStyle {
                fg: Some(border_color.to_string()),
                ..Default::default()
            },
        });
        line.push(StyledSpan {
            text: format!("{:>12} ", label),
            style: SpanStyle {
                fg: Some(color.clone()),
                bold: true,
                ..Default::default()
            },
        });
        line.push(StyledSpan {
            text: "█".repeat(bar_len),
            style: SpanStyle {
                fg: Some(color.clone()),
                ..Default::default()
            },
        });
        line.push(StyledSpan {
            text: format!(" {:.1}%", pct),
            style: SpanStyle {
                fg: Some(color.clone()),
                ..Default::default()
            },
        });
        lines.push(line);
    }

    lines.push(make_footer(border_color, margin, width));
    lines.push(StyledLine::empty());

    lines
}

fn render_gantt(source: &str, theme: &Theme, margin: &str, width: usize) -> Vec<StyledLine> {
    let mut lines = Vec::new();
    let border_color = &theme.colors.table_border;
    let text_color = &theme.colors.code_fg;
    let colors = [
        &theme.colors.heading1,
        &theme.colors.heading2,
        &theme.colors.heading3,
    ];

    let mut title = "Gantt Chart".to_string();
    let mut tasks: Vec<String> = Vec::new();
    let mut current_section;

    for raw_line in source.lines().skip(1) {
        let l = raw_line.trim();
        if l.starts_with("title ") {
            title = l.strip_prefix("title ").unwrap_or("").to_string();
        } else if l.starts_with("section ") {
            current_section = l.strip_prefix("section ").unwrap_or("").to_string();
            tasks.push(format!("§{current_section}"));
        } else if l.contains(':') && !l.starts_with("dateFormat") && !l.starts_with("axisFormat") {
            let parts: Vec<&str> = l.splitn(2, ':').collect();
            tasks.push(parts[0].trim().to_string());
        }
    }

    lines.push(make_header(
        &title,
        border_color,
        &theme.colors.heading2,
        margin,
        width,
    ));

    let mut color_idx = 0;
    for task in &tasks {
        let mut line = StyledLine::new();
        push_margin(&mut line, margin);
        line.push(StyledSpan {
            text: "│  ".to_string(),
            style: SpanStyle {
                fg: Some(border_color.to_string()),
                ..Default::default()
            },
        });

        if let Some(section) = task.strip_prefix('§') {
            line.push(StyledSpan {
                text: format!("  ── {section} ──"),
                style: SpanStyle {
                    fg: Some(text_color.to_string()),
                    bold: true,
                    ..Default::default()
                },
            });
        } else {
            let color = colors[color_idx % colors.len()];
            let bar_len = 8 + (task.len() % 8); // Vary bar width
            line.push(StyledSpan {
                text: format!("  {:>16} ", task),
                style: SpanStyle {
                    fg: Some(text_color.to_string()),
                    ..Default::default()
                },
            });
            line.push(StyledSpan {
                text: "█".repeat(bar_len),
                style: SpanStyle {
                    fg: Some(color.clone()),
                    ..Default::default()
                },
            });
            color_idx += 1;
        }
        lines.push(line);
    }

    lines.push(make_footer(border_color, margin, width));
    lines.push(StyledLine::empty());

    lines
}

fn render_unknown(
    source: &str,
    border_color: &str,
    text_color: &str,
    title_color: &str,
    margin: &str,
    width: usize,
) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    lines.push(make_header(
        "mermaid diagram",
        border_color,
        title_color,
        margin,
        width,
    ));

    for raw_line in source.lines() {
        let mut line = StyledLine::new();
        push_margin(&mut line, margin);
        line.push(StyledSpan {
            text: "│ ".to_string(),
            style: SpanStyle {
                fg: Some(border_color.to_string()),
                ..Default::default()
            },
        });
        line.push(StyledSpan {
            text: raw_line.to_string(),
            style: SpanStyle {
                fg: Some(text_color.to_string()),
                ..Default::default()
            },
        });
        lines.push(line);
    }

    let mut footer = StyledLine::new();
    push_margin(&mut footer, margin);
    footer.push(StyledSpan {
        text: format!("╰{}╯", "─".repeat(55)),
        style: SpanStyle {
            fg: Some(border_color.to_string()),
            ..Default::default()
        },
    });
    lines.push(footer);
    lines.push(StyledLine::empty());

    lines
}

// --- Helpers ---

/// Create a diagram header line: ╭─ title ─────────────╮
fn make_header(
    title: &str,
    border_color: &str,
    title_color: &str,
    margin: &str,
    width: usize,
) -> StyledLine {
    let mut line = StyledLine::new();
    push_margin(&mut line, margin);
    line.push(StyledSpan {
        text: "╭─ ".to_string(),
        style: SpanStyle {
            fg: Some(border_color.to_string()),
            ..Default::default()
        },
    });
    line.push(StyledSpan {
        text: format!("{title} "),
        style: SpanStyle {
            fg: Some(title_color.to_string()),
            bold: true,
            ..Default::default()
        },
    });
    // Display columns, not bytes: a CJK or emoji title occupies more columns
    // than `len()` reports characters (and multibyte text fewer), which used
    // to skew the top border.
    let used = 3 + title.width() + 2; // "╭─ " + title + " ╮"
    let remaining = width.saturating_sub(used);
    line.push(StyledSpan {
        text: format!("{}╮", "─".repeat(remaining)),
        style: SpanStyle {
            fg: Some(border_color.to_string()),
            ..Default::default()
        },
    });
    line
}

/// Create a diagram footer line: ╰─────────────────────╯
fn make_footer(border_color: &str, margin: &str, width: usize) -> StyledLine {
    let mut line = StyledLine::new();
    push_margin(&mut line, margin);
    line.push(StyledSpan {
        text: format!("╰{}╯", "─".repeat(width.saturating_sub(2))),
        style: SpanStyle {
            fg: Some(border_color.to_string()),
            ..Default::default()
        },
    });
    line
}

fn push_margin(line: &mut StyledLine, margin: &str) {
    if !margin.is_empty() {
        line.push(StyledSpan {
            text: margin.to_string(),
            style: SpanStyle::default(),
        });
    }
}

fn split_arrow(line: &str) -> Option<(String, &str)> {
    let arrows = ["-->", "---", "-.-", "==>", "-.->", "->>"];
    for arrow in &arrows {
        if let Some(pos) = line.find(arrow) {
            let from = line[..pos].trim().to_string();
            let rest = &line[pos + arrow.len()..];
            return Some((from, rest.trim()));
        }
    }
    None
}

fn extract_node_id(s: &str) -> String {
    let s = s.trim();
    // Handle A[Label], A(Label), A{Label}, A((Label))
    if let Some(pos) = s.find(['[', '(', '{']) {
        s[..pos].trim().to_string()
    } else {
        s.to_string()
    }
}

fn extract_node_label(s: &str) -> String {
    let s = s.trim();
    // Extract label from brackets: A[Label] -> Label
    for (open, close) in &[('[', ']'), ('(', ')'), ('{', '}')] {
        if let Some(start) = s.find(*open) {
            if let Some(end) = s.rfind(*close) {
                if end > start {
                    let label = &s[start + 1..end];
                    // Handle double brackets ((Label))
                    let label = label.trim_start_matches('(').trim_end_matches(')');
                    return label.to_string();
                }
            }
        }
    }
    extract_node_id(s)
}

fn extract_edge_label(rest: &str) -> (String, String) {
    let rest = rest.trim();
    // Handle |label| syntax
    if let Some(rest_after_pipe) = rest.strip_prefix('|') {
        if let Some(end) = rest_after_pipe.find('|') {
            let label = rest_after_pipe[..end].to_string();
            let to = rest_after_pipe[end + 1..].trim().to_string();
            return (label, to);
        }
    }
    (String::new(), rest.to_string())
}

fn parse_sequence_line(line: &str) -> (String, String, String, String) {
    let arrows = ["-->>", "->>", "-->", "->"];
    for arrow in &arrows {
        if let Some(pos) = line.find(arrow) {
            let from = line[..pos].trim().to_string();
            let after = &line[pos + arrow.len()..];
            let (to, msg) = if let Some(colon_pos) = after.find(':') {
                (
                    after[..colon_pos].trim().to_string(),
                    after[colon_pos + 1..].trim().to_string(),
                )
            } else {
                (after.trim().to_string(), String::new())
            };
            return (arrow.to_string(), from, to, msg);
        }
    }
    (
        String::new(),
        line.to_string(),
        String::new(),
        String::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_width(line: &StyledLine) -> usize {
        line.spans.iter().map(|s| s.text.as_str().width()).sum()
    }

    // The header border must be sized by the title's display columns, not its
    // byte length: a CJK title (3 bytes but 2 columns per char) skewed the top
    // border 10 columns off the footer.
    #[test]
    fn header_width_matches_footer_regardless_of_title_script() {
        for title in ["flowchart", "日本語のパイ図", "⚠️ warning ⚠️", "Grüße"] {
            let header = make_header(title, "#888888", "#ffffff", "", 56);
            let footer = make_footer("#888888", "", 56);
            assert_eq!(
                line_width(&header),
                56,
                "header for {title:?} is {} cols",
                line_width(&header)
            );
            assert_eq!(line_width(&header), line_width(&footer));
        }
    }
}
