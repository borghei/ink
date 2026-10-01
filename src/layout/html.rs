//! Raw HTML embedded in markdown.

/// True when an inline HTML fragment is a line-break tag: `<br>`, `<br/>`,
/// `<br />`, in any letter case.
pub fn is_br_tag(html: &str) -> bool {
    let tag = html.trim().to_ascii_lowercase();
    tag == "<br>" || tag == "<br/>" || tag == "<br />"
}
