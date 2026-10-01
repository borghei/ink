//! Mindmaps: an indentation tree, drawn with `├─` / `└─` branches.

use super::canvas::Class;
use super::text;

type Row = Vec<(String, Class)>;

struct Node {
    label: String,
    children: Vec<usize>,
}

/// Strip a node's shape (`id((text))`, `id[text]`, `))text((`, …) and
/// class, leaving its text.
fn node_text(s: &str) -> String {
    let s = s.split_once(":::").map_or(s, |(a, _)| a).trim();
    // The text sits inside the first opening delimiter run and its match.
    let open = s.find(['(', '[', ')', '{']);
    let inner = match open {
        Some(i) => {
            let body = &s[i..];
            let body = body.trim_start_matches(['(', '[', ')', '{']);
            body.trim_end_matches([')', ']', '(', '}'])
        }
        None => s,
    };
    text::decode_label(inner)
}

fn parse(body: &[String]) -> (Vec<Node>, Option<usize>) {
    let mut nodes: Vec<Node> = Vec::new();
    // (indent, node) of the current ancestry.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut root: Option<usize> = None;
    for raw in body {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") || trimmed.starts_with("::") {
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        let label = node_text(trimmed);
        let i = nodes.len();
        nodes.push(Node {
            label,
            children: Vec::new(),
        });
        while stack.last().is_some_and(|&(d, _)| d >= indent) {
            stack.pop();
        }
        match stack.last() {
            Some(&(_, parent)) => nodes[parent].children.push(i),
            None => match root {
                // A second top-level line hangs off the root.
                Some(r) => nodes[r].children.push(i),
                None => root = Some(i),
            },
        }
        stack.push((indent, i));
    }
    (nodes, root)
}

/// The tree as rows at most `inner` columns wide.
pub fn render(body: &[String], inner: usize) -> Vec<Row> {
    let (nodes, root) = parse(body);
    let Some(root) = root else {
        return vec![vec![("(empty)".to_string(), Class::Label)]];
    };
    let inner = inner.max(8);
    let mut rows = Vec::new();
    for l in text::wrap(&nodes[root].label, inner) {
        rows.push(vec![(l, Class::Title)]);
    }
    // Iterative depth-first walk: (node, prefix for its children).
    let mut stack: Vec<(usize, String, bool)> = Vec::new();
    for (k, &c) in nodes[root].children.iter().enumerate().rev() {
        stack.push((c, String::new(), k + 1 == nodes[root].children.len()));
    }
    while let Some((n, prefix, last)) = stack.pop() {
        // Deep trees stop indenting once half the width is used.
        let prefix = if text::width(&prefix) + 3 > inner / 2 {
            text::truncate(&prefix, inner / 2 - 3)
        } else {
            prefix
        };
        let branch = if last { "└─ " } else { "├─ " };
        let cont = if last { "   " } else { "│  " };
        let room = inner.saturating_sub(text::width(&prefix) + 3).max(4);
        let class = if nodes[n].children.is_empty() {
            Class::Plain
        } else {
            Class::Text
        };
        for (i, l) in text::wrap(&nodes[n].label, room).into_iter().enumerate() {
            let lead = format!("{prefix}{}", if i == 0 { branch } else { cont });
            rows.push(vec![(lead, Class::Edge), (l, class)]);
        }
        let child_prefix = format!("{prefix}{cont}");
        let kids = &nodes[n].children;
        for (k, &c) in kids.iter().enumerate().rev() {
            stack.push((c, child_prefix.clone(), k + 1 == kids.len()));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| r.iter().map(|(t, _)| t.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn indentation_becomes_a_tree() {
        let body: Vec<String> = "  root((Ideas))\n    A[First]\n      A1\n      ::icon(fa fa-book)\n      A2\n    B))Second((\n      B1:::urgent"
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(
            plain(&render(&body, 40)),
            "Ideas\n├─ First\n│  ├─ A1\n│  └─ A2\n└─ Second\n   └─ B1"
        );
    }

    #[test]
    fn deep_and_wide_trees_stay_in_width() {
        let body: Vec<String> = (0..2000)
            .map(|i| format!("{}node number {i}", " ".repeat(i % 300)))
            .collect();
        let rows = render(&body, 40);
        for r in &rows {
            let w: usize = r.iter().map(|(t, _)| text::width(t)).sum();
            assert!(w <= 40, "{w}");
        }
    }
}
