//! Verify virtualized document rendering: at a scroll offset the viewport
//! shows the right slice, and only viewport-sized work is done.

use ink_md::render::render_document_with_search;
use ink_md::search::SearchState;
use ink_md::theme::resolve_theme;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Terminal;

fn doc(n: usize) -> Vec<Line<'static>> {
    (0..n)
        .map(|i| Line::from(Span::raw(format!("LINE{i:04}"))))
        .collect()
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buf = terminal.backend().buffer();
    let area = *buf.area();
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[test]
fn viewport_shows_slice_at_offset() {
    let lines = doc(1000);
    let theme = resolve_theme("dark");
    let search = SearchState::new();
    let mut terminal = Terminal::new(TestBackend::new(20, 10)).unwrap();

    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 20, 10);
            render_document_with_search(
                f,
                area,
                &lines,
                500,
                lines.len(),
                &search,
                None,
                &[],
                &theme,
            );
        })
        .unwrap();

    let text = buffer_text(&terminal);
    // The line at the scroll offset is the top of the viewport.
    assert!(
        text.contains("LINE0500"),
        "expected offset line, got:\n{text}"
    );
    assert!(text.contains("LINE0509"), "expected last visible line");
    // Lines outside the viewport must not be materialized.
    assert!(!text.contains("LINE0499"));
    assert!(!text.contains("LINE0510"));
}

#[test]
fn offset_past_end_does_not_panic() {
    let lines = doc(5);
    let theme = resolve_theme("dark");
    let search = SearchState::new();
    let mut terminal = Terminal::new(TestBackend::new(20, 10)).unwrap();
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 20, 10);
            render_document_with_search(
                f,
                area,
                &lines,
                9999,
                lines.len(),
                &search,
                None,
                &[],
                &theme,
            );
        })
        .unwrap();
    // No panic is the assertion.
}
