/// Strip YAML or TOML frontmatter from markdown source.
/// Returns (frontmatter, remaining_content).
///
/// Frontmatter is recognized only when the very first line is exactly the
/// opening delimiter (`---` for YAML, `+++` for TOML; trailing whitespace ok),
/// a later line is exactly a closing delimiter (`---` or `...` for YAML,
/// `+++` for TOML), and at least one line in between looks like a key
/// (`key:` / `key =`). Anything else — e.g. a document that merely opens with
/// a thematic break — is returned unchanged.
pub fn strip_frontmatter(source: &str) -> (Option<String>, String) {
    strip_delimited(source, "---", &["---", "..."], ':')
        .or_else(|| strip_delimited(source, "+++", &["+++"], '='))
        .unwrap_or_else(|| (None, source.to_string()))
}

fn strip_delimited(
    source: &str,
    open: &str,
    closers: &[&str],
    key_sep: char,
) -> Option<(Option<String>, String)> {
    let mut lines = source.split_inclusive('\n');
    if lines.next()?.trim_end() != open {
        return None;
    }

    let mut body: Vec<&str> = Vec::new();
    let mut has_key = false;
    for line in lines.by_ref() {
        let trimmed = line.trim_end();
        if closers.contains(&trimmed) {
            if !has_key {
                // `---` immediately followed by `---` with no keys between is
                // markdown (thematic breaks / setext), not frontmatter.
                return None;
            }
            let fm = body.concat().trim().to_string();
            let rest: String = lines.collect();
            return Some((Some(fm), rest));
        }
        has_key = has_key || is_key_line(trimmed, key_sep);
        body.push(line);
    }
    // No closing delimiter: not frontmatter.
    None
}

/// A YAML/TOML-ish key line: `^[A-Za-z0-9_-]+\s*<sep>`.
fn is_key_line(line: &str, sep: char) -> bool {
    let key_len = line
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        .count();
    key_len > 0 && line[key_len..].trim_start().starts_with(sep)
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
}
