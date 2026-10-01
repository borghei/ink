//! A character grid that diagrams are drawn on. Lines are stored as
//! direction bits per cell and turned into box-drawing glyphs at the end, so
//! crossings, corners and junctions come out right whatever order things are
//! drawn in.

use super::graph::LineStyle;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;

/// What a cell is drawn as, for colouring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Subgraph frames and other chrome.
    Frame,
    /// Edges and their markers.
    Edge,
    /// Node outlines.
    Node,
    /// Text inside nodes.
    Text,
    /// Edge labels and annotations.
    Label,
    /// Titles (subgraph names, class names, section headers).
    Title,
}

#[derive(Debug, Clone)]
enum Cell {
    Empty,
    Line {
        bits: u8,
        /// Bits drawn with a double line (subroutine borders).
        dbl: u8,
        style: LineStyle,
        class: Class,
    },
    Glyph {
        text: String,
        class: Class,
    },
    /// Right half of a double-width glyph.
    Cont,
}

pub struct Canvas {
    pub rows: usize,
    pub cols: usize,
    cells: Vec<Cell>,
}

impl Canvas {
    pub fn new(rows: usize, cols: usize) -> Self {
        Canvas {
            rows,
            cols,
            cells: vec![Cell::Empty; rows * cols],
        }
    }

    fn at(&mut self, r: usize, c: usize) -> Option<&mut Cell> {
        if r < self.rows && c < self.cols {
            Some(&mut self.cells[r * self.cols + c])
        } else {
            None
        }
    }

    /// True when (r, c) holds nothing but a horizontal line.
    pub fn plain_h(&self, r: usize, c: usize) -> bool {
        r < self.rows
            && c < self.cols
            && matches!(self.cells[r * self.cols + c], Cell::Line { bits, .. } if bits == LEFT | RIGHT)
    }

    /// Add line `bits` to a cell. Glyph and text cells keep their glyph.
    pub fn add_bits(&mut self, r: usize, c: usize, bits: u8, style: LineStyle, class: Class) {
        let Some(cell) = self.at(r, c) else { return };
        match cell {
            Cell::Empty => {
                *cell = Cell::Line {
                    bits,
                    dbl: 0,
                    style,
                    class,
                }
            }
            Cell::Line {
                bits: b,
                style: s,
                class: k,
                ..
            } => {
                *b |= bits;
                if class == Class::Edge {
                    *s = style;
                }
                if class > *k {
                    *k = class;
                }
            }
            Cell::Glyph { .. } | Cell::Cont => {}
        }
    }

    /// Mark a cell's bits as double-line.
    pub fn add_double(&mut self, r: usize, c: usize, bits: u8, class: Class) {
        self.add_bits(r, c, bits, LineStyle::Solid, class);
        if let Some(Cell::Line { dbl, .. }) = self.at(r, c) {
            *dbl |= bits;
        }
    }

    /// Draw an axis-aligned line between two cells (inclusive).
    pub fn line(
        &mut self,
        r0: usize,
        c0: usize,
        r1: usize,
        c1: usize,
        style: LineStyle,
        class: Class,
    ) {
        if r0 == r1 {
            let (a, b) = (c0.min(c1), c0.max(c1));
            for c in a..b {
                self.add_bits(r0, c, RIGHT, style, class);
                self.add_bits(r0, c + 1, LEFT, style, class);
            }
        } else if c0 == c1 {
            let (a, b) = (r0.min(r1), r0.max(r1));
            for r in a..b {
                self.add_bits(r, c0, DOWN, style, class);
                self.add_bits(r + 1, c0, UP, style, class);
            }
        }
    }

    /// Draw a polyline through `pts` (each step axis-aligned).
    pub fn path(&mut self, pts: &[(usize, usize)], style: LineStyle, class: Class) {
        for w in pts.windows(2) {
            self.line(w[0].0, w[0].1, w[1].0, w[1].1, style, class);
        }
    }

    /// Put a single glyph (one grapheme) at a cell, replacing what is there.
    pub fn glyph(&mut self, r: usize, c: usize, g: &str, class: Class) {
        let w = UnicodeWidthStr::width(g);
        if w == 0 || c + w > self.cols || r >= self.rows {
            return;
        }
        // Never leave half of a double-width glyph behind.
        for cc in c..c + w {
            self.clear_wide(r, cc);
        }
        if let Some(cell) = self.at(r, c) {
            *cell = Cell::Glyph {
                text: g.to_string(),
                class,
            };
        }
        if w == 2 {
            if let Some(cell) = self.at(r, c + 1) {
                *cell = Cell::Cont;
            }
        }
    }

    /// Clear the double-width glyph that covers (r, c), if any.
    fn clear_wide(&mut self, r: usize, c: usize) {
        let i = r * self.cols + c;
        match &self.cells[i] {
            Cell::Cont if c > 0 => {
                self.cells[i] = Cell::Empty;
                self.cells[i - 1] = Cell::Empty;
            }
            Cell::Glyph { text, .. } if UnicodeWidthStr::width(text.as_str()) == 2 => {
                self.cells[i] = Cell::Empty;
                if c + 1 < self.cols {
                    self.cells[i + 1] = Cell::Empty;
                }
            }
            _ => {}
        }
    }

    /// Write text starting at (r, c), clipped at `max_c` (exclusive).
    pub fn text(&mut self, r: usize, c: usize, s: &str, max_c: usize, class: Class) {
        let mut col = c;
        let limit = max_c.min(self.cols);
        for g in s.graphemes(true) {
            let w = UnicodeWidthStr::width(g);
            if w == 0 {
                continue;
            }
            if col + w > limit {
                break;
            }
            self.glyph(r, col, g, class);
            col += w;
        }
    }

    /// The finished rows: each a list of (text, class) runs, with trailing
    /// blanks kept so every row is exactly `cols` wide.
    pub fn render(&self) -> Vec<Vec<(String, Class)>> {
        let mut out = Vec::with_capacity(self.rows);
        for r in 0..self.rows {
            let mut runs: Vec<(String, Class)> = Vec::new();
            for c in 0..self.cols {
                let (s, class): (String, Class) = match &self.cells[r * self.cols + c] {
                    Cell::Empty => (" ".into(), Class::Frame),
                    Cell::Cont => continue,
                    Cell::Glyph { text, class } => (text.clone(), *class),
                    Cell::Line {
                        bits,
                        dbl,
                        style,
                        class,
                    } => (line_glyph(*bits, *dbl, *style).to_string(), *class),
                };
                match runs.last_mut() {
                    Some((t, k)) if *k == class || s == " " => t.push_str(&s),
                    _ => runs.push((s, class)),
                }
            }
            out.push(runs);
        }
        out
    }
}

/// Box-drawing glyph for a set of direction bits.
fn line_glyph(bits: u8, dbl: u8, style: LineStyle) -> char {
    if dbl != 0 {
        let extra = bits & !dbl;
        if dbl & (LEFT | RIGHT) != 0 && dbl & (UP | DOWN) == 0 {
            return match extra & (UP | DOWN) {
                0 => '═',
                DOWN => '╤',
                UP => '╧',
                _ => '╪',
            };
        }
        if dbl & (UP | DOWN) != 0 && dbl & (LEFT | RIGHT) == 0 {
            return match extra & (LEFT | RIGHT) {
                0 => '║',
                RIGHT => '╟',
                LEFT => '╢',
                _ => '╫',
            };
        }
    }
    let vertical = UP | DOWN;
    let horizontal = LEFT | RIGHT;
    match bits {
        b if b == vertical || b == UP || b == DOWN => match style {
            LineStyle::Dotted => '┆',
            LineStyle::Thick => '┃',
            _ => '│',
        },
        b if b == horizontal || b == LEFT || b == RIGHT => match style {
            LineStyle::Dotted => '┄',
            LineStyle::Thick => '━',
            _ => '─',
        },
        b if b == DOWN | RIGHT => '┌',
        b if b == DOWN | LEFT => '┐',
        b if b == UP | RIGHT => '└',
        b if b == UP | LEFT => '┘',
        b if b == vertical | RIGHT => '├',
        b if b == vertical | LEFT => '┤',
        b if b == horizontal | DOWN => '┬',
        b if b == horizontal | UP => '┴',
        b if b == vertical | horizontal => '┼',
        _ => ' ',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(c: &Canvas) -> Vec<String> {
        c.render()
            .into_iter()
            .map(|runs| runs.into_iter().map(|(t, _)| t).collect())
            .collect()
    }

    #[test]
    fn lines_merge_into_junctions() {
        let mut c = Canvas::new(3, 5);
        c.line(1, 0, 1, 4, LineStyle::Solid, Class::Edge);
        c.line(0, 2, 2, 2, LineStyle::Solid, Class::Edge);
        c.path(&[(0, 0), (0, 1)], LineStyle::Solid, Class::Edge);
        assert_eq!(plain(&c), vec!["──│  ", "──┼──", "  │  "]);
    }

    #[test]
    fn wide_glyphs_take_two_cells() {
        let mut c = Canvas::new(1, 4);
        c.text(0, 0, "日本", 4, Class::Text);
        assert_eq!(plain(&c), vec!["日本"]);
        let mut c = Canvas::new(1, 3);
        c.text(0, 0, "日本", 3, Class::Text);
        assert_eq!(plain(&c), vec!["日 "]);
        c.glyph(0, 1, "x", Class::Text);
        assert_eq!(plain(&c), vec![" x "]);
    }
}
