use crate::parser::Heading;
use std::collections::HashSet;

/// Table of contents state.
#[derive(Debug)]
pub struct TocState {
    pub visible: bool,
    pub headings: Vec<TocEntry>,
    pub selected: usize,
    pub width: u16,
    /// What the reader is doing in the sidebar: focus, cursor, filter,
    /// folded subtrees, how far it is scrolled.
    pub nav: TocNav,
}

#[derive(Debug, Clone)]
pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub line_index: usize,
    /// 1-based line in the markdown source, for copying a section's source.
    pub source_line: usize,
}

/// Interaction state of the sidebar. Survives a re-layout of the same
/// document (resize, theme change, TOC toggle); a new document starts fresh.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TocNav {
    /// Keys go to the sidebar instead of the document.
    pub focused: bool,
    /// The heading under the cursor while focused (an index into `headings`).
    pub cursor: usize,
    /// `Some` while the reader is typing a filter; only matching headings
    /// are listed, folds ignored.
    pub filter: Option<String>,
    /// Headings whose subtrees are folded away in the sidebar.
    pub collapsed: HashSet<usize>,
    /// First list row shown at the top of the sidebar.
    pub top: usize,
    /// The heading and row count the sidebar last scrolled to keep in view.
    /// It only re-follows when that changes, so a wheel scroll is not undone
    /// by the next frame.
    followed: Option<(usize, usize)>,
}

/// A key the focused sidebar understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TocKey {
    Up,
    Down,
    First,
    Last,
    PageUp,
    PageDown,
    /// Jump the document to the heading under the cursor.
    Jump,
    /// Give focus back to the document without moving it.
    Leave,
    /// Start typing a filter.
    StartFilter,
    FilterChar(char),
    FilterBack,
    /// Drop the filter, staying in the sidebar.
    ClearFilter,
    /// Fold the subtree under the cursor (or step to its parent).
    Collapse,
    /// Unfold it (or step to its first child).
    Expand,
}

/// How a row's subtree marker is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fold {
    /// No subheadings.
    Leaf,
    Open,
    Closed,
}

/// Whether `text` matches a filter `query`: every whitespace-separated word
/// of the query appears in it, ignoring case. An empty query matches all.
pub fn filter_matches(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}

impl TocState {
    #[allow(dead_code)]
    pub fn new(headings: &[Heading], line_mapping: &[(usize, usize)]) -> Self {
        let entries = headings
            .iter()
            .map(|h| {
                let line_index = line_mapping
                    .iter()
                    .find(|(offset, _)| *offset == h.byte_offset)
                    .map(|(_, line)| *line)
                    .unwrap_or(0);
                TocEntry {
                    level: h.level,
                    text: h.text.clone(),
                    line_index,
                    source_line: h.byte_offset,
                }
            })
            .collect();

        Self {
            visible: false,
            headings: entries,
            selected: 0,
            width: 30,
            nav: TocNav::default(),
        }
    }

    pub fn empty() -> Self {
        Self {
            visible: false,
            headings: Vec::new(),
            selected: 0,
            width: 30,
            nav: TocNav::default(),
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.leave();
        }
    }

    /// Update the selected heading based on the current scroll position.
    ///
    /// A heading on the viewport's second row already counts: that is where
    /// `n`/`N` and TOC jumps put it, under its blank spacing line, and the
    /// sidebar used to mark the heading before it.
    pub fn update_selection(&mut self, current_line: usize) {
        self.selected = self.current_heading(current_line).unwrap_or(0);
    }

    /// The heading a viewport whose top is `line` is in: the last one at or
    /// above it, one on its second row included (see
    /// [`Self::update_selection`]). `None` above the first heading.
    pub fn current_heading(&self, line: usize) -> Option<usize> {
        self.headings
            .iter()
            .rposition(|entry| entry.line_index <= line + 1)
    }

    /// Does heading `i` have subheadings?
    pub fn has_children(&self, i: usize) -> bool {
        match (self.headings.get(i), self.headings.get(i + 1)) {
            (Some(h), Some(next)) => next.level > h.level,
            _ => false,
        }
    }

    /// The fold marker for heading `i`.
    pub fn fold(&self, i: usize) -> Fold {
        if !self.has_children(i) {
            Fold::Leaf
        } else if self.nav.collapsed.contains(&i) {
            Fold::Closed
        } else {
            Fold::Open
        }
    }

    /// The headings listed in the sidebar, in order: those matching the
    /// filter while one is typed, otherwise every heading not inside a
    /// folded subtree.
    pub fn rows(&self) -> Vec<usize> {
        if let Some(query) = &self.nav.filter {
            return (0..self.headings.len())
                .filter(|&i| filter_matches(&self.headings[i].text, query))
                .collect();
        }
        let mut rows = Vec::with_capacity(self.headings.len());
        let mut folded_under: Option<u8> = None;
        for (i, h) in self.headings.iter().enumerate() {
            if let Some(level) = folded_under {
                if h.level > level {
                    continue;
                }
                folded_under = None;
            }
            rows.push(i);
            if self.nav.collapsed.contains(&i) && self.has_children(i) {
                folded_under = Some(h.level);
            }
        }
        rows
    }

    /// The row heading `i` is shown on, or the row of the nearest listed
    /// heading above it (its folded ancestor) when it is hidden.
    fn row_of(rows: &[usize], i: usize) -> Option<usize> {
        match rows.binary_search(&i) {
            Ok(r) => Some(r),
            Err(0) => (!rows.is_empty()).then_some(0),
            Err(r) => Some(r - 1),
        }
    }

    /// The heading the sidebar keeps in view: the cursor while focused,
    /// otherwise the one the document is scrolled to.
    fn anchor(&self) -> usize {
        if self.nav.focused {
            self.nav.cursor
        } else {
            self.selected
        }
    }

    /// The row highlighted in the sidebar.
    pub fn anchor_row(&self) -> Option<usize> {
        Self::row_of(&self.rows(), self.anchor())
    }

    /// The heading the cursor is on, if it is listed.
    pub fn selected_heading(&self) -> Option<usize> {
        let rows = self.rows();
        let r = Self::row_of(&rows, self.nav.cursor)?;
        rows.get(r).copied()
    }

    /// Move keyboard focus into the sidebar, the cursor on the current
    /// heading (or the folded heading that hides it).
    pub fn focus(&mut self) {
        self.nav.filter = None;
        self.nav.focused = true;
        let rows = self.rows();
        self.nav.cursor = Self::row_of(&rows, self.selected)
            .and_then(|r| rows.get(r).copied())
            .unwrap_or(0);
    }

    /// Give focus back to the document; any filter is dropped.
    pub fn leave(&mut self) {
        self.nav.focused = false;
        self.nav.filter = None;
    }

    /// Move the cursor `delta` rows, clamped to the list.
    pub fn move_by(&mut self, delta: isize) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let at = Self::row_of(&rows, self.nav.cursor).unwrap_or(0) as isize;
        let to = at.saturating_add(delta).clamp(0, rows.len() as isize - 1);
        self.nav.cursor = rows[to as usize];
    }

    /// Act on a key while focused. Returns the heading to jump the document
    /// to, if the key asked for one; the caller handles the jump.
    pub fn handle(&mut self, key: TocKey, page: usize) -> Option<usize> {
        let page = page.saturating_sub(1).max(1) as isize;
        match key {
            TocKey::Up => self.move_by(-1),
            TocKey::Down => self.move_by(1),
            TocKey::First => self.move_by(isize::MIN),
            TocKey::Last => self.move_by(isize::MAX),
            TocKey::PageUp => self.move_by(-page),
            TocKey::PageDown => self.move_by(page),
            TocKey::Jump => {
                let target = self.selected_heading();
                self.leave();
                return target;
            }
            TocKey::Leave => self.leave(),
            TocKey::StartFilter => {
                self.nav.filter = Some(String::new());
            }
            // A changed query puts the cursor on the best (first) match,
            // the way finders do.
            TocKey::FilterChar(c) => {
                if let Some(q) = &mut self.nav.filter {
                    q.push(c);
                }
                self.nav.cursor = self.rows().first().copied().unwrap_or(self.nav.cursor);
            }
            TocKey::FilterBack => {
                if let Some(q) = &mut self.nav.filter {
                    q.pop();
                }
                self.nav.cursor = self.rows().first().copied().unwrap_or(self.nav.cursor);
            }
            TocKey::ClearFilter => {
                self.nav.filter = None;
                self.snap_cursor_to_rows();
            }
            TocKey::Collapse => self.collapse(),
            TocKey::Expand => self.expand(),
        }
        None
    }

    /// After the list changed: keep the cursor where it is if that heading
    /// is still listed, otherwise put it on the first listed one after it
    /// (or the last one).
    fn snap_cursor_to_rows(&mut self) {
        let rows = self.rows();
        if rows.contains(&self.nav.cursor) {
            return;
        }
        if let Some(&next) = rows.iter().find(|&&i| i >= self.nav.cursor).or(rows.last()) {
            self.nav.cursor = next;
        }
    }

    fn parent(&self, i: usize) -> Option<usize> {
        let level = self.headings.get(i)?.level;
        (0..i).rev().find(|&j| self.headings[j].level < level)
    }

    /// Fold an open subtree; on a leaf or an already folded heading, step
    /// out to the parent (the way tree views do).
    fn collapse(&mut self) {
        if self.nav.filter.is_some() {
            return;
        }
        let i = self.nav.cursor;
        if self.fold(i) == Fold::Open {
            self.nav.collapsed.insert(i);
        } else if let Some(p) = self.parent(i) {
            self.nav.cursor = p;
        }
    }

    /// Unfold a folded subtree; on an open one, step into its first child.
    fn expand(&mut self) {
        if self.nav.filter.is_some() {
            return;
        }
        let i = self.nav.cursor;
        match self.fold(i) {
            Fold::Closed => {
                self.nav.collapsed.remove(&i);
            }
            Fold::Open => self.nav.cursor = i + 1,
            Fold::Leaf => {}
        }
    }

    /// Scroll the sidebar so the highlighted row is inside a list `height`
    /// rows tall. Called before each frame; only re-follows when the
    /// highlighted heading or the list changed since last time, so the
    /// reader can wheel the sidebar away from it.
    pub fn follow(&mut self, height: usize) {
        let rows = self.rows();
        let key = (self.anchor(), rows.len());
        if self.nav.followed != Some(key) {
            self.nav.followed = Some(key);
            if let Some(r) = Self::row_of(&rows, self.anchor()) {
                if r < self.nav.top {
                    self.nav.top = r;
                } else if height > 0 && r >= self.nav.top + height {
                    self.nav.top = r + 1 - height;
                }
            }
        }
        self.nav.top = self.nav.top.min(rows.len().saturating_sub(height.max(1)));
    }

    /// Wheel-scroll the sidebar by `delta` rows.
    pub fn scroll_by(&mut self, delta: isize, height: usize) {
        let max = self.rows().len().saturating_sub(height.max(1));
        self.nav.top = (self.nav.top as isize)
            .saturating_add(delta)
            .clamp(0, max as isize) as usize;
    }

    /// The heading on list row `y` of the sidebar (0 = first row under the
    /// title), as currently scrolled.
    pub fn heading_at(&self, y: usize) -> Option<usize> {
        self.rows().get(self.nav.top + y).copied()
    }

    /// Carry the interaction state over to a re-layout of the same document.
    pub fn inherit(&mut self, mut nav: TocNav) {
        let n = self.headings.len();
        nav.collapsed.retain(|&i| i < n);
        nav.cursor = nav.cursor.min(n.saturating_sub(1));
        nav.followed = None;
        self.nav = nav;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Alpha / ## Alpha one / ### Deep dive / ## Alpha two / # Beta /
    /// ## Beta one / # Gamma
    fn toc() -> TocState {
        let mut t = TocState::empty();
        t.headings = [
            (1, "Alpha"),
            (2, "Alpha one"),
            (3, "Deep dive"),
            (2, "Alpha two"),
            (1, "Beta"),
            (2, "Beta one"),
            (1, "Gamma"),
        ]
        .iter()
        .enumerate()
        .map(|(i, (level, text))| TocEntry {
            level: *level,
            text: (*text).to_string(),
            line_index: i * 10,
            source_line: i * 3 + 1,
        })
        .collect();
        t.visible = true;
        t
    }

    #[test]
    fn focus_starts_on_the_current_heading_and_moves_clamped() {
        let mut t = toc();
        t.update_selection(35); // inside "Alpha two"
        t.focus();
        assert!(t.nav.focused);
        assert_eq!(t.nav.cursor, 3);
        t.handle(TocKey::Down, 5);
        assert_eq!(t.nav.cursor, 4);
        t.handle(TocKey::Last, 5);
        assert_eq!(t.nav.cursor, 6);
        t.handle(TocKey::Down, 5);
        assert_eq!(t.nav.cursor, 6);
        t.handle(TocKey::First, 5);
        assert_eq!(t.nav.cursor, 0);
        t.handle(TocKey::Up, 5);
        assert_eq!(t.nav.cursor, 0);
        // A page is the list height less one row of overlap.
        t.handle(TocKey::PageDown, 4);
        assert_eq!(t.nav.cursor, 3);
        t.handle(TocKey::PageUp, 4);
        assert_eq!(t.nav.cursor, 0);
    }

    #[test]
    fn enter_returns_the_heading_and_leaves_focus() {
        let mut t = toc();
        t.focus();
        t.handle(TocKey::Down, 5);
        assert_eq!(t.handle(TocKey::Jump, 5), Some(1));
        assert!(!t.nav.focused);
        t.focus();
        assert_eq!(t.handle(TocKey::Leave, 5), None);
        assert!(!t.nav.focused);
    }

    #[test]
    fn collapsing_hides_the_subtree_in_the_sidebar_only() {
        let mut t = toc();
        t.focus();
        t.handle(TocKey::Collapse, 5); // fold Alpha
        assert_eq!(t.rows(), vec![0, 4, 5, 6]);
        assert_eq!(t.fold(0), Fold::Closed);
        assert_eq!(t.fold(4), Fold::Open);
        assert_eq!(t.fold(6), Fold::Leaf);
        // Moving skips the hidden rows.
        t.handle(TocKey::Down, 5);
        assert_eq!(t.nav.cursor, 4);
        t.handle(TocKey::Up, 5);
        t.handle(TocKey::Expand, 5);
        assert_eq!(t.rows().len(), 7);
        // Expand on an open heading steps into it; collapse on a leaf
        // steps out to the parent.
        t.handle(TocKey::Expand, 5);
        assert_eq!(t.nav.cursor, 1);
        t.handle(TocKey::Expand, 5);
        assert_eq!(t.nav.cursor, 2);
        t.handle(TocKey::Collapse, 5);
        assert_eq!(t.nav.cursor, 1);
        // The document's headings are untouched.
        assert_eq!(t.headings.len(), 7);
    }

    #[test]
    fn a_hidden_current_heading_highlights_its_folded_ancestor() {
        let mut t = toc();
        t.nav.collapsed.insert(0);
        t.update_selection(25); // "Deep dive", inside folded Alpha
        assert_eq!(t.anchor_row(), Some(0));
        t.focus();
        assert_eq!(t.nav.cursor, 0);
    }

    #[test]
    fn filter_is_case_insensitive_and_every_word_must_match() {
        assert!(filter_matches("Alpha Two", "two"));
        assert!(filter_matches("Alpha Two", "TWO al"));
        assert!(!filter_matches("Alpha Two", "alpha three"));
        assert!(filter_matches("anything", "  "));

        let mut t = toc();
        t.nav.collapsed.insert(0);
        t.focus();
        t.handle(TocKey::StartFilter, 5);
        for c in "one".chars() {
            t.handle(TocKey::FilterChar(c), 5);
        }
        // Folds do not hide filter matches.
        assert_eq!(t.rows(), vec![1, 5]);
        assert_eq!(t.nav.cursor, 1);
        t.handle(TocKey::Down, 5);
        assert_eq!(t.nav.cursor, 5);
        for _ in 0..3 {
            t.handle(TocKey::FilterBack, 5);
        }
        assert_eq!(t.rows().len(), 7);
        // Nothing matches: no heading to jump to.
        t.handle(TocKey::FilterChar('z'), 5);
        assert!(t.rows().is_empty());
        assert_eq!(t.selected_heading(), None);
        t.handle(TocKey::ClearFilter, 5);
        assert!(t.nav.focused && t.nav.filter.is_none());
        // Enter from a filtered list jumps to the match.
        t.handle(TocKey::StartFilter, 5);
        t.handle(TocKey::FilterChar('g'), 5);
        t.handle(TocKey::FilterChar('a'), 5);
        assert_eq!(t.handle(TocKey::Jump, 5), Some(6));
        assert!(t.nav.filter.is_none());
    }

    #[test]
    fn the_sidebar_scrolls_to_keep_the_cursor_visible_and_wheel_sticks() {
        let mut t = toc();
        t.focus();
        t.follow(3);
        assert_eq!(t.nav.top, 0);
        t.handle(TocKey::Last, 3);
        t.follow(3);
        assert_eq!(t.nav.top, 4);
        assert_eq!(t.heading_at(2), Some(6));
        t.handle(TocKey::First, 3);
        t.follow(3);
        assert_eq!(t.nav.top, 0);
        // The wheel moves the list; the next frame does not snap back.
        t.scroll_by(3, 3);
        t.follow(3);
        assert_eq!(t.nav.top, 3);
        assert_eq!(t.heading_at(0), Some(3));
        // Clamped at both ends.
        t.scroll_by(100, 3);
        assert_eq!(t.nav.top, 4);
        t.scroll_by(-100, 3);
        assert_eq!(t.nav.top, 0);
        assert_eq!(t.heading_at(7), None);
    }

    #[test]
    fn closing_the_sidebar_drops_focus_and_relayout_keeps_folds() {
        let mut t = toc();
        t.focus();
        t.handle(TocKey::Collapse, 5);
        t.toggle();
        assert!(!t.visible && !t.nav.focused);
        let mut fresh = toc();
        fresh.headings.truncate(3);
        let mut nav = t.nav.clone();
        nav.collapsed.insert(5);
        fresh.inherit(nav);
        assert_eq!(fresh.nav.collapsed, HashSet::from([0]));
    }
}
