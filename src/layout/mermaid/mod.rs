//! Mermaid diagrams drawn as terminal text.
//!
//! Flowcharts, state, class and ER diagrams are parsed into one graph model
//! ([`graph`]) and drawn by a layered layout engine ([`engine`]) on a
//! character grid ([`canvas`]). Sequence diagrams are drawn on the same grid
//! by a layout of their own ([`sequence`]): lifelines, arrows, notes and
//! block frames. Pie and Gantt charts and mindmaps have renderers of their
//! own; other diagram types are shown as their source in a labelled box.
//!
//! Every renderer must fit the width it is given, finish quickly on hostile
//! input, and produce the same output for the same input.

mod canvas;
mod class;
mod engine;
mod er;
mod flowchart;
mod gantt;
mod graph;
mod mindmap;
mod sequence;
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
    // The pie chart and the sequence list (the form a sequence diagram takes
    // when it cannot be drawn in the width) lay out for 56 columns, and the
    // ASCII note marker is wider than the icon: wrap what does not fit.
    let bar = if ascii { "|" } else { "│" };
    fit_lines(
        lines,
        margin + width,
        &format!("{}{bar}    ", " ".repeat(margin)),
        &theme.colors.table_border,
    )
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

/// Diagram types mermaid knows that are not drawn here: shown as source.
const UNSUPPORTED: &[&str] = &[
    "journey",
    "gitGraph",
    "timeline",
    "quadrantChart",
    "sankey-beta",
    "sankey",
    "requirementDiagram",
    "xychart-beta",
    "xychart",
    "block-beta",
    "block",
    "packet-beta",
    "packet",
    "kanban",
    "architecture-beta",
    "architecture",
    "C4Context",
    "C4Container",
    "C4Component",
    "C4Dynamic",
    "C4Deployment",
    "zenuml",
    "radar-beta",
    "treemap-beta",
];

fn render_diagram(source: &str, theme: &Theme, width: usize, margin: usize) -> Vec<StyledLine> {
    let cleaned = text::clean_source(source);
    let src = split_source(&cleaned);
    let diagram_color = &theme.colors.heading2;
    let text_color = &theme.colors.code_fg;
    let border_color = &theme.colors.table_border;
    let arrow_color = &theme.colors.heading3;

    let margin_str = " ".repeat(margin);

    let chart_width = width.min(56);
    // The pie renderer reads the source from its first line.
    let legacy = || {
        // `pie title X` carries the title on the first line.
        let mut s = match src.header.strip_prefix("title ") {
            Some(t) if src.keyword == "pie" => format!("pie\ntitle {t}\n"),
            _ => format!("{} {}\n", src.keyword, src.header),
        };
        for l in &src.body {
            s.push_str(l);
            s.push('\n');
        }
        s
    };

    if let Some((g, title)) = parse_graph(&src) {
        return render_graph(&g, &title, theme, width, margin);
    }
    let mut lines = match src.keyword.as_str() {
        "sequenceDiagram" => {
            let d = sequence::parse(src.title.clone(), &src.body);
            let title = d
                .title
                .clone()
                .unwrap_or_else(|| "sequence diagram".to_string());
            match sequence::layout(&d, width.saturating_sub(4)) {
                Some(rows) => framed(&title, &rows, &[], theme, width, margin),
                None => render_sequence(
                    &d,
                    &title,
                    diagram_color,
                    text_color,
                    border_color,
                    arrow_color,
                    &margin_str,
                    chart_width,
                ),
            }
        }
        "pie" => render_pie(&legacy(), theme, &margin_str, chart_width),
        "gantt" => {
            let (title, rows) =
                gantt::render(src.title.clone(), &src.body, width.saturating_sub(4));
            framed(&title, &rows, &[], theme, width, margin)
        }
        "mindmap" => {
            let rows = mindmap::render(&src.body, width.saturating_sub(4));
            let title = src.title.clone().unwrap_or_else(|| "mindmap".to_string());
            framed_left(&title, &rows, theme, width, margin)
        }
        _ => unsupported(&src, theme, width, margin),
    };
    if lines.last().is_some_and(|l| !l.spans.is_empty()) {
        lines.push(StyledLine::empty());
    }
    lines
}

/// A diagram type that is not drawn: its source in a titled box.
fn unsupported(src: &Source, theme: &Theme, width: usize, margin: usize) -> Vec<StyledLine> {
    let inner = width.saturating_sub(4).max(1);
    let kind = if src.keyword.is_empty() {
        "diagram".to_string()
    } else if UNSUPPORTED.contains(&src.keyword.as_str()) {
        src.keyword.clone()
    } else {
        text::truncate(&src.keyword, 24)
    };
    let title = format!("mermaid: {kind} (not rendered)");
    let mut rows: Vec<Vec<(String, Class)>> = Vec::new();
    let header = format!("{} {}", src.keyword, src.header);
    for line in std::iter::once(header.trim_end()).chain(src.body.iter().map(|l| l.trim_end())) {
        // Hard-wrap, keeping indentation (this is code, not prose).
        let mut piece = String::new();
        let mut w = 0;
        for g in unicode_segmentation::UnicodeSegmentation::graphemes(line, true) {
            let gw = UnicodeWidthStr::width(g);
            if w + gw > inner && w > 0 {
                rows.push(vec![(std::mem::take(&mut piece), Class::Plain)]);
                w = 0;
            }
            piece.push_str(g);
            w += gw;
        }
        rows.push(vec![(piece, Class::Plain)]);
    }
    while rows
        .last()
        .is_some_and(|r| r.iter().all(|(t, _)| t.trim().is_empty()))
    {
        rows.pop();
    }
    framed_left(&title, &rows, theme, width, margin)
}

/// Wrap any line wider than `max` columns: the overflow continues on the
/// next line behind the left border.
fn fit_lines(lines: Vec<StyledLine>, max: usize, lead: &str, border: &str) -> Vec<StyledLine> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        if line.width() <= max {
            out.push(line);
            continue;
        }
        let lead_w = UnicodeWidthStr::width(lead);
        let mut cur = StyledLine::new();
        let mut w = 0;
        for span in line.spans {
            let mut piece = String::new();
            for g in span.text.graphemes(true) {
                let gw = UnicodeWidthStr::width(g);
                if w + gw > max {
                    if !piece.is_empty() {
                        cur.push(StyledSpan {
                            text: std::mem::take(&mut piece),
                            style: span.style.clone(),
                        });
                    }
                    out.push(std::mem::take(&mut cur));
                    cur.push(StyledSpan {
                        text: lead.to_string(),
                        style: SpanStyle {
                            fg: Some(border.to_string()),
                            ..Default::default()
                        },
                    });
                    w = lead_w;
                    if w + gw > max {
                        // Not even one more grapheme fits behind the border.
                        w = 0;
                        cur = StyledLine::new();
                    }
                }
                piece.push_str(g);
                w += gw;
            }
            if !piece.is_empty() {
                cur.push(StyledSpan {
                    text: piece,
                    style: span.style,
                });
            }
        }
        out.push(cur);
    }
    out
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
        Class::Plain => (&c.code_fg, false, false),
        Class::Alert => (&c.admonition_warning, true, false),
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
    let mut outgoing_of: Vec<Vec<&graph::Edge>> = vec![Vec::new(); g.nodes.len()];
    for e in &g.edges {
        has_edges[e.from] = true;
        has_edges[e.to] = true;
        outgoing_of[e.from].push(e);
    }
    for (i, n) in g.nodes.iter().enumerate() {
        let outgoing = std::mem::take(&mut outgoing_of[i]);
        if outgoing.is_empty() && has_edges[i] {
            continue;
        }
        push_wrapped(&mut out, 0, vec![(one_line(&n.label), Class::Text)]);
        for e in outgoing {
            // The edge drawn left to right: its markers, line style, label.
            let glyph = |m: Marker, at_end: bool| match m {
                Marker::None => "",
                Marker::Arrow => {
                    if at_end {
                        "▶"
                    } else {
                        "◀"
                    }
                }
                Marker::Triangle => {
                    if at_end {
                        "▷"
                    } else {
                        "◁"
                    }
                }
                Marker::DiamondFilled => "◆",
                Marker::DiamondOpen => "◇",
                Marker::Circle => "○",
                Marker::Cross => "×",
            };
            let line = match e.style {
                graph::LineStyle::Dotted => "┄┄",
                graph::LineStyle::Thick => "━━",
                _ => "──",
            };
            let mut arrow = format!("{}{line}", glyph(e.start, false));
            if !e.label.is_empty() {
                arrow.push_str(&one_line(&e.label));
                arrow.push_str(line);
            }
            if let Some((a, b)) = &e.cards {
                arrow = format!("{a} {arrow} {b}").trim().to_string();
            }
            arrow.push_str(glyph(e.end, true));
            arrow.push(' ');
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
    framed_impl(title, &out, &g.notes, theme, width, margin, false)
}

/// [`framed`] with the rows left-aligned (text, not a drawing).
fn framed_left(
    title: &str,
    rows: &[Vec<(String, Class)>],
    theme: &Theme,
    width: usize,
    margin: usize,
) -> Vec<StyledLine> {
    framed_impl(title, rows, &[], theme, width, margin, false)
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
    framed_impl(title, rows, notes, theme, width, margin, true)
}

fn framed_impl(
    title: &str,
    rows: &[Vec<(String, Class)>],
    notes: &[String],
    theme: &Theme,
    width: usize,
    margin: usize,
    center: bool,
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
    let offset = if center {
        (inner.saturating_sub(content_w)) / 2
    } else {
        0
    };
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

/// A sequence diagram too wide to draw: one line per message or note, with
/// block contents indented under their keyword.
#[allow(clippy::too_many_arguments)]
fn render_sequence(
    d: &sequence::Diagram,
    title: &str,
    diagram_color: &str,
    text_color: &str,
    border_color: &str,
    arrow_color: &str,
    margin: &str,
    width: usize,
) -> Vec<StyledLine> {
    use sequence::{Event, Head, Place};
    let mut lines = Vec::new();
    lines.push(make_header(
        &text::truncate(title, width.saturating_sub(6)),
        border_color,
        diagram_color,
        margin,
        width,
    ));
    let style = |fg: &str, bold: bool, italic: bool| SpanStyle {
        fg: Some(fg.to_string()),
        bold,
        italic,
        ..Default::default()
    };
    let span = |text: String, style: SpanStyle| StyledSpan { text, style };
    let name = |i: usize| d.participants[i].label.replace('\n', " ");
    let one_line = |s: &str| s.replace('\n', " ");
    let mut depth = 0usize;
    for e in &d.events {
        let mut spans = Vec::new();
        let indent = |depth: usize| " ".repeat(2 * depth);
        match e {
            Event::Message {
                from,
                to,
                dotted,
                head,
                both,
                label,
                number,
                ..
            } => {
                let stroke = if *dotted { "┄" } else { "─" };
                let tip = match head {
                    Head::None => stroke,
                    Head::Arrow => "▶",
                    Head::Open => "▷",
                    Head::Cross => "×",
                };
                let tail = if *both { "◀" } else { stroke };
                let mut lead = indent(depth);
                if let Some(k) = number {
                    lead.push_str(&format!("{k}. "));
                }
                spans.push(span(
                    format!("{lead}{} ", name(*from)),
                    style(text_color, true, false),
                ));
                spans.push(span(
                    format!("{tail}{stroke}{tip} "),
                    style(arrow_color, false, false),
                ));
                spans.push(span(name(*to), style(text_color, true, false)));
                if !label.is_empty() {
                    spans.push(span(
                        format!(": {}", one_line(label)),
                        style(text_color, false, true),
                    ));
                }
            }
            Event::Note {
                place,
                lo,
                hi,
                text: t,
            } => {
                let at = match place {
                    Place::Left => format!("left of {}", name(*lo)),
                    Place::Right => format!("right of {}", name(*lo)),
                    Place::Over if lo == hi => format!("over {}", name(*lo)),
                    Place::Over => format!("over {}, {}", name(*lo), name(*hi)),
                };
                spans.push(span(
                    format!("{}  📝 Note {at}: {}", indent(depth), one_line(t)),
                    style(text_color, false, true),
                ));
            }
            Event::Open { kind, label } | Event::Divider { kind, label } => {
                let opens = matches!(e, Event::Open { .. });
                let at = if opens {
                    depth
                } else {
                    depth.saturating_sub(1)
                };
                spans.push(span(
                    format!("{}{kind}", indent(at)),
                    style(diagram_color, true, false),
                ));
                if !label.is_empty() {
                    spans.push(span(
                        format!(" [{}]", one_line(label)),
                        style(text_color, false, true),
                    ));
                }
                if opens {
                    depth += 1;
                }
            }
            Event::Close => {
                depth = depth.saturating_sub(1);
                spans.push(span(
                    format!("{}end", indent(depth)),
                    style(diagram_color, true, false),
                ));
            }
            Event::Activate(_) | Event::Deactivate(_) => continue,
        }
        // Wrap at words, continuing two columns in from the item's indent.
        let lead = spans
            .first()
            .map_or(0, |s| s.text.len() - s.text.trim_start_matches(' ').len());
        for row in wrap_spans(spans, width.saturating_sub(3), lead + 2) {
            let mut line = StyledLine::new();
            push_margin(&mut line, margin);
            line.push(span("│  ".to_string(), style(border_color, false, false)));
            for s in row {
                line.push(s);
            }
            lines.push(line);
        }
    }

    lines.push(make_footer(border_color, margin, width));
    lines.push(StyledLine::empty());

    lines
}

/// Word-wrap a line of spans to `avail` columns, each continuation line
/// starting with `cont` spaces. A word wider than a whole line is left for
/// [`fit_lines`] to break.
fn wrap_spans(spans: Vec<StyledSpan>, avail: usize, cont: usize) -> Vec<Vec<StyledSpan>> {
    let mut rows: Vec<Vec<StyledSpan>> = vec![Vec::new()];
    let mut w = 0;
    for s in spans {
        for word in s.text.split_inclusive(' ') {
            if w > cont && w + text::width(word.trim_end()) > avail {
                rows.push(vec![StyledSpan {
                    text: " ".repeat(cont),
                    style: SpanStyle::default(),
                }]);
                w = cont;
            }
            w += text::width(word);
            let row = rows.last_mut().expect("never empty");
            match row.last_mut() {
                Some(last) if last.style == s.style => last.text.push_str(word),
                _ => row.push(StyledSpan {
                    text: word.to_string(),
                    style: s.style.clone(),
                }),
            }
        }
    }
    rows
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

    // Render as horizontal bar chart. The label column is as wide as the
    // widest label (at least 12), so every bar starts in the same column; a
    // fixed `{:>12}` let longer labels push their bar right. Labels are cut
    // only when the chart is too narrow to hold them beside a 10-cell bar.
    // "│  " + label + " " + bar + " 100.0%"
    const FIXED: usize = 3 + 1 + 7;
    let widest = slices
        .iter()
        .map(|(l, _)| text::width(l))
        .max()
        .unwrap_or(0);
    let label_w = widest.max(12).min(width.saturating_sub(FIXED + 10)).max(1);
    // 30 cells at the usual 56-column chart width; less when narrower.
    let max_bar = 30usize
        .min(width.saturating_sub(FIXED + label_w + 1))
        .max(1);
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
            text: {
                let label = text::truncate(label, label_w);
                let pad = label_w.saturating_sub(text::width(&label));
                format!("{}{label} ", " ".repeat(pad))
            },
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
