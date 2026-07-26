//! Presentation mode: split a markdown document into slides on top-level
//! horizontal rules (`---`), ignoring `---` that sits inside a fenced code
//! block or is really YAML frontmatter.

/// Split `source` into slide sources. A slide break is a line that is exactly
/// `---` (or `***` / `___`, the other thematic-break markers) at column 0 and
/// outside any code fence. Always returns at least one slide.
pub fn split_slides(source: &str) -> Vec<String> {
    let mut slides: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut fence: Option<char> = None;
    // Whether the previous line was blank (or absent): a `---` right under
    // text is a setext H2 underline, not a slide break.
    let mut prev_blank = true;

    for line in source.lines() {
        let trimmed = line.trim_start();
        // Track fenced code blocks so a `---` inside them isn't a slide break.
        if let Some(f) = fence {
            if trimmed.starts_with(f) && trimmed.chars().take_while(|&c| c == f).count() >= 3 {
                fence = None;
            }
            current.push(line);
            prev_blank = line.trim().is_empty();
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(trimmed.chars().next().unwrap());
            current.push(line);
            prev_blank = false;
            continue;
        }

        if is_break(line, prev_blank) {
            slides.push(current.join("\n"));
            current.clear();
        } else {
            current.push(line);
        }
        prev_blank = line.trim().is_empty();
    }
    slides.push(current.join("\n"));

    // Drop empty leading/trailing slides (e.g. a document that starts with ---)
    // but keep at least one slide so the viewer always has content.
    let mut result: Vec<String> = slides
        .into_iter()
        .map(|s| s.trim_matches('\n').to_string())
        .filter(|s| !s.trim().is_empty())
        .collect();
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

/// A thematic break used as a slide separator: a line of only `-`, `*`, or `_`
/// (at least 3), with no other non-space content and fewer than 4 leading
/// spaces (4+ is indented code). A dash line directly under non-blank text is
/// a setext H2 underline, so `---` counts only after a blank (or absent)
/// previous line.
fn is_break(line: &str, prev_blank: bool) -> bool {
    let indent = line.chars().take_while(|&c| c == ' ').count();
    if indent >= 4 {
        return false;
    }
    let t = line.trim();
    if t.len() < 3 {
        return false;
    }
    if t.chars().all(|c| c == '-') {
        return prev_blank;
    }
    for marker in ['*', '_'] {
        if t.chars().all(|c| c == marker) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_top_level_rule() {
        let s = "# A\n\nfirst\n\n---\n\n# B\n\nsecond\n";
        let slides = split_slides(s);
        assert_eq!(slides.len(), 2);
        assert!(slides[0].contains("first"));
        assert!(slides[1].contains("second"));
    }

    #[test]
    fn ignores_rule_inside_code_fence() {
        let s = "# A\n\n```\n---\n```\n\nstill slide one\n";
        let slides = split_slides(s);
        assert_eq!(slides.len(), 1);
        assert!(slides[0].contains("still slide one"));
    }

    #[test]
    fn setext_underline_is_not_a_break() {
        // `---` directly under text is a setext H2 underline.
        let s = "Heading Two\n---\n\nbody\n";
        let slides = split_slides(s);
        assert_eq!(slides.len(), 1);
        assert!(slides[0].contains("Heading Two"));
        assert!(slides[0].contains("---"));
    }

    #[test]
    fn indented_dashes_are_not_a_break() {
        // 4+ leading spaces is indented code, not a thematic break.
        let s = "# A\n\n    ---\n\nstill slide one\n";
        let slides = split_slides(s);
        assert_eq!(slides.len(), 1);
        assert!(slides[0].contains("still slide one"));
    }

    #[test]
    fn rule_after_blank_line_still_breaks() {
        let s = "para\n\n---\n\nnext\n";
        let slides = split_slides(s);
        assert_eq!(slides.len(), 2);
        assert!(slides[0].contains("para"));
        assert!(slides[1].contains("next"));
    }

    #[test]
    fn rule_on_first_line_breaks() {
        // Absent previous line counts as blank; the empty leading slide is
        // dropped by the existing filter.
        let s = "---\n\nonly slide\n";
        let slides = split_slides(s);
        assert_eq!(slides.len(), 1);
        assert!(slides[0].contains("only slide"));
    }

    #[test]
    fn single_slide_when_no_breaks() {
        assert_eq!(split_slides("# Only\n\ntext\n").len(), 1);
    }

    #[test]
    fn empty_source_yields_one_slide() {
        assert_eq!(split_slides("").len(), 1);
    }
}
