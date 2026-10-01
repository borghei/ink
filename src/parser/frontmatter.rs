//! Frontmatter: a YAML (`---`), TOML (`+++`) or JSON (`{`) metadata block at
//! the very start of a document.
//!
//! By default it is stripped before parsing. With `--frontmatter` it is
//! replaced by a fenced block with the info string [`FENCE_INFO`], which the
//! layout draws as a key/value box (see `layout::frontmatter`) instead of
//! letting comrak read the metadata as markdown (a rule plus a setext
//! heading). There is no YAML/TOML/JSON parser here — only a light scan of
//! the top-level keys for display ([`entries`]); nested values are shown raw.

/// Info string of the fenced block standing in for shown frontmatter. The
/// word after it names the format (`yaml`, `toml`, `json`).
pub const FENCE_INFO: &str = "ink-frontmatter";

/// The syntax a frontmatter block is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Yaml,
    Toml,
    Json,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Yaml => "yaml",
            Format::Toml => "toml",
            Format::Json => "json",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "yaml" => Some(Format::Yaml),
            "toml" => Some(Format::Toml),
            "json" => Some(Format::Json),
            _ => None,
        }
    }
}

/// A frontmatter block found at the start of a document.
#[derive(Debug, Clone, PartialEq)]
pub struct Frontmatter<'s> {
    pub format: Format,
    /// The metadata: for YAML and TOML the lines between the delimiters,
    /// for JSON the whole object including its braces.
    pub body: &'s str,
    /// The document after the block (closing delimiter line included).
    pub rest: &'s str,
}

/// Find frontmatter at byte 0 of `source`.
///
/// YAML and TOML are recognized only when the very first line is exactly the
/// opening delimiter (`---` / `+++`; trailing whitespace ok), a later line is
/// exactly a closing delimiter (`---` or `...` / `+++`), and at least one line
/// in between looks like a key (`key:` / `key =`). JSON is recognized when
/// the first line is exactly `{`, the object closes (braces counted outside
/// strings) at the end of a line, and a line inside starts with `"key":`.
/// Anything else — e.g. a document that merely opens with a thematic break,
/// or an unterminated block — is not frontmatter.
pub fn split(source: &str) -> Option<Frontmatter<'_>> {
    split_delimited(source, "---", &["---", "..."], ':', Format::Yaml)
        .or_else(|| split_delimited(source, "+++", &["+++"], '=', Format::Toml))
        .or_else(|| split_json(source))
}

/// Strip frontmatter from markdown source.
/// Returns (frontmatter, remaining_content).
pub fn strip_frontmatter(source: &str) -> (Option<String>, String) {
    match split(source) {
        Some(fm) => (Some(fm.body.trim().to_string()), fm.rest.to_string()),
        None => (None, source.to_string()),
    }
}

/// The markdown to parse for `source`: frontmatter stripped, or — when
/// `show` — replaced by a fenced [`FENCE_INFO`] block the layout draws as a
/// metadata box. Without frontmatter, `source` unchanged.
pub fn prepare(source: &str, show: bool) -> String {
    let Some(fm) = split(source) else {
        return source.to_string();
    };
    if !show {
        return fm.rest.to_string();
    }
    // A fence longer than any backtick run inside, so no line of the
    // metadata can close it early.
    let longest = fm.body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    let body = fm.body.trim_end_matches(['\n', '\r']);
    format!(
        "{fence}{FENCE_INFO} {}\n{body}\n{fence}\n{}",
        fm.format.name(),
        fm.rest
    )
}

fn split_delimited<'s>(
    source: &'s str,
    open: &str,
    closers: &[&str],
    key_sep: char,
    format: Format,
) -> Option<Frontmatter<'s>> {
    let mut lines = source.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != open {
        return None;
    }
    let body_start = first.len();
    let mut pos = body_start;
    let mut has_key = false;
    for line in lines {
        let trimmed = line.trim_end();
        if closers.contains(&trimmed) {
            if !has_key {
                // `---` immediately followed by `---` with no keys between is
                // markdown (thematic breaks / setext), not frontmatter.
                return None;
            }
            return Some(Frontmatter {
                format,
                body: &source[body_start..pos],
                rest: &source[pos + line.len()..],
            });
        }
        has_key = has_key || is_key_line(trimmed, key_sep);
        pos += line.len();
    }
    // No closing delimiter: not frontmatter.
    None
}

fn split_json(source: &str) -> Option<Frontmatter<'_>> {
    let first = source.split_inclusive('\n').next()?;
    if first.trim_end() != "{" {
        return None;
    }
    let (mut depth, mut in_str, mut escaped) = (0usize, false, false);
    for (i, c) in source.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                // JSON strings never span lines: an open quote at a newline
                // means this is not JSON.
                '\n' => return None,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    if c != '}' {
                        return None;
                    }
                    let end = i + 1;
                    let line_end = source[end..]
                        .find('\n')
                        .map_or(source.len(), |n| end + n + 1);
                    if !source[end..line_end].trim().is_empty() {
                        return None;
                    }
                    let body = &source[..end];
                    let has_key = body.lines().skip(1).any(|l| {
                        let l = l.trim_start();
                        l.starts_with('"')
                            && l[1..]
                                .find('"')
                                .is_some_and(|q| l[1 + q + 1..].trim_start().starts_with(':'))
                    });
                    return has_key.then_some(Frontmatter {
                        format: Format::Json,
                        body,
                        rest: &source[line_end..],
                    });
                }
            }
            _ => {}
        }
    }
    None
}

/// A YAML/TOML-ish key line: `^[A-Za-z0-9_-]+\s*<sep>`.
fn is_key_line(line: &str, sep: char) -> bool {
    let key_len = key_len(line);
    key_len > 0 && line[key_len..].trim_start().starts_with(sep)
}

fn key_len(line: &str) -> usize {
    line.bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        .count()
}

/// The top-level keys of a frontmatter body and their values for display,
/// in order. A value is one or more lines: scalars unquoted, lists
/// comma-joined, nested structures and block text as their raw lines.
/// Best effort: malformed input yields whatever keys could be read.
pub fn entries(format: Format, body: &str) -> Vec<(String, Vec<String>)> {
    match format {
        Format::Yaml => yaml_entries(body),
        Format::Toml => toml_entries(body),
        Format::Json => json_entries(body),
    }
}

/// Strip one pair of matching quotes.
fn unquote(v: &str) -> String {
    let v = v.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            let inner = &v[1..v.len() - 1];
            return if q == '"' {
                inner.replace("\\\"", "\"").replace("\\\\", "\\")
            } else {
                inner.replace("''", "'")
            };
        }
    }
    v.to_string()
}

/// `[a, "b", c]` → `a, b, c` (one level; nested brackets kept raw).
fn flow_list(v: &str) -> Option<String> {
    let inner = v.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut items = Vec::new();
    let (mut depth, mut start, mut quote) = (0usize, 0usize, None::<char>);
    for (i, c) in inner.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[' | '{') => depth += 1,
            (None, ']' | '}') => depth = depth.saturating_sub(1),
            (None, ',') if depth == 0 => {
                items.push(unquote(&inner[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = unquote(&inner[start..]);
    if !last.is_empty() || !items.is_empty() {
        items.push(last);
    }
    items.retain(|s| !s.is_empty());
    Some(items.join(", "))
}

/// A scalar value: unquoted, flow lists joined; an unquoted value loses a
/// trailing ` # comment`.
fn scalar(v: &str) -> String {
    let v = v.trim();
    if let Some(list) = flow_list(v) {
        return list;
    }
    if v.starts_with(['"', '\'']) {
        return unquote(v);
    }
    match v.find(" #") {
        Some(i) => v[..i].trim_end().to_string(),
        None => v.to_string(),
    }
}

fn yaml_entries(body: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    // Per entry: list items gathered so far, and whether it holds a block
    // scalar (`|` keeps lines, `>` folds them).
    let mut items: Vec<String> = Vec::new();
    let mut block: Option<char> = None;

    fn flush(out: &mut [(String, Vec<String>)], items: &mut Vec<String>) {
        if let Some(last) = out.last_mut() {
            if !items.is_empty() {
                last.1.push(items.join(", "));
                items.clear();
            }
        }
    }

    for line in body.lines() {
        let trimmed = line.trim();
        let indented = line.starts_with([' ', '\t']);
        if !indented && is_key_line(line, ':') && !trimmed.starts_with('#') {
            flush(&mut out, &mut items);
            let k = key_len(line);
            let value = line[k..].trim_start()[1..].trim();
            block = match value.chars().next() {
                Some(c @ ('|' | '>')) if value.len() <= 3 => Some(c),
                _ => None,
            };
            let first = if block.is_some() || value.is_empty() {
                Vec::new()
            } else {
                vec![scalar(value)]
            };
            out.push((line[..k].to_string(), first));
            continue;
        }
        if trimmed.is_empty() || (trimmed.starts_with('#') && block.is_none()) {
            continue;
        }
        let Some(last) = out.last_mut() else {
            continue;
        };
        match block {
            Some('>') => match last.1.last_mut() {
                Some(text) => {
                    text.push(' ');
                    text.push_str(trimmed);
                }
                None => last.1.push(trimmed.to_string()),
            },
            Some(_) => last.1.push(trimmed.to_string()),
            None => {
                if let Some(item) = trimmed.strip_prefix("- ").or_else(|| {
                    // A bare `-` item.
                    (trimmed == "-").then_some("")
                }) {
                    if last.1.is_empty() || !items.is_empty() {
                        items.push(scalar(item));
                        continue;
                    }
                }
                // Nested mapping or anything else: shown raw.
                flush(&mut out, &mut items);
                if let Some(last) = out.last_mut() {
                    last.1.push(line.trim_end().to_string());
                }
            }
        }
    }
    flush(&mut out, &mut items);
    dedent_raw(out)
}

/// Raw nested lines keep their indentation relative to each other.
fn dedent_raw(entries: Vec<(String, Vec<String>)>) -> Vec<(String, Vec<String>)> {
    entries
        .into_iter()
        .map(|(k, lines)| {
            // Only ASCII blanks count, so the cut is always a char boundary.
            let blanks = |l: &str| l.bytes().take_while(|b| matches!(b, b' ' | b'\t')).count();
            let indent = lines
                .iter()
                .map(|l| blanks(l))
                .filter(|&n| n > 0)
                .min()
                .unwrap_or(0);
            let lines = lines
                .into_iter()
                .map(|l| {
                    if blanks(&l) >= indent {
                        l[indent..].to_string()
                    } else {
                        l
                    }
                })
                .collect();
            (k, lines)
        })
        .collect()
}

fn toml_entries(body: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut section = String::new();
    // A multi-line array being gathered: its text so far.
    let mut pending: Option<String> = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(acc) = pending.as_mut() {
            acc.push(' ');
            acc.push_str(trimmed);
            if bracket_balance(acc) <= 0 {
                let v = scalar(acc);
                pending = None;
                if let Some(last) = out.last_mut() {
                    last.1 = vec![v];
                }
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && !trimmed.contains('=') {
            section = trimmed.trim_matches(['[', ']']).trim().to_string();
            continue;
        }
        let Some((k, v)) = trimmed.split_once('=') else {
            if let Some(last) = out.last_mut() {
                last.1.push(trimmed.to_string());
            }
            continue;
        };
        let key = unquote(k);
        let key = if section.is_empty() {
            key
        } else {
            format!("{section}.{key}")
        };
        let v = v.trim();
        if v.starts_with('[') && bracket_balance(v) > 0 {
            pending = Some(v.to_string());
            out.push((key, vec![v.to_string()]));
            continue;
        }
        out.push((key, vec![scalar(v)]));
    }
    out
}

/// Open minus close brackets outside quotes.
fn bracket_balance(s: &str) -> isize {
    let (mut n, mut quote) = (0isize, None::<char>);
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[') => n += 1,
            (None, ']') => n -= 1,
            _ => {}
        }
    }
    n
}

fn json_entries(body: &str) -> Vec<(String, Vec<String>)> {
    let chars: Vec<char> = body.chars().collect();
    let mut out = Vec::new();
    let Some(open) = chars.iter().position(|&c| c == '{') else {
        return out;
    };
    let mut i = open + 1;
    loop {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '"' {
            break;
        }
        let (key, after) = json_string(&chars, i);
        i = after;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != ':' {
            break;
        }
        i += 1;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let end = json_value_end(&chars, i);
        let value = json_display(&chars[i..end]);
        out.push((key, vec![value]));
        i = end;
    }
    out
}

/// The string starting at the quote `chars[i]`, unescaped, and the index
/// after its closing quote.
fn json_string(chars: &[char], i: usize) -> (String, usize) {
    let mut s = String::new();
    let mut j = i + 1;
    while j < chars.len() {
        match chars[j] {
            '"' => return (s, j + 1),
            '\\' if j + 1 < chars.len() => {
                j += 1;
                match chars[j] {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'u' => {
                        let hex: String = chars[j + 1..chars.len().min(j + 5)].iter().collect();
                        if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            s.push(c);
                            j += 4;
                        } else {
                            s.push_str("\\u");
                        }
                    }
                    c => s.push(c),
                }
            }
            c => s.push(c),
        }
        j += 1;
    }
    (s, chars.len())
}

/// Index just past the JSON value starting at `i` (at depth 0: up to the
/// next `,` or the closing `}` of the enclosing object).
fn json_value_end(chars: &[char], i: usize) -> usize {
    let (mut depth, mut in_str, mut escaped) = (0usize, false, false);
    let mut j = i;
    while j < chars.len() {
        let c = chars[j];
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                '"' => in_str = true,
                '{' | '[' => depth += 1,
                '}' | ']' if depth == 0 => return j,
                '}' | ']' => depth -= 1,
                ',' if depth == 0 => return j,
                _ => {}
            }
        }
        j += 1;
    }
    j
}

/// A JSON value for display: strings unescaped, arrays of scalars
/// comma-joined, objects and nested arrays compacted onto one line.
fn json_display(value: &[char]) -> String {
    let text: String = value.iter().collect();
    let text = text.trim();
    let chars: Vec<char> = text.chars().collect();
    match chars.first() {
        Some('"') => json_string(&chars, 0).0,
        Some('[') => {
            let mut items = Vec::new();
            let mut i = 1;
            loop {
                while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
                    i += 1;
                }
                if i >= chars.len() || chars[i] == ']' {
                    break;
                }
                let end = json_value_end(&chars, i);
                let item: String = chars[i..end].iter().collect();
                let item = item.trim();
                items.push(if item.starts_with('"') {
                    let c: Vec<char> = item.chars().collect();
                    json_string(&c, 0).0
                } else {
                    compact(item)
                });
                if end == i {
                    break;
                }
                i = end;
            }
            items.join(", ")
        }
        _ => compact(text),
    }
}

/// Collapse runs of whitespace (outside nothing in particular: display only).
fn compact(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_frontmatter() {
        let source = "---\ntitle: Hello\nauthor: World\n---\n# Content";
        let (fm, rest) = strip_frontmatter(source);
        assert_eq!(fm, Some("title: Hello\nauthor: World".to_string()));
        assert!(rest.contains("# Content"));
    }

    #[test]
    fn test_no_frontmatter() {
        let source = "# Just a heading\nSome content.";
        let (fm, rest) = strip_frontmatter(source);
        assert!(fm.is_none());
        assert_eq!(rest, source);
    }

    #[test]
    fn test_toml_frontmatter() {
        let source = "+++\ntitle = \"Hello\"\n+++\n# Content";
        let (fm, rest) = strip_frontmatter(source);
        assert!(fm.is_some());
        assert!(rest.contains("# Content"));
    }

    #[test]
    fn yaml_closed_by_dots() {
        let source = "---\ntitle: Hello\n...\n# Content";
        let (fm, rest) = strip_frontmatter(source);
        assert_eq!(fm, Some("title: Hello".to_string()));
        assert!(rest.contains("# Content"));
    }

    #[test]
    fn leading_thematic_break_is_not_frontmatter() {
        // A doc opening with a thematic break must not be eaten up to the
        // next dash run (which is 5 dashes here, not a closing delimiter).
        let source = "---\n\n# Intro\n\nBody text\n\n-----\n\nMore\n";
        let (fm, rest) = strip_frontmatter(source);
        assert!(fm.is_none());
        assert_eq!(rest, source);
        assert!(rest.contains("Intro"));
        assert!(rest.contains("Body text"));
    }

    #[test]
    fn delimiters_without_keys_are_not_frontmatter() {
        let source = "---\nnot a key line\n---\ntext";
        let (fm, rest) = strip_frontmatter(source);
        assert!(fm.is_none());
        assert_eq!(rest, source);
    }

    #[test]
    fn unclosed_opener_is_not_frontmatter() {
        let source = "---\ntitle: Hello\nno closer here";
        let (fm, rest) = strip_frontmatter(source);
        assert!(fm.is_none());
        assert_eq!(rest, source);
    }

    #[test]
    fn opener_must_be_first_line() {
        let source = "intro\n---\ntitle: Hello\n---\n";
        let (fm, rest) = strip_frontmatter(source);
        assert!(fm.is_none());
        assert_eq!(rest, source);
    }

    #[test]
    fn trailing_whitespace_on_delimiters_ok() {
        let source = "--- \ntitle: Hello\n---\t\n# Content";
        let (fm, rest) = strip_frontmatter(source);
        assert_eq!(fm, Some("title: Hello".to_string()));
        assert!(rest.contains("# Content"));
    }

    #[test]
    fn json_frontmatter() {
        let source = "{\n  \"title\": \"Hi {there}\",\n  \"tags\": [\"a\", \"b\"]\n}\n# Content\n";
        let fm = split(source).unwrap();
        assert_eq!(fm.format, Format::Json);
        assert_eq!(fm.rest, "# Content\n");
        assert_eq!(
            entries(fm.format, fm.body),
            [
                ("title".to_string(), vec!["Hi {there}".to_string()]),
                ("tags".to_string(), vec!["a, b".to_string()]),
            ]
        );
    }

    #[test]
    fn json_needs_a_key_and_a_closed_object() {
        for source in [
            "{\n\"title\": \"x\"\n",          // unterminated
            "{\nnot json\n}\n",               // no key
            "{\n\"a\": 1\n} trailing text\n", // text after the brace
            "{\n\"a\": \"open string\n}\n",   // string runs off the line
            "{ \"a\": 1 }\n",                 // opener not alone on its line
            "{\n\"a\": [1, 2}\n]\n",          // mismatched
        ] {
            assert!(split(source).is_none(), "{source:?}");
        }
    }

    #[test]
    fn yaml_entries_scalars_lists_and_nesting() {
        let body = "title: \"Hello: world\"\n# a comment\ntags: [a, 'b c']\nauthors:\n  - Ann\n  - Bob\nmeta:\n  draft: true\n  nested:\n    deep: 1\ndesc: >\n  folded\n  text\nurl: https://x.y/#frag\nnote: plain # trailing comment\n";
        let e = entries(Format::Yaml, body);
        let get = |k: &str| e.iter().find(|(key, _)| key == k).unwrap().1.clone();
        assert_eq!(get("title"), ["Hello: world"]);
        assert_eq!(get("tags"), ["a, b c"]);
        assert_eq!(get("authors"), ["Ann, Bob"]);
        assert_eq!(get("meta"), ["draft: true", "nested:", "  deep: 1"]);
        assert_eq!(get("desc"), ["folded text"]);
        assert_eq!(get("url"), ["https://x.y/#frag"]);
        assert_eq!(get("note"), ["plain"]);
    }

    #[test]
    fn toml_entries_sections_and_arrays() {
        let body = "title = \"Hello\"\ntags = [\"a\", \"b\"]\nlist = [\n  1,\n  2,\n]\n[params]\ndraft = false\n";
        let e = entries(Format::Toml, body);
        assert_eq!(
            e,
            [
                ("title".to_string(), vec!["Hello".to_string()]),
                ("tags".to_string(), vec!["a, b".to_string()]),
                ("list".to_string(), vec!["1, 2".to_string()]),
                ("params.draft".to_string(), vec!["false".to_string()]),
            ]
        );
    }

    #[test]
    fn prepare_hides_or_fences_the_block() {
        let source = "---\ntitle: Hi\n---\n# Body\n";
        assert_eq!(prepare(source, false), "# Body\n");
        assert_eq!(
            prepare(source, true),
            "```ink-frontmatter yaml\ntitle: Hi\n```\n# Body\n"
        );
        // A backtick run inside the metadata gets a longer fence.
        let source = "---\nx: ````y\n---\nz\n";
        assert!(prepare(source, true).starts_with("`````ink-frontmatter yaml\n"));
        // No frontmatter: unchanged either way.
        assert_eq!(prepare("# a\n", true), "# a\n");
    }

    #[test]
    fn hostile_bodies_do_not_panic() {
        for body in [
            "\"",
            "{",
            "{\"a\":",
            "{\"a\": \"\\",
            "{\"a\": \"\\u12",
            "{\"\\u{zz}\": [[[[",
            "a: [",
            "a: '",
            "- x\n- y",
            "  indented: first",
            "k: |\n",
            "é: ü\nü: é",
        ] {
            for f in [Format::Yaml, Format::Toml, Format::Json] {
                let _ = entries(f, body);
            }
        }
        let deep = format!(
            "{{\n\"a\": {}{}\n}}\n",
            "[".repeat(10000),
            "]".repeat(10000)
        );
        let fm = split(&deep).unwrap();
        let _ = entries(fm.format, fm.body);
    }
}
