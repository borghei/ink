//! Text helpers shared by the diagram renderers: label decoding, display
//! width, wrapping and truncation. Everything here measures in terminal
//! columns, grapheme by grapheme, the same way the canvas places text.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Display width of `s` in terminal columns, summed per grapheme cluster
/// (the unit the canvas places), so measurement and placement agree.
pub fn width(s: &str) -> usize {
    s.graphemes(true).map(UnicodeWidthStr::width).sum()
}

/// Make diagram source safe to measure: drop terminal control characters
/// (ESC, C0, C1, DEL — a label is untrusted document text), turn tabs into
/// spaces and CRLF into LF. The layout sanitizer runs again on the output;
/// this pass exists so control bytes never skew width arithmetic.
pub fn clean_source(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for c in src.chars() {
        match c {
            '\n' => out.push('\n'),
            '\t' => out.push_str("    "),
            c if c.is_control() || ('\u{80}'..='\u{9f}').contains(&c) => {}
            // Zero-width format characters that some terminals render
            // unpredictably (bidi overrides, isolates).
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Decode a mermaid label as written in the source into display text:
/// surrounding quotes and markdown-string backticks removed, `<br>` turned
/// into a line break, simple HTML tags dropped, HTML entities and mermaid's
/// `#name;` / `#123;` codes decoded. Lines are trimmed; the result uses `\n`
/// between lines.
pub fn decode_label(raw: &str) -> String {
    let mut s = raw.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s = &s[1..s.len() - 1];
    }
    let mut s = s.trim().to_string();
    // Markdown strings: "`**bold** text`".
    if s.len() >= 2 && s.starts_with('`') && s.ends_with('`') {
        s = s[1..s.len() - 1].replace("**", "");
    }
    let s = replace_tags(&s);
    let s = decode_entities(&s);
    // Font Awesome icon tokens (`fa:fa-car`) have no terminal rendering.
    let is_icon = |w: &&str| {
        w.split_once(":fa-")
            .is_some_and(|(p, _)| matches!(p, "fa" | "fab" | "fas" | "far" | "fal" | "fak"))
    };
    let lines: Vec<String> = s
        .split('\n')
        .map(|l| {
            l.split_whitespace()
                .filter(|w| !is_icon(w))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    // Drop leading/trailing empty lines but keep interior ones.
    let start = lines
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(start, |e| e + 1);
    lines[start..end].join("\n")
}

/// `<br>`, `<br/>`, `<br />` become line breaks; other simple tags
/// (`<b>`, `</i>`, `<span ...>`) are removed. Anything that does not look
/// like a tag is kept as text.
fn replace_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('>') else {
            out.push_str(&rest[open..]);
            return out;
        };
        // A tag starts right after `<` (`a < b > c` is text).
        if !after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') {
            out.push('<');
            rest = after;
            continue;
        }
        let inner = after[..close].trim();
        let name = inner.trim_start_matches('/').trim_end_matches('/').trim();
        let tag = name.split(|c: char| c.is_whitespace()).next().unwrap_or("");
        let is_tag = !tag.is_empty()
            && tag.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && tag.chars().all(|c| c.is_ascii_alphanumeric());
        if is_tag {
            if tag.eq_ignore_ascii_case("br") {
                out.push('\n');
            }
            rest = &after[close + 1..];
        } else {
            out.push('<');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "hash" | "num" => '#',
        "semi" => ';',
        "colon" => ':',
        "copy" => '©',
        "reg" => '®',
        "deg" => '°',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "hearts" => '♥',
        _ => return None,
    })
}

/// Decode `&name;`, `&#123;`, `&#x1F;` and mermaid's `#name;` / `#123;`.
/// Decoded control characters are dropped.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '&' || c == '#' {
            // Look ahead for `;` within a short window.
            let mut j = i + 1;
            let start = if c == '&' && chars.get(j) == Some(&'#') {
                j += 1;
                j
            } else {
                j
            };
            while j < chars.len() && j - i <= 10 && chars[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j < chars.len() && chars[j] == ';' && j > start {
                let body: String = chars[start..j].iter().collect();
                let numeric = c == '#' || start == i + 2;
                let decoded = if numeric {
                    if let Some(hex) = body.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                    } else if body.chars().all(|d| d.is_ascii_digit()) {
                        body.parse::<u32>().ok().and_then(char::from_u32)
                    } else if c == '#' {
                        named_entity(&body)
                    } else {
                        None
                    }
                } else {
                    named_entity(&body)
                };
                if let Some(d) = decoded {
                    if !d.is_control() {
                        out.push(d);
                    }
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Word-wrap `text` (which may contain `\n`) to at most `max` columns per
/// line. Words longer than `max` are broken at grapheme boundaries. Always
/// returns at least one line.
pub fn wrap(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut lw = 0;
        for word in para.split(' ').filter(|w| !w.is_empty()) {
            let ww = width(word);
            if ww > max {
                if lw > 0 {
                    out.push(std::mem::take(&mut line));
                    lw = 0;
                }
                for g in word.graphemes(true) {
                    let gw = UnicodeWidthStr::width(g);
                    if lw + gw > max && lw > 0 {
                        out.push(std::mem::take(&mut line));
                        lw = 0;
                    }
                    line.push_str(g);
                    lw += gw;
                }
                continue;
            }
            let need = if lw == 0 { ww } else { lw + 1 + ww };
            if need > max {
                out.push(std::mem::take(&mut line));
                line.push_str(word);
                lw = ww;
            } else {
                if lw > 0 {
                    line.push(' ');
                }
                line.push_str(word);
                lw = need;
            }
        }
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Cut `s` to at most `max` columns, ending in `…` when anything was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for g in s.graphemes(true) {
        let gw = UnicodeWidthStr::width(g);
        if w + gw > max - 1 {
            break;
        }
        out.push_str(g);
        w += gw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_decode_quotes_breaks_and_entities() {
        assert_eq!(decode_label("\"Hello <br/> world\""), "Hello\nworld");
        assert_eq!(decode_label("a<br>b<BR />c"), "a\nb\nc");
        assert_eq!(
            decode_label("A #quot;quoted#quot; &amp; done"),
            "A \"quoted\" & done"
        );
        assert_eq!(decode_label("#9829; love"), "♥ love");
        assert_eq!(decode_label("<b>bold</b> text"), "bold text");
        assert_eq!(decode_label("a < b > c"), "a < b > c");
        assert_eq!(decode_label("\"`**md** string`\""), "md string");
        assert_eq!(
            decode_label("x&#27;y"),
            "xy",
            "decoded controls are dropped"
        );
    }

    #[test]
    fn wrap_breaks_words_and_long_tokens() {
        assert_eq!(wrap("one two three", 7), vec!["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("a\nb", 10), vec!["a", "b"]);
        assert_eq!(wrap("日本語テキスト", 6), vec!["日本語", "テキス", "ト"]);
        assert_eq!(wrap("", 5), vec![""]);
    }

    #[test]
    fn truncate_marks_the_cut() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
    }

    #[test]
    fn clean_source_strips_controls() {
        assert_eq!(clean_source("a\u{1b}[31mb\tc\r\n"), "a[31mb    c\n");
    }
}
