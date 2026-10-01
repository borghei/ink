//! Raw HTML embedded in markdown.
//!
//! READMEs wrap their headers in HTML (`<p align="center"><img …></p>`,
//! `<h1 align="center">`), fold sections into `<details>`, and sprinkle
//! `<kbd>`/`<sub>`/`<br>` through prose. A terminal cannot render any of that
//! markup, and printing it raw is noise, so this module runs a small,
//! tolerant, dependency-free pass over HTML fragments: well-formed tags are
//! mapped onto ink's own blocks and inline styles (or stripped), their text
//! is kept, comments and `<script>`/`<style>` elements are dropped. Anything
//! that does not scan as a tag (`a < b`, an unterminated `<img`) stays literal
//! text — the pass never fails and never panics.
//!
//! Documents are untrusted. Everything produced here lands in `StyledLine`s
//! that `sanitize_lines` cleans (control bytes stripped, link destinations
//! run through `sanitize_url`, exactly as for markdown links); heading text
//! recorded for the TOC is sanitized where it is recorded.

use super::{
    add_spacing, image_block_lines, layout_code_block, layout_heading_spans, layout_hr,
    scan_tag_attrs, wrap_spans, LayoutContext, SpanStyle, StyledLine, StyledSpan,
};
use crate::image::ImageMode;
use std::borrow::Cow;

/// True when an inline HTML fragment is a line-break tag: `<br>`, `<br/>`,
/// `<br />`, in any letter case.
pub fn is_br_tag(html: &str) -> bool {
    let tag = html.trim().to_ascii_lowercase();
    tag == "<br>" || tag == "<br/>" || tag == "<br />"
}

/// One piece of an HTML fragment.
#[derive(Debug, PartialEq)]
pub(super) enum Token<'a> {
    /// Raw text between tags, entities not yet decoded.
    Text(&'a str),
    /// An opening (or void / self-closing) tag; `pos` is the byte offset of
    /// its `<` in the fragment. Names are ASCII-lowercased.
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        pos: usize,
    },
    Close {
        name: String,
    },
}

/// Split an HTML fragment into text and tags. Comments, `<!…>` declarations
/// and `<?…?>` instructions are dropped. A `<script>`/`<style>` element is
/// dropped with its content; an unclosed one yields an `Open` token so the
/// caller can hide what follows (inline HTML arrives one tag per node).
/// A `<` that does not begin a well-formed tag is kept as text.
pub(super) fn tokenize(html: &str) -> Vec<Token<'_>> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let mut text_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &html[i..];
        let skip_to = if let Some(body) = rest.strip_prefix("<!--") {
            // An unterminated comment runs to the end, as in a browser.
            Some(body.find("-->").map_or(html.len(), |e| i + 4 + e + 3))
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            rest.find('>').map(|e| i + e + 1)
        } else {
            None
        };
        if let Some(end) = skip_to {
            if text_start < i {
                out.push(Token::Text(&html[text_start..i]));
            }
            i = end;
            text_start = end;
            continue;
        }

        let closing = bytes.get(i + 1) == Some(&b'/');
        let name_at = i + 1 + usize::from(closing);
        if !bytes.get(name_at).is_some_and(u8::is_ascii_alphabetic) {
            i += 1;
            continue;
        }
        let name_len = html[name_at..]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
            .count();
        let body = name_at + name_len;
        // Require a real tag boundary so `<imgs` is not `<img`.
        let at_boundary = html[body..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_whitespace() || c == '>' || c == '/');
        let scanned = if at_boundary {
            scan_tag_attrs(&html[body..])
        } else {
            None
        };
        let Some((attrs, consumed)) = scanned else {
            // Not a tag (or one that never closes): literal text.
            i += 1;
            continue;
        };
        if text_start < i {
            out.push(Token::Text(&html[text_start..i]));
        }
        let name = html[name_at..body].to_ascii_lowercase();
        let pos = i;
        let end = body + consumed;
        i = end;
        if closing {
            out.push(Token::Close { name });
        } else if name == "script" || name == "style" {
            // ASCII lowercasing keeps byte offsets, so `lower` indexes `html`.
            let lower = html[end..].to_ascii_lowercase();
            match lower.find(&format!("</{name}")) {
                Some(off) => {
                    let close = end + off;
                    i = html[close..]
                        .find('>')
                        .map_or(html.len(), |g| close + g + 1);
                }
                None => out.push(Token::Open { name, attrs, pos }),
            }
        } else {
            out.push(Token::Open { name, attrs, pos });
        }
        text_start = i;
    }
    if text_start < html.len() {
        out.push(Token::Text(&html[text_start..]));
    }
    out
}

/// Decode the character references READMEs actually use: `&amp; &lt; &gt;
/// &quot; &apos; &nbsp;` and numeric `&#NN;` / `&#xHH;`. Unknown or malformed
/// references stay literal. A decoded control character is left for the
/// sanitizer to strip, like any other.
pub(super) fn decode_entities(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        // References are short; bound the `;` search so a text full of bare
        // ampersands stays linear.
        let decoded = after
            .char_indices()
            .take(12)
            .find(|&(_, c)| c == ';')
            .and_then(|(semi, _)| decode_entity(&after[..semi]).map(|c| (c, semi)));
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn decode_entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse::<u32>().ok()?,
            };
            Some(
                char::from_u32(code)
                    .filter(|&c| c != '\0')
                    .unwrap_or('\u{fffd}'),
            )
        }
    }
}

/// HTML text rendering: entities decoded, then every whitespace run (source
/// newlines included) collapsed to one space.
fn html_text(raw: &str) -> String {
    let decoded = decode_entities(raw);
    let mut out = String::with_capacity(decoded.len());
    let mut in_ws = false;
    for c in decoded.chars() {
        if c.is_ascii_whitespace() {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// `(src, alt)` of an `<img>` tag's attributes, entities decoded; `None`
/// without a `src`.
pub(super) fn img_src_alt(attrs: &[(String, String)]) -> Option<(String, String)> {
    let attr = |name: &str| {
        attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| decode_entities(v).into_owned())
    };
    Some((attr("src")?, attr("alt").unwrap_or_default()))
}

/// The inline-style tags that are open at this point of a fragment (or of a
/// run of inline HTML siblings), innermost last.
#[derive(Default)]
pub(super) struct HtmlStyles {
    /// (tag name, style inside it, hides its content)
    stack: Vec<(String, SpanStyle, bool)>,
}

impl HtmlStyles {
    /// The style text gets here: the innermost open tag's, else `base`.
    pub(super) fn style<'s>(&'s self, base: &'s SpanStyle) -> &'s SpanStyle {
        self.stack.last().map_or(base, |(_, s, _)| s)
    }

    /// Inside an unclosed `<script>`/`<style>`: content is not shown.
    pub(super) fn hidden(&self) -> bool {
        self.stack.iter().any(|(_, _, hide)| *hide)
    }

    /// Track an opening tag. Only tags that change how text looks (or hide
    /// it) are tracked; the rest are simply stripped.
    fn open(
        &mut self,
        name: &str,
        attrs: &[(String, String)],
        base: &SpanStyle,
        ctx: &LayoutContext,
    ) {
        let cur = self.style(base).clone();
        let colors = &ctx.theme.colors;
        let (style, hide) = match name {
            "b" | "strong" | "summary" => (
                SpanStyle {
                    bold: true,
                    fg: Some(colors.bold.clone()),
                    ..cur
                },
                false,
            ),
            "i" | "em" | "cite" | "var" => (
                SpanStyle {
                    italic: true,
                    ..cur
                },
                false,
            ),
            "code" | "kbd" | "tt" | "samp" => (
                SpanStyle {
                    fg: Some(colors.code_fg.clone()),
                    bg: Some(colors.code_bg.clone()),
                    ..cur
                },
                false,
            ),
            "s" | "del" | "strike" => (
                SpanStyle {
                    strikethrough: true,
                    fg: Some(colors.strikethrough.clone()),
                    ..cur
                },
                false,
            ),
            "a" => {
                let Some((_, href)) = attrs.iter().find(|(n, _)| n == "href") else {
                    return;
                };
                // Same treatment as a markdown link: the destination rides on
                // the span and `sanitize_lines` runs it through `sanitize_url`
                // (unsafe schemes and embedded controls are dropped there).
                (
                    SpanStyle {
                        fg: Some(colors.link.clone()),
                        underline: true,
                        link_url: Some(decode_entities(href).into_owned()),
                        ..cur
                    },
                    false,
                )
            }
            "script" | "style" => (cur, true),
            _ => return,
        };
        self.stack.push((name.to_string(), style, hide));
    }

    /// Close the innermost open tag of this name (and anything left open
    /// inside it). A stray closing tag is ignored.
    fn close(&mut self, name: &str) {
        if let Some(idx) = self.stack.iter().rposition(|(n, _, _)| n == name) {
            self.stack.truncate(idx);
        }
    }
}

/// Apply one inline HTML node (comrak emits each tag as its own node, with the
/// enclosed text as sibling nodes) to the running style state, pushing any
/// visible output.
pub(super) fn inline_fragment(
    fragment: &str,
    ctx: &LayoutContext,
    styles: &mut HtmlStyles,
    base: &SpanStyle,
    spans: &mut Vec<StyledSpan>,
) {
    for token in tokenize(fragment) {
        match token {
            Token::Text(raw) => {
                if !styles.hidden() {
                    spans.push(StyledSpan {
                        text: html_text(raw),
                        style: styles.style(base).clone(),
                    });
                }
            }
            Token::Open { name, attrs, .. } => match name.as_str() {
                "br" => spans.push(StyledSpan {
                    text: "\n".to_string(),
                    style: styles.style(base).clone(),
                }),
                // A mid-text <img> can't render as a pixel block, but it must
                // stay visible — show the standard inline placeholder.
                "img" => {
                    if let Some((src, alt)) = img_src_alt(&attrs) {
                        spans.push(image_placeholder_span(&src, &alt, styles.style(base), ctx));
                    }
                }
                _ => styles.open(&name, &attrs, base, ctx),
            },
            Token::Close { name } => styles.close(&name),
        }
    }
}

fn image_placeholder_span(
    src: &str,
    alt: &str,
    style: &SpanStyle,
    ctx: &LayoutContext,
) -> StyledSpan {
    let label = if alt.is_empty() { src } else { alt };
    StyledSpan {
        text: format!("🖼 {label}"),
        style: SpanStyle {
            fg: Some(ctx.theme.colors.link.clone()),
            ..style.clone()
        },
    }
}

/// Tags that break the text flow: their content becomes its own paragraph.
fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "center"
            | "details"
            | "summary"
            | "picture"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "nav"
            | "aside"
            | "main"
            | "figure"
            | "figcaption"
            | "blockquote"
            | "address"
            | "ul"
            | "ol"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "caption"
            | "form"
            | "fieldset"
    )
}

fn heading_level(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Lay out an HTML block. `first_line` is the 1-based source line the block
/// starts on (used to give `<hN>` headings a source line for the TOC).
pub(super) fn layout_html_block(
    literal: &str,
    first_line: usize,
    ctx: &LayoutContext,
    lines: &mut Vec<StyledLine>,
) {
    let base = SpanStyle::default();
    let mut block = HtmlBlock {
        ctx,
        para: Vec::new(),
        heading: None,
        image_gap: false,
    };
    let mut styles = HtmlStyles::default();
    // Inside `<pre>`: raw text, whitespace kept, rendered as a code block.
    let mut pre: Option<String> = None;

    for token in tokenize(literal) {
        if let Some(buf) = pre.as_mut() {
            match token {
                Token::Close { name } if name == "pre" => {
                    let code = std::mem::take(buf);
                    pre = None;
                    block.code(&code, lines);
                }
                Token::Text(raw) => buf.push_str(&decode_entities(raw)),
                Token::Open { name, .. } if name == "br" => buf.push('\n'),
                _ => {}
            }
            continue;
        }
        match token {
            Token::Text(raw) => {
                if !styles.hidden() {
                    block.para.push(StyledSpan {
                        text: html_text(raw),
                        style: styles.style(&base).clone(),
                    });
                }
            }
            Token::Open { name, attrs, pos } => {
                match name.as_str() {
                    "br" => block.para.push(StyledSpan {
                        text: "\n".to_string(),
                        style: styles.style(&base).clone(),
                    }),
                    "hr" => {
                        block.flush(lines);
                        layout_hr(ctx, lines);
                    }
                    "img" => {
                        if let Some((src, alt)) = img_src_alt(&attrs) {
                            block.image(&src, &alt, styles.style(&base), lines);
                        }
                    }
                    "pre" => {
                        block.flush(lines);
                        pre = Some(String::new());
                    }
                    "td" | "th" => block.para.push(StyledSpan {
                        text: " ".to_string(),
                        style: SpanStyle::default(),
                    }),
                    n => {
                        if let Some(level) = heading_level(n) {
                            block.flush(lines);
                            let line = first_line + literal[..pos].matches('\n').count();
                            block.heading = Some((level, line));
                        } else if is_block_tag(n) {
                            block.flush(lines);
                        }
                    }
                }
                styles.open(&name, &attrs, &base, ctx);
            }
            Token::Close { name } => {
                styles.close(&name);
                if is_block_tag(&name) || heading_level(&name).is_some() {
                    block.flush(lines);
                }
            }
        }
    }
    if let Some(code) = pre {
        block.code(&code, lines);
    }
    block.flush(lines);
    if block.image_gap {
        add_spacing(ctx, lines);
    }
}

struct HtmlBlock<'c, 'a> {
    ctx: &'c LayoutContext<'a>,
    /// Inline content of the paragraph (or heading) being built.
    para: Vec<StyledSpan>,
    /// `Some((level, source_line))` while inside `<h1>`..`<h6>`.
    heading: Option<(u8, usize)>,
    /// Images were just emitted: separate them from what follows.
    image_gap: bool,
}

impl HtmlBlock<'_, '_> {
    /// Emit the pending paragraph or heading, if it has any visible text.
    fn flush(&mut self, lines: &mut Vec<StyledLine>) {
        let mut spans = std::mem::take(&mut self.para);
        let heading = self.heading.take();
        // Leading/trailing whitespace (including stray breaks) is markup
        // indentation, not content.
        while spans.first().is_some_and(|s| s.text.trim().is_empty()) {
            spans.remove(0);
        }
        while spans.last().is_some_and(|s| s.text.trim().is_empty()) {
            spans.pop();
        }
        let Some(first) = spans.first_mut() else {
            return;
        };
        first.text = first.text.trim_start().to_string();
        if let Some(last) = spans.last_mut() {
            last.text = last.text.trim_end().to_string();
        }
        self.end_image_gap(lines);
        match heading {
            Some((level, source_line)) => {
                let text: String = spans.iter().map(|s| s.text.replace('\n', " ")).collect();
                layout_heading_spans(level, spans, &text, source_line, self.ctx, lines);
            }
            None => {
                let ctx = self.ctx;
                lines.extend(wrap_spans(spans, ctx.width, ctx.indent, ctx.margin));
                add_spacing(ctx, lines);
            }
        }
    }

    fn end_image_gap(&mut self, lines: &mut Vec<StyledLine>) {
        if std::mem::take(&mut self.image_gap) {
            add_spacing(self.ctx, lines);
        }
    }

    /// An `<img>`: a block image (the same path markdown images take), or —
    /// with images off — the inline placeholder markdown images get then.
    fn image(&mut self, src: &str, alt: &str, style: &SpanStyle, lines: &mut Vec<StyledLine>) {
        if self.ctx.images == ImageMode::Off {
            self.para
                .push(image_placeholder_span(src, alt, style, self.ctx));
            return;
        }
        self.flush(lines);
        let link = style.link_url.as_deref();
        let block = image_block_lines(src, alt, link, self.ctx, lines.len());
        lines.extend(block);
        self.image_gap = true;
    }

    /// A `<pre>` element: a code block, so it keeps its whitespace and can be
    /// copied with `c` like a fenced block.
    fn code(&mut self, text: &str, lines: &mut Vec<StyledLine>) {
        self.flush(lines);
        // Per HTML, a newline right after `<pre>` is not content.
        let text = text
            .strip_prefix("\r\n")
            .or_else(|| text.strip_prefix('\n'))
            .unwrap_or(text);
        if text.trim().is_empty() {
            return;
        }
        self.end_image_gap(lines);
        layout_code_block("", text, self.ctx, lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_basic_and_numeric_entities() {
        assert_eq!(
            decode_entities("a &amp; b &lt;c&gt; &quot;q&quot; &#39;s&apos; x&nbsp;y"),
            "a & b <c> \"q\" 's' x\u{a0}y"
        );
        assert_eq!(decode_entities("&#65;&#x42;&#X43;"), "ABC");
        // Unknown, malformed and out-of-range references.
        assert_eq!(
            decode_entities("&bogus; & &#; &#x; AT&T"),
            "&bogus; & &#; &#x; AT&T"
        );
        assert_eq!(decode_entities("&#xD800;&#0;"), "\u{fffd}\u{fffd}");
    }

    #[test]
    fn malformed_markup_stays_literal() {
        assert_eq!(tokenize("a < b"), vec![Token::Text("a < b")]);
        assert_eq!(
            tokenize("x <img src=\"y"),
            vec![Token::Text("x <img src=\"y")]
        );
        assert_eq!(tokenize("<"), vec![Token::Text("<")]);
        assert_eq!(tokenize("1 <2> 3"), vec![Token::Text("1 <2> 3")]);
        // `<imgs>` is a tag, just not an image tag.
        assert!(matches!(&tokenize("<imgs>")[..], [Token::Open { name, .. }] if name == "imgs"));
    }

    #[test]
    fn comments_and_script_elements_vanish() {
        assert_eq!(tokenize("<!-- hi -->"), vec![]);
        assert_eq!(tokenize("a<!-- open"), vec![Token::Text("a")]);
        assert_eq!(
            tokenize("x<script>alert('<b>')</script>y<STYLE>p{}</style>z"),
            vec![Token::Text("x"), Token::Text("y"), Token::Text("z")]
        );
    }
}
