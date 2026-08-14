//! Text selection over the laid-out document.
//!
//! Positions are **document** coordinates — a display-line index plus a display
//! *column* (terminal cells, not bytes and not chars). Screen coordinates never
//! get stored, so scrolling, resizing, and `--watch` reloads cannot rot a
//! selection into pointing at the wrong text.
//!
//! Columns are counted the same way the layout counts them (grapheme clusters
//! measured with `unicode-width`), which is what keeps a selection edge from
//! landing inside a ZWJ emoji or on the second cell of a CJK character.

use crate::layout::StyledLine;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A caret position in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    /// Display-line index into the tab's laid-out lines.
    pub line: usize,
    /// Display column (terminal cells from the left edge of the line).
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// Character-wise (`v`) or whole-line (`V`) selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelMode {
    Char,
    Line,
}

/// An active selection: fixed `anchor`, moving `cursor`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    pub anchor: Pos,
    pub cursor: Pos,
    pub mode: SelMode,
}

impl Selection {
    pub fn new(at: Pos, mode: SelMode) -> Self {
        Self {
            anchor: at,
            cursor: at,
            mode,
        }
    }

    /// Anchor and cursor in document order. The end is **inclusive** of the
    /// cell it names, matching vim's visual mode (and matching what a user
    /// expects a drag to have grabbed).
    pub fn range(&self) -> (Pos, Pos) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// The half-open column range `[start, end)` to paint on `line`, or `None`
    /// when the line is outside the selection. `width` is the line's rendered
    /// width, used to bound line-wise selection and end-of-line runs.
    pub fn highlight_span(&self, line: usize, width: usize) -> Option<(usize, usize)> {
        let (start, end) = self.range();
        if line < start.line || line > end.line {
            return None;
        }
        if self.mode == SelMode::Line {
            return Some((0, width.max(1)));
        }
        // Highlighting one cell past a line's text marks the newline the copy
        // will include — without it, a multi-line selection looks like it stops
        // at the last glyph.
        let full_end = if line < end.line { width + 1 } else { width };
        let from = if line == start.line { start.col } else { 0 };
        let to = if line == end.line {
            // `end` is inclusive: extend past the whole grapheme under it.
            (end.col + 1).min(full_end.max(end.col + 1))
        } else {
            full_end
        };
        if to <= from {
            return None;
        }
        Some((from, to))
    }

    /// The selected text, ready for the clipboard.
    ///
    /// `plain` is the document's per-line text (see [`plain_lines`]). Trailing
    /// padding is dropped from every line. Line-wise selections are dedented by
    /// their common leading whitespace, which strips ink's centering margin
    /// while preserving the relative indentation of code and nested lists.
    pub fn extract(&self, plain: &[String]) -> String {
        let (start, end) = self.range();
        if plain.is_empty() || start.line >= plain.len() {
            return String::new();
        }
        let last = end.line.min(plain.len() - 1);

        let mut out: Vec<String> = Vec::with_capacity(last - start.line + 1);
        for (line, text) in plain.iter().enumerate().take(last + 1).skip(start.line) {
            let piece = match self.mode {
                SelMode::Line => text.clone(),
                SelMode::Char => {
                    let from = if line == start.line { start.col } else { 0 };
                    let to = if line == end.line {
                        Some(end.col)
                    } else {
                        None
                    };
                    slice_columns(text, from, to)
                }
            };
            out.push(piece.trim_end().to_string());
        }

        if self.mode == SelMode::Line {
            dedent(&mut out);
        }
        out.join("\n")
    }
}

/// Strip the leading-whitespace prefix that every non-empty line shares.
///
/// Compared character by character, not by byte count: leading whitespace is
/// not always one byte wide (U+00A0, U+3000), and a byte offset measured on one
/// line lands inside a character on another — which panics. Matching the actual
/// prefix (rather than counting whitespace) also means a document's own
/// indentation survives when it happens to use a different space character than
/// ink's centering margin.
fn dedent(lines: &mut [String]) {
    let mut prefix: Option<Vec<char>> = None;
    for line in lines.iter().filter(|l| !l.is_empty()) {
        let ws: Vec<char> = line.chars().take_while(|c| c.is_whitespace()).collect();
        prefix = Some(match prefix {
            None => ws,
            Some(common) => common
                .iter()
                .zip(ws.iter())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    let indent = prefix.map_or(0, |p| p.len());
    if indent == 0 {
        return;
    }
    // Safe for every line: a non-empty one starts with at least `indent`
    // whitespace characters by construction, and an empty one yields "".
    for line in lines.iter_mut() {
        *line = line.chars().skip(indent).collect();
    }
}

/// The text of `text` covering display columns `[from, to]` (`to` inclusive,
/// `None` = to end of line).
///
/// A grapheme is included when it overlaps the range at all, so a range edge
/// that falls on the second cell of a double-width character still yields that
/// whole character rather than half of it or nothing.
pub fn slice_columns(text: &str, from: usize, to: Option<usize>) -> String {
    let end = to.map(|t| t + 1).unwrap_or(usize::MAX);
    if end <= from {
        return String::new();
    }
    let mut out = String::new();
    let mut col = 0usize;
    for g in text.graphemes(true) {
        let w = g.width().max(1);
        let (gs, ge) = (col, col + w);
        if gs >= end {
            break;
        }
        if ge > from {
            out.push_str(g);
        }
        col = ge;
    }
    out
}

/// Flatten styled lines into the plain text the clipboard and the column math
/// both work against.
pub fn plain_lines(lines: &[StyledLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let mut s = String::new();
            for span in &line.spans {
                s.push_str(&span.text);
            }
            s
        })
        .collect()
}

/// Display width of a line, in terminal cells.
pub fn line_width(text: &str) -> usize {
    text.width()
}

/// Keep a position inside the document: line clamped to the last line, column
/// to one past the last cell (so a caret can sit at end of line).
pub fn clamp(pos: Pos, plain: &[String]) -> Pos {
    if plain.is_empty() {
        return Pos::default();
    }
    let line = pos.line.min(plain.len() - 1);
    let col = pos.col.min(line_width(&plain[line]));
    Pos::new(line, col)
}

/// Glyphs ink draws as structure rather than content: heading bars, list
/// bullets, blockquote rules, code-box borders.
const DECORATION: &[char] = &[
    '█', '▌', '▎', '▏', '│', '┃', '╭', '╰', '├', '•', '▪', '◦', '‣', '▸', '»',
];

/// The column where a rendered line's *content* starts, past ink's own
/// decoration.
///
/// Used to place the visual-mode cursor: a selection that starts on the `█` of
/// a heading copies a glyph the document never contained.
pub fn content_start_col(text: &str) -> usize {
    let mut col = 0usize;
    let mut seen_decoration = false;
    for g in text.graphemes(true) {
        let is_space = g.chars().all(char::is_whitespace);
        let is_decoration = g.chars().all(|c| DECORATION.contains(&c));
        if is_space {
            col += g.width().max(1);
            continue;
        }
        if is_decoration && !seen_decoration {
            seen_decoration = true;
            col += g.width().max(1);
            continue;
        }
        break;
    }
    col
}

/// Snap a column to the leading edge of the grapheme covering it.
///
/// A click on the right half of a wide character must select that character,
/// not the gap after it.
pub fn snap_col(text: &str, col: usize) -> usize {
    let mut at = 0usize;
    for g in text.graphemes(true) {
        let w = g.width().max(1);
        if col < at + w {
            return at;
        }
        at += w;
    }
    at
}

/// Columns `[start, end]` (end inclusive) of the word under `col`.
///
/// Falls back to the run of whitespace when the click landed between words, so
/// a double-click always selects *something* visible.
pub fn word_bounds(text: &str, col: usize) -> (usize, usize) {
    let mut bounds: Vec<(usize, usize)> = Vec::new(); // (start_col, end_col_inclusive)
    let mut at = 0usize;
    for word in text.split_word_bounds() {
        let w = word.width().max(1);
        bounds.push((at, at + w - 1));
        at += w;
    }
    for (s, e) in bounds {
        if col >= s && col <= e {
            return (s, e);
        }
    }
    (col, col)
}

/// Start column of the word after `col`, or end of line.
pub fn next_word_col(text: &str, col: usize) -> usize {
    let mut at = 0usize;
    let mut candidate = None;
    for word in text.split_word_bounds() {
        if at > col && !word.trim().is_empty() {
            candidate = Some(at);
            break;
        }
        at += word.width().max(1);
    }
    candidate.unwrap_or_else(|| line_width(text))
}

/// Start column of the word before `col`, or 0.
pub fn prev_word_col(text: &str, col: usize) -> usize {
    let mut at = 0usize;
    let mut best = 0usize;
    for word in text.split_word_bounds() {
        if at >= col {
            break;
        }
        if !word.trim().is_empty() {
            best = at;
        }
        at += word.width().max(1);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Vec<String> {
        vec![
            "the quick brown fox".to_string(),
            "jumps over".to_string(),
            "the lazy dog".to_string(),
        ]
    }

    #[test]
    fn char_selection_spans_partial_first_and_last_lines() {
        let sel = Selection {
            anchor: Pos::new(0, 4),
            cursor: Pos::new(2, 7),
            mode: SelMode::Char,
        };
        assert_eq!(sel.extract(&doc()), "quick brown fox\njumps over\nthe lazy");
    }

    #[test]
    fn selection_is_direction_agnostic() {
        let forward = Selection {
            anchor: Pos::new(0, 4),
            cursor: Pos::new(2, 7),
            mode: SelMode::Char,
        };
        let backward = Selection {
            anchor: Pos::new(2, 7),
            cursor: Pos::new(0, 4),
            mode: SelMode::Char,
        };
        assert_eq!(forward.extract(&doc()), backward.extract(&doc()));
    }

    #[test]
    fn line_selection_takes_whole_lines_and_strips_the_margin() {
        let margined: Vec<String> = doc().iter().map(|l| format!("    {l}   ")).collect();
        let sel = Selection {
            anchor: Pos::new(0, 4),
            cursor: Pos::new(2, 7),
            mode: SelMode::Line,
        };
        assert_eq!(
            sel.extract(&margined),
            "the quick brown fox\njumps over\nthe lazy dog"
        );
    }

    /// Regression: the shared indent used to be counted in *bytes* and then
    /// sliced off by byte offset. With leading whitespace of mixed width — an
    /// ideographic space on one line, non-breaking spaces on the next — that
    /// offset lands inside a character and slicing panics, taking ink down
    /// mid-selection.
    #[test]
    fn line_selection_survives_mixed_width_leading_whitespace() {
        let lines = vec![
            "  \u{3000}alpha".to_string(),    // 2 spaces + 3-byte ideographic space
            "  \u{a0}\u{a0}beta".to_string(), // 2 spaces + two 2-byte NBSPs
        ];
        let sel = Selection {
            anchor: Pos::new(0, 0),
            cursor: Pos::new(1, 4),
            mode: SelMode::Line,
        };
        // The common prefix is the two ASCII spaces; everything past it stays.
        assert_eq!(sel.extract(&lines), "\u{3000}alpha\n\u{a0}\u{a0}beta");
    }

    #[test]
    fn line_selection_keeps_relative_indentation() {
        let lines = vec![
            "    fn main() {".to_string(),
            "        println!();".to_string(),
            "    }".to_string(),
        ];
        let sel = Selection {
            anchor: Pos::new(0, 0),
            cursor: Pos::new(2, 4),
            mode: SelMode::Line,
        };
        assert_eq!(sel.extract(&lines), "fn main() {\n    println!();\n}");
    }

    #[test]
    fn a_selection_edge_inside_a_wide_character_takes_the_whole_character() {
        // 世 and 界 are two cells each: columns 0-1 and 2-3.
        let lines = vec!["世界a".to_string()];
        // Start on the *second* cell of 世, end on the second cell of 界.
        let sel = Selection {
            anchor: Pos::new(0, 1),
            cursor: Pos::new(0, 3),
            mode: SelMode::Char,
        };
        assert_eq!(sel.extract(&lines), "世界");
        assert!(sel.extract(&lines).contains('世'));
    }

    #[test]
    fn single_cell_selection_takes_one_grapheme() {
        let lines = vec!["abc".to_string()];
        let sel = Selection::new(Pos::new(0, 1), SelMode::Char);
        assert_eq!(sel.extract(&lines), "b");
    }

    #[test]
    fn selection_past_the_end_of_the_document_is_empty_not_a_panic() {
        let sel = Selection {
            anchor: Pos::new(9, 0),
            cursor: Pos::new(12, 4),
            mode: SelMode::Char,
        };
        assert_eq!(sel.extract(&doc()), "");
    }

    #[test]
    fn highlight_span_covers_the_cursor_cell() {
        let sel = Selection {
            anchor: Pos::new(0, 2),
            cursor: Pos::new(0, 5),
            mode: SelMode::Char,
        };
        assert_eq!(sel.highlight_span(0, 19), Some((2, 6)));
        assert_eq!(sel.highlight_span(1, 10), None);
    }

    #[test]
    fn line_mode_highlights_the_whole_line() {
        let sel = Selection {
            anchor: Pos::new(0, 7),
            cursor: Pos::new(1, 2),
            mode: SelMode::Line,
        };
        assert_eq!(sel.highlight_span(0, 19), Some((0, 19)));
        assert_eq!(sel.highlight_span(1, 10), Some((0, 10)));
    }

    #[test]
    fn content_start_col_skips_inks_own_decoration() {
        // Heading bar, list bullet, blockquote rule — none of these are in the
        // markdown the user is reading.
        assert_eq!(content_start_col("  █ Install ink"), 4);
        assert_eq!(content_start_col("  • an item"), 4);
        assert_eq!(content_start_col("  │ quoted"), 4);
        // Plain text keeps everything past the margin.
        assert_eq!(content_start_col("    hello"), 4);
        // Only one run is skipped: a second bar is content (nested quote).
        assert_eq!(content_start_col("  │ │ nested"), 4);
        assert_eq!(content_start_col(""), 0);
    }

    #[test]
    fn snap_col_lands_on_grapheme_starts() {
        assert_eq!(snap_col("世界a", 0), 0);
        assert_eq!(snap_col("世界a", 1), 0);
        assert_eq!(snap_col("世界a", 2), 2);
        assert_eq!(snap_col("世界a", 3), 2);
        assert_eq!(snap_col("世界a", 4), 4);
    }

    #[test]
    fn word_bounds_grabs_the_word_under_the_column() {
        let text = "the quick brown";
        assert_eq!(word_bounds(text, 5), (4, 8));
        assert_eq!(word_bounds(text, 4), (4, 8));
        assert_eq!(word_bounds(text, 8), (4, 8));
    }

    #[test]
    fn word_motions_step_between_words() {
        let text = "the quick brown fox";
        assert_eq!(next_word_col(text, 0), 4);
        assert_eq!(next_word_col(text, 4), 10);
        assert_eq!(next_word_col(text, 16), line_width(text));
        assert_eq!(prev_word_col(text, 10), 4);
        assert_eq!(prev_word_col(text, 2), 0);
    }

    #[test]
    fn clamp_keeps_positions_inside_the_document() {
        let d = doc();
        assert_eq!(clamp(Pos::new(99, 99), &d), Pos::new(2, 12));
        assert_eq!(clamp(Pos::new(1, 3), &d), Pos::new(1, 3));
        assert_eq!(clamp(Pos::new(0, 0), &[]), Pos::default());
    }

    #[test]
    fn plain_lines_concatenates_spans() {
        use crate::layout::{SpanStyle, StyledSpan};
        let line = StyledLine {
            spans: vec![
                StyledSpan {
                    text: "  ".into(),
                    style: SpanStyle::default(),
                },
                StyledSpan {
                    text: "hello".into(),
                    style: SpanStyle::default(),
                },
            ],
        };
        assert_eq!(plain_lines(&[line]), vec!["  hello".to_string()]);
    }
}
