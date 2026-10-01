//! Mermaid diagrams drawn as terminal text.
//!
//! Flowcharts, state, class and ER diagrams are parsed into one graph model
//! ([`graph`]) and drawn by a layered layout engine ([`engine`]) on a
//! character grid ([`canvas`]). Sequence diagrams, pie and Gantt charts and
//! mindmaps have renderers of their own; other diagram types are shown as
//! their source in a labelled box.
//!
//! Every renderer must fit the width it is given, finish quickly on hostile
//! input, and produce the same output for the same input.

mod canvas;
mod class;
mod engine;
mod er;
mod flowchart;
mod graph;
mod state;
mod text;

#[cfg(test)]
mod tests;

use super::{SpanStyle, StyledLine, StyledSpan};
use crate::theme::Theme;
use canvas::Class;
use graph::{Dir, Graph, Marker};
use unicode_width::UnicodeWidthStr;

/// Parse and render a mermaid diagram as styled terminal text.
pub fn render_mermaid(source: &str, theme: &Theme, width: usize, margin: usize) -> Vec<StyledLine> {
    render_mermaid_with(source, theme, width, margin, crate::glyphs::current().ascii)
}

/// [`render_mermaid`] with the glyph set given explicitly (`ascii`: draw with
/// 7-bit stand-ins). For tests; the reader uses the process-wide setting.
#[doc(hidden)]
pub fn render_mermaid_with(
    source: &str,
    theme: &Theme,
    width: usize,
    margin: usize,
    ascii: bool,
) -> Vec<StyledLine> {
    let mut lines = render_diagram(source, theme, width, margin);
    // The diagram renderers draw with box and arrow characters; in ASCII
    // mode translate them once here rather than in every renderer. (Each
    // stand-in has the same width, except the note icon, which ends a line.)
    if ascii {
        for line in &mut lines {
            for span in &mut line.spans {
                if let std::borrow::Cow::Owned(t) = asciify(&span.text) {
                    span.text = t;
                }
            }
        }
    }
    lines
}

/// ASCII stand-ins for every drawing character the diagram renderers emit
/// (each the same width as what it replaces), on top of the shared set in
/// [`crate::glyphs::asciify`].
fn asciify(text: &str) -> std::borrow::Cow<'_, str> {
    fn stand_in(c: char) -> Option<char> {
        Some(match c {
            '─' | '╌' => '-',
            '┄' => '.',
            '━' | '═' => '=',
            '│' | '┃' | '║' => '|',
            '┆' => ':',
            '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╰' | '╯' | '╔'
            | '╗' | '╚' | '╝' | '╤' | '╧' | '╪' | '╟' | '╢' | '╫' => '+',
            '╱' => '/',
            '╲' => '\\',
            '▼' | '▽' => 'v',
            '▲' | '△' => '^',
            '▶' | '▷' => '>',
            '◀' | '◁' => '<',
            '◆' | '●' | '•' | '◦' => '*',
            '◇' | '○' => 'o',
            '◉' => '@',
            '×' => 'x',
            '█' | '▌' | '▎' => '#',
            '▓' => '=',
            '░' => '.',
            '…' => '~',
            '·' => '.',
            _ => return None,
        })
    }
    let mapped: std::borrow::Cow<'_, str> = if text.chars().any(|c| stand_in(c).is_some()) {
        std::borrow::Cow::Owned(text.chars().map(|c| stand_in(c).unwrap_or(c)).collect())
    } else {
        std::borrow::Cow::Borrowed(text)
    };
    match crate::glyphs::asciify(&mapped) {
        std::borrow::Cow::Owned(s) => std::borrow::Cow::Owned(s),
        std::borrow::Cow::Borrowed(_) => mapped,
    }
}

/// The parts of a diagram's source: an optional frontmatter title, the
/// keyword on its first line, the rest of that line, and the body lines.
struct Source {
    title: Option<String>,
    keyword: String,
    header: String,
    body: Vec<String>,
}

fn split_source(src: &str) -> Source {
    let mut lines = src.lines().peekable();
    let mut title = None;
    // Skip blank lines and directives before a frontmatter block.
    while lines
        .peek()
        .is_some_and(|l| l.trim().is_empty() || l.trim().starts_with("%%"))
    {
        lines.next();
    }
    if lines.peek().is_some_and(|l| l.trim() == "---") {
        lines.next();
        for l in lines.by_ref() {
            let t = l.trim();
            if t == "---" {
                break;
            }
            if let Some(v) = t.strip_prefix("title:") {
                title = Some(text::decode_label(v));
            }
        }
    }
    let mut keyword = String::new();
    let mut header = String::new();
    let mut body = Vec::new();
    for l in lines {
        let t = l.trim();
        if keyword.is_empty() {
            if t.is_empty() || t.starts_with("%%") {
                continue;
            }
            let (k, rest) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
            // `graph TD;` / `graph;`
            let (k, rest) = match k.split_once(';') {
                Some((k, r)) => (k, format!(";{r} {rest}")),
                None => (k, rest.to_string()),
            };
            keyword = k.to_string();
            header = rest.trim().to_string();
            continue;
        }
        body.push(l.to_string());
    }
    Source {
        title,
        keyword,
        header,
        body,
    }
}

fn render_diagram(source: &str, theme: &Theme, width: usize, margin: usize) -> Vec<StyledLine> {
    let cleaned = text::clean_source(source);
    let src = split_source(&cleaned);
    let diagram_color = &theme.colors.heading2;
    let text_color = &theme.colors.code_fg;
    let border_color = &theme.colors.table_border;
    let arrow_color = &theme.colors.heading3;

    let margin_str = " ".repeat(margin);

    let chart_width = width.min(56);
    // The legacy renderers below read the source from its first line.
    let legacy = || {
        let mut s = format!("{} {}\n", src.keyword, src.header);
        for l in &src.body {
            s.push_str(l);
            s.push('\n');
        }
        s
    };

    if let Some((g, title)) = parse_graph(&src) {
        return render_graph(&g, &title, theme, width, margin);
    }
    match src.keyword.as_str() {
        "sequenceDiagram" => render_sequence(
            &legacy(),
            diagram_color,
            text_color,
            border_color,
            arrow_color,
            &margin_str,
            chart_width,
        ),
        "pie" => render_pie(&legacy(), theme, &margin_str, chart_width),
        "gantt" => render_gantt(&legacy(), theme, &margin_str, chart_width),
        _ => render_unknown(
            &legacy(),
            border_color,
            text_color,
            diagram_color,
            &margin_str,
            chart_width,
        ),
    }
}

/// Parse a node-and-edge diagram into the graph model, with its title.
fn parse_graph(src: &Source) -> Option<(Graph, String)> {
    let (g, kind) = match src.keyword.as_str() {
        "graph" | "flowchart" | "flowchart-elk" => {
            (flowchart::parse(&src.header, &src.body), "flowchart")
        }
        "stateDiagram" | "stateDiagram-v2" => (state::parse(&src.body), "state diagram"),
        "classDiagram" | "classDiagram-v2" => (class::parse(&src.body), "class diagram"),
        "erDiagram" => (er::parse(&src.body), "ER diagram"),
        _ => return None,
    };
    Some((g, src.title.clone().unwrap_or_else(|| kind.to_string())))
}

/// Colour and attributes for a canvas cell class.
fn class_style(theme: &Theme, class: Class) -> SpanStyle {
    let c = &theme.colors;
    let (fg, bold, italic) = match class {
        Class::Frame => (&c.table_border, false, false),
        Class::Edge => (&c.heading3, false, false),
        Class::Node => (&c.heading2, false, false),
        Class::Text => (&c.code_fg, true, false),
        Class::Label => (&c.code_fg, false, true),
        Class::Title => (&c.heading2, true, false),
    };
    SpanStyle {
        fg: Some(fg.clone()),
        bold,
        italic,
        ..Default::default()
    }
}

/// Lay out a node-and-edge diagram, trying narrower label wraps and tighter
/// spacing until it fits; `LR`/`RL` diagrams that cannot fit are drawn top
/// down, and a diagram that still cannot fit (or is too big to lay out) is
/// listed edge by edge.
fn render_graph(
    g: &Graph,
    title: &str,
    theme: &Theme,
    width: usize,
    margin: usize,
) -> Vec<StyledLine> {
    if g.nodes.is_empty() {
        let mut lines = framed(title, &[], &g.notes, theme, width, margin);
        lines.push(StyledLine::empty());
        return lines;
    }
    if let Some(drawn) = lay_out(g, width) {
        let mut notes = drawn.notes;
        notes.extend(g.notes.iter().cloned());
        let mut lines = framed(title, &drawn.rows, &notes, theme, width, margin);
        lines.push(StyledLine::empty());
        return lines;
    }
    let mut lines = edge_list(g, title, theme, width, margin);
    lines.push(StyledLine::empty());
    lines
}

/// The layout attempts for a graph at `width` (the frame included): the
/// first that fits, or `None` when the diagram has to be listed instead.
fn lay_out(g: &Graph, width: usize) -> Option<engine::Drawn> {
    let avail = width.saturating_sub(4);
    let mut dirs = vec![g.dir];
    if g.dir.horizontal() {
        dirs.push(Dir::Down);
    }
    let attempts: [(usize, usize); 5] = [(28, 2), (20, 2), (14, 1), (10, 1), (6, 1)];
    for dir in dirs {
        for (wrap, sep) in attempts {
            let sep = if dir.horizontal() { 1 } else { sep };
            let p = engine::Params { wrap, sep, dir };
            match engine::layout(g, &p, avail) {
                Ok(drawn) => return Some(drawn),
                Err(engine::Fail::TooBig) => return None,
                Err(engine::Fail::TooWide(_)) => {}
            }
        }
    }
    None
}

/// The last-resort form: every node with its outgoing edges, one per line.
fn edge_list(
    g: &Graph,
    title: &str,
    theme: &Theme,
    width: usize,
    margin: usize,
) -> Vec<StyledLine> {
    let inner = width.saturating_sub(4).max(8);
    let mut out: Vec<Vec<(String, Class)>> = Vec::new();
    let one_line = |s: &str| s.replace('\n', " ");
    let push_wrapped =
        |out: &mut Vec<Vec<(String, Class)>>, indent: usize, parts: Vec<(String, Class)>| {
            // Wrap the concatenated text, keeping the first part's class for
            // the arrow and the last for the target.
            let full: String = parts.iter().map(|(t, _)| t.as_str()).collect();
            let lines = text::wrap(&full, inner.saturating_sub(indent).max(4));
            for (i, l) in lines.into_iter().enumerate() {
                let pad = " ".repeat(if i == 0 { indent } else { indent + 2 });
                let class = if i == 0 {
                    parts[0].1
                } else {
                    parts[parts.len() - 1].1
                };
                if i == 0 && parts.len() > 1 {
                    // Re-split the first line at the arrow boundary when it is intact.
                    let head = &parts[0].0;
                    if let Some(rest) = l.strip_prefix(head.as_str()) {
                        out.push(vec![
                            (pad, Class::Frame),
                            (head.clone(), parts[0].1),
                            (rest.to_string(), parts[1].1),
                        ]);
                        continue;
                    }
                }
                out.push(vec![(pad, Class::Frame), (l, class)]);
            }
        };
    let mut has_edges = vec![false; g.nodes.len()];
    for e in &g.edges {
        has_edges[e.from] = true;
        has_edges[e.to] = true;
    }
    for (i, n) in g.nodes.iter().enumerate() {
        let outgoing: Vec<&graph::Edge> = g.edges.iter().filter(|e| e.from == i).collect();
        if outgoing.is_empty() && has_edges[i] {
            continue;
        }
        push_wrapped(&mut out, 0, vec![(one_line(&n.label), Class::Text)]);
        for e in outgoing {
            let head = if e.end == Marker::None {
                "──"
            } else {
                "──▶"
            };
            let arrow = if e.label.is_empty() {
                format!("{head} ")
            } else {
                format!("──{}{} ", one_line(&e.label), head)
            };
            push_wrapped(
                &mut out,
                2,
                vec![
                    (arrow, Class::Edge),
                    (one_line(&g.nodes[e.to].label), Class::Text),
                ],
            );
        }
    }
    framed(title, &out, &g.notes, theme, width, margin)
}

/// Wrap drawn rows in the diagram frame: a titled top border, `│` sides and
/// a bottom border, as wide as the content needs (at most `width`). Notes
/// follow the rows inside the frame.
fn framed(
    title: &str,
    rows: &[Vec<(String, Class)>],
    notes: &[String],
    theme: &Theme,
    width: usize,
    margin: usize,
) -> Vec<StyledLine> {
    let border = &theme.colors.table_border;
    let margin_str = " ".repeat(margin);
    let row_w = |r: &Vec<(String, Class)>| r.iter().map(|(t, _)| text::width(t)).sum::<usize>();
    let content_w = rows.iter().map(row_w).max().unwrap_or(0);
    let note_lines: Vec<String> = notes
        .iter()
        .flat_map(|n| text::wrap(n, width.saturating_sub(4).max(4)))
        .collect();
    let notes_w = note_lines.iter().map(|l| text::width(l)).max().unwrap_or(0);
    let title_w = text::width(title);
    let frame_w = (content_w.max(notes_w) + 4)
        .max(title_w + 6)
        .max(20)
        .min(width);
    let inner = frame_w.saturating_sub(4);
    let title = text::truncate(title, frame_w.saturating_sub(6));
    let mut lines = vec![make_header(
        &title,
        border,
        &theme.colors.heading2,
        &margin_str,
        frame_w,
    )];
    let side = |s: &str| StyledSpan {
        text: s.to_string(),
        style: SpanStyle {
            fg: Some(border.clone()),
            ..Default::default()
        },
    };
    let mut emit = |runs: Vec<StyledSpan>, w: usize| {
        let mut line = StyledLine::new();
        push_margin(&mut line, &margin_str);
        line.push(side("│ "));
        for r in runs {
            line.push(r);
        }
        line.push(StyledSpan {
            text: " ".repeat(inner.saturating_sub(w)),
            style: SpanStyle::default(),
        });
        line.push(side(" │"));
        lines.push(line);
    };
    let offset = (inner.saturating_sub(content_w)) / 2;
    for r in rows {
        let w = row_w(r);
        let mut runs = Vec::new();
        if offset > 0 {
            runs.push(StyledSpan {
                text: " ".repeat(offset),
                style: SpanStyle::default(),
            });
        }
        // Trailing blanks are padding; keep rows within the frame.
        for (t, class) in r {
            runs.push(StyledSpan {
                text: t.clone(),
                style: class_style(theme, *class),
            });
        }
        emit(runs, w + offset);
    }
    if !note_lines.is_empty() && !rows.is_empty() {
        emit(Vec::new(), 0);
    }
    for l in note_lines {
        let l = text::truncate(&l, inner);
        let w = text::width(&l);
        emit(
            vec![StyledSpan {
                text: l,
                style: class_style(theme, Class::Label),
            }],
            w,
        );
    }
    lines.push(make_footer(border, &margin_str, frame_w));
    lines
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
mod header_tests {
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
