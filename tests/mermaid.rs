//! Mermaid diagrams through the whole document pipeline: what reaches the
//! terminal in `--plain` output.

use ink_md::render::plain::render_plain_with_color;
use ink_md::{Args, Spacing};

fn args(width: u16) -> Args {
    Args {
        inputs: vec![],
        theme: "dark".to_string(),
        width: Some(width),
        slides: false,
        plain: true,
        watch: false,
        toc: false,
        images: ink_md::image::ImageMode::Off,
        image_protocol: ink_md::graphics::ProtocolChoice::HalfBlocks,
        frontmatter: false,
        spacing: Spacing::Normal,
        mouse_capture: true,
        clipboard: ink_md::clipboard::ClipboardMode::Off,
    }
}

fn plain(md: &str, width: u16) -> String {
    render_plain_with_color(md, &args(width), false).unwrap()
}

/// A node label is untrusted document text: an ESC (or any other control
/// byte) in it must not reach the terminal, whichever diagram type and
/// wherever in the diagram it appears.
#[test]
fn escape_bytes_in_diagram_labels_are_stripped() {
    let esc = "\u{1b}";
    for body in [
        format!("graph TD\n  A[{esc}]0;evil title{esc}\\\\ok] --> B[{esc}[2Jcleared]"),
        format!("graph LR\n  A -->|{esc}[31mred| B"),
        format!("graph TD\n  subgraph s[{esc}]8;;http://x{esc}\\\\frame]\n  A\n  end"),
        format!("stateDiagram-v2\n  [*] --> S{esc}[1m1\n  S{esc}[1m1 : desc{esc}[0m"),
        format!("classDiagram\n  class A{esc}x {{\n    +f{esc}[31m() int\n  }}"),
        format!("erDiagram\n  A{esc}B ||--o{{ C : {esc}[5mlabel"),
        format!("gantt\n  title {esc}[2J\n  section {esc}[31ms\n  task{esc} : a1, 2024-01-01, 3d"),
        format!("mindmap\n  root{esc}[31m\n    child{esc}]2;x"),
        format!("journey\n  title {esc}[2J"),
        format!("sequenceDiagram\n  A->>B: {esc}[2Jhi"),
    ] {
        let md = format!("```mermaid\n{body}\n```\n");
        let out = plain(&md, 80);
        assert!(!out.contains(esc), "ESC survived in:\n{out}");
        assert!(
            !out.chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
            "control byte survived in:\n{out}"
        );
    }
}

/// Plain output of a diagram never exceeds the requested width.
#[test]
fn diagrams_fit_the_requested_width() {
    let md = "```mermaid\nflowchart LR\n  A[Push Code] --> B[Run Tests] --> C{Tests Pass?}\n  C -->|Yes| D[Build Docker] --> E[Deploy]\n  C -->|No| F[Notify Team]\n```\n";
    for width in [30u16, 50, 80, 120] {
        for line in plain(md, width).lines() {
            let w = unicode_width::UnicodeWidthStr::width(line);
            assert!(w <= width as usize, "{w} > {width}: {line}");
        }
    }
}
