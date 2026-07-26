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
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

#[derive(Debug, Clone, Default, PartialEq)]
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

/// Merge adjacent spans that share a style into one, so a run of same-styled
/// words stays a contiguous string (keeps `--plain` output greppable and cuts
/// redundant escape sequences).
fn coalesce_line(line: &mut StyledLine) {
    let mut merged: Vec<StyledSpan> = Vec::with_capacity(line.spans.len());
    for span in line.spans.drain(..) {
        if let Some(last) = merged.last_mut() {
            if last.style == span.style {
                last.text.push_str(&span.text);
                continue;
            }
        }
        merged.push(span);
    }
    line.spans = merged;
}

/// A heading recorded during layout, with the exact display-line index where
/// it lands. Used to build the TOC without a fragile substring reverse-scan.
#[derive(Debug, Clone)]
pub struct LayoutHeading {
    pub level: u8,
    pub text: String,
    pub line_index: usize,
}

/// A block image reserved in the text flow for graphics-protocol rendering.
/// The `image` is decoded once here; `build_tab` turns it into a protocol.
pub struct ImageSpec {
    pub line_index: usize,
    pub col_offset: u16,
    pub cols: u16,
    pub rows: u16,
    pub image: std::sync::Arc<image::DynamicImage>,
}

/// Result of laying out a document: the display lines plus the headings and
/// their line positions.
pub struct LayoutResult {
    pub lines: Vec<StyledLine>,
    pub headings: Vec<LayoutHeading>,
    pub images: Vec<ImageSpec>,
}

/// Convert a comrak AST into a flat list of styled lines for rendering.
///
/// `graphics_font`: when `Some((cell_w, cell_h))`, block images are reserved as
/// blank rows and collected into `LayoutResult.images` for graphics-protocol
/// rendering; when `None`, images render inline as Unicode half-blocks.
#[allow(clippy::too_many_arguments)]
pub fn layout_document<'a>(
    root: &'a AstNode<'a>,
    theme: &Theme,
    width: u16,
    spacing: Spacing,
    center_margin: usize,
    base_dir: Option<&std::path::Path>,
    images: ImageMode,
    graphics_font: Option<(u16, u16)>,
) -> LayoutResult {
    let mut lines: Vec<StyledLine> = Vec::new();
    let headings = std::cell::RefCell::new(Vec::new());
    let image_specs = std::cell::RefCell::new(Vec::new());
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
        graphics_font,
        headings: &headings,
        image_specs: &image_specs,
        record_headings: true,
    };
    layout_node(root, &ctx, &mut lines);
    sanitize_lines(&mut lines);
    LayoutResult {
        lines,
        headings: headings.into_inner(),
        images: image_specs.into_inner(),
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
    /// `Some((cell_w, cell_h))` → graphics mode: reserve rows + collect specs.
    graphics_font: Option<(u16, u16)>,
    headings: &'a std::cell::RefCell<Vec<LayoutHeading>>,
    image_specs: &'a std::cell::RefCell<Vec<ImageSpec>>,
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

/// Drop trailing blank lines (all-whitespace spans) from a buffer. Used before
/// wrapping child content in a bar/indent so trailing spacing doesn't become a
/// stray `│ ` bar line or an indented run of spaces.
fn trim_trailing_blank(lines: &mut Vec<StyledLine>) {
    while lines
        .last()
        .is_some_and(|l| l.spans.iter().all(|s| s.text.trim().is_empty()))
    {
        lines.pop();
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
            // `<img>` tags render as real images (READMEs use raw HTML for
            // sized/centered logos); the surrounding markup is dropped.
            if ctx.images != ImageMode::Off {
                let html_images = extract_html_images(&literal);
                if !html_images.is_empty() {
                    for (src, alt) in &html_images {
                        let block = image_block_lines(src, alt, None, ctx, lines.len());
                        lines.extend(block);
                    }
                    add_spacing(ctx, lines);
                    return;
                }
            }
            // Other raw HTML is shown as dim text; wrap it so long tags/URLs
            // stay within the width instead of overflowing.
            for line_text in literal.lines() {
                if line_text.trim().is_empty() {
                    continue;
                }
                let spans = vec![StyledSpan {
                    text: line_text.to_string(),
                    style: SpanStyle {
                        fg: Some(ctx.theme.colors.link_url.clone()),
                        ..Default::default()
                    },
                }];
                lines.extend(wrap_spans(spans, ctx.width, 0, ctx.margin));
            }
        }
        NodeValue::FootnoteDefinition(ref fd) => {
            let name = fd.name.clone();
            drop(data);
            let label = StyledSpan {
                text: format!("[^{name}]: "),
                style: SpanStyle {
                    fg: Some(ctx.theme.colors.link.clone()),
                    bold: true,
                    ..Default::default()
                },
            };
            let children: Vec<_> = node.children().collect();
            // Common case: a single paragraph. Render the label inline with the
            // text (was on its own line with a dangling colon).
            let single_para = children.len() == 1
                && matches!(children[0].data.borrow().value, NodeValue::Paragraph);
            if single_para {
                let mut spans = vec![label];
                spans.extend(collect_inline_spans(children[0], ctx));
                lines.extend(wrap_spans(spans, ctx.width, 0, ctx.margin));
            } else {
                let mut line = StyledLine::new();
                ctx.add_margin(&mut line);
                line.push(label);
                lines.push(line);
                for child in node.children() {
                    layout_node(child, ctx, lines);
                }
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

    let prefix = match level {
        1 => "█ ",
        2 => "▌ ",
        3 => "▎ ",
        _ => "  ",
    };
    let prefix_w = prefix.width();

    // Record the heading's exact display-line index for the TOC.
    if ctx.record_headings {
        ctx.headings.borrow_mut().push(LayoutHeading {
            level,
            text: collect_child_text(node),
            line_index: lines.len(),
        });
    }

    // Wrap the heading text so long headings don't overflow; the colored
    // prefix goes on the first line, continuation lines align under the text.
    let mut spans = collect_inline_spans(node, ctx);
    for span in &mut spans {
        span.style.fg = Some(color.clone());
        span.style.bold = true;
    }
    let content_lines = wrap_spans(spans, ctx.width.saturating_sub(prefix_w), 0, 0);
    for (i, cline) in content_lines.into_iter().enumerate() {
        let mut line = StyledLine::new();
        ctx.add_margin(&mut line);
        if i == 0 {
            line.push(StyledSpan {
                text: prefix.to_string(),
                style: SpanStyle {
                    fg: Some(color.clone()),
                    ..Default::default()
                },
            });
        } else {
            line.push(StyledSpan {
                text: " ".repeat(prefix_w),
                style: SpanStyle::default(),
            });
        }
        line.spans.extend(cline.spans);
        lines.push(line);
    }
    add_spacing(ctx, lines);
}

fn layout_paragraph<'a>(node: &'a AstNode<'a>, ctx: &LayoutContext, lines: &mut Vec<StyledLine>) {
    // Try to render standalone images as block-level elements (graphics
    // protocol when available, else half-block pixels).
    if ctx.images != ImageMode::Off {
        if let Some(image_lines) = try_render_image_block(node, ctx, lines.len()) {
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

/// An image a paragraph can render as a block: source, alt text, and the
/// link destination when the image is wrapped in one (`[![alt](img)](url)`
/// badges/linked screenshots).
struct BlockImage {
    url: String,
    alt: String,
    link: Option<String>,
}

/// Collect the paragraph's images when the paragraph is images and nothing
/// else — a single one, several in a row (galleries), or link-wrapped —
/// separated only by whitespace/breaks. Returns `None` when images mix with
/// text (those stay inline).
fn collect_block_images<'a>(node: &'a AstNode<'a>) -> Option<Vec<BlockImage>> {
    let mut images = Vec::new();
    for child in node.children() {
        let data = child.data.borrow();
        match data.value {
            NodeValue::Image(ref img) => {
                let url = img.url.clone();
                drop(data);
                images.push(BlockImage {
                    url,
                    alt: collect_child_text(child),
                    link: None,
                });
            }
            NodeValue::Link(ref l) => {
                let link = l.url.clone();
                drop(data);
                let inner: Vec<_> = child.children().collect();
                if inner.len() != 1 {
                    return None;
                }
                let inner_data = inner[0].data.borrow();
                let NodeValue::Image(ref img) = inner_data.value else {
                    return None;
                };
                let url = img.url.clone();
                drop(inner_data);
                images.push(BlockImage {
                    url,
                    alt: collect_child_text(inner[0]),
                    link: Some(link),
                });
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => {}
            NodeValue::Text(ref t) if t.trim().is_empty() => {}
            _ => return None,
        }
    }
    (!images.is_empty()).then_some(images)
}

/// If the paragraph contains only images, render each as a block: in
/// graphics mode, reserve blank rows and record an `ImageSpec` (the draw loop
/// paints the pixels); otherwise render Unicode half-blocks. `start_line` is
/// the index the returned lines will occupy in the document.
fn try_render_image_block<'a>(
    node: &'a AstNode<'a>,
    ctx: &LayoutContext,
    start_line: usize,
) -> Option<Vec<StyledLine>> {
    let images = collect_block_images(node)?;
    let mut lines: Vec<StyledLine> = Vec::new();
    for img in &images {
        let block = image_block_lines(
            &img.url,
            &img.alt,
            img.link.as_deref(),
            ctx,
            start_line + lines.len(),
        );
        lines.extend(block);
    }
    Some(lines)
}

/// Render one image as block-level lines: pixels (graphics protocol or
/// half-blocks) plus caption, or a placeholder line that states why the
/// image could not be shown — a silently degraded image is indistinguishable
/// from a rendering bug (issue #3).
fn image_block_lines(
    url: &str,
    alt: &str,
    link: Option<&str>,
    ctx: &LayoutContext,
    start_line: usize,
) -> Vec<StyledLine> {
    let placeholder = |detail: String| {
        let alt_display = if alt.is_empty() { url } else { alt };
        let mut line = StyledLine::new();
        ctx.add_margin(&mut line);
        line.push(StyledSpan {
            text: format!("🖼 {alt_display} ({detail})"),
            style: SpanStyle {
                fg: Some(ctx.theme.colors.link_url.clone()),
                italic: true,
                link_url: link.map(str::to_string),
                ..Default::default()
            },
        });
        vec![line]
    };
    let image_data = match crate::image::load_decoded(url, ctx.base_dir, ctx.images) {
        Ok(data) => data,
        Err(crate::image::ImageUnavailable::RemoteBlocked) => {
            return placeholder("remote image — pass --remote-images to load".into());
        }
        Err(crate::image::ImageUnavailable::NotFound) => {
            return placeholder(if alt.is_empty() {
                "image not found".into()
            } else {
                format!("image not found: {url}")
            });
        }
        Err(crate::image::ImageUnavailable::Failed) => {
            return placeholder(if alt.is_empty() {
                "cannot decode image".into()
            } else {
                format!("cannot decode image: {url}")
            });
        }
    };

    // Graphics mode (top-level only, like heading indices): reserve blank rows
    // sized to the image and record a spec for the draw loop to paint.
    if let Some(font) = ctx.graphics_font {
        if ctx.record_headings {
            use image::GenericImageView;
            let (iw, ih) = image_data.dimensions();
            // `ctx.width` is already the content width; the centering margin
            // is applied via `col_offset` (the half-block branch below uses
            // `ctx.width` the same way). Subtracting the margin here again
            // shrank graphics images by the margin.
            let max_cols = ctx.width.max(1) as u16;
            let (cols, rows) =
                crate::graphics::cell_dimensions(iw, ih, font, max_cols, MAX_IMAGE_ROWS);
            let mut image_lines: Vec<StyledLine> = Vec::with_capacity(rows as usize + 2);
            image_lines.push(StyledLine::empty()); // gap before
            let img_start = start_line + image_lines.len();
            for _ in 0..rows {
                image_lines.push(StyledLine::empty());
            }
            ctx.image_specs.borrow_mut().push(ImageSpec {
                line_index: img_start,
                col_offset: ctx.margin as u16,
                cols,
                rows,
                image: image_data,
            });
            push_image_caption(alt, link, ctx, &mut image_lines);
            return image_lines;
        }
        // Nested (blockquote/list) graphics images aren't positioned reliably;
        // fall through to half-blocks.
    }

    let Some(mut image_lines) = crate::image::render_halfblock(&image_data, ctx.width, ctx.margin)
    else {
        return placeholder("cannot decode image".into());
    };
    push_image_caption(alt, link, ctx, &mut image_lines);
    image_lines
}

/// Maximum rows a graphics-protocol image may occupy in the text flow.
const MAX_IMAGE_ROWS: u16 = 30;

/// Extract `(src, alt)` from every `<img>` tag in a raw HTML fragment.
/// Tolerant by design: any attribute order, single/double/no quotes,
/// self-closing or not. Tags without a `src` are skipped. Quote-aware:
/// `src=` inside another attribute's quoted value is never mistaken for the
/// real attribute, and a `>` inside quotes does not end the tag.
fn extract_html_images(html: &str) -> Vec<(String, String)> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(found) = lower[pos..].find("<img") {
        let start = pos + found;
        let body = start + 4; // after "<img"
                              // Require a real tag boundary so `<imgs ...>` is not an image tag.
        let at_boundary = html[body..]
            .chars()
            .next()
            .is_none_or(|c| c.is_ascii_whitespace() || c == '>' || c == '/');
        if !at_boundary {
            pos = body;
            continue;
        }
        match scan_tag_attrs(&html[body..]) {
            Some((attrs, consumed)) => {
                let attr = |name: &str| {
                    attrs
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, v)| v.clone())
                };
                if let Some(src) = attr("src") {
                    out.push((src, attr("alt").unwrap_or_default()));
                }
                pos = body + consumed;
            }
            // Tag never closes: nothing trustworthy to extract.
            None => pos = body,
        }
    }
    out
}

/// Scan an HTML tag body (everything after the tag name) into attribute
/// `(name, value)` pairs, names ASCII-lowercased. Walks character by
/// character: attribute name, optional `=`, value in single/double quotes
/// (a `>` inside quotes does NOT end the tag) or unquoted. The tag ends at
/// the first unquoted `>`. Returns the attributes and the byte offset just
/// past that `>`, or `None` when the tag never closes.
fn scan_tag_attrs(rest: &str) -> Option<(Vec<(String, String)>, usize)> {
    let bytes = rest.as_bytes();
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace and the self-closing slash.
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'>' {
            return Some((attrs, i + 1));
        }
        // Attribute name: up to whitespace, `=`, `>`, or `/`.
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'=' | b'>' | b'/')
        {
            i += 1;
        }
        let name = rest[name_start..i].to_ascii_lowercase();
        // Optional `=` (whitespace allowed around it), then the value.
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut value = String::new();
        if j < bytes.len() && bytes[j] == b'=' {
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= bytes.len() {
                return None;
            }
            match bytes[j] {
                quote @ (b'"' | b'\'') => {
                    // Unterminated quote ⇒ the tag never closes.
                    let close = rest[j + 1..].find(quote as char)?;
                    value = rest[j + 1..j + 1 + close].to_string();
                    i = j + 1 + close + 1;
                }
                _ => {
                    let value_start = j;
                    while j < bytes.len()
                        && !bytes[j].is_ascii_whitespace()
                        && !matches!(bytes[j], b'>' | b'/')
                    {
                        j += 1;
                    }
                    value = rest[value_start..j].to_string();
                    i = j;
                }
            }
        }
        if !name.is_empty() {
            attrs.push((name, value));
        }
    }
    None
}

/// Append the image's alt text as an italic caption line. A link-wrapped
/// image carries its destination on the caption so hint-mode/opening still
/// reach it (the pixels themselves aren't clickable); with a link but no alt,
/// the destination is the caption.
fn push_image_caption(
    alt: &str,
    link: Option<&str>,
    ctx: &LayoutContext,
    lines: &mut Vec<StyledLine>,
) {
    let text = match (alt.is_empty(), link) {
        (false, _) => alt.to_string(),
        (true, Some(url)) => url.to_string(),
        (true, None) => return,
    };
    let mut caption = StyledLine::new();
    ctx.add_margin(&mut caption);
    caption.push(StyledSpan {
        text: format!("  {text}"),
        style: SpanStyle {
            fg: Some(ctx.theme.colors.link_url.clone()),
            italic: true,
            link_url: link.map(str::to_string),
            ..Default::default()
        },
    });
    lines.push(caption);
}

/// Expand tabs to spaces at 4-column tab stops, tracking the display column
/// per line. Terminal cells can't render `\t`; left unexpanded it breaks the
/// code-box borders and every downstream width calculation.
fn expand_tabs(text: &str) -> String {
    const TAB_STOP: usize = 4;
    let mut out = String::with_capacity(text.len());
    let mut col = 0usize;
    for ch in text.chars() {
        match ch {
            '\t' => {
                let spaces = TAB_STOP - (col % TAB_STOP);
                out.extend(std::iter::repeat_n(' ', spaces));
                col += spaces;
            }
            '\n' => {
                out.push('\n');
                col = 0;
            }
            _ => {
                out.push(ch);
                col += UnicodeWidthChar::width(ch).unwrap_or(0);
            }
        }
    }
    out
}

fn layout_code_block(info: &str, literal: &str, ctx: &LayoutContext, lines: &mut Vec<StyledLine>) {
    let lang = info.split_whitespace().next().unwrap_or("");

    // Mermaid diagrams get special rendering
    if lang == "mermaid" {
        let mermaid_lines = mermaid::render_mermaid(literal, ctx.theme, ctx.width, ctx.margin);
        lines.extend(mermaid_lines);
        return;
    }

    // Expand tabs before highlighting and width math (tab-indented Go code
    // otherwise misaligns the box borders and loses its indentation).
    let literal = expand_tabs(literal);
    let literal = literal.as_str();

    // Match the code box to the content width (was capped at 80, making code
    // blocks narrower than paragraphs on wide layouts).
    let border_width = ctx.width.max(8);
    let content_w = border_width.saturating_sub(4); // │ + space + content + space + │

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
        // ╭─ (2) + label + <remaining ─> + ╮ (1) == border_width
        let remaining = border_width.saturating_sub(label.width() + 3);
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
    let border_style = SpanStyle {
        fg: Some(ctx.theme.colors.table_border.clone()),
        ..Default::default()
    };

    for code_line in LinesWithEndings::from(literal) {
        // Build the highlighted spans for this source line.
        let mut content: Vec<StyledSpan> = Vec::new();
        if let Ok(ranges) = highlighter.highlight_line(code_line, ctx.syntax_set) {
            for (style, text) in ranges {
                let trimmed = text.trim_end_matches('\n');
                if !trimmed.is_empty() {
                    content.push(StyledSpan {
                        text: trimmed.to_string(),
                        style: SpanStyle {
                            fg: Some(format!(
                                "#{:02x}{:02x}{:02x}",
                                style.foreground.r, style.foreground.g, style.foreground.b
                            )),
                            ..Default::default()
                        },
                    });
                }
            }
        } else {
            content.push(StyledSpan {
                text: code_line.trim_end_matches('\n').to_string(),
                style: SpanStyle {
                    fg: Some(ctx.theme.colors.code_fg.clone()),
                    ..Default::default()
                },
            });
        }

        // Wrap long lines to the box interior and draw a closed right border,
        // padding each visual line so the border stays aligned.
        for visual in wrap_code_spans(content, content_w) {
            let mut line = StyledLine::new();
            ctx.add_margin(&mut line);
            line.push(StyledSpan {
                text: "│ ".to_string(),
                style: border_style.clone(),
            });
            let mut w = 0;
            for s in &visual {
                w += s.text.width();
                line.push(s.clone());
            }
            let pad = content_w.saturating_sub(w);
            line.push(StyledSpan {
                text: format!("{} │", " ".repeat(pad)),
                style: border_style.clone(),
            });
            lines.push(line);
        }
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
        image_specs: ctx.image_specs,
        graphics_font: ctx.graphics_font,
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
    trim_trailing_blank(&mut inner_lines);
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

    // Spacing after the quote so consecutive blockquotes/admonitions and a
    // following block don't run together. Only at the top level — nested calls
    // build into a buffer the parent trims.
    if depth == 0 {
        add_spacing(ctx, lines);
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
        image_specs: ctx.image_specs,
        graphics_font: ctx.graphics_font,
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

        trim_trailing_blank(&mut item_lines);
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
                // No literal padding spaces: they produced doubled spaces before
                // the code and a stray space before following punctuation. The
                // background color alone marks the inline code.
                spans.push(StyledSpan {
                    text: c.literal.clone(),
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
            NodeValue::Math(ref math) => {
                // No LaTeX rendering in a terminal; show the literal in code
                // style so `$E=mc^2$` reads as math rather than vanishing.
                spans.push(StyledSpan {
                    text: math.literal.clone(),
                    style: SpanStyle {
                        fg: Some(ctx.theme.colors.code_fg.clone()),
                        italic: true,
                        ..parent_style.clone()
                    },
                });
            }
            NodeValue::ShortCode(ref sc) => {
                // comrak resolves known emoji shortcodes to their glyph.
                spans.push(StyledSpan {
                    text: sc.emoji.clone(),
                    style: parent_style.clone(),
                });
            }
            NodeValue::HtmlInline(ref html) => {
                // Honor common inline tags; drop the rest instead of emitting
                // raw markup. <br> becomes a break handled by wrapping later;
                // here we just skip structural tags and keep nothing visible.
                // A mid-text <img> can't render as a pixel block, but it must
                // stay visible — show the standard inline placeholder.
                if let Some((src, alt)) = extract_html_images(html).into_iter().next() {
                    let label = if alt.is_empty() { src } else { alt };
                    spans.push(StyledSpan {
                        text: format!("🖼 {label}"),
                        style: SpanStyle {
                            fg: Some(ctx.theme.colors.link.clone()),
                            ..parent_style.clone()
                        },
                    });
                    continue;
                }
                let tag = html.trim().to_ascii_lowercase();
                if tag == "<br>" || tag == "<br/>" || tag == "<br />" {
                    spans.push(StyledSpan {
                        text: " ".to_string(),
                        style: parent_style.clone(),
                    });
                }
                // <sub>/<sup>/<kbd>/etc.: ignore the tag, inner text is a
                // sibling text node and renders normally.
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

                // Advance only past the link text: punctuation trimmed off the
                // URL is not part of the link, but it is part of the sentence
                // and must remain in the rendered output as plain text.
                remaining = &remaining[start + url.len()..];
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

/// Hard-wrap styled code spans to a display width, preserving every character
/// exactly (including leading indentation). Returns one span list per visual
/// line; always at least one (possibly empty) line.
///
/// Walks grapheme clusters, not chars: renderers (`str::width`, ratatui)
/// measure clusters, and per-char sums disagree for emoji with VS16 or ZWJ
/// sequences. Never splits inside a cluster.
fn wrap_code_spans(spans: Vec<StyledSpan>, width: usize) -> Vec<Vec<StyledSpan>> {
    let width = width.max(1);
    let mut lines: Vec<Vec<StyledSpan>> = Vec::new();
    let mut cur: Vec<StyledSpan> = Vec::new();
    let mut col = 0usize;
    for span in spans {
        let mut piece = String::new();
        for grapheme in span.text.graphemes(true) {
            let gw = grapheme.width();
            if col + gw > width && (col > 0 || !piece.is_empty()) {
                if !piece.is_empty() {
                    cur.push(StyledSpan {
                        text: std::mem::take(&mut piece),
                        style: span.style.clone(),
                    });
                }
                lines.push(std::mem::take(&mut cur));
                col = 0;
            }
            piece.push_str(grapheme);
            col += gw;
        }
        if !piece.is_empty() {
            cur.push(StyledSpan {
                text: piece,
                style: span.style.clone(),
            });
        }
    }
    lines.push(cur);
    lines
}

/// Split a string into pieces whose display width is at most `width`, breaking
/// only between grapheme clusters (splitting inside one would change both the
/// rendered glyphs and the measured width). Used to hard-break tokens too long
/// to wrap on a space.
fn split_by_width(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut w = 0;
    for grapheme in s.graphemes(true) {
        let gw = grapheme.width();
        if w + gw > width && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            w = 0;
        }
        cur.push_str(grapheme);
        w += gw;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
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

    // Start a fresh line carrying the margin + indent prefix.
    let new_prefixed_line = || -> StyledLine {
        let mut l = StyledLine::new();
        if margin > 0 {
            l.push(StyledSpan {
                text: margin_str.clone(),
                style: SpanStyle::default(),
            });
        }
        if indent > 0 {
            l.push(StyledSpan {
                text: indent_str.clone(),
                style: SpanStyle::default(),
            });
        }
        l
    };

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

            // A single token wider than the line can't be wrapped on a space —
            // hard-break it into width-sized chunks so it never overflows.
            if w > effective_width {
                if col > 0 {
                    lines.push(std::mem::replace(&mut current, new_prefixed_line()));
                    col = 0;
                }
                for chunk in split_by_width(word, effective_width) {
                    if col > 0 {
                        lines.push(std::mem::replace(&mut current, new_prefixed_line()));
                        col = 0;
                    }
                    let cw = chunk.width();
                    current.push(StyledSpan {
                        text: chunk,
                        style: span.style.clone(),
                    });
                    col += cw;
                }
                continue;
            }

            let need_space = col > 0 && i > 0;
            let total = col + w + if need_space { 1 } else { 0 };

            if total > effective_width && col > 0 {
                lines.push(std::mem::replace(&mut current, new_prefixed_line()));
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

    for line in &mut lines {
        coalesce_line(line);
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

    // The `<img>` scanner is deliberately tolerant (any attribute order,
    // single/double/no quotes, self-closing or not); each tolerance is
    // asserted here, since real READMEs use all of them.
    #[test]
    fn extracts_html_images_in_every_quoting_style() {
        // Double quotes, alt before src, extra attributes, self-closing.
        assert_eq!(
            extract_html_images(r#"<img alt="Logo" width="200" src="a.png" />"#),
            vec![("a.png".to_string(), "Logo".to_string())]
        );
        // Single quotes and no quotes at all.
        assert_eq!(
            extract_html_images("<img src='b.png' alt='Chart'>"),
            vec![("b.png".to_string(), "Chart".to_string())]
        );
        assert_eq!(
            extract_html_images("<img src=c.png alt=Plain>"),
            vec![("c.png".to_string(), "Plain".to_string())]
        );
        // Uppercase tag/attribute names, and a missing alt.
        assert_eq!(
            extract_html_images(r#"<IMG SRC="d.png">"#),
            vec![("d.png".to_string(), String::new())]
        );
        // Several images in one fragment, e.g. a badge row.
        assert_eq!(
            extract_html_images(r#"<p><img src="1.png"><img src="2.png"></p>"#),
            vec![
                ("1.png".to_string(), String::new()),
                ("2.png".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn html_scanner_is_quote_aware() {
        // `src=` inside another attribute's quoted value must not be read as
        // the real attribute (the old scanner loaded `here` for this input).
        assert_eq!(
            extract_html_images(r#"<img alt="pass src=here to override" src="diagram.svg">"#),
            vec![(
                "diagram.svg".to_string(),
                "pass src=here to override".to_string()
            )]
        );
        // A `>` inside a quoted value does not end the tag.
        assert_eq!(
            extract_html_images(r#"<img alt="a > b" src="x.png">"#),
            vec![("x.png".to_string(), "a > b".to_string())]
        );
    }

    #[test]
    fn expand_tabs_uses_four_column_stops() {
        assert_eq!(expand_tabs("\tx"), "    x");
        assert_eq!(expand_tabs("ab\tx"), "ab  x");
        assert_eq!(expand_tabs("abc\tx"), "abc x");
        assert_eq!(expand_tabs("abcd\tx"), "abcd    x");
        // Column resets at each newline.
        assert_eq!(expand_tabs("a\n\tb"), "a\n    b");
    }

    #[test]
    fn split_by_width_measures_grapheme_clusters() {
        // 40 warning-sign emoji: 1 column per char but 2 per cluster — the
        // per-char accounting packed 76 columns into a 40-column budget.
        let s = "⚠️".repeat(40);
        let parts = split_by_width(&s, 40);
        for p in &parts {
            assert!(p.width() <= 40, "chunk {p:?} is {} cols", p.width());
        }
        assert_eq!(parts.concat(), s, "no character may be lost");

        // ZWJ family emoji: ~6 columns per-char but 2 per cluster; a split
        // inside the cluster would leave dangling ZWJ fragments.
        let fam = "👨‍👩‍👧‍👦".repeat(5);
        for p in split_by_width(&fam, 4) {
            assert!(p.width() <= 4);
            assert_eq!(
                p.chars().filter(|c| *c == '\u{200D}').count() % 3,
                0,
                "split inside a ZWJ cluster: {p:?}"
            );
        }
    }

    #[test]
    fn wrap_code_spans_measures_grapheme_clusters() {
        let spans = vec![span(&"⚠️".repeat(30), false)];
        for visual in wrap_code_spans(spans, 36) {
            let w: usize = visual.iter().map(|s| s.text.width()).sum();
            assert!(w <= 36, "visual line is {w} cols");
        }
    }

    #[test]
    fn html_image_scanner_rejects_lookalikes() {
        // `srcset`/`data-src` must not be mistaken for `src`: the first has a
        // different name, the second is not preceded by whitespace.
        assert_eq!(
            extract_html_images(r#"<img srcset="wide.png 2x" src="real.png">"#),
            vec![("real.png".to_string(), String::new())]
        );
        assert!(extract_html_images(r#"<img data-src="lazy.png">"#).is_empty());
        // A tag with no src yields nothing, and an unterminated tag does not
        // hang or panic.
        assert!(extract_html_images("<img>").is_empty());
        assert!(extract_html_images(r#"<img src="unterminated.png"#).is_empty());
        assert!(extract_html_images("<p>no images here</p>").is_empty());
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
