pub mod frontmatter;
pub mod line_range;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena, Options};

/// Parse markdown source into a comrak AST.
pub fn parse(source: &str) -> ParsedDocument {
    let arena = Arena::new();
    let options = options_for(source);
    let root = parse_document(&arena, source, &options);
    let headings = extract_headings(root);
    ParsedDocument {
        source: source.to_string(),
        headings,
    }
}

/// Parsed markdown document with extracted metadata.
#[allow(dead_code)]
pub struct ParsedDocument {
    pub source: String,
    pub headings: Vec<Heading>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub byte_offset: usize,
}

pub fn options() -> Options<'static> {
    let mut opts = Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    opts.extension.footnotes = true;
    opts.extension.header_ids = Some(String::new());
    // `$…$`, `$$…$$` (GitHub's rules: `$5 and $10` is not math), and
    // `` $`…`$ `` / ```` ```math ````.
    opts.extension.math_dollars = true;
    opts.extension.math_code = true;
    // Definition lists (`Term` then `: definition`). Not `superscript`
    // (`^x^`) or `spoiler` (`||x||`): neither is GitHub markdown, and both
    // pair up ordinary prose (`2^10 and 3^5`, `(a||b) or (c||d)`);
    // `<sup>`/`<sub>` still work. Not `subscript`: it turns GitHub's
    // single-tilde `~strike~` into subscript. Not `underline`: it turns
    // `__bold__` into underlined text.
    opts.extension.description_lists = true;
    // In ASCII mode, an ASCII document must stay ASCII: no `:smile:` → emoji
    // and no smart quotes, dashes or ellipses.
    let ascii = crate::glyphs::current().ascii;
    opts.extension.shortcodes = !ascii;
    // `[[target]]` / `[[target|label]]`, parsed where inline markdown is
    // (never inside code or raw HTML); see `crate::wikilink`.
    opts.extension.wikilinks_title_after_pipe = true;
    opts.parse.smart = !ascii;
    opts
}

/// AST depth beyond which `options_for` turns footnotes off. comrak 0.36
/// resolves footnotes with a recursive walk over the whole AST, one stack
/// frame per nesting level; everything else it does is iterative. Measured
/// on a 1 MB stack in a debug build, that walk overflows at a depth of about
/// 1940 — whether the levels are blockquotes, list items or emphasis — so
/// this keeps a margin of over a third. No real document comes near it; a
/// hostile one (20 KB of `>`) is 20000 deep.
const FOOTNOTE_DEPTH_LIMIT: usize = 1200;

/// Parse options for a specific source: `options()`, except that footnotes
/// are disabled where comrak's footnote pass could overflow the stack (see
/// [`FOOTNOTE_DEPTH_LIMIT`]); ink's own layout caps its recursion, but cannot
/// reach into the parser.
///
/// The depth is measured, not guessed from characters: what nests is
/// container blocks and emphasis, and emphasis openers need not be adjacent
/// (`*a _*a _…`) or on one line, while long lines in code blocks or tables
/// full of links nest nothing. Without footnotes the parse is iterative at
/// any depth, so a probe parse without them finds the real depth. It only
/// runs when footnotes could matter (`[^` occurs) and the document has
/// enough nesting-capable characters to possibly reach the limit.
pub fn options_for(source: &str) -> Options<'static> {
    let mut opts = options();
    if !source.contains("[^") {
        // No footnote syntax: the extension changes nothing but the pass.
        opts.extension.footnotes = false;
    } else if nesting_bound(source) > FOOTNOTE_DEPTH_LIMIT {
        let mut probe = opts.clone();
        probe.extension.footnotes = false;
        let arena = Arena::new();
        if ast_depth(parse_document(&arena, source, &probe)) > FOOTNOTE_DEPTH_LIMIT {
            opts.extension.footnotes = false;
        }
    }
    opts
}

/// Upper bound on AST depth: every nesting level is opened by at least one
/// of these bytes (`>` blockquotes; `-` `*` `+` `.` `)` list markers; `*`
/// `_` `~` emphasis and strikethrough; `:` definition lists; `[` links,
/// images, footnotes), plus the
/// few fixed levels (document, paragraph, text).
fn nesting_bound(source: &str) -> usize {
    8 + source
        .bytes()
        .filter(|b| b"<>*_~:[-+.)".contains(b))
        .count()
}

/// Depth of the deepest node, walked without recursion.
fn ast_depth<'a>(root: &'a AstNode<'a>) -> usize {
    use comrak::arena_tree::NodeEdge;
    let (mut depth, mut max) = (0usize, 0usize);
    for edge in root.traverse() {
        match edge {
            NodeEdge::Start(_) => {
                depth += 1;
                max = max.max(depth);
            }
            NodeEdge::End(_) => depth -= 1,
        }
    }
    max
}

pub fn extract_headings_from_ast<'a>(root: &'a AstNode<'a>) -> Vec<Heading> {
    extract_headings(root)
}

fn extract_headings<'a>(root: &'a AstNode<'a>) -> Vec<Heading> {
    // Iterative (`descendants` walks with an explicit cursor, not recursion):
    // a document can nest blocks thousands deep, and a recursive walk
    // overflows the stack on it.
    let mut headings = Vec::new();
    for node in root.descendants() {
        let data = node.data.borrow();
        if let NodeValue::Heading(ref h) = data.value {
            let level = h.level;
            let byte_offset = data.sourcepos.start.line;
            drop(data);
            headings.push(Heading {
                level,
                text: collect_text(node),
                byte_offset,
            });
        }
    }
    headings
}

fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    for n in node.descendants() {
        let data = n.data.borrow();
        if let NodeValue::Text(ref t) = data.value {
            text.push_str(t);
        } else if let NodeValue::Code(ref c) = data.value {
            text.push_str(&c.literal);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "\n\nText[^1].\n\n[^1]: note\n";

    #[test]
    fn footnotes_stay_on_for_ordinary_documents() {
        let src = "> quote\n> > nested\n\n- a\n  - b\n    1. c\n\nText[^1].\n\n[^1]: note\n";
        assert!(options_for(src).extension.footnotes);
    }

    #[test]
    fn footnotes_go_off_for_hostile_nesting() {
        for src in [
            format!("{} x", ">".repeat(20000)),
            format!("{}x", "> ".repeat(20000)),
            format!("{}x{}", "*".repeat(20000), "*".repeat(20000)),
            // Openers need not be adjacent to nest.
            format!("{}x{}", "*a _".repeat(2000), "_ a*".repeat(2000)),
        ] {
            let src = src + NOTE;
            let arena = Arena::new();
            let mut probe = options();
            probe.extension.footnotes = false;
            let depth = ast_depth(parse_document(&arena, &src, &probe));
            assert!(
                !options_for(&src).extension.footnotes,
                "{:?}… depth {depth}",
                &src[..12]
            );
        }
    }

    #[test]
    fn footnotes_survive_long_lines_that_nest_nothing() {
        let code = format!("```\n{}\n```{NOTE}", "_[*~".repeat(1250));
        assert!(options_for(&code).extension.footnotes, "fenced code");
        let row: String = (0..300).map(|i| format!("[l{i}](u{i}) ")).collect();
        let table = format!("| a | b |\n|---|---|\n| {row} | x |{NOTE}");
        assert!(options_for(&table).extension.footnotes, "wide table row");
        let prose = format!("{}{NOTE}", "*a* _b_ ~c~ [d](e). ".repeat(400));
        assert!(options_for(&prose).extension.footnotes, "long prose");
        // comrak stops list nesting at a fixed depth, far below the limit.
        let lists = format!("{}x{NOTE}", "- ".repeat(5000));
        assert!(
            options_for(&lists).extension.footnotes,
            "capped list nesting"
        );
    }

    /// Just under the limit, footnotes stay on and comrak's recursive
    /// footnote pass still fits a 1 MB stack (debug build) with room left.
    #[test]
    fn the_limit_leaves_a_margin_on_a_small_stack() {
        for src in [
            format!("{} deep", ">".repeat(FOOTNOTE_DEPTH_LIMIT - 10)),
            format!(
                "{}deep{}",
                "*".repeat(2 * FOOTNOTE_DEPTH_LIMIT - 20),
                "*".repeat(2 * FOOTNOTE_DEPTH_LIMIT - 20)
            ),
        ] {
            let src = src + NOTE;
            std::thread::Builder::new()
                .stack_size(1 << 20)
                .spawn(move || {
                    let opts = options_for(&src);
                    assert!(opts.extension.footnotes);
                    let arena = Arena::new();
                    let root = parse_document(&arena, &src, &opts);
                    assert!(ast_depth(root) > FOOTNOTE_DEPTH_LIMIT - 20);
                })
                .unwrap()
                .join()
                .expect("footnote pass overflowed below the limit");
        }
    }
}
