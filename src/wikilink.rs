/// Pre-process markdown source to convert wikilinks to standard markdown links.
///
/// Converts:
/// - `[[target]]` → `[target](target.md)`
/// - `[[target|display]]` → `[display](target.md)`
/// - `[[target.pdf]]` → `[target.pdf](target.pdf)` (keeps existing extensions)
///
/// Skips wikilinks inside fenced code blocks and inline code. When no
/// wikilinks are present the output equals the input byte for byte.
pub fn process_wikilinks(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    // Open fence: (marker char, run length). Per CommonMark, a fence closes
    // only on a line made of >= that many of the SAME marker (and nothing
    // else) — a ``` line inside a ~~~ block is content, not a toggle.
    let mut fence: Option<(char, usize)> = None;

    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();

        if let Some((ch, len)) = fence {
            let run = trimmed.chars().take_while(|&c| c == ch).count();
            if run >= len && trimmed.chars().all(|c| c == ch) {
                fence = None;
            }
            result.push_str(line);
            continue;
        }

        if let Some(ch @ ('`' | '~')) = trimmed.chars().next() {
            let run = trimmed.chars().take_while(|&c| c == ch).count();
            if run >= 3 {
                fence = Some((ch, run));
                result.push_str(line);
                continue;
            }
        }

        process_line(line, &mut result);
    }

    result
}

/// Convert wikilinks in a single line (which may include its trailing
/// newline), leaving inline code spans untouched.
fn process_line(line: &str, result: &mut String) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Inline code: a run of N backticks opens a span that only a run of
        // exactly N backticks closes. An unmatched run is literal text.
        if bytes[i] == b'`' {
            let run = backtick_run(bytes, i);
            if let Some(close) = find_closing_run(bytes, i + run, run) {
                result.push_str(&line[i..close + run]);
                i = close + run;
            } else {
                result.push_str(&line[i..i + run]);
                i += run;
            }
            continue;
        }

        // Check for wikilink: [[...]]
        if i + 1 < len && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(close) = find_close_brackets(bytes, i + 2) {
                let inner = &line[i + 2..close];
                if !inner.is_empty() && !inner.contains('\n') {
                    let (display, target_path) = if let Some(pipe) = inner.find('|') {
                        let target = &inner[..pipe];
                        let display = &inner[pipe + 1..];
                        (display.to_string(), normalize_target(target))
                    } else {
                        (inner.to_string(), normalize_target(inner))
                    };
                    result.push_str(&format!("[{display}]({target_path})"));
                    i = close + 2; // skip past ]]
                    continue;
                }
            }
        }

        // Regular character (UTF-8 safe)
        let ch = line[i..].chars().next().unwrap_or('?');
        result.push(ch);
        i += ch.len_utf8();
    }
}

/// Length of the backtick run starting at `start`.
fn backtick_run(bytes: &[u8], start: usize) -> usize {
    bytes[start..].iter().take_while(|&&b| b == b'`').count()
}

/// Find the start of the next backtick run of exactly `n` backticks at or
/// after `start`.
fn find_closing_run(bytes: &[u8], start: usize, n: usize) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let run = backtick_run(bytes, i);
            if run == n {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

/// Find the position of `]]` starting from `start`.
fn find_close_brackets(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Normalize a wikilink target to a file path.
/// Adds `.md` extension if the target doesn't already have one.
fn normalize_target(target: &str) -> String {
    let target = target.trim();
    if target.contains('.') {
        target.to_string()
    } else {
        format!("{target}.md")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_wikilink() {
        assert_eq!(
            process_wikilinks("See [[my page]] for details"),
            "See [my page](my page.md) for details"
        );
    }

    #[test]
    fn wikilink_with_display() {
        assert_eq!(
            process_wikilinks("See [[target|click here]]"),
            "See [click here](target.md)"
        );
    }

    #[test]
    fn wikilink_with_extension() {
        assert_eq!(
            process_wikilinks("See [[doc.pdf]]"),
            "See [doc.pdf](doc.pdf)"
        );
    }

    #[test]
    fn skip_code_block() {
        let input = "```\n[[not a link]]\n```";
        assert_eq!(process_wikilinks(input), input);
    }

    #[test]
    fn skip_inline_code() {
        let input = "Use `[[not a link]]` syntax";
        assert_eq!(process_wikilinks(input), input);
    }

    #[test]
    fn no_wikilinks() {
        let input = "Just normal [markdown](link.md) text";
        assert_eq!(process_wikilinks(input), input);
    }

    #[test]
    fn backtick_fence_inside_tilde_fence_does_not_toggle() {
        // A ``` line inside a ~~~ block is content: the fence stays open
        // until the matching ~~~, and links after it still convert.
        let input = "~~~\nfence demo:\n```\n[[real link]]\n~~~\n\nLater: [[notes]]\n";
        let expected = "~~~\nfence demo:\n```\n[[real link]]\n~~~\n\nLater: [notes](notes.md)\n";
        assert_eq!(process_wikilinks(input), expected);
    }

    #[test]
    fn tilde_fence_inside_backtick_fence_does_not_toggle() {
        let input = "```\n~~~\n[[not a link]]\n```\n[[link]]";
        assert_eq!(
            process_wikilinks(input),
            "```\n~~~\n[[not a link]]\n```\n[link](link.md)"
        );
    }

    #[test]
    fn longer_fence_needed_to_close() {
        // A four-backtick fence is not closed by three backticks.
        let input = "````\n```\n[[not a link]]\n````\n[[link]]";
        assert_eq!(
            process_wikilinks(input),
            "````\n```\n[[not a link]]\n````\n[link](link.md)"
        );
    }

    #[test]
    fn double_backtick_span_protects_wikilink() {
        let input = "Use ``[[a|b]]`` here";
        assert_eq!(process_wikilinks(input), input);
    }

    #[test]
    fn unmatched_backtick_is_literal() {
        // A lone backtick does not swallow the rest of the line.
        assert_eq!(
            process_wikilinks("a ` stray and [[link]]"),
            "a ` stray and [link](link.md)"
        );
    }

    #[test]
    fn preserves_exact_trailing_newline() {
        // With a trailing newline: byte-for-byte identity when no wikilinks.
        let with_nl = "# Title\n\nSome text\n";
        assert_eq!(process_wikilinks(with_nl), with_nl);
        // Without one: also identical, no newline appended.
        let without_nl = "# Title\n\nSome text";
        assert_eq!(process_wikilinks(without_nl), without_nl);
    }
}
