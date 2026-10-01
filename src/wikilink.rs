//! Wikilinks: `[[target]]` and `[[target|label]]`.
//!
//! Parsing is comrak's (`extension.wikilinks_title_after_pipe`, enabled in
//! `parser::options`), so a wikilink is recognised only where markdown
//! inline content is — never in fenced or indented code, code spans of any
//! length (even across lines), or raw HTML such as `<pre>`. Layout turns each
//! `NodeValue::WikiLink` into a link to `resolve_target(url)`.

/// Formerly rewrote `[[x]]` into `[x](x.md)` in the raw source before
/// parsing. That pre-pass could not see markdown structure and corrupted
/// wikilink-looking text inside indented code, multi-line code spans and
/// `<pre>` (which `c` then copied), and produced broken destinations for
/// targets with spaces or parentheses. Wikilinks are now parsed by comrak,
/// so the source is returned unchanged.
pub fn process_wikilinks(source: &str) -> String {
    source.to_string()
}

/// The link a wikilink target points at: the target itself when it already
/// has an extension (`doc.pdf`), else `target.md`. As in Obsidian, a `#`
/// starts a section: `[[page#Section]]` → `page.md#Section`, and
/// `[[#Section]]` is a jump within the document. The section is heading
/// text; anchors are matched by slugging it the way headings are slugged.
pub fn resolve_target(target: &str) -> String {
    let target = target.trim();
    let (page, section) = match target.split_once('#') {
        Some((page, section)) => (page.trim_end(), Some(section.trim())),
        None => (target, None),
    };
    let file = if page.is_empty() || page.contains('.') {
        page.to_string()
    } else {
        format!("{page}.md")
    };
    match section {
        Some(section) => format!("{file}#{section}"),
        None => file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::nodes::{AstNode, NodeValue};
    use comrak::{parse_document, Arena};

    /// Every wikilink in `src` as `(label, resolved destination)`, plus the
    /// literal text of every code node, code block and HTML node.
    fn scan(src: &str) -> (Vec<(String, String)>, String) {
        let arena = Arena::new();
        let root = parse_document(&arena, src, &crate::parser::options());
        let mut links = Vec::new();
        let mut code = String::new();
        for node in root.descendants() {
            match &node.data.borrow().value {
                NodeValue::WikiLink(wl) => links.push((label(node), resolve_target(&wl.url))),
                NodeValue::Code(c) => code.push_str(&c.literal),
                NodeValue::CodeBlock(cb) => code.push_str(&cb.literal),
                NodeValue::HtmlBlock(hb) => code.push_str(&hb.literal),
                NodeValue::HtmlInline(h) => code.push_str(h),
                _ => {}
            }
        }
        (links, code)
    }

    fn label<'a>(node: &'a AstNode<'a>) -> String {
        node.descendants()
            .filter_map(|n| match &n.data.borrow().value {
                NodeValue::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    fn links(src: &str) -> Vec<(String, String)> {
        scan(src).0
    }

    fn pair(label: &str, dest: &str) -> (String, String) {
        (label.to_string(), dest.to_string())
    }

    /// No wikilink was made, and the `[[...]]` text survives verbatim in code.
    fn assert_untouched(src: &str, literal: &str) {
        let (links, code) = scan(src);
        assert!(links.is_empty(), "{src:?} produced links {links:?}");
        assert!(
            code.contains(literal),
            "{literal:?} not verbatim in {code:?}"
        );
    }

    #[test]
    fn basic_wikilink() {
        assert_eq!(
            links("See [[my page]] for details"),
            vec![pair("my page", "my page.md")]
        );
    }

    #[test]
    fn wikilink_with_display() {
        assert_eq!(
            links("See [[target|click here]]"),
            vec![pair("click here", "target.md")]
        );
    }

    #[test]
    fn wikilink_sections() {
        assert_eq!(
            links("See [[page#Install it]] and [[#Usage|usage]]"),
            vec![
                pair("page#Install it", "page.md#Install it"),
                pair("usage", "#Usage")
            ]
        );
        assert_eq!(resolve_target("doc.md#a"), "doc.md#a");
    }

    #[test]
    fn wikilink_with_extension() {
        assert_eq!(links("See [[doc.pdf]]"), vec![pair("doc.pdf", "doc.pdf")]);
    }

    #[test]
    fn skip_code_block() {
        assert_untouched("```\n[[not a link]]\n```", "[[not a link]]");
    }

    #[test]
    fn skip_inline_code() {
        assert_untouched("Use `[[not a link]]` syntax", "[[not a link]]");
    }

    #[test]
    fn no_wikilinks() {
        // A regular markdown link stays a regular link.
        assert!(links("Just normal [markdown](link.md) text").is_empty());
    }

    #[test]
    fn backtick_fence_inside_tilde_fence_does_not_toggle() {
        // A ``` line inside a ~~~ block is content: the fence stays open
        // until the matching ~~~, and links after it still convert.
        let input = "~~~\nfence demo:\n```\n[[real link]]\n~~~\n\nLater: [[notes]]\n";
        let (links, code) = scan(input);
        assert_eq!(links, vec![pair("notes", "notes.md")]);
        assert!(code.contains("[[real link]]"));
    }

    #[test]
    fn tilde_fence_inside_backtick_fence_does_not_toggle() {
        let (links, code) = scan("```\n~~~\n[[not a link]]\n```\n[[link]]");
        assert_eq!(links, vec![pair("link", "link.md")]);
        assert!(code.contains("[[not a link]]"));
    }

    #[test]
    fn longer_fence_needed_to_close() {
        // A four-backtick fence is not closed by three backticks.
        let (links, code) = scan("````\n```\n[[not a link]]\n````\n[[link]]");
        assert_eq!(links, vec![pair("link", "link.md")]);
        assert!(code.contains("[[not a link]]"));
    }

    #[test]
    fn double_backtick_span_protects_wikilink() {
        assert_untouched("Use ``[[a|b]]`` here", "[[a|b]]");
    }

    #[test]
    fn unmatched_backtick_is_literal() {
        // A lone backtick does not swallow the rest of the line.
        assert_eq!(
            links("a ` stray and [[link]]"),
            vec![pair("link", "link.md")]
        );
    }

    #[test]
    fn source_is_passed_through_unchanged() {
        for src in [
            "# Title\n\nSome text\n",
            "# Title\n\nSome text",
            "See [[my page]] and `[[code]]`\n",
        ] {
            assert_eq!(process_wikilinks(src), src);
        }
    }

    #[test]
    fn skip_indented_code_block() {
        assert_untouched(
            "Text\n\n    let x = [[1, 2]];\n    [[indented]]\n",
            "[[indented]]",
        );
    }

    #[test]
    fn skip_code_span_spanning_lines() {
        // comrak joins the span's lines with a space.
        assert_untouched("span `` x\n[[multi]] `` end\n", "[[multi]]");
    }

    #[test]
    fn skip_raw_html_pre() {
        assert_untouched("<pre>\n[[pre]]\n</pre>\n", "[[pre]]");
    }

    #[test]
    fn target_with_spaces_and_parentheses() {
        assert_eq!(
            links("See [[a b (c)]] and [[x y (z)|label]]."),
            vec![pair("a b (c)", "a b (c).md"), pair("label", "x y (z).md")]
        );
    }

    /// End to end: the rendered link carries the resolved destination, and
    /// `[[...]]` inside code renders (and is offered for copying) verbatim.
    #[test]
    fn layout_renders_links_and_leaves_code_alone() {
        let src = "See [[a b (c)]] here.\n\n```\nlet v = [[x]];\n```\n\n<pre>\n[[pre]]\n</pre>\n";
        let src = process_wikilinks(src);
        let arena = Arena::new();
        let root = parse_document(&arena, &src, &crate::parser::options());
        let theme = crate::theme::resolve_theme("dark");
        let result = crate::layout::layout_document(
            root,
            &theme,
            60,
            crate::Spacing::Normal,
            0,
            None,
            crate::image::ImageMode::Off,
            None,
        );
        let spans: Vec<_> = result.lines.iter().flat_map(|l| &l.spans).collect();
        let link = spans
            .iter()
            .find(|s| s.text == "a b (c)")
            .expect("link text");
        assert_eq!(link.style.link_url.as_deref(), Some("a b (c).md"));
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert!(!text.contains("]("), "markdown link syntax leaked: {text}");
        let sources: Vec<_> = result
            .code_blocks
            .iter()
            .map(|c| c.source.as_str())
            .collect();
        assert_eq!(sources, ["let v = [[x]];", "[[pre]]"]);
    }
}
