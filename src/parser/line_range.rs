//! `--line-range`: keep only some SOURCE lines of a markdown document before
//! it is rendered.
//!
//! Ranges are 1-based and inclusive, and may overlap; the kept lines stay in
//! document order. A run of kept lines that starts or ends inside a fenced
//! code block gets the fence reopened (with the original info string, so it
//! is still highlighted) or closed, keeping the block well-formed. Disjoint
//! runs are separated by a blank line so they do not merge into one
//! paragraph.

/// An inclusive, 1-based line range; `None` bounds are open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl LineRange {
    /// Parse `N`, `START:END`, `START:` or `:END`.
    pub fn parse(s: &str) -> Result<Self, String> {
        let num = |t: &str| -> Result<usize, String> {
            match t.trim().parse::<usize>() {
                Ok(0) => Err("line numbers start at 1".to_string()),
                Ok(n) => Ok(n),
                Err(_) => Err(format!(
                    "expected N, START:END, START: or :END (1-based line numbers), got {s:?}"
                )),
            }
        };
        let range = match s.split_once(':') {
            None => {
                let n = num(s)?;
                LineRange {
                    start: Some(n),
                    end: Some(n),
                }
            }
            Some((a, b)) => {
                let start = (!a.trim().is_empty()).then(|| num(a)).transpose()?;
                let end = (!b.trim().is_empty()).then(|| num(b)).transpose()?;
                if start.is_none() && end.is_none() {
                    return Err("a range needs a start, an end, or both".to_string());
                }
                LineRange { start, end }
            }
        };
        if let (Some(a), Some(b)) = (range.start, range.end) {
            if b < a {
                return Err(format!("the range ends ({b}) before it starts ({a})"));
            }
        }
        Ok(range)
    }

    fn contains(&self, line: usize) -> bool {
        self.start.is_none_or(|s| line >= s) && self.end.is_none_or(|e| line <= e)
    }
}

/// An open fenced code block: its indentation, fence and info string.
#[derive(Clone)]
struct Fence {
    indent: usize,
    ch: u8,
    len: usize,
    info: String,
}

impl Fence {
    fn opener(&self) -> String {
        let fence = (self.ch as char).to_string().repeat(self.len);
        format!("{}{fence}{}\n", " ".repeat(self.indent), self.info)
    }

    fn closer(&self) -> String {
        let fence = (self.ch as char).to_string().repeat(self.len);
        format!("{}{fence}\n", " ".repeat(self.indent))
    }
}

/// A fence line: up to three spaces, then three or more backticks or tildes.
fn fence_marker(line: &str) -> Option<(usize, u8, usize, &str)> {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ch = *rest.as_bytes().first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let len = rest.bytes().take_while(|&b| b == ch).count();
    if len < 3 {
        return None;
    }
    let info = rest[len..].trim_end_matches(['\n', '\r']);
    if ch == b'`' && info.contains('`') {
        return None;
    }
    Some((indent, ch, len, info))
}

/// The lines of `source` that fall in any of `ranges`, fences repaired.
/// No ranges: `source` unchanged.
pub fn select(source: &str, ranges: &[LineRange]) -> String {
    if ranges.is_empty() {
        return source.to_string();
    }
    let mut out = String::new();
    // The fence open at the current line (in the full document).
    let mut open: Option<Fence> = None;
    // Whether the previous line was kept, and the fence the output has open.
    let mut prev_kept = false;
    let mut out_open: Option<Fence> = None;
    let mut any_kept = false;

    for (i, line) in source.split_inclusive('\n').enumerate() {
        let n = i + 1;
        // Classify this line against the document's own fences.
        let marker = fence_marker(line);
        let (inside_before, closes, opens) = match (&open, marker) {
            (Some(f), Some((_, ch, len, info)))
                if ch == f.ch && len >= f.len && info.trim().is_empty() =>
            {
                (true, true, None)
            }
            (Some(_), _) => (true, false, None),
            (None, Some((indent, ch, len, info))) => (
                false,
                false,
                Some(Fence {
                    indent,
                    ch,
                    len,
                    info: info.to_string(),
                }),
            ),
            (None, None) => (false, false, None),
        };

        let kept = ranges.iter().any(|r| r.contains(n));
        if kept {
            if !prev_kept {
                if any_kept {
                    // A gap: close what the output left open, then a blank
                    // line so the runs stay separate blocks.
                    if let Some(f) = out_open.take() {
                        out.push_str(&f.closer());
                    }
                    out.push('\n');
                }
                // Starting inside a block body: reopen it.
                if inside_before && !closes {
                    if let Some(f) = &open {
                        out.push_str(&f.opener());
                        out_open = Some(f.clone());
                    }
                }
            }
            // A closing fence whose opener was not kept would open a new
            // block in the output: drop it.
            if !(closes && out_open.is_none()) {
                out.push_str(line);
                if !line.ends_with('\n') {
                    out.push('\n');
                }
            }
            any_kept = true;
            if closes {
                out_open = None;
            } else if let Some(f) = &opens {
                out_open = Some(f.clone());
            }
        }
        prev_kept = kept;

        if closes {
            open = None;
        } else if opens.is_some() {
            open = opens;
        }
    }
    if let Some(f) = out_open {
        out.push_str(&f.closer());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> LineRange {
        LineRange::parse(s).unwrap()
    }

    #[test]
    fn parses_every_form() {
        assert_eq!(
            r("3"),
            LineRange {
                start: Some(3),
                end: Some(3)
            }
        );
        assert_eq!(
            r("2:5"),
            LineRange {
                start: Some(2),
                end: Some(5)
            }
        );
        assert_eq!(
            r("2:"),
            LineRange {
                start: Some(2),
                end: None
            }
        );
        assert_eq!(
            r(":5"),
            LineRange {
                start: None,
                end: Some(5)
            }
        );
        for bad in ["", ":", "0", "0:3", "5:2", "a", "1:b", "-1", "1:2:3", "1-3"] {
            assert!(LineRange::parse(bad).is_err(), "{bad:?}");
        }
    }

    const DOC: &str = "a\nb\nc\nd\ne\n";

    #[test]
    fn selects_inclusive_ranges_in_document_order() {
        assert_eq!(select(DOC, &[r("2:3")]), "b\nc\n");
        assert_eq!(select(DOC, &[r("4:")]), "d\ne\n");
        assert_eq!(select(DOC, &[r(":2")]), "a\nb\n");
        assert_eq!(select(DOC, &[r("3")]), "c\n");
        // Overlapping and out-of-order ranges: the union, in order.
        assert_eq!(select(DOC, &[r("4:5"), r("2:4")]), "b\nc\nd\ne\n");
        // Disjoint runs are kept apart by a blank line.
        assert_eq!(select(DOC, &[r("1"), r("4")]), "a\n\nd\n");
        // Past the end: nothing.
        assert_eq!(select(DOC, &[r("9:")]), "");
        assert_eq!(select(DOC, &[]), DOC);
    }

    #[test]
    fn a_range_inside_a_fence_reopens_and_closes_it() {
        let doc = "intro\n```rust title=x\nfn a() {}\nfn b() {}\nfn c() {}\n```\nafter\n";
        assert_eq!(select(doc, &[r("4")]), "```rust title=x\nfn b() {}\n```\n");
        // Starting at the opener, ending inside: closed.
        assert_eq!(
            select(doc, &[r("2:3")]),
            "```rust title=x\nfn a() {}\n```\n"
        );
        // Starting inside, running past the closer: reopened only.
        assert_eq!(
            select(doc, &[r("5:")]),
            "```rust title=x\nfn c() {}\n```\nafter\n"
        );
        // Two runs in one block: each well-formed.
        assert_eq!(
            select(doc, &[r("3"), r("5")]),
            "```rust title=x\nfn a() {}\n```\n\n```rust title=x\nfn c() {}\n```\n"
        );
        // Starting on the closing fence: it is dropped, not left to open
        // a block of its own.
        assert_eq!(select(doc, &[r("6:")]), "after\n");
    }

    #[test]
    fn tilde_and_long_fences_are_tracked() {
        let doc = "~~~~ py\n```\nx\n~~~~\ny\n";
        // The ``` line is content of the ~~~~ block, not a fence of its own.
        assert_eq!(select(doc, &[r("3")]), "~~~~ py\nx\n~~~~\n");
        assert_eq!(select(doc, &[r("5")]), "y\n");
    }
}
