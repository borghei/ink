//! Sanitization of untrusted markdown content before it reaches the terminal.
//!
//! Markdown files are untrusted input: raw ESC bytes in text or code blocks
//! could rewrite the window title, move the cursor, spoof hyperlinks, or (on
//! some terminals) write the clipboard. Everything rendered to the screen —
//! TUI and `--plain` alike — flows through `layout_document`, so sanitizing
//! its output covers every ingestion site (text nodes, code literals, raw
//! HTML blocks, alt text, link destinations).

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Strip terminal control characters from text destined for the screen.
///
/// Removes ESC, C0 controls (except `\t`), DEL, and C1 controls
/// (U+0080–U+009F). Newlines are stripped too: layout emits spans that never
/// span lines, so an embedded `\n` is itself suspect.
pub fn sanitize_text(s: &str) -> Cow<'_, str> {
    if s.chars().all(is_allowed_char) {
        return Cow::Borrowed(s);
    }
    Cow::Owned(s.chars().filter(|&c| is_allowed_char(c)).collect())
}

fn is_allowed_char(c: char) -> bool {
    if c == '\t' {
        return true;
    }
    !(c.is_control() || ('\u{80}'..='\u{9f}').contains(&c))
}

/// Validate and clean a link destination before it is emitted as an OSC 8
/// hyperlink or acted upon (open in browser / follow).
///
/// Control characters are rejected outright (a String Terminator inside a URL
/// can close the OSC 8 sequence early and inject arbitrary escapes). Allowed
/// schemes are `http`, `https`, and `mailto`; scheme-less values (relative
/// paths, absolute paths, `#anchors`) pass through. Everything else —
/// `file:`, `javascript:`, `data:`, custom schemes — is rejected.
pub fn sanitize_url(url: &str) -> Option<String> {
    if url
        .chars()
        .any(|c| c.is_control() || ('\u{80}'..='\u{9f}').contains(&c))
    {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        return Some(url.to_string());
    }
    // A scheme is an ASCII-alpha-led run of [a-zA-Z0-9+.-] followed by ':'
    // appearing before any '/', '?' or '#'. Reject any other scheme.
    let head = url.split(['/', '?', '#']).next().unwrap_or("");
    if let Some(colon) = head.find(':') {
        let scheme = &head[..colon];
        let scheme_like = scheme
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
        if scheme_like {
            return None;
        }
    }
    Some(url.to_string())
}

/// Resolve a local path referenced by a document (image source or `.md` link
/// target). Absolute paths load as-is; relative paths resolve against `base`,
/// the document's own directory — including `../` escapes (issue #3: real
/// documents reference files anywhere on disk, and rendering a local file to
/// the local user's screen discloses nothing to anyone else). Markdown
/// destinations are often percent-encoded (`my%20notes.md`), so when the
/// literal path does not exist the decoded form is tried as a fallback —
/// literal first, so a file actually named with `%` still wins.
///
/// Returns the canonicalized path of an existing filesystem entry, or `None`.
pub fn resolve_local(base: &Path, target: &str) -> Option<PathBuf> {
    resolve_variants(base, target).or_else(|| {
        // GitHub-style suffixes (`pic.png?raw=true`, `pic.png#gh-light-mode`)
        // — stripped only as a fallback, since '?' and '#' are legal in
        // filenames and a literal match must win.
        let stripped = target.split(['?', '#']).next().unwrap_or(target);
        (stripped != target)
            .then(|| resolve_variants(base, stripped))
            .flatten()
    })
}

fn resolve_variants(base: &Path, target: &str) -> Option<PathBuf> {
    try_resolve(base, target).or_else(|| {
        percent_decode(target)
            .as_deref()
            .and_then(|decoded| try_resolve(base, decoded))
    })
}

fn try_resolve(base: &Path, target: &str) -> Option<PathBuf> {
    if target.is_empty() {
        return None;
    }
    let joined = if let Some(rest) = target.strip_prefix("~/") {
        dirs::home_dir()?.join(rest)
    } else {
        let path = Path::new(target);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        }
    };
    joined.canonicalize().ok()
}

/// Decode `%XX` escapes. Returns `None` when there is nothing to decode or
/// the decoded bytes are not valid UTF-8; malformed escapes pass through.
pub(crate) fn percent_decode(s: &str) -> Option<String> {
    if !s.contains('%') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_clean_text_through_borrowed() {
        assert!(matches!(sanitize_text("hello world"), Cow::Borrowed(_)));
        assert!(matches!(sanitize_text("tabs\tok"), Cow::Borrowed(_)));
    }

    #[test]
    fn strips_escape_sequences() {
        assert_eq!(sanitize_text("a\x1b[2Jb"), "a[2Jb");
        assert_eq!(sanitize_text("t\x1b]0;pwned\x07x"), "t]0;pwnedx");
        assert_eq!(sanitize_text("c1\u{9b}31mred"), "c131mred");
        assert_eq!(sanitize_text("del\x7fx"), "delx");
        assert_eq!(sanitize_text("nl\ncr\r"), "nlcr");
    }

    #[test]
    fn url_allows_web_schemes_and_relative() {
        assert!(sanitize_url("https://example.com/a?b#c").is_some());
        assert!(sanitize_url("http://example.com").is_some());
        assert!(sanitize_url("mailto:a@b.com").is_some());
        assert!(sanitize_url("docs/readme.md").is_some());
        assert!(sanitize_url("../sibling.md").is_some());
        assert!(sanitize_url("#anchor").is_some());
    }

    #[test]
    fn url_rejects_dangerous() {
        assert!(sanitize_url("javascript:alert(1)").is_none());
        assert!(sanitize_url("file:///etc/passwd").is_none());
        assert!(sanitize_url("data:text/html,x").is_none());
        assert!(sanitize_url("https://x.com/\x1b\\\x1b]0;t\x07").is_none());
        assert!(sanitize_url("https://x.com/\x07").is_none());
    }

    #[test]
    fn resolve_local_paths() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::create_dir(base.join("docs")).unwrap();
        std::fs::write(base.join("ok.md"), "x").unwrap();
        // Relative to base.
        assert!(resolve_local(base, "ok.md").is_some());
        // Parent-relative escapes the base dir and is allowed.
        assert!(resolve_local(&base.join("docs"), "../ok.md").is_some());
        // Absolute paths are allowed.
        assert!(resolve_local(base, base.join("ok.md").to_str().unwrap()).is_some());
        // Nonexistent targets resolve to nothing.
        assert!(resolve_local(base, "missing.md").is_none());
        assert!(resolve_local(base, "").is_none());
    }

    #[test]
    fn resolve_local_percent_decodes_as_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::write(base.join("my notes.md"), "x").unwrap();
        // Markdown destinations encode spaces; the decoded form is found.
        let resolved = resolve_local(base, "my%20notes.md").expect("decoded fallback");
        assert!(resolved.ends_with("my notes.md"));
        // A file literally named with the escape wins over the decoded form.
        std::fs::write(base.join("a%20b.md"), "x").unwrap();
        std::fs::write(base.join("a b.md"), "x").unwrap();
        let resolved = resolve_local(base, "a%20b.md").unwrap();
        assert!(resolved.ends_with("a%20b.md"));
        // Malformed escapes pass through undecoded.
        assert!(resolve_local(base, "%zz.md").is_none());
    }

    #[test]
    fn resolve_local_strips_query_and_fragment_as_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::write(base.join("pic.png"), "x").unwrap();
        // GitHub-habit suffixes resolve to the underlying file.
        let resolved = resolve_local(base, "pic.png?raw=true").expect("query stripped");
        assert!(resolved.ends_with("pic.png"));
        let resolved =
            resolve_local(base, "pic.png#gh-light-mode-only").expect("fragment stripped");
        assert!(resolved.ends_with("pic.png"));
        // A file literally named with the suffix wins over stripping. '#' is
        // a legal filename character everywhere; '?' is not — Win32 rejects it
        // outright, so that half of the check only runs off Windows.
        std::fs::write(base.join("odd.png#v2"), "x").unwrap();
        let resolved = resolve_local(base, "odd.png#v2").unwrap();
        assert!(resolved.ends_with("odd.png#v2"));
        #[cfg(not(windows))]
        {
            std::fs::write(base.join("odd.png?v=2"), "x").unwrap();
            let resolved = resolve_local(base, "odd.png?v=2").unwrap();
            assert!(resolved.ends_with("odd.png?v=2"));
        }
    }
}
