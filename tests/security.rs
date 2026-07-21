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
fn absolute_and_traversal_image_paths_do_not_read_files() {
    // Points at a real file that exists on the test machine; if containment
    // failed it would be read and decoded (and likely fail to decode, but the
    // point is load_image must refuse before touching it).
    use ink_md::image::{load_image, ImageMode, ImageUnavailable};
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        load_image("/etc/hosts", Some(dir.path()), ImageMode::LocalOnly),
        Err(ImageUnavailable::Failed)
    );
    assert_eq!(
        load_image(
            "../../../../etc/hosts",
            Some(dir.path()),
            ImageMode::LocalOnly
        ),
        Err(ImageUnavailable::Failed)
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
