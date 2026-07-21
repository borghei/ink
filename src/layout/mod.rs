pub mod mermaid;
pub mod table;

use crate::image::ImageMode;
use crate::theme::Theme;
use crate::Spacing;
use comrak::nodes::{AstNode, ListType, NodeValue};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
// SyntaxSet/ThemeSet are shared singletons; see crate::highlight.
use unicode_width::UnicodeWidthStr;

/// A rendered line of styled text, ready for display.
#[derive(Debug, Clone)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
}

impl Default for StyledLine {
    fn default() -> Self {
        Self::new()
    }
}

impl StyledLine {
    pub fn new() -> Self {
        Self { spans: Vec::new() }
    }

    pub fn push(&mut self, span: StyledSpan) {
        self.spans.push(span);
    }

    #[allow(dead_code)]
    pub fn plain(text: &str) -> Self {
        Self {
            spans: vec![StyledSpan {
                text: text.to_string(),
                style: SpanStyle::default(),
            }],
        }
    }

    pub fn empty() -> Self {
        Self { spans: Vec::new() }
    }

    #[allow(dead_code)]
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| s.text.width()).sum()
    }
}

#[derive(Debug, Clone)]
pub struct StyledSpan {
    pub text: String,
    pub style: SpanStyle,
}

#[derive(Debug, Clone, Default)]
pub struct SpanStyle {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub dim: bool,
    pub link_url: Option<String>,
}

/// A heading recorded during layout, with the exact display-line index where
/// it lands. Used to build the TOC without a fragile substring reverse-scan.
#[derive(Debug, Clone)]
pub struct LayoutHeading {
    pub level: u8,
    pub text: String,
    pub line_index: usize,
}

/// Result of laying out a document: the display lines plus the headings and
/// their line positions.
pub struct LayoutResult {
    pub lines: Vec<StyledLine>,
    pub headings: Vec<LayoutHeading>,
}

/// Convert a comrak AST into a flat list of styled lines for rendering.
pub fn layout_document<'a>(
    root: &'a AstNode<'a>,
    theme: &Theme,
    width: u16,
    spacing: Spacing,
    center_margin: usize,
    base_dir: Option<&std::path::Path>,
    images: ImageMode,
) -> LayoutResult {
    let mut lines: Vec<StyledLine> = Vec::new();
    let headings = std::cell::RefCell::new(Vec::new());
    let ctx = LayoutContext {
        theme,
        width: width as usize,
        indent: 0,
        list_depth: 0,
        spacing,
        margin: center_margin,
        syntax_set: crate::highlight::syntax_set(),
        theme_set: crate::highlight::theme_set(),
        base_dir,
        images,
        headings: &headings,
        record_headings: true,
    };
    layout_node(root, &ctx, &mut lines);
    sanitize_lines(&mut lines);
    LayoutResult {
        lines,
        headings: headings.into_inner(),
    }
}

/// Final pass over everything headed for the terminal: strip control bytes
/// from text and drop link destinations with disallowed schemes or embedded
/// controls. Untrusted markdown must not be able to emit escape sequences.
fn sanitize_lines(lines: &mut [StyledLine]) {
    use crate::sanitize::{sanitize_text, sanitize_url};
    use std::borrow::Cow;
    for line in lines.iter_mut() {
        for span in line.spans.iter_mut() {
            if let Cow::Owned(clean) = sanitize_text(&span.text) {
                span.text = clean;
            }
            if let Some(url) = span.style.link_url.take() {
                span.style.link_url = sanitize_url(&url);
            }
        }
    }
}

struct LayoutContext<'a> {
    theme: &'a Theme,
    width: usize,
    indent: usize,
    list_depth: usize,
    spacing: Spacing,
    margin: usize,
    syntax_set: &'a SyntaxSet,
    theme_set: &'a ThemeSet,
    base_dir: Option<&'a std::path::Path>,
    images: ImageMode,
    headings: &'a std::cell::RefCell<Vec<LayoutHeading>>,
    // Only the top-level walk writes into the shared `lines`, so only it can
    // record correct absolute line indices. Nested walks (blockquotes, list
    // items) build into their own buffers, so they don't record headings.
    record_headings: bool,
}

impl<'a> LayoutContext<'a> {
    /// Add left margin to a line for content centering.
    fn add_margin(&self, line: &mut StyledLine) {
        if self.margin > 0 {
            line.spans.insert(
                0,
                StyledSpan {
                    text: " ".repeat(self.margin),
                    style: SpanStyle::default(),
                },
            );
        }
    }

    fn spacing_lines(&self) -> usize {
        match self.spacing {
            Spacing::Compact => 0,
            Spacing::Normal => 1,
            Spacing::Relaxed => 2,
        }
    }
}

fn add_spacing(ctx: &LayoutContext, lines: &mut Vec<StyledLine>) {
    for _ in 0..ctx.spacing_lines() {
        lines.push(StyledLine::empty());
    }
}

fn layout_node<'a>(node: &'a AstNode<'a>, ctx: &LayoutContext, lines: &mut Vec<StyledLine>) {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Document => {
            drop(data);
            for child in node.children() {
                layout_node(child, ctx, lines);
            }
        }
        NodeValue::Heading(heading) => {
            let level = heading.level;
            drop(data);
            layout_heading(node, level, ctx, lines);
        }
        NodeValue::Paragraph => {
            drop(data);
            layout_paragraph(node, ctx, lines);
        }
        NodeValue::CodeBlock(cb) => {
            let info = cb.info.clone();
            let literal = cb.literal.clone();
            drop(data);
            layout_code_block(&info, &literal, ctx, lines);
        }
        NodeValue::BlockQuote => {
            drop(data);
            layout_blockquote(node, ctx, lines, 0);
        }
        NodeValue::List(list) => {
            let list_type = list.list_type;
            let start = list.start;
            drop(data);
            layout_list(node, list_type, start, ctx, lines);
        }
        NodeValue::Item(_) => {
            drop(data);
            for child in node.children() {
                layout_node(child, ctx, lines);
            }
        }
        NodeValue::ThematicBreak => {
            drop(data);
            layout_hr(ctx, lines);
        }
        NodeValue::Table(..) => {
            drop(data);
            table::layout_table(node, ctx.theme, ctx.width, ctx.margin, lines);
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => {
            drop(data);
        }
        NodeValue::HtmlBlock(hb) => {
            let literal = hb.literal.clone();
            drop(data);
            for line_text in literal.lines() {
                let mut line = StyledLine::new();
                ctx.add_margin(&mut line);
                line.push(StyledSpan {
                    text: line_text.to_string(),
                    style: SpanStyle {
                        ..Default::default()
                    },
                });
                lines.push(line);
            }
        }
        NodeValue::FootnoteDefinition(ref fd) => {
            let name = fd.name.clone();
            drop(data);
            let mut line = StyledLine::new();
            ctx.add_margin(&mut line);
            line.push(StyledSpan {
                text: format!("[^{name}]: "),
                style: SpanStyle {
                    fg: Some(ctx.theme.colors.link.clone()),
                    bold: true,
                    ..Default::default()
                },
            });
            lines.push(line);
            for child in node.children() {
                layout_node(child, ctx, lines);
            }
            add_spacing(ctx, lines);
        }
        _ => {
            drop(data);
            for child in node.children() {
                layout_node(child, ctx, lines);
            }
        }
    }
}

fn layout_heading<'a>(
    node: &'a AstNode<'a>,
    level: u8,
    ctx: &LayoutContext,
    lines: &mut Vec<StyledLine>,
) {
    let color = match level {
        1 => &ctx.theme.colors.heading1,
        2 => &ctx.theme.colors.heading2,
        3 => &ctx.theme.colors.heading3,
        4 => &ctx.theme.colors.heading4,
        5 => &ctx.theme.colors.heading5,
        _ => &ctx.theme.colors.heading6,
    };

    // Extra spacing before headings
    lines.push(StyledLine::empty());
    if level <= 2 {
        lines.push(StyledLine::empty());
    }

    let mut line = StyledLine::new();
    ctx.add_margin(&mut line);

    let prefix = match level {
        1 => "█ ",
        2 => "▌ ",
        3 => "▎ ",
        4 => "  ",
        5 => "  ",
        _ => "  ",
    };

    line.push(StyledSpan {
        text: prefix.to_string(),
        style: SpanStyle {
            fg: Some(color.clone()),
            ..Default::default()
        },
    });

    // Record the heading's exact display-line index for the TOC.
    if ctx.record_headings {
        ctx.headings.borrow_mut().push(LayoutHeading {
            level,
            text: collect_child_text(node),
            line_index: lines.len(),
        });
    }

    let spans = collect_inline_spans(node, ctx);
    for mut span in spans {
        span.style.fg = Some(color.clone());
        span.style.bold = true;
        line.push(span);
    }

    lines.push(line);
    add_spacing(ctx, lines);
}

fn layout_paragraph<'a>(node: &'a AstNode<'a>, ctx: &LayoutContext, lines: &mut Vec<StyledLine>) {
    // Try to render standalone images as block-level elements with half-block pixels
    if ctx.images != ImageMode::Off {
        if let Some(image_lines) = try_render_image_block(node, ctx) {
            lines.extend(image_lines);
            add_spacing(ctx, lines);
            return;
        }
    }

    let spans = collect_inline_spans(node, ctx);
    let wrapped = wrap_spans(spans, ctx.width, ctx.indent, ctx.margin);
    lines.extend(wrapped);
    add_spacing(ctx, lines);
}

/// If the paragraph contains only a single image, try to render it as a block
/// using half-block Unicode characters. Falls back to None (inline placeholder).
fn try_render_image_block<'a>(
    node: &'a AstNode<'a>,
    ctx: &LayoutContext,
) -> Option<Vec<StyledLine>> {
    let children: Vec<_> = node.children().collect();
    if children.len() != 1 {
        return None;
    }

    let child = children[0];
    let data = child.data.borrow();
    let url = if let NodeValue::Image(ref img) = data.value {
        img.url.clone()
    } else {
        return None;
    };
    drop(data);

    let alt = collect_child_text(child);

    // Try to load and render the image
    let image_data = match crate::image::load_decoded(&url, ctx.base_dir, ctx.images) {
        Ok(data) => data,
        Err(crate::image::ImageUnavailable::RemoteBlocked) => {
            let alt_display = if alt.is_empty() { &url } else { &alt };
            let mut line = StyledLine::new();
            ctx.add_margin(&mut line);
            line.push(StyledSpan {
                text: format!("🖼 {alt_display} (remote image — pass --remote-images to load)"),
                style: SpanStyle {
                    fg: Some(ctx.theme.colors.link_url.clone()),
                    italic: true,
                    ..Default::default()
                },
            });
            return Some(vec![line]);
        }
        Err(crate::image::ImageUnavailable::Failed) => return None,
    };
    let mut image_lines = crate::image::render_halfblock(&image_data, ctx.width, ctx.margin)?;

    // Add alt text as caption below the image
    if !alt.is_empty() {
        let mut caption = StyledLine::new();
        ctx.add_margin(&mut caption);
        caption.push(StyledSpan {
            text: format!("  {alt}"),
            style: SpanStyle {
                fg: Some(ctx.theme.colors.link_url.clone()),
                italic: true,
                ..Default::default()
            },
        });
        image_lines.push(caption);
    }

    Some(image_lines)
}

fn layout_code_block(info: &str, literal: &str, ctx: &LayoutContext, lines: &mut Vec<StyledLine>) {
    let lang = info.split_whitespace().next().unwrap_or("");

    // Mermaid diagrams get special rendering
    if lang == "mermaid" {
        let mermaid_lines = mermaid::render_mermaid(literal, ctx.theme, ctx.width, ctx.margin);
        lines.extend(mermaid_lines);
        return;
    }

    let border_width = ctx.width.min(80);

    // Top border
    let mut header = StyledLine::new();
    ctx.add_margin(&mut header);
    if lang.is_empty() {
        header.push(StyledSpan {
            text: format!("╭{}╮", "─".repeat(border_width.saturating_sub(2))),
            style: SpanStyle {
                fg: Some(ctx.theme.colors.table_border.clone()),

                ..Default::default()
            },
        });
    } else {
        let label = format!(" {} ", lang);
        let remaining = border_width.saturating_sub(label.len() + 2);
        header.push(StyledSpan {
            text: "╭─".to_string(),
            style: SpanStyle {
                fg: Some(ctx.theme.colors.table_border.clone()),

                ..Default::default()
            },
        });
        header.push(StyledSpan {
            text: label,
            style: SpanStyle {
                fg: Some(ctx.theme.colors.heading3.clone()),

                ..Default::default()
            },
        });
        header.push(StyledSpan {
            text: format!("{}╮", "─".repeat(remaining)),
            style: SpanStyle {
                fg: Some(ctx.theme.colors.table_border.clone()),

                ..Default::default()
            },
        });
    }
    lines.push(header);

    // Syntax-highlighted code lines
    let syntax = ctx
        .syntax_set
        .find_syntax_by_token(lang)
        .unwrap_or_else(|| ctx.syntax_set.find_syntax_plain_text());

    let highlight_theme = ctx
        .theme_set
        .themes
        .get(&ctx.theme.code_theme)
        .or_else(|| ctx.theme_set.themes.get("base16-ocean.dark"))
        .unwrap_or_else(|| ctx.theme_set.themes.values().next().unwrap());

    let mut highlighter = HighlightLines::new(syntax, highlight_theme);

    for code_line in LinesWithEndings::from(literal) {
        let mut line = StyledLine::new();
        ctx.add_margin(&mut line);
        line.push(StyledSpan {
            text: "│ ".to_string(),
            style: SpanStyle {
                fg: Some(ctx.theme.colors.table_border.clone()),

                ..Default::default()
            },
        });

        // Apply syntax highlighting
        if let Ok(ranges) = highlighter.highlight_line(code_line, ctx.syntax_set) {
            for (style, text) in ranges {
                let fg_color = format!(
                    "#{:02x}{:02x}{:02x}",
                    style.foreground.r, style.foreground.g, style.foreground.b
                );
                let trimmed = text.trim_end_matches('\n');
                if !trimmed.is_empty() {
                    line.push(StyledSpan {
                        text: trimmed.to_string(),
                        style: SpanStyle {
                            fg: Some(fg_color),
                            ..Default::default()
                        },
                    });
                }
            }
        } else {
            line.push(StyledSpan {
                text: code_line.trim_end_matches('\n').to_string(),
                style: SpanStyle {
                    fg: Some(ctx.theme.colors.code_fg.clone()),
                    ..Default::default()
                },
            });
        }

        lines.push(line);
    }

    // Bottom border
    let mut footer = StyledLine::new();
    ctx.add_margin(&mut footer);
    footer.push(StyledSpan {
        text: format!("╰{}╯", "─".repeat(border_width.saturating_sub(2))),
        style: SpanStyle {
            fg: Some(ctx.theme.colors.table_border.clone()),
            ..Default::default()
        },
    });
    lines.push(footer);
    add_spacing(ctx, lines);
}

fn layout_blockquote<'a>(
    node: &'a AstNode<'a>,
    ctx: &LayoutContext,
    lines: &mut Vec<StyledLine>,
    depth: usize,
) {
    // Check first child for admonition pattern [!TYPE]
    let admonition = detect_admonition(node);

    // Choose bar color based on depth or admonition type
    let bar_color = if let Some(ref adm) = admonition {
        match adm.as_str() {
            "NOTE" => &ctx.theme.colors.admonition_note,
            "TIP" => &ctx.theme.colors.admonition_tip,
            "IMPORTANT" => &ctx.theme.colors.admonition_important,
            "WARNING" => &ctx.theme.colors.admonition_warning,
            "CAUTION" => &ctx.theme.colors.admonition_caution,
            _ => &ctx.theme.colors.blockquote_bar,
        }
    } else {
        // Different colors for nested blockquote depths
        match depth % 4 {
            0 => &ctx.theme.colors.blockquote_bar,
            1 => &ctx.theme.colors.heading2,
            2 => &ctx.theme.colors.heading3,
            _ => &ctx.theme.colors.heading4,
        }
    };

    // Render admonition header if detected
    if let Some(ref adm_type) = admonition {
        let icon = match adm_type.as_str() {
            "NOTE" => "ℹ ",
            "TIP" => "💡 ",
            "IMPORTANT" => "❗ ",
            "WARNING" => "⚠ ",
            "CAUTION" => "🔴 ",
            _ => "▎ ",
        };
        let mut header_line = StyledLine::new();
        ctx.add_margin(&mut header_line);
        header_line.push(StyledSpan {
            text: "  │ ".to_string(),
            style: SpanStyle {
                fg: Some(bar_color.clone()),
                ..Default::default()
            },
        });
        header_line.push(StyledSpan {
            text: format!("{icon}{adm_type}"),
            style: SpanStyle {
                fg: Some(bar_color.clone()),
                bold: true,
                ..Default::default()
            },
        });
        lines.push(header_line);
    }

    // Render children into temporary buffer
    let inner_ctx = LayoutContext {
        theme: ctx.theme,
        width: ctx.width.saturating_sub(4),
        indent: 0,
        list_depth: ctx.list_depth,
        spacing: ctx.spacing,
        margin: 0, // margin handled by parent
        syntax_set: ctx.syntax_set,
        theme_set: ctx.theme_set,
        base_dir: ctx.base_dir,
        images: ctx.images,
        headings: ctx.headings,
        record_headings: false,
    };
    let mut inner_lines = Vec::new();
    let mut skip_first = admonition.is_some();
    for child in node.children() {
        let child_data = child.data.borrow();
        // Handle nested blockquotes with increased depth
        if let NodeValue::BlockQuote = &child_data.value {
            drop(child_data);
            layout_blockquote(child, &inner_ctx, &mut inner_lines, depth + 1);
        } else {
            drop(child_data);
            if skip_first {
                // For admonitions, we render first paragraph without the [!TYPE] prefix
                skip_first = false;
                if let NodeValue::Paragraph = &child.data.borrow().value {
                    let spans = collect_inline_spans(child, &inner_ctx);
                    // Filter out the admonition marker text
                    let filtered: Vec<StyledSpan> = spans
                        .into_iter()
                        .filter(|s| !s.text.starts_with("[!"))
                        .collect();
                    if !filtered.is_empty() {
                        let wrapped = wrap_spans(filtered, inner_ctx.width, 0, 0);
                        inner_lines.extend(wrapped);
                        add_spacing(&inner_ctx, &mut inner_lines);
                    }
                    continue;
                }
            }
            layout_node(child, &inner_ctx, &mut inner_lines);
        }
    }

    // Prepend blockquote bar to each line
    for inner_line in inner_lines {
        let mut line = StyledLine::new();
        ctx.add_margin(&mut line);
        line.push(StyledSpan {
            text: "  │ ".to_string(),
            style: SpanStyle {
                fg: Some(bar_color.clone()),
                ..Default::default()
            },
        });
        for mut span in inner_line.spans {
            if span.style.fg.is_none() {
                span.style.fg = Some(ctx.theme.colors.blockquote_text.clone());
                span.style.italic = true;
            }
            line.push(span);
        }
        lines.push(line);
    }
}

/// Detect GitHub-style admonition: > [!NOTE], > [!WARNING], etc.
fn detect_admonition<'a>(node: &'a AstNode<'a>) -> Option<String> {
    if let Some(child) = node.children().next() {
        let data = child.data.borrow();
        if let NodeValue::Paragraph = &data.value {
            drop(data);
            if let Some(inline) = child.children().next() {
                let idata = inline.data.borrow();
                if let NodeValue::Text(ref text) = idata.value {
                    let trimmed = text.trim();
                    if trimmed.starts_with("[!") && trimmed.contains(']') {
                        let end = trimmed.find(']').unwrap();
                        let adm_type = &trimmed[2..end];
                        return Some(adm_type.to_uppercase());
                    }
                }
            }
        }
    }
    None
}

fn layout_list<'a>(
    node: &'a AstNode<'a>,
    list_type: ListType,
    start: usize,
    ctx: &LayoutContext,
    lines: &mut Vec<StyledLine>,
) {
    let inner_ctx = LayoutContext {
        theme: ctx.theme,
        width: ctx.width.saturating_sub(4),
        indent: ctx.indent + 4,
        list_depth: ctx.list_depth + 1,
        spacing: ctx.spacing,
        margin: 0,
        syntax_set: ctx.syntax_set,
        theme_set: ctx.theme_set,
        base_dir: ctx.base_dir,
        images: ctx.images,
        headings: ctx.headings,
        record_headings: false,
    };

    for (i, item) in node.children().enumerate() {
        let marker = match list_type {
            ListType::Bullet => "  ◦ ".to_string(),
            ListType::Ordered => format!("  {}. ", start + i),
        };

        let marker_color = match list_type {
            ListType::Bullet => &ctx.theme.colors.list_bullet,
            ListType::Ordered => &ctx.theme.colors.list_number,
        };

        let item_data = item.data.borrow();
        let is_checked = if let NodeValue::TaskItem(Some(c)) = &item_data.value {
            Some(*c)
        } else if let NodeValue::TaskItem(None) = &item_data.value {
            Some(' ')
        } else {
            None
        };
        drop(item_data);

        let mut item_lines: Vec<StyledLine> = Vec::new();
        for child in item.children() {
            let child_data = child.data.borrow();
            match &child_data.value {
                NodeValue::Paragraph => {
                    drop(child_data);
                    let spans = collect_inline_spans(child, &inner_ctx);
                    let wrapped = wrap_spans(spans, inner_ctx.width, 0, 0);
                    item_lines.extend(wrapped);
                }
                NodeValue::List(list) => {
                    let lt = list.list_type;
                    let s = list.start;
                    drop(child_data);
                    layout_list(child, lt, s, &inner_ctx, &mut item_lines);
                }
                _ => {
                    drop(child_data);
                    layout_node(child, &inner_ctx, &mut item_lines);
                }
            }
        }

        for (j, item_line) in item_lines.into_iter().enumerate() {
            let mut line = StyledLine::new();
            ctx.add_margin(&mut line);
            if j == 0 {
                if let Some(checked) = is_checked {
                    let (icon, color) = if checked != ' ' {
                        ("  ✓ ", &ctx.theme.colors.task_done)
                    } else {
                        ("  ○ ", &ctx.theme.colors.task_pending)
                    };
                    line.push(StyledSpan {
                        text: icon.to_string(),
                        style: SpanStyle {
                            fg: Some(color.clone()),
                            ..Default::default()
                        },
                    });
                } else {
                    line.push(StyledSpan {
                        text: marker.clone(),
                        style: SpanStyle {
                            fg: Some(marker_color.clone()),
                            ..Default::default()
                        },
                    });
                }
            } else {
                line.push(StyledSpan {
                    text: "    ".to_string(),
                    style: SpanStyle::default(),
                });
            }
            for span in item_line.spans {
                line.push(span);
            }
            lines.push(line);
        }
    }
    add_spacing(ctx, lines);
}

fn layout_hr(ctx: &LayoutContext, lines: &mut Vec<StyledLine>) {
    let width = ctx.width.min(60);
    let left_pad = (ctx.width.saturating_sub(width)) / 2;
    let mut line = StyledLine::new();
    ctx.add_margin(&mut line);
    if left_pad > 0 {
        line.push(StyledSpan {
            text: " ".repeat(left_pad),
            style: SpanStyle::default(),
        });
    }
    let half = width / 2;
    line.push(StyledSpan {
        text: format!(
            "{}  ◆  {}",
            "╌".repeat(half.saturating_sub(3)),
            "╌".repeat(half.saturating_sub(3))
        ),
        style: SpanStyle {
            fg: Some(ctx.theme.colors.hr.clone()),
            ..Default::default()
        },
    });
    lines.push(StyledLine::empty());
    lines.push(line);
    lines.push(StyledLine::empty());
}

fn collect_inline_spans<'a>(node: &'a AstNode<'a>, ctx: &LayoutContext) -> Vec<StyledSpan> {
    let mut spans = Vec::new();
    collect_inlines(node, ctx, &mut spans, &SpanStyle::default());
    spans
}

fn collect_inlines<'a>(
    node: &'a AstNode<'a>,
    ctx: &LayoutContext,
    spans: &mut Vec<StyledSpan>,
    parent_style: &SpanStyle,
) {
    for child in node.children() {
        let data = child.data.borrow();
        match &data.value {
            NodeValue::Text(text) => {
                // Auto-detect bare URLs and make them hyperlinks
                split_text_with_urls(text, parent_style, ctx, spans);
            }
            NodeValue::SoftBreak => {
                spans.push(StyledSpan {
                    text: " ".to_string(),
                    style: parent_style.clone(),
                });
            }
            NodeValue::LineBreak => {
                spans.push(StyledSpan {
                    text: "\n".to_string(),
                    style: parent_style.clone(),
                });
            }
            NodeValue::Code(c) => {
                spans.push(StyledSpan {
                    text: format!(" {} ", c.literal),
                    style: SpanStyle {
                        fg: Some(ctx.theme.colors.code_fg.clone()),
                        bg: Some(ctx.theme.colors.code_bg.clone()),
                        ..parent_style.clone()
                    },
                });
            }
            NodeValue::Emph => {
                let style = SpanStyle {
                    italic: true,
                    ..parent_style.clone()
                };
                drop(data);
                collect_inlines(child, ctx, spans, &style);
                continue;
            }
            NodeValue::Strong => {
                let style = SpanStyle {
                    bold: true,
                    fg: Some(ctx.theme.colors.bold.clone()),
                    ..parent_style.clone()
                };
                drop(data);
                collect_inlines(child, ctx, spans, &style);
                continue;
            }
            NodeValue::Strikethrough => {
                let style = SpanStyle {
                    strikethrough: true,
                    fg: Some(ctx.theme.colors.strikethrough.clone()),
                    ..parent_style.clone()
                };
                drop(data);
                collect_inlines(child, ctx, spans, &style);
                continue;
            }
            NodeValue::Link(link) => {
                let url = link.url.clone();
                let style = SpanStyle {
                    fg: Some(ctx.theme.colors.link.clone()),
                    underline: true,
                    link_url: Some(url),
                    ..parent_style.clone()
                };
                drop(data);
                collect_inlines(child, ctx, spans, &style);
                continue;
            }
            NodeValue::Image(img) => {
                let alt = collect_child_text(child);
                let alt_display = if alt.is_empty() { img.url.clone() } else { alt };
                spans.push(StyledSpan {
                    text: format!("🖼 {alt_display}"),
                    style: SpanStyle {
                        fg: Some(ctx.theme.colors.link.clone()),

                        ..Default::default()
                    },
                });
            }
            NodeValue::FootnoteReference(ref fr) => {
                spans.push(StyledSpan {
                    text: format!("[^{}]", fr.name),
                    style: SpanStyle {
                        fg: Some(ctx.theme.colors.link.clone()),
                        ..Default::default()
                    },
                });
            }
            _ => {
                drop(data);
                collect_inlines(child, ctx, spans, parent_style);
                continue;
            }
        }
        drop(data);
    }
}

/// Split text into plain text and URL spans. Bare URLs become clickable hyperlinks.
fn split_text_with_urls(
    text: &str,
    parent_style: &SpanStyle,
    ctx: &LayoutContext,
    spans: &mut Vec<StyledSpan>,
) {
    let mut remaining = text;
    while !remaining.is_empty() {
        // Find the next URL
        let url_start = remaining
            .find("https://")
            .or_else(|| remaining.find("http://"));

        match url_start {
            Some(start) => {
                // Push text before the URL
                if start > 0 {
                    spans.push(StyledSpan {
                        text: remaining[..start].to_string(),
                        style: parent_style.clone(),
                    });
                }
                // Find URL end
                let url_part = &remaining[start..];
                let end = url_part
                    .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '>' | '"' | '\''))
                    .unwrap_or(url_part.len());
                let url = url_part[..end].trim_end_matches(['.', ',', ';']);

                spans.push(StyledSpan {
                    text: url.to_string(),
                    style: SpanStyle {
                        fg: Some(ctx.theme.colors.link.clone()),
                        underline: true,
                        link_url: Some(url.to_string()),
                        ..parent_style.clone()
                    },
                });

                remaining = &remaining[start + end..];
            }
            None => {
                // No more URLs, push remaining text
                spans.push(StyledSpan {
                    text: remaining.to_string(),
                    style: parent_style.clone(),
                });
                break;
            }
        }
    }
}

fn collect_child_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    for child in node.children() {
        let data = child.data.borrow();
        if let NodeValue::Text(ref t) = data.value {
            text.push_str(t);
        }
        drop(data);
        let child_text = collect_child_text(child);
        text.push_str(&child_text);
    }
    text
}

fn wrap_spans(
    spans: Vec<StyledSpan>,
    max_width: usize,
    indent: usize,
    margin: usize,
) -> Vec<StyledLine> {
    let effective_width = max_width.saturating_sub(indent);
    if effective_width == 0 || spans.is_empty() {
        let mut line = StyledLine { spans };
        if margin > 0 {
            line.spans.insert(
                0,
                StyledSpan {
                    text: " ".repeat(margin),
                    style: SpanStyle::default(),
                },
            );
        }
        return vec![line];
    }

    let indent_str = " ".repeat(indent);
    let margin_str = " ".repeat(margin);
    let mut lines: Vec<StyledLine> = Vec::new();
    let mut current = StyledLine::new();
    let mut col = 0;

    if margin > 0 {
        current.push(StyledSpan {
            text: margin_str.clone(),
            style: SpanStyle::default(),
        });
    }
    if indent > 0 {
        current.push(StyledSpan {
            text: indent_str.clone(),
            style: SpanStyle::default(),
        });
    }

    for span in spans {
        let words: Vec<&str> = span.text.split(' ').collect();
        for (i, word) in words.iter().enumerate() {
            let w = word.width();
            if w == 0 && i > 0 {
                // Empty word means a literal space here (trailing space of this
                // span, or a run of consecutive spaces). Emit it so spacing is
                // preserved across span boundaries — e.g. "word **bold**" must
                // not collapse to "wordbold".
                if col > 0 && col < effective_width {
                    current.push(StyledSpan {
                        text: " ".to_string(),
                        style: span.style.clone(),
                    });
                    col += 1;
                }
                continue;
            }
            let need_space = col > 0 && i > 0;
            let total = col + w + if need_space { 1 } else { 0 };

            if total > effective_width && col > 0 {
                lines.push(current);
                current = StyledLine::new();
                if margin > 0 {
                    current.push(StyledSpan {
                        text: margin_str.clone(),
                        style: SpanStyle::default(),
                    });
                }
                if indent > 0 {
                    current.push(StyledSpan {
                        text: indent_str.clone(),
                        style: SpanStyle::default(),
                    });
                }
                col = 0;
            }

            if col > 0 && need_space {
                current.push(StyledSpan {
                    text: " ".to_string(),
                    style: span.style.clone(),
                });
                col += 1;
            }

            current.push(StyledSpan {
                text: word.to_string(),
                style: span.style.clone(),
            });
            col += w;
        }
    }

    if !current.spans.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(StyledLine::empty());
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, bold: bool) -> StyledSpan {
        StyledSpan {
            text: text.to_string(),
            style: SpanStyle {
                bold,
                ..Default::default()
            },
        }
    }

    fn line_text(line: &StyledLine) -> String {
        line.spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn keeps_space_before_styled_span() {
        // "roughly " (plain) + "17,800 km" (bold) must not collapse.
        let spans = vec![span("roughly ", false), span("17,800 km", true)];
        let lines = wrap_spans(spans, 80, 0, 0);
        assert_eq!(line_text(&lines[0]), "roughly 17,800 km");
    }

    #[test]
    fn keeps_space_after_styled_span() {
        // "17,800 km" (bold) + " logged" (plain leading space).
        let spans = vec![span("17,800 km", true), span(" logged", false)];
        let lines = wrap_spans(spans, 80, 0, 0);
        assert_eq!(line_text(&lines[0]), "17,800 km logged");
    }

    #[test]
    fn keeps_space_between_adjacent_styled_spans() {
        let spans = vec![span("touched ", false), span("44 bpm", true)];
        let lines = wrap_spans(spans, 80, 0, 0);
        assert_eq!(line_text(&lines[0]), "touched 44 bpm");
    }
}
