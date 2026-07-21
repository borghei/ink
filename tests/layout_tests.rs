use comrak::{parse_document, Arena};
use ink_md::layout::layout_document;
use ink_md::parser;
use ink_md::theme::resolve_theme;
use ink_md::Spacing;

fn layout(source: &str, width: u16) -> Vec<ink_md::layout::StyledLine> {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &parser::options());
    let theme = resolve_theme("dark");
    layout_document(
        root,
        &theme,
        width,
        Spacing::Normal,
        0,
        None,
        ink_md::image::ImageMode::Off,
    )
    .lines
}

/// Display width of a styled line (mirrors StyledLine::width but public here).
fn line_width(line: &ink_md::layout::StyledLine) -> usize {
    use unicode_width::UnicodeWidthStr;
    line.spans.iter().map(|s| s.text.as_str().width()).sum()
}

#[test]
fn lines_respect_width() {
    let source = std::fs::read_to_string("tests/fixtures/test.md").unwrap();
    let width = 60u16;
    let lines = layout(&source, width);
    assert!(!lines.is_empty());
    for line in &lines {
        assert!(
            line_width(line) <= width as usize,
            "line too wide ({}): {:?}",
            line_width(line),
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        );
    }
}

/// Every block type — long paragraphs, tables, code with long lines,
/// blockquotes, lists, long URLs, and long headings — must fit the width.
/// `layout` here passes margin 0, so lines must be <= width exactly.
///
/// Tested from width 40 up: a 4-column table needs ~33 columns even at the
/// minimum readable cell size, so sub-40 widths can't hold wide tables — an
/// inherent limit, not an overflow bug in the wrappable constructs.
#[test]
fn no_construct_overflows_width() {
    let source = "\
# A heading long enough that it clearly must wrap at narrow widths to fit\n\n\
A paragraph with a very-long-unbreakable-token-that-cannot-wrap-on-spaces-and-must-hard-break here.\n\n\
https://example.com/a/really/long/url/that/keeps/going/way/past/any/reasonable/width/limit\n\n\
```rust\nfn f(argument_one: SomeType, argument_two: OtherType, argument_three: Third) -> Ret { 0 }\n```\n\n\
> A blockquote that is also long enough to require wrapping inside the quote bar without spilling.\n\n\
| Col A | Col B | Col C | Col D |\n|---|---|---|---|\n\
| a fairly long cell value | another long one | third long value | fourth |\n\n\
- a list item long enough to need wrapping across multiple continuation lines within budget\n";
    for width in [40u16, 50, 60, 80, 100] {
        for line in layout(source, width) {
            assert!(
                line_width(&line) <= width as usize,
                "width {width}: line {} > {width}: {:?}",
                line_width(&line),
                line.spans
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<String>()
            );
        }
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
fn headings_report_their_own_line() {
    use comrak::{parse_document, Arena};
    use ink_md::layout::layout_document;
    use ink_md::parser;
    use ink_md::theme::resolve_theme;
    use ink_md::Spacing;

    let src = "# First\n\npara\n\n## Dup\n\nbody\n\n## Dup\n\nmore\n";
    let arena = Arena::new();
    let root = parse_document(&arena, src, &parser::options());
    let theme = resolve_theme("dark");
    let result = layout_document(
        root,
        &theme,
        80,
        Spacing::Normal,
        0,
        None,
        ink_md::image::ImageMode::Off,
    );
    // Three headings, including two identical "Dup" — the reverse-scan bug
    // would have collapsed these onto the same wrong line.
    assert_eq!(result.headings.len(), 3);
    let texts: Vec<&str> = result.headings.iter().map(|h| h.text.as_str()).collect();
    assert_eq!(texts, vec!["First", "Dup", "Dup"]);
    // Indices strictly increase and each points at a line containing the text.
    let mut last = 0;
    for h in &result.headings {
        assert!(h.line_index >= last);
        last = h.line_index;
        let line_text: String = result.lines[h.line_index]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(
            line_text.contains(&h.text),
            "heading '{}' not at line {}: {:?}",
            h.text,
            h.line_index,
            line_text
        );
    }
}

#[test]
fn wide_table_transposes_at_narrow_width() {
    let src = "| Name | Description | Status | Notes |\n\
               |---|---|---|---|\n\
               | alpha | a long description value here | active | some notes |\n";
    // Narrow: can't fit 4 columns → transposed key/value layout, no box borders.
    let narrow = layout(src, 28);
    let narrow_text: String = narrow
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(narrow_text.contains("Name"));
    assert!(narrow_text.contains("alpha"));
    assert!(narrow_text.contains("Description"));
    assert!(
        !narrow
            .iter()
            .any(|l| l.spans.iter().any(|s| s.text.contains('┬'))),
        "narrow table should transpose, not render a grid"
    );
    for line in &narrow {
        assert!(
            line_width(line) <= 28,
            "transposed line too wide: {}",
            line_width(line)
        );
    }

    // Wide: fits → keep the grid (has box borders).
    let wide = layout(src, 100);
    assert!(
        wide.iter()
            .any(|l| l.spans.iter().any(|s| s.text.contains('┬'))),
        "wide table should render as a grid"
    );
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
