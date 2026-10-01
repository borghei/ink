//! `--line-range`: keep only some SOURCE lines of a markdown document before
//! it is rendered.
//!
//! Ranges are 1-based and inclusive, and may overlap; the kept lines stay in
//! document order. A run of kept lines that starts or ends inside a fenced
//! code block gets the fence reopened (with the original info string, so it
//! is still highlighted) or closed, keeping the block well-formed; a closing
//! fence whose opener was cut off is dropped. Fences inside blockquotes
//! (`> ```py`) and list items (indented, or on the marker line) are tracked
//! too, and repaired with the same `>` prefix so the block stays in its
//! quote. A block reopened inside a list item loses the list (its marker
//! line is not in the range) and is drawn at the top level. Disjoint runs
//! are separated by a blank line so they do not merge into one paragraph.

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

/// An open fenced code block: the blockquote markers in front of it (as
/// written), the column of its fence after them, the fence and its info
/// string.
#[derive(Clone)]
struct Fence {
    quote: String,
    depth: usize,
    indent: usize,
    ch: u8,
    len: usize,
    info: String,
}

impl Fence {
    fn opener(&self) -> String {
        let fence = (self.ch as char).to_string().repeat(self.len);
        format!(
            "{}{}{fence}{}\n",
            self.quote,
            " ".repeat(self.indent),
            self.info
        )
    }

    fn closer(&self) -> String {
        let fence = (self.ch as char).to_string().repeat(self.len);
        format!("{}{}{fence}\n", self.quote, " ".repeat(self.indent))
    }

    /// This fence as reopened mid-block: its list context is gone, so at
    /// most three spaces of indent (four would make an indented code block).
    fn reopened(&self) -> Fence {
        Fence {
            indent: self.indent.min(3),
            ..self.clone()
        }
    }
}

/// The blockquote markers at the start of `line` (each up to three spaces,
/// `>`, an optional space): their byte length and how many there are.
fn quote_prefix(line: &str) -> (usize, usize) {
    let b = line.as_bytes();
    let (mut end, mut depth) = (0usize, 0usize);
    loop {
        let mut j = end;
        while j < b.len() && j - end < 3 && b[j] == b' ' {
            j += 1;
        }
        if b.get(j) != Some(&b'>') {
            return (end, depth);
        }
        j += 1;
        if b.get(j) == Some(&b' ') {
            j += 1;
        }
        end = j;
        depth += 1;
    }
}

/// The width of a list marker (`-`, `*`, `+`, `1.`, `1)`) and the spaces
/// after it at the start of `s`, or 0.
fn list_marker(s: &str) -> usize {
    let b = s.as_bytes();
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    let marker = match b.first() {
        Some(b'-' | b'*' | b'+') => 1,
        _ if (1..=9).contains(&digits) && matches!(b.get(digits), Some(b'.' | b')')) => digits + 1,
        _ => return 0,
    };
    let spaces = b[marker..].iter().take_while(|&&c| c == b' ').count();
    if spaces == 0 {
        0
    } else {
        marker + spaces
    }
}

/// A line read for fences: its blockquote markers and, when it is a fence
/// (three or more backticks or tildes after the markers, any indentation
/// and an optional list marker), the fence.
struct FenceLine<'s> {
    quote: &'s str,
    depth: usize,
    fence: Option<(usize, u8, usize, &'s str)>,
}

fn fence_line(line: &str) -> FenceLine<'_> {
    let (end, depth) = quote_prefix(line);
    let rest = &line[end..];
    let spaces = rest.bytes().take_while(|&b| b == b' ').count();
    let col = spaces + list_marker(&rest[spaces..]);
    let fence = (|| {
        let f = &rest[col..];
        let ch = *f.as_bytes().first()?;
        if ch != b'`' && ch != b'~' {
            return None;
        }
        let len = f.bytes().take_while(|&b| b == ch).count();
        if len < 3 {
            return None;
        }
        let info = f[len..].trim_end_matches(['\n', '\r']);
        if ch == b'`' && info.contains('`') {
            return None;
        }
        Some((col, ch, len, info))
    })();
    FenceLine {
        quote: &line[..end],
        depth,
        fence,
    }
}

fn kept_at(ranges: &[LineRange], n: usize) -> bool {
    ranges.iter().any(|r| r.contains(n))
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
        let fl = fence_line(line);
        // A line with fewer `>` than the open block's ends the blockquote,
        // and the code block with it (no closing fence needed).
        if open.as_ref().is_some_and(|f| fl.depth < f.depth) {
            open = None;
            if kept_at(ranges, n) && prev_kept {
                // The output's copy ends the same way.
                out_open = None;
            }
        }
        let (inside_before, closes, opens) = match (&open, fl.fence) {
            // A closer is at the opener's quote depth (a deeper `> ```` is
            // content of the block).
            (Some(f), Some((_, ch, len, info)))
                if fl.depth == f.depth && ch == f.ch && len >= f.len && info.trim().is_empty() =>
            {
                (true, true, None)
            }
            (Some(_), _) => (true, false, None),
            (None, Some((indent, ch, len, info))) => (
                false,
                false,
                Some(Fence {
                    quote: fl.quote.to_string(),
                    depth: fl.depth,
                    indent,
                    ch,
                    len,
                    info: info.to_string(),
                }),
            ),
            (None, None) => (false, false, None),
        };

        let kept = kept_at(ranges, n);
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
                        let f = f.reopened();
                        out.push_str(&f.opener());
                        out_open = Some(f);
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

    // Regression: only fences at indent 0-3 of the raw line were seen, so a
    // block in a blockquote was neither reopened (`7` printed the code as
    // quote prose) nor had its orphan closer dropped (`7:` opened an empty
    // code box that swallowed the rest).
    #[test]
    fn fences_inside_blockquotes_are_repaired_in_the_quote() {
        let doc = "# T\n\nText\n\n> Quote\n> ```py\n> x = 1\n> y = 2\n> ```\n\nafter\n";
        assert_eq!(select(doc, &[r("7")]), "> ```py\n> x = 1\n> ```\n");
        assert_eq!(
            select(doc, &[r("8:")]),
            "> ```py\n> y = 2\n> ```\n\nafter\n"
        );
        // Starting on the closer: dropped.
        assert_eq!(select(doc, &[r("9:")]), "\nafter\n");
        // Starting at the opener, ending inside: closed in the quote.
        assert_eq!(select(doc, &[r("6:7")]), "> ```py\n> x = 1\n> ```\n");
        // A deeper `> ```` inside a top-level block is content, not a closer.
        let doc = "```\n> ```\nz\n```\n";
        assert_eq!(select(doc, &[r("3")]), "```\nz\n```\n");
    }

    #[test]
    fn nested_blockquote_fences_keep_every_marker() {
        let doc = "> > ~~~ sh\n> > ls\n> > pwd\n> > ~~~\n> > after\n";
        assert_eq!(select(doc, &[r("3")]), "> > ~~~ sh\n> > pwd\n> > ~~~\n");
        assert_eq!(select(doc, &[r("4:")]), "> > after\n");
        // A line without the markers ends the quote and the block with it.
        let doc = "> ```\n> a\nplain\n";
        assert_eq!(select(doc, &[r("2:")]), "> ```\n> a\nplain\n");
    }

    #[test]
    fn fences_inside_list_items_are_tracked() {
        let doc = "- item\n\n    ```rust\n    let a = 1;\n    let b = 2;\n    ```\n- next\n";
        // Reopened without the list: a well-formed top-level block (at most
        // three spaces of indent, never an indented code block).
        assert_eq!(
            select(doc, &[r("5")]),
            "   ```rust\n    let b = 2;\n   ```\n"
        );
        assert_eq!(select(doc, &[r("6:")]), "- next\n");
        // Kept from the item's start: closed at the fence's own indent.
        assert_eq!(
            select(doc, &[r("1:4")]),
            "- item\n\n    ```rust\n    let a = 1;\n    ```\n"
        );
        // The fence on the marker line itself.
        let doc = "1. ```sh\n   ls\n   ```\nafter\n";
        assert_eq!(select(doc, &[r("2")]), "   ```sh\n   ls\n   ```\n");
        assert_eq!(select(doc, &[r("3:")]), "after\n");
    }

    #[test]
    fn a_closer_longer_than_its_opener_closes_it() {
        let doc = "```\na\n`````\nb\n";
        assert_eq!(select(doc, &[r("2")]), "```\na\n```\n");
        assert_eq!(select(doc, &[r("3:")]), "b\n");
    }

    #[test]
    fn tilde_and_long_fences_are_tracked() {
        let doc = "~~~~ py\n```\nx\n~~~~\ny\n";
        // The ``` line is content of the ~~~~ block, not a fence of its own.
        assert_eq!(select(doc, &[r("3")]), "~~~~ py\nx\n~~~~\n");
        assert_eq!(select(doc, &[r("5")]), "y\n");
    }
}
