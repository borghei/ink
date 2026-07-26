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
        None,
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

/// 40 warning-sign emoji (VS16 clusters: 1 column per char, 2 per cluster)
/// must stay within a 40-column budget — per-char width accounting rendered
/// 76 columns here.
#[test]
fn emoji_paragraph_fits_width() {
    let source = format!("{}\n", "⚠️".repeat(40));
    let lines = layout(&source, 40);
    for line in &lines {
        assert!(
            line_width(line) <= 40,
            "line {} > 40: {:?}",
            line_width(line),
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        );
    }
    // Nothing may be dropped: all 40 emoji survive the wrap.
    let all_text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(all_text.matches('⚠').count(), 40);
}

/// Emoji inside a fenced code block: every box line (borders included) must be
/// exactly the border width so the closing `│` aligns.
#[test]
fn emoji_code_block_borders_align() {
    let source = format!("```\n{}\n```\n", "⚠️".repeat(30));
    let lines = layout(&source, 40);
    let box_lines: Vec<&ink_md::layout::StyledLine> = lines
        .iter()
        .filter(|l| l.spans.iter().any(|s| s.text.contains(['│', '╭', '╰'])))
        .collect();
    assert!(box_lines.len() >= 3, "expected a full code box");
    for line in box_lines {
        assert_eq!(
            line_width(line),
            40,
            "misaligned box line: {:?}",
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        );
    }
}

/// Tab-indented code (the Go convention) must expand to spaces at 4-column
/// stops: visible indentation, aligned box borders, no raw `\t` in the output.
#[test]
fn tabs_in_code_blocks_expand_to_spaces() {
    let source = "```go\nfunc main() {\n\tfmt.Println(\"hi\")\n}\n```\n";
    let lines = layout(source, 60);
    let all_text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(!all_text.contains('\t'), "tabs must be expanded");
    assert!(
        all_text.contains("    fmt.Println"),
        "indentation must stay visible: {all_text:?}"
    );
    for line in lines
        .iter()
        .filter(|l| l.spans.iter().any(|s| s.text.contains(['│', '╭', '╰'])))
    {
        assert_eq!(
            line_width(line),
            60,
            "misaligned box line: {:?}",
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
        );
    }
}

/// Punctuation trimmed off the end of a bare URL is not part of the link, but
/// it is part of the sentence — it must remain in the rendered text.
#[test]
fn url_trailing_punctuation_stays_in_text() {
    let lines = layout("Visit https://example.com/foo. Done\n", 80);
    let all_text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        all_text.contains("https://example.com/foo. Done"),
        "trailing '.' was deleted from the text: {all_text:?}"
    );
    let link = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.style.link_url.is_some())
        .expect("bare URL should become a link");
    assert_eq!(
        link.style.link_url.as_deref(),
        Some("https://example.com/foo")
    );
    assert_eq!(link.text, "https://example.com/foo");
}

/// Graphics-mode images size to the full content width: `ctx.width` is
/// already the content width and the centering margin is applied through
/// `col_offset`, so subtracting the margin again shrank every image.
#[test]
fn graphics_image_spans_full_content_width() {
    let dir = tempfile::tempdir().unwrap();
    let wide_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="40">
  <rect width="800" height="40" fill="lime" /></svg>"##;
    std::fs::write(dir.path().join("wide.svg"), wide_svg).unwrap();
    let arena = Arena::new();
    let root = parse_document(&arena, "![wide](wide.svg)\n", &parser::options());
    let theme = resolve_theme("dark");
    let result = layout_document(
        root,
        &theme,
        40,
        Spacing::Normal,
        10, // centering margin
        Some(dir.path()),
        ink_md::image::ImageMode::LocalOnly,
        Some((8, 16)), // graphics mode, 8x16 px cells
    );
    assert_eq!(result.images.len(), 1);
    let spec = &result.images[0];
    assert_eq!(spec.col_offset, 10, "margin arrives via col_offset");
    assert_eq!(
        spec.cols, 40,
        "a wide image must fill the content width, not width - margin"
    );
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
        None,
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

// Issue #3 reopened: `![](/tmp/sample.svg)` — an image referenced by absolute
// path from a document in a different directory — must render, not fall back
// to the inline 🖼 placeholder.
#[test]
fn absolute_path_image_renders_as_halfblocks() {
    let img_dir = tempfile::tempdir().unwrap();
    let doc_dir = tempfile::tempdir().unwrap();
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120">
  <rect x="14" y="23" width="200" height="50" fill="lime" stroke="black" />
</svg>"##;
    let svg_path = img_dir.path().join("sample_from_wikipedia.svg");
    std::fs::write(&svg_path, svg).unwrap();

    // Forward slashes even on Windows: a raw `C:\…\Temp\.tmpXXXX\…` path is
    // mangled by the markdown parser before it ever reaches the image loader,
    // because `\.` is a valid CommonMark escape and the separator is eaten.
    // `C:/…` is equally absolute to `Path`, and is what a Windows author has
    // to write anyway.
    let source = format!(
        "![]({})\n",
        svg_path.display().to_string().replace('\\', "/")
    );
    let arena = Arena::new();
    let root = parse_document(&arena, &source, &parser::options());
    let theme = resolve_theme("dark");
    let lines = layout_document(
        root,
        &theme,
        80,
        Spacing::Normal,
        0,
        Some(doc_dir.path()),
        ink_md::image::ImageMode::LocalOnly,
        None,
    )
    .lines;

    let all_text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        all_text.contains('▄'),
        "absolute-path image should render half-blocks, got: {all_text:?}"
    );
    assert!(
        !all_text.contains('🖼'),
        "must not fall back to the placeholder"
    );
}

// A missing local image must say so — a silent placeholder is
// indistinguishable from a rendering bug (how issue #3 went undiagnosed).
#[test]
fn missing_image_placeholder_states_the_reason() {
    let doc_dir = tempfile::tempdir().unwrap();
    let arena = Arena::new();
    let root = parse_document(&arena, "![](nope.png)\n", &parser::options());
    let theme = resolve_theme("dark");
    let lines = layout_document(
        root,
        &theme,
        80,
        Spacing::Normal,
        0,
        Some(doc_dir.path()),
        ink_md::image::ImageMode::LocalOnly,
        None,
    )
    .lines;
    let all_text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        all_text.contains("image not found"),
        "placeholder should explain the failure, got: {all_text:?}"
    );
}

/// Layout `source` with images enabled (LocalOnly) against `base_dir`,
/// returning the concatenated text of all lines.
fn layout_images_text(source: &str, base_dir: &std::path::Path) -> String {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &parser::options());
    let theme = resolve_theme("dark");
    let lines = layout_document(
        root,
        &theme,
        80,
        Spacing::Normal,
        0,
        Some(base_dir),
        ink_md::image::ImageMode::LocalOnly,
        None,
    )
    .lines;
    lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect()
}

const LIME_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120">
  <rect x="14" y="23" width="200" height="50" fill="lime" stroke="black" /></svg>"##;

// `[![alt](img)](target)` — the linked-image/badge pattern must render the
// image as a block, not degrade to an inline placeholder.
#[test]
fn linked_image_renders_as_block() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shot.svg"), LIME_SVG).unwrap();
    let text = layout_images_text(
        "[![screenshot](shot.svg)](https://example.com)\n",
        dir.path(),
    );
    assert!(
        text.contains('▄'),
        "linked image should render pixels: {text:?}"
    );
    assert!(
        text.contains("screenshot"),
        "caption should carry the alt text"
    );
    assert!(!text.contains('🖼'));
}

// Several images in one paragraph (a gallery) all render as blocks.
#[test]
fn multiple_images_in_paragraph_all_render() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.svg"), LIME_SVG).unwrap();
    std::fs::write(dir.path().join("b.svg"), LIME_SVG).unwrap();
    let text = layout_images_text("![first](a.svg) ![second](b.svg)\n", dir.path());
    assert!(
        text.contains("first") && text.contains("second"),
        "both captions: {text:?}"
    );
    assert!(text.contains('▄'));
    assert!(!text.contains('🖼'));
}

// Raw HTML <img> blocks (the README way to size/center a logo) render as
// real images; the markup itself is not shown.
#[test]
fn html_img_block_renders_as_image() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("logo.svg"), LIME_SVG).unwrap();
    let text = layout_images_text(
        "<p align=\"center\">\n  <img src=\"logo.svg\" alt=\"Logo\" width=\"200\">\n</p>\n",
        dir.path(),
    );
    assert!(
        text.contains('▄'),
        "html img should render pixels: {text:?}"
    );
    assert!(text.contains("Logo"), "caption from alt attribute");
    assert!(!text.contains("<img"), "raw markup must not be shown");
}

// A mid-text inline <img> stays visible as a placeholder instead of vanishing.
#[test]
fn inline_html_img_is_visible() {
    let dir = tempfile::tempdir().unwrap();
    let text = layout_images_text(
        "Look at <img src=\"pic.png\" alt=\"the chart\"> here.\n",
        dir.path(),
    );
    assert!(
        text.contains("🖼 the chart"),
        "inline img placeholder: {text:?}"
    );
}
