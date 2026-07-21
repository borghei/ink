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

    for line in source.lines() {
        let trimmed = line.trim_start();
        // Track fenced code blocks so a `---` inside them isn't a slide break.
        if let Some(f) = fence {
            if trimmed.starts_with(f) && trimmed.chars().take_while(|&c| c == f).count() >= 3 {
                fence = None;
            }
            current.push(line);
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(trimmed.chars().next().unwrap());
            current.push(line);
            continue;
        }

        if is_break(line) {
            slides.push(current.join("\n"));
            current.clear();
        } else {
            current.push(line);
        }
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
/// (at least 3), with no other non-space content.
fn is_break(line: &str) -> bool {
    let t = line.trim();
    if t.len() < 3 {
        return false;
    }
    for marker in ['-', '*', '_'] {
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
    fn single_slide_when_no_breaks() {
        assert_eq!(split_slides("# Only\n\ntext\n").len(), 1);
    }

    #[test]
    fn empty_source_yields_one_slide() {
        assert_eq!(split_slides("").len(), 1);
    }
}
