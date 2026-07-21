use ink_md::render::plain::render_plain;
use ink_md::{Args, Spacing};

fn test_args() -> Args {
    Args {
        inputs: vec![],
        theme: "dark".to_string(),
        width: Some(80),
        slides: false,
        plain: true,
        watch: false,
        toc: false,
        images: ink_md::image::ImageMode::Off,
        frontmatter: false,
        spacing: Spacing::Normal,
    }
}

#[test]
fn snapshot_test_fixture() {
    let source = std::fs::read_to_string("tests/fixtures/test.md").unwrap();
    let rendered = render_plain(&source, &test_args()).unwrap();
    insta::assert_snapshot!("plain_test_md", rendered);
}

#[test]
fn snapshot_demo_fixture() {
    let source = std::fs::read_to_string("tests/fixtures/demo.md").unwrap();
    let rendered = render_plain(&source, &test_args()).unwrap();
    insta::assert_snapshot!("plain_demo_md", rendered);
}

#[test]
fn snapshot_diagrams_fixture() {
    let source = std::fs::read_to_string("tests/fixtures/diagrams.md").unwrap();
    let rendered = render_plain(&source, &test_args()).unwrap();
    insta::assert_snapshot!("plain_diagrams_md", rendered);
}

#[test]
fn snapshot_core_constructs() {
    let source = "# Title\n\nSome **bold** and *italic* and `code`.\n\n\
        - item one\n- item two\n  - nested\n\n\
        1. first\n2. second\n\n\
        > a blockquote\n\n\
        | a | b |\n|---|---|\n| 1 | 2 |\n\n\
        ```rust\nfn main() {}\n```\n\n\
        [a link](https://example.com)\n\n---\n\n\
        - [x] done\n- [ ] todo\n";
    let rendered = render_plain(source, &test_args()).unwrap();
    insta::assert_snapshot!("plain_core_constructs", rendered);
}
