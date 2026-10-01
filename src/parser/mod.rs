pub mod frontmatter;

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
    opts.extension.math_dollars = true;
    opts.extension.shortcodes = true;
    opts.parse.smart = true;
    opts
}

/// Block nesting beyond which `options_for` turns footnotes off. No real
/// document comes near it; a hostile one (20 KB of `>`) is 20000 deep.
const FOOTNOTE_NESTING_LIMIT: usize = 1000;

/// Parse options for a specific source: `options()`, except that footnotes
/// are disabled for absurdly deep block nesting. comrak 0.36 resolves
/// footnotes with a recursive walk over the whole AST (one stack frame per
/// nesting level), which overflows the stack on a few KB of `>`; ink's own
/// layout caps its recursion, but cannot reach into the parser.
pub fn options_for(source: &str) -> Options<'static> {
    let mut opts = options();
    if max_block_nesting(source) > FOOTNOTE_NESTING_LIMIT {
        opts.extension.footnotes = false;
    }
    opts
}

/// Cheap upper bound on AST nesting, per line: the blockquote and list
/// markers that open it, one level per two columns of indentation, and every
/// character that can open an inline container (emphasis, strikethrough,
/// link/image brackets).
fn max_block_nesting(source: &str) -> usize {
    source
        .lines()
        .map(|line| {
            let bytes = line.as_bytes();
            let (mut depth, mut indent) = (0, 0);
            for (i, &b) in bytes.iter().enumerate() {
                match b {
                    b' ' => indent += 1,
                    b'\t' => indent += 4,
                    b'>' => depth += 1,
                    b'-' | b'*' | b'+' if bytes.get(i + 1).is_none_or(|b| *b == b' ') => depth += 1,
                    b'0'..=b'9' | b'.' | b')' => {}
                    _ => break,
                }
            }
            let inline = bytes
                .iter()
                .filter(|b| matches!(b, b'*' | b'_' | b'~' | b'['))
                .count();
            depth + indent / 2 + inline
        })
        .max()
        .unwrap_or(0)
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
            format!("{}x", "- ".repeat(5000)),
            format!("{}x{}", "*".repeat(20000), "*".repeat(20000)),
        ] {
            assert!(!options_for(&src).extension.footnotes);
        }
    }
}
