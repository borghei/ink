/// Search state for in-document search.
#[derive(Debug, Default)]
pub struct SearchState {
    pub active: bool,
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub current_match: usize,
}

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub line_index: usize,
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
    }

    /// Find all lines containing the query. `lowered` is the document's
    /// per-line text, pre-lowercased once at build time — so a keystroke only
    /// scans (no per-line allocation) instead of rebuilding + lowercasing the
    /// whole document each time.
    pub fn update_matches(&mut self, lowered: &[String]) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }
        // Char-level lowercasing — identical to how the per-tab `lowered`
        // lines and the highlighter lowercase, so both sides always agree
        // (str::to_lowercase applies Greek final-sigma rules chars don't).
        let query_lower: String = self.query.chars().flat_map(char::to_lowercase).collect();

        for (line_idx, text_lower) in lowered.iter().enumerate() {
            if text_lower.contains(&query_lower) {
                self.matches.push(SearchMatch {
                    line_index: line_idx,
                });
            }
        }

        if self.matches.is_empty() || self.current_match >= self.matches.len() {
            self.current_match = 0;
        }
    }

    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = if self.current_match == 0 {
                self.matches.len() - 1
            } else {
                self.current_match - 1
            };
        }
    }

    pub fn current_line(&self) -> Option<usize> {
        self.matches.get(self.current_match).map(|m| m.line_index)
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Check if a given line index has a match. `matches` is built in
    /// ascending line order, so this is a binary search, not a linear scan.
    pub fn is_match_line(&self, line_idx: usize) -> bool {
        self.matches
            .binary_search_by_key(&line_idx, |m| m.line_index)
            .is_ok()
    }

    /// Check if a given line is the current match.
    pub fn is_current_match_line(&self, line_idx: usize) -> bool {
        self.matches
            .get(self.current_match)
            .map(|m| m.line_index == line_idx)
            .unwrap_or(false)
    }
}
