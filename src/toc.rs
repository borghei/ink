use crate::parser::Heading;

/// Table of contents state.
#[derive(Debug)]
pub struct TocState {
    pub visible: bool,
    pub headings: Vec<TocEntry>,
    pub selected: usize,
    pub width: u16,
}

#[derive(Debug, Clone)]
pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub line_index: usize,
    /// 1-based line in the markdown source, for copying a section's source.
    pub source_line: usize,
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
        }
    }

    pub fn empty() -> Self {
        Self {
            visible: false,
            headings: Vec::new(),
            selected: 0,
            width: 30,
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Update the selected heading based on the current scroll position.
    pub fn update_selection(&mut self, current_line: usize) {
        for (i, entry) in self.headings.iter().enumerate().rev() {
            if entry.line_index <= current_line {
                self.selected = i;
                return;
            }
        }
        self.selected = 0;
    }
}
