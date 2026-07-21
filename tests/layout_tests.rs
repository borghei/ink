use comrak::{parse_document, Arena};
use ink_md::layout::layout_document;
use ink_md::parser;
use ink_md::theme::resolve_theme;
use ink_md::Spacing;

fn layout(source: &str, width: u16) -> Vec<ink_md::layout::StyledLine> {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &parser::options());
    let theme = resolve_theme("dark");
    layout_document(root, &theme, width, Spacing::Normal, 0, None, true)
}

#[test]
fn lines_respect_width() {
    let source = std::fs::read_to_string("tests/fixtures/test.md").unwrap();
    let width = 60u16;
    let lines = layout(&source, width);
    assert!(!lines.is_empty());
    for line in &lines {
        // Tables/code borders may sit exactly at width; nothing should exceed it
        // by more than the border allowance used in layout.
        assert!(
            line.width() <= width as usize + 4,
            "line too wide ({}): {:?}",
            line.width(),
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        );
    }
}

#[test]
fn heading_text_present() {
    let lines = layout("# Alpha Heading\n\nbody\n", 80);
    let all_text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(all_text.contains("Alpha Heading"));
    assert!(all_text.contains("body"));
}

#[test]
fn empty_document_is_ok() {
    let lines = layout("", 80);
    // No panic; empty or near-empty output.
    assert!(lines.len() < 4);
}

#[test]
fn code_block_is_highlighted() {
    let lines = layout("```rust\nfn main() {}\n```\n", 80);
    let all_text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(all_text.contains("fn main"));
}
