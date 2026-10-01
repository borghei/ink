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
