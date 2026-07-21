pub mod frontmatter;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena, Options};

/// Parse markdown source into a comrak AST.
pub fn parse(source: &str) -> ParsedDocument {
    let arena = Arena::new();
    let options = options();
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

pub fn extract_headings_from_ast<'a>(root: &'a AstNode<'a>) -> Vec<Heading> {
    extract_headings(root)
}

fn extract_headings<'a>(root: &'a AstNode<'a>) -> Vec<Heading> {
    let mut headings = Vec::new();
    collect_headings(root, &mut headings);
    headings
}

fn collect_headings<'a>(node: &'a AstNode<'a>, headings: &mut Vec<Heading>) {
    let data = node.data.borrow();
    if let NodeValue::Heading(ref h) = data.value {
        let text = collect_text(node);
        headings.push(Heading {
            level: h.level,
            text,
            byte_offset: data.sourcepos.start.line,
        });
    }
    drop(data);
    for child in node.children() {
        collect_headings(child, headings);
    }
}

fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    collect_text_inner(node, &mut text);
    text
}

fn collect_text_inner<'a>(node: &'a AstNode<'a>, buf: &mut String) {
    let data = node.data.borrow();
    if let NodeValue::Text(ref t) = data.value {
        buf.push_str(t);
    } else if let NodeValue::Code(ref c) = data.value {
        buf.push_str(&c.literal);
    }
    drop(data);
    for child in node.children() {
        collect_text_inner(child, buf);
    }
}
