//! Parser for `erDiagram`.
//!
//! Entities become boxes with one row per attribute; relationships become
//! labelled edges whose ends carry the crow's-foot cardinality as text
//! (`1`, `0..1`, `0..*`, `1..*`). Non-identifying (`..`) relationships are
//! dotted.

use super::graph::{Dir, Graph, LineStyle, Marker, Shape};
use super::text::decode_label;

pub fn parse(body: &[String]) -> Graph {
    let mut g = Graph::new(Dir::Down);
    let mut block: Option<usize> = None;
    let mut attrs: Vec<(usize, String)> = Vec::new();
    for raw in body {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        if let Some(e) = block {
            if line.starts_with('}') {
                block = None;
            } else {
                attrs.push((e, attribute(line)));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("direction") {
            if let Some(d) = Dir::parse(rest) {
                g.dir = d;
            }
            continue;
        }
        if [
            "classDef", "class", "style", "accTitle", "accDescr", "title",
        ]
        .iter()
        .any(|k| line.starts_with(&format!("{k} ")) || line.starts_with(&format!("{k}:")))
        {
            continue;
        }
        if relationship(&mut g, line) {
            continue;
        }
        // `ENTITY {`, `ENTITY`, `p[Person] {`
        let opens = line.ends_with('{');
        let head = line.trim_end_matches('{').trim();
        if let Some(i) = entity(&mut g, head) {
            if opens {
                block = Some(i);
            }
        }
    }
    for i in 0..g.nodes.len() {
        let rows: Vec<String> = attrs
            .iter()
            .filter(|(e, _)| *e == i)
            .map(|(_, r)| r.clone())
            .filter(|r| !r.is_empty())
            .collect();
        g.nodes[i].sections = vec![rows];
    }
    g
}

/// An entity reference: `NAME`, `"NAME"`, `id[Alias]`, `id["Alias"]`.
fn entity(g: &mut Graph, s: &str) -> Option<usize> {
    let s = s.trim();
    let (id, alias) = match s.split_once('[') {
        Some((a, b)) => (a.trim(), Some(decode_label(b.trim_end_matches(']')))),
        None => (s, None),
    };
    let id = id.trim_matches('"');
    if id.is_empty() || id.contains(char::is_whitespace) && !s.starts_with('"') {
        return None;
    }
    let i = g.touch(id, None);
    g.nodes[i].shape = Shape::Sections;
    g.nodes[i].declared = true;
    if let Some(a) = alias {
        g.nodes[i].label = a;
    }
    Some(i)
}

/// `type name PK, FK "comment"` → `type name PK,FK`.
fn attribute(line: &str) -> String {
    let line = match line.find('"') {
        Some(q) => &line[..q],
        None => line,
    };
    let mut words: Vec<&str> = line.split_whitespace().collect();
    // Keys may be written `PK, FK`: glue them back together.
    let mut out = Vec::new();
    while !words.is_empty() {
        let w = words.remove(0);
        if w.ends_with(',') && !words.is_empty() {
            let next = words.remove(0);
            out.push(format!("{w}{next}"));
        } else {
            out.push(w.to_string());
        }
    }
    decode_label(&out.join(" ").replace('~', ""))
}

fn left_card(s: &str) -> Option<&'static str> {
    Some(match s {
        "|o" => "0..1",
        "||" => "1",
        "}o" => "0..*",
        "}|" => "1..*",
        _ => return None,
    })
}

fn right_card(s: &str) -> Option<&'static str> {
    Some(match s {
        "o|" => "0..1",
        "||" => "1",
        "o{" => "0..*",
        "|{" => "1..*",
        _ => return None,
    })
}

/// `A ||--o{ B : label` (`..` for non-identifying). False when the line
/// is not a relationship.
fn relationship(g: &mut Graph, line: &str) -> bool {
    let (head, label) = match line.split_once(':') {
        Some((h, l)) => (h.trim(), decode_label(l)),
        None => (line, String::new()),
    };
    let chars: Vec<char> = head.chars().collect();
    let n = chars.len();
    for i in 2..n.saturating_sub(3) {
        let mid: String = chars[i..i + 2].iter().collect();
        if mid != "--" && mid != ".." {
            continue;
        }
        let l: String = chars[i - 2..i].iter().collect();
        let r: String = chars[i + 2..i + 4].iter().collect();
        let (Some(lc), Some(rc)) = (left_card(&l), right_card(&r)) else {
            continue;
        };
        let a: String = chars[..i - 2].iter().collect();
        let b: String = chars[i + 4..].iter().collect();
        let (Some(ia), Some(ib)) = (entity(g, &a), entity(g, &b)) else {
            return false;
        };
        let style = if mid == ".." {
            LineStyle::Dotted
        } else {
            LineStyle::Solid
        };
        g.add_edge(ia, ib, &label, style, Marker::None, Marker::None);
        if let Some(e) = g.edges.last_mut() {
            e.cards = Some((lc.to_string(), rc.to_string()));
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> Graph {
        let body: Vec<String> = src.lines().map(str::to_string).collect();
        parse(&body)
    }

    #[test]
    fn entities_attributes_and_cardinalities() {
        let g = parse_src(
            "CUSTOMER ||--o{ ORDER : places\nORDER ||--|{ LINE-ITEM : contains\nCUSTOMER }|..|{ DELIVERY-ADDRESS : uses\nCUSTOMER {\n  string name PK \"the name\"\n  string email UK\n  int orderId FK, PK\n}\np[Person] |o--o| CAR : \"drives\"",
        );
        let c = &g.nodes[g.find("CUSTOMER").unwrap()];
        assert_eq!(
            c.sections[0],
            vec![
                "string name PK".to_string(),
                "string email UK".into(),
                "int orderId FK,PK".into()
            ]
        );
        let cards: Vec<_> = g.edges.iter().map(|e| e.cards.clone().unwrap()).collect();
        assert_eq!(cards[0], ("1".into(), "0..*".into()));
        assert_eq!(cards[1], ("1".into(), "1..*".into()));
        assert_eq!(cards[2], ("1..*".into(), "1..*".into()));
        assert_eq!(cards[3], ("0..1".into(), "0..1".into()));
        assert_eq!(g.edges[2].style, LineStyle::Dotted);
        assert_eq!(g.edges[3].label, "drives");
        assert_eq!(g.nodes[g.find("p").unwrap()].label, "Person");
        assert!(g.find("LINE-ITEM").is_some());
    }
}
