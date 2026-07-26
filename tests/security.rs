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
