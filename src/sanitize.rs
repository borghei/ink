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

/// Resolve `target` relative to `base` and require the result to stay inside
/// one of `roots` (after symlink resolution). Absolute targets are rejected.
///
/// Returns the canonicalized path only when containment holds.
pub fn resolve_within(base: &Path, roots: &[&Path], target: &str) -> Option<PathBuf> {
    let target_path = Path::new(target);
    if target_path.is_absolute() || has_windows_prefix(target) {
        return None;
    }
    let joined = base.join(target_path);
    let canonical = joined.canonicalize().ok()?;
    let allowed = roots.iter().any(|root| {
        root.canonicalize()
            .map(|r| canonical.starts_with(&r))
            .unwrap_or(false)
    });
    allowed.then_some(canonical)
}

/// Windows-style absolute targets (`C:\...`, `\\server\share`) that
/// `Path::is_absolute` does not flag on Unix.
fn has_windows_prefix(target: &str) -> bool {
    let bytes = target.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || target.starts_with("\\\\")
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
    fn resolve_within_contains() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::write(base.join("ok.md"), "x").unwrap();
        assert!(resolve_within(base, &[base], "ok.md").is_some());
        assert!(resolve_within(base, &[base], "../ok.md").is_none());
        assert!(resolve_within(base, &[base], "/etc/hosts").is_none());
        assert!(resolve_within(base, &[base], "C:\\windows").is_none());
        assert!(resolve_within(base, &[base], "missing.md").is_none());
    }
}
