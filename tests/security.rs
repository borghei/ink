//! End-to-end checks that untrusted markdown cannot emit terminal escape
//! sequences or exfiltrate files through the renderer.

use ink_md::render::plain::render_plain;
use ink_md::{Args, Spacing};

fn args() -> Args {
    Args {
        inputs: vec![],
        theme: "dark".to_string(),
        width: Some(80),
        slides: false,
        plain: true,
        watch: false,
        toc: false,
        images: ink_md::image::ImageMode::LocalOnly,
        image_protocol: ink_md::graphics::ProtocolChoice::HalfBlocks,
        frontmatter: false,
        spacing: Spacing::Normal,
        mouse_capture: true,
        clipboard: ink_md::clipboard::ClipboardMode::Off,
    }
}

/// Every ESC byte in the output must begin one of ink's own sequences:
/// an SGR (`\x1b[`), an OSC 8 hyperlink (`\x1b]8;;`), or its ST terminator
/// (`\x1b\\`). Any other ESC means injected content leaked through.
fn assert_only_ink_escapes(out: &str) {
    let bytes = out.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            let rest = &out[i..];
            let ok = rest.starts_with("\x1b[")
                || rest.starts_with("\x1b]8;;")
                || rest.starts_with("\x1b\\");
            assert!(
                ok,
                "unexpected escape at byte {i}: {:?}",
                &rest[..rest.len().min(12)]
            );
        }
        i += 1;
    }
    // No other C0 controls except newline/tab.
    for c in out.chars() {
        if c.is_control() {
            assert!(
                matches!(c, '\n' | '\t' | '\u{1b}'),
                "leaked control char: {:?}",
                c
            );
        }
    }
}

#[test]
fn hostile_fixture_produces_no_injected_escapes() {
    let source = std::fs::read_to_string("tests/fixtures/hostile/escapes.md").unwrap();
    let out = render_plain(&source, &args()).unwrap();
    assert_only_ink_escapes(&out);
}

#[test]
fn javascript_and_file_urls_not_emitted_as_links() {
    let source =
        "[js](javascript:alert(1)) [file](file:///etc/passwd) [ok](https://example.com/a)\n";
    let out = render_plain(source, &args()).unwrap();
    // The one safe link survives as an OSC 8 hyperlink; the dangerous ones do not.
    assert!(out.contains("\x1b]8;;https://example.com/a"));
    assert!(!out.contains("javascript:"));
    assert!(!out.contains("file:///etc/passwd"));
}

#[test]
fn c1_control_bytes_stripped() {
    let source = "text \u{9b}31m more\n"; // C1 CSI
    let out = render_plain(source, &args()).unwrap();
    assert!(!out.contains('\u{9b}'));
}

#[test]
fn image_paths_may_be_read_but_never_surface_as_text() {
    // Any local path may be *read* (issue #3: documents legitimately reference
    // images by absolute path), but the bytes must decode as an image to reach
    // the screen — always as pixels, never as text. A hostile document
    // pointing an image at a text file gets a clean failure, not the contents.
    use ink_md::image::{load_decoded, ImageMode, ImageUnavailable};
    let doc_dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    // A readable text file outside the document's directory, named by absolute
    // path — exactly what a hostile document would point an image at. (Not a
    // fixed system path like /etc/hosts: that does not exist on Windows, where
    // the miss would look like this assertion passing for the wrong reason.)
    let secret = elsewhere.path().join("secret.txt");
    std::fs::write(&secret, "TOP-SECRET-CONTENTS").unwrap();
    assert_eq!(
        load_decoded(
            secret.to_str().unwrap(),
            Some(doc_dir.path()),
            ImageMode::LocalOnly,
            None
        )
        .err(),
        Some(ImageUnavailable::Failed),
        "a text file must fail to decode, so its contents never reach the screen"
    );
    // A directory target fails cleanly too.
    assert_eq!(
        load_decoded(
            elsewhere.path().to_str().unwrap(),
            Some(doc_dir.path()),
            ImageMode::LocalOnly,
            None
        )
        .err(),
        Some(ImageUnavailable::NotFound)
    );
}

#[test]
fn remote_images_blocked_by_default() {
    use ink_md::image::{load_image, ImageMode, ImageUnavailable};
    assert_eq!(
        load_image("https://example.com/x.png", None, ImageMode::LocalOnly),
        Err(ImageUnavailable::RemoteBlocked)
    );
}

#[test]
fn raw_html_cannot_smuggle_escapes_or_unsafe_links() {
    // Raw HTML is rendered (tags mapped onto ink's styles), not dropped, so
    // its text and attributes are attacker-controlled input like any other:
    // an ESC byte in a block, a decoded `&#27;`, a `javascript:` href (plain
    // or entity-obfuscated) and an href carrying an ESC must all be stopped.
    let source = "<div>esc \u{1b}]52;c;cHduZWQ=\u{7} here &#27;[2J</div>\n\n\
        <p><a href=\"javascript:alert(1)\">js link</a> \
        <a href=\"&#x6A;avascript:alert(2)\">obfuscated</a> \
        <a href=\"https://ok.example/\u{1b}]8;;https://evil.example\">esc link</a></p>\n\n\
        Inline <a href=\"javascript:alert(3)\">inline js</a> \
        and <a href=\"https://inline.example/\u{1b}[31m\">inline esc</a>.\n";
    let out = render_plain(source, &args()).unwrap();
    assert_only_ink_escapes(&out);
    // The text survives; the tags and the dangerous destinations do not.
    for text in [
        "esc",
        "here",
        "js link",
        "obfuscated",
        "esc link",
        "inline js",
    ] {
        assert!(out.contains(text), "missing {text:?} in {out:?}");
    }
    assert!(!out.contains("<a"), "raw tag leaked: {out:?}");
    assert!(!out.contains("javascript:"));
    assert!(!out.contains("alert"));
    assert!(!out.contains("evil.example"));
    assert!(!out.contains("ok.example"));
    assert!(!out.contains("inline.example"));
}

#[test]
fn absurdly_deep_nesting_cannot_crash_the_renderer() {
    // 50 KB of `>` is 50000 nested blockquotes. Layout caps its own
    // recursion; comrak's footnote pass recurses per level too, so the parse
    // turns footnotes off for such input (`parser::options_for`). Run the
    // whole pipeline on a 1 MB stack so a per-level recursion cannot hide.
    let out = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let source = format!("{} deep\n", ">".repeat(50_000));
            render_plain(&source, &args()).unwrap()
        })
        .unwrap()
        .join()
        .expect("renderer overflowed its stack");
    assert!(out.contains("deep"));
}

/// A document whose heading and body carry raw ESC/OSC/BEL bytes: an OSC 0
/// window-title set, a CSI clear-screen, and a bare BEL.
const HOSTILE_DOC: &str =
    "# a\x1b]0;TITLE\x07b\n\nbody \x1b[2J text\x07\n\n## second \x1b]8;;http://x\x1b\\link\n";

fn ink_cmd() -> assert_cmd::Command {
    assert_cmd::Command::cargo_bin("ink").unwrap()
}

// Regression: `ink outline` printed heading text without the sanitizer, so
// a heading could set the terminal title.
#[test]
fn outline_strips_escapes_from_headings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hostile.md");
    std::fs::write(&path, HOSTILE_DOC).unwrap();
    let out = ink_cmd().arg("outline").arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("a]0;TITLEb"),
        "heading text kept: {stdout:?}"
    );
    assert!(!stdout.contains('\x1b'), "ESC leaked: {stdout:?}");
    assert!(!stdout.contains('\x07'), "BEL leaked: {stdout:?}");
}

// Regression: `ink diff` printed changed lines verbatim.
#[test]
fn diff_strips_escapes_from_document_lines() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    std::fs::write(&a, "# plain\n").unwrap();
    std::fs::write(&b, HOSTILE_DOC).unwrap();
    // Piped stdout: no styling, so no ESC may appear at all.
    let out = ink_cmd()
        .env_remove("CLICOLOR_FORCE")
        .env_remove("FORCE_COLOR")
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("body [2J text"),
        "line text kept: {stdout:?}"
    );
    assert!(!stdout.contains('\x1b'), "ESC leaked: {stdout:?}");
    assert!(!stdout.contains('\x07'), "BEL leaked: {stdout:?}");
    // With color forced, only ink's own SGR styling may appear — no OSC, no
    // injected CSI.
    let out = ink_cmd()
        .args(["diff", "--color=always"])
        .arg(&a)
        .arg(&b)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("\x1b[32m"), "styled: {stdout:?}");
    assert!(!stdout.contains('\x07'), "BEL leaked: {stdout:?}");
    assert!(!stdout.contains("\x1b]"), "OSC leaked: {stdout:?}");
    assert!(!stdout.contains("\x1b[2J"), "CSI leaked: {stdout:?}");
}

/// Text carried by the markdown extensions (callout titles, spoilers,
/// super/subscripts, definition terms, aligned table cells) goes through the
/// same sanitizer as everything else.
#[test]
fn extension_text_cannot_inject_escapes() {
    let source = "> [!NOTE] title \x1b]52;c;SGVsbG8=\x07 \x1b[2J\n> body\n\n\
        x^\x1b[31m^ and H~\x1b[2J~O and ||spoil \x1b]0;pwn\x07 er||\n\n\
        Term \x1b[2J\n: def \x1b[2J\n\n\
        | a |\n|--:|\n| \x1b[2J |\n";
    let out = render_plain(source, &args()).unwrap();
    assert_only_ink_escapes(&out);
    assert!(!out.contains("\x1b[2J") && !out.contains("\x1b]52") && !out.contains("\x1b]0"));
}

/// Math is rendered from document text: `\text{…}`, unknown commands and
/// environment cells all pass through the sanitizer.
#[test]
fn math_text_cannot_inject_escapes() {
    let source = "Inline $\\text{\x1b[2J} \\foo{\x1b]0;t\x07} x^{\x1b[31m}$.\n\n\
        $$\n\\begin{pmatrix} \x1b[2J & b \\\\ c & \x1b]52;c;AA==\x07 \\end{pmatrix}\n$$\n\n\
        ```math\n\\text{\x1b[2J}\n```\n\n| m |\n|---|\n| $\\text{\x1b[2J}$ |\n";
    let out = render_plain(source, &args()).unwrap();
    assert_only_ink_escapes(&out);
    assert!(!out.contains("\x1b[2J") && !out.contains("\x1b]0") && !out.contains("\x1b]52"));
}
