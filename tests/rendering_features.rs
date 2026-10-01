//! Rendering features through the real `--plain` pipeline (parse, layout,
//! colourless output): table alignment, extensions, frontmatter, math.

use ink_md::render::plain::render_plain_with_color;
use ink_md::{Args, Spacing};
use unicode_width::UnicodeWidthStr;

fn args() -> Args {
    Args {
        inputs: vec![],
        theme: "dark".to_string(),
        width: Some(80),
        slides: false,
        plain: true,
        watch: false,
        toc: false,
        images: ink_md::image::ImageMode::Off,
        image_protocol: ink_md::graphics::ProtocolChoice::HalfBlocks,
        frontmatter: false,
        spacing: Spacing::Normal,
        mouse_capture: true,
        clipboard: ink_md::clipboard::ClipboardMode::Off,
    }
}

fn render(src: &str) -> String {
    render_plain_with_color(src, &args(), false).unwrap()
}

fn render_with(src: &str, f: impl FnOnce(&mut Args)) -> String {
    let mut a = args();
    f(&mut a);
    render_plain_with_color(src, &a, false).unwrap()
}

/// The table's rows (lines starting with the cell border), margin removed.
fn table_rows(out: &str) -> Vec<String> {
    out.lines()
        .map(|l| l.trim_start().to_string())
        .filter(|l| l.starts_with('│'))
        .collect()
}

// ── Table alignment ──

#[test]
fn table_right_alignment_pads_on_the_left() {
    let out = render("| n | name |\n|--:|------|\n| 1 | a |\n| 12345 | b |\n");
    let rows = table_rows(&out);
    assert_eq!(rows[0], "│     n │ name │");
    assert_eq!(rows[1], "│     1 │ a    │");
    assert_eq!(rows[2], "│ 12345 │ b    │");
}

#[test]
fn table_center_alignment_splits_the_padding() {
    let out = render("| centered |\n|:--------:|\n| x |\n| ab |\n");
    let rows = table_rows(&out);
    assert_eq!(rows[0], "│ centered │");
    // 7 free columns: 3 left, 4 right (leans left on an odd remainder).
    assert_eq!(rows[1], "│    x     │");
    assert_eq!(rows[2], "│    ab    │");
}

#[test]
fn table_default_and_left_alignment_pad_on_the_right() {
    let out = render("| a | b |\n|---|:--|\n| x | y |\n| long | longer |\n");
    let rows = table_rows(&out);
    assert_eq!(rows[1], "│ x    │ y      │");
}

#[test]
fn table_mixed_alignment() {
    let out = render(
        "| left | right | center | def |\n|:-----|------:|:------:|-----|\n| a | 1 | x | d |\n| longer text | 12345 | mid | e |\n",
    );
    let rows = table_rows(&out);
    assert_eq!(rows[0], "│ left        │ right │ center │ def │");
    assert_eq!(rows[1], "│ a           │     1 │   x    │ d   │");
    assert_eq!(rows[2], "│ longer text │ 12345 │  mid   │ e   │");
}

#[test]
fn table_alignment_applies_to_each_wrapped_line() {
    // A 30-column view forces the long right-aligned cell to wrap; every
    // visual line of it is right-aligned within the column.
    let out = render_with(
        "| k | value |\n|---|------:|\n| a | one two three four five six seven eight |\n",
        |a| a.width = Some(30),
    );
    assert_eq!(
        table_rows(&out),
        [
            "│ k     │            value │",
            "│ a     │    one two three │",
            "│       │    four five six │",
            "│       │      seven eight │",
        ]
    );
}

#[test]
fn table_alignment_measures_wide_characters() {
    let out = render("| 名前 | v |\n|:----:|--:|\n| 日本 | 1 |\n| a | 22 |\n");
    let rows = table_rows(&out);
    // Every row has the same display width despite double-width cells.
    let w = rows[0].width();
    for r in &rows {
        assert_eq!(r.width(), w, "{r:?}");
    }
    assert_eq!(rows[0], "│ 名前 │  v │");
    assert_eq!(rows[1], "│ 日本 │  1 │");
    // "a" centred in four columns: 1 left, 2 right.
    assert_eq!(rows[2], "│  a   │ 22 │");
}

// ── Markdown extensions ──

/// Layout lines for `src` (dark theme, width 80, no margin).
fn layout(src: &str) -> Vec<ink_md::layout::StyledLine> {
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, src, &ink_md::parser::options_for(src));
    ink_md::layout::layout_document(
        root,
        &ink_md::theme::resolve_theme("dark"),
        80,
        Spacing::Normal,
        0,
        None,
        ink_md::image::ImageMode::Off,
        None,
    )
    .lines
}

fn find_span<'l>(
    lines: &'l [ink_md::layout::StyledLine],
    text: &str,
) -> &'l ink_md::layout::StyledSpan {
    lines
        .iter()
        .flat_map(|l| &l.spans)
        .find(|s| s.text.contains(text))
        .unwrap_or_else(|| panic!("no span containing {text:?}"))
}

#[test]
fn superscript_and_subscript_use_unicode_forms() {
    let out = render("H~2~O, e = mc^2^, x^n+1^ and a~ij~.\n");
    assert!(out.contains("H₂O, e = mc², xⁿ⁺¹ and aᵢⱼ."), "{out}");
}

#[test]
fn superscript_without_a_unicode_form_falls_back() {
    // No superscript `q`; no subscript `b`: the whole run falls back.
    let out = render("x^q^ and y~ab~\n");
    assert!(out.contains("x^(q) and y_(ab)"), "{out}");
}

#[test]
fn subscript_leaves_strikethrough_and_lone_tildes_alone() {
    let lines = layout("~~struck~~ and about ~5ms\n");
    let struck = find_span(&lines, "struck");
    assert!(struck.style.strikethrough);
    assert_eq!(struck.text, "struck");
    assert!(render("about ~5ms here\n").contains("about ~5ms here"));
}

#[test]
fn double_underscore_stays_bold() {
    let lines = layout("__bold__ text\n");
    let span = find_span(&lines, "bold");
    assert!(span.style.bold && !span.style.underline, "{span:?}");
}

#[test]
fn spoiler_text_is_concealed_between_visible_bars() {
    let out = render("Vader is ||his father||.\n");
    assert!(out.contains("Vader is ||his father||."), "{out}");
    let lines = layout("Vader is ||his *father*||.\n");
    for word in ["his ", "father"] {
        let span = find_span(&lines, word);
        assert!(span.style.fg.is_some() && span.style.fg == span.style.bg);
    }
    assert!(find_span(&lines, "||").style.dim);
}

#[test]
fn definition_list_term_bold_and_definitions_marked() {
    let out = render("Term\n: First definition\n: Second one\n\nOther\n: More\n");
    assert!(
        out.contains("  Term\n    ▸ First definition\n\n    ▸ Second one\n\n  Other\n    ▸ More\n"),
        "{out}"
    );
    let lines = layout("Term\n: def\n");
    assert!(find_span(&lines, "Term").style.bold);
}

#[test]
fn definition_with_several_paragraphs_indents_them_all() {
    let out = render("Term\n\n: First para\n\n  Second para\n");
    assert!(
        out.contains("    ▸ First para\n\n      Second para\n"),
        "{out}"
    );
}

#[test]
fn github_alert_without_title_shows_its_type() {
    let out = render("> [!WARNING]\n> Be careful.\n");
    assert!(out.contains("│ ⚠ WARNING\n    │ Be careful.\n"), "{out}");
}

#[test]
fn github_alert_custom_title_replaces_the_type() {
    let out = render("> [!NOTE] Read *this* first\n> Body text.\n");
    assert!(
        out.contains("│ ℹ Read this first\n    │ Body text.\n"),
        "{out}"
    );
    assert!(!out.contains("NOTE"), "{out}");
}

#[test]
fn obsidian_callouts_map_types_and_ignore_fold_markers() {
    let out = render("> [!info]- Folded section\n> Hidden body.\n");
    assert!(
        out.contains("│ ℹ Folded section\n    │ Hidden body.\n"),
        "{out}"
    );
    let out = render("> [!bug]+\n> It crashes.\n");
    assert!(out.contains("│ 🔴 BUG\n    │ It crashes.\n"), "{out}");
    let out = render("> [!example] Title only\n");
    assert!(out.contains("│ ▎ Title only\n"), "{out}");
    // The callout colour follows its family.
    let lines = layout("> [!danger] Hot\n> x\n");
    let theme = ink_md::theme::resolve_theme("dark");
    assert_eq!(
        find_span(&lines, "Hot").style.fg.as_deref(),
        Some(theme.colors.admonition_caution.as_str())
    );
}

#[test]
fn bracketed_text_that_is_not_a_type_stays_a_quote() {
    let out = render("> [!not a type] text\n");
    assert!(out.contains("│ [!not a type] text"), "{out}");
}

#[test]
fn table_cells_render_scripts_and_spoilers() {
    let out = render("| a | b |\n|---|---|\n| x^2^ | ||s|| |\n");
    let rows = table_rows(&out);
    assert_eq!(rows[1], "│ x² │ ||s|| │");
}
