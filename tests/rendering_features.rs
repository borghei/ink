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
fn superscript_and_html_subscript_use_unicode_forms() {
    let out = render("H<sub>2</sub>O, e = mc^2^, x^n+1^ and a<sub>ij</sub>.\n");
    assert!(out.contains("H₂O, e = mc², xⁿ⁺¹ and aᵢⱼ."), "{out}");
}

#[test]
fn superscript_without_a_unicode_form_falls_back() {
    // No superscript `q`; no subscript `b`: the whole run falls back.
    let out = render("x^q^ and y<sub>ab</sub>\n");
    assert!(out.contains("x^(q) and y_(ab)"), "{out}");
}

#[test]
fn single_and_double_tildes_strike_through_as_on_github() {
    for src in ["~~struck~~ and about ~5ms\n", "~struck~ and about ~5ms\n"] {
        let lines = layout(src);
        let struck = find_span(&lines, "struck");
        assert!(struck.style.strikethrough, "{src:?}");
        assert_eq!(struck.text, "struck");
    }
    let out = render("H~2~O and about ~5ms here\n");
    assert!(out.contains("H2O and about ~5ms here"), "{out}");
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

// ── Frontmatter ──

fn with_frontmatter(src: &str) -> String {
    render_with(src, |a| a.frontmatter = true)
}

#[test]
fn frontmatter_hidden_by_default() {
    for src in [
        "---\ntitle: Hello\nauthor: me\n---\n# Doc\n",
        "+++\ntitle = \"Hello\"\n+++\n# Doc\n",
        "{\n  \"title\": \"Hello\"\n}\n# Doc\n",
    ] {
        let out = render(src);
        assert!(
            !out.contains("Hello") && !out.contains("frontmatter"),
            "{out}"
        );
        assert!(out.contains("Doc"), "{out}");
    }
}

#[test]
fn yaml_frontmatter_renders_as_a_metadata_box() {
    let out = with_frontmatter(
        "---\ntitle: \"Hello: world\"\nauthor: me\ntags: [a, b]\nmeta:\n  draft: true\n---\n# Doc\n",
    );
    let want = "  ╭─ frontmatter ────────╮\n\
                \x20 │ title   Hello: world │\n\
                \x20 │ author  me           │\n\
                \x20 │ tags    a, b         │\n\
                \x20 │ meta    draft: true  │\n\
                \x20 ╰──────────────────────╯\n";
    assert!(out.starts_with(want), "{out}");
    // Not read as markdown: no rule, no setext heading made of the keys.
    assert!(!out.contains("◆"), "{out}");
    assert!(out.contains("█ Doc"), "{out}");
}

#[test]
fn toml_frontmatter_renders_as_a_metadata_box() {
    let out = with_frontmatter(
        "+++\ntitle = \"T\"\ntags = [\"x\", \"y\"]\n[params]\ndraft = false\n+++\nBody\n",
    );
    assert!(out.contains("│ title         T     │"), "{out}");
    assert!(out.contains("│ tags          x, y  │"), "{out}");
    assert!(out.contains("│ params.draft  false │"), "{out}");
    assert!(!out.contains("+++"), "{out}");
}

#[test]
fn json_frontmatter_renders_as_a_metadata_box() {
    let out = with_frontmatter(
        "{\n  \"title\": \"J\",\n  \"tags\": [\"a\", \"b\"],\n  \"o\": {\"k\": 1}\n}\nBody\n",
    );
    assert!(out.contains("│ title  J        │"), "{out}");
    assert!(out.contains("│ tags   a, b     │"), "{out}");
    assert!(out.contains("│ o      {\"k\": 1} │"), "{out}");
    assert!(!out.contains("\"title\""), "{out}");
}

#[test]
fn unterminated_frontmatter_falls_back_to_markdown() {
    // No closing delimiter: rendered as markdown, exactly as before.
    let src = "---\ntitle: Hello\n\nBody text\n";
    assert_eq!(with_frontmatter(src), render(src));
    assert!(render(src).contains("Body text"));
    let src = "{\n\"title\": \"x\"\nBody\n";
    assert_eq!(with_frontmatter(src), render(src));
}

#[test]
fn frontmatter_values_are_sanitized() {
    let src = "---\ntitle: evil \x1b]52;c;SGVsbG8=\x07 \x1b[2J value\n\x1b[31mkey: v\n---\nBody\n";
    let mut a = args();
    a.frontmatter = true;
    let out = render_plain_with_color(src, &a, true).unwrap();
    assert!(!out.contains("\x1b]52") && !out.contains("\x1b[2J") && !out.contains('\x07'));
    assert!(out.contains("evil"), "{out}");
}

#[test]
fn frontmatter_box_wraps_long_values_within_the_width() {
    let out = render_with(
        "---\ndescription: one two three four five six seven eight nine ten eleven\n---\n",
        |a| {
            a.frontmatter = true;
            a.width = Some(40);
        },
    );
    for line in out.lines() {
        assert!(line.width() <= 40, "{line:?}");
    }
    assert!(out.lines().filter(|l| l.contains('│')).count() > 1, "{out}");
}

// ── Math ──

#[test]
fn lone_dollar_signs_are_not_math() {
    let out = render("It costs $5 and $10, or $ 20.\n");
    assert!(out.contains("It costs $5 and $10, or $ 20."), "{out}");
}

#[test]
fn inline_math_renders_in_unicode_and_code_colour() {
    let out = render("Energy $E=mc^2$ with $\\alpha_i \\leq \\beta$ and $`x^2`$.\n");
    assert!(out.contains("Energy E=mc² with αᵢ ≤ β and x²."), "{out}");
    let lines = layout("Energy $E=mc^2$ here.\n");
    let theme = ink_md::theme::resolve_theme("dark");
    assert_eq!(
        find_span(&lines, "E=mc²").style.fg.as_deref(),
        Some(theme.colors.code_fg.as_str())
    );
}

#[test]
fn display_math_gets_its_own_indented_lines() {
    let out =
        render("The formula\n$$\nx = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}\n$$\nsolves it.\n");
    assert!(
        out.contains("  The formula\n      x = (-b ± √(b² - 4ac))/2a\n  solves it.\n"),
        "{out}"
    );
}

#[test]
fn display_matrix_and_math_fence() {
    let out = render("$$\nA = \\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}\n$$\n");
    assert!(
        out.contains("      A = ⎛ a  b ⎞\n          ⎝ c  d ⎠\n"),
        "{out}"
    );
    let out = render("```math\n\\sum_{i=1}^{n} i\n```\n");
    assert!(out.contains("      ∑ᵢ₌₁ⁿ i\n"), "{out}");
    assert!(!out.contains("─ math ─"), "not a code box: {out}");
}

#[test]
fn math_fence_copies_its_latex_source() {
    let src = "```math\n\\frac{a}{b}\n```\n";
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, src, &ink_md::parser::options_for(src));
    let result = ink_md::layout::layout_document(
        root,
        &ink_md::theme::resolve_theme("dark"),
        80,
        Spacing::Normal,
        0,
        None,
        ink_md::image::ImageMode::Off,
        None,
    );
    assert_eq!(result.code_blocks.len(), 1);
    assert_eq!(result.code_blocks[0].lang, "math");
    assert_eq!(result.code_blocks[0].source, "\\frac{a}{b}");
}

#[test]
fn math_in_table_cells() {
    let out = render("| f | v |\n|---|--:|\n| $\\sqrt{2}$ | $\\approx 1.41$ |\n");
    assert_eq!(table_rows(&out)[1], "│ √2 │ ≈ 1.41 │");
}

#[test]
fn display_math_wider_than_the_view_is_broken_not_clipped() {
    let long = "x + ".repeat(30);
    let out = render_with(&format!("$$\n{long}y\n$$\n"), |a| a.width = Some(40));
    for line in out.lines() {
        assert!(line.width() <= 40, "{line:?}");
    }
}
