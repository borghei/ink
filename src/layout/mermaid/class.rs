//! Parser for `classDiagram`.
//!
//! Classes become three-part boxes (name, attributes, methods);
//! relationships become edges with an end glyph per kind: `△` inheritance
//! and realization, `◆` composition, `◇` aggregation, an arrowhead for
//! association and dependency (dotted lines for the `..` forms). A legend
//! line names the kinds used. Namespaces are drawn as frames.

use super::graph::{Dir, Graph, LineStyle, Marker, Shape};
use super::text::decode_label;
use std::collections::BTreeMap;

#[derive(Default)]
struct Members {
    annotations: Vec<String>,
    attrs: Vec<String>,
    methods: Vec<String>,
}

pub fn parse(body: &[String]) -> Graph {
    let mut g = Graph::new(Dir::Down);
    let mut members: BTreeMap<usize, Members> = BTreeMap::new();
    // Open blocks: a class body (Some(node)) or a namespace (None).
    let mut open: Vec<Option<usize>> = Vec::new();
    let mut namespaces: Vec<usize> = Vec::new();
    for raw in body {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let cluster = namespaces.last().copied();
        if let Some(Some(class)) = open.last().copied() {
            // Inside `class X { … }`.
            for part in line.split('}') {
                let part = part.trim();
                if !part.is_empty() {
                    add_member(members.entry(class).or_default(), part);
                }
            }
            if line.contains('}') {
                open.pop();
            }
            continue;
        }
        if line.starts_with('}') {
            if open.pop() == Some(None) {
                namespaces.pop();
            }
            continue;
        }
        if let Some(rest) = word(line, "direction") {
            if let Some(d) = Dir::parse(rest) {
                g.dir = d;
            }
            continue;
        }
        if let Some(rest) = word(line, "namespace") {
            let name = rest.trim_end_matches('{').trim();
            let c = g.add_cluster(name, name, cluster);
            namespaces.push(c);
            open.push(None);
            continue;
        }
        if let Some(rest) = word(line, "note") {
            let (target, text) = match rest.strip_prefix("for ") {
                Some(r) => {
                    let (t, x) = r.trim().split_once(char::is_whitespace).unwrap_or((r, ""));
                    (Some(t.to_string()), x)
                }
                None => (None, rest),
            };
            let text = decode_label(text).replace("\\n", " ");
            g.notes.push(match target {
                Some(t) => format!("note for {t}: {text}"),
                None => format!("note: {text}"),
            });
            continue;
        }
        if [
            "link", "click", "callback", "cssClass", "style", "classDef", "accTitle", "accDescr",
            "title",
        ]
        .iter()
        .any(|k| word(line, k).is_some() || line.starts_with(&format!("{k}:")))
        {
            continue;
        }
        if let Some(rest) = line.strip_prefix("<<") {
            // `<<interface>> Animal`
            if let Some((ann, name)) = rest.split_once(">>") {
                let (id, _) = class_name(name.trim());
                if !id.is_empty() {
                    let i = touch_class(&mut g, &id, cluster);
                    members
                        .entry(i)
                        .or_default()
                        .annotations
                        .push(ann.trim().to_string());
                }
            }
            continue;
        }
        if let Some(rest) = word(line, "class") {
            let opens = rest.ends_with('{') || rest.contains('{');
            let (head, inline) = match rest.split_once('{') {
                Some((h, b)) => (h.trim(), Some(b)),
                None => (rest, None),
            };
            let (id, label) = class_name(head);
            if id.is_empty() {
                continue;
            }
            let i = touch_class(&mut g, &id, cluster);
            if let Some(l) = label {
                g.nodes[i].label = l;
            }
            if let Some(b) = inline {
                for part in b.split('}') {
                    let part = part.trim();
                    if !part.is_empty() {
                        add_member(members.entry(i).or_default(), part);
                    }
                }
            }
            if opens && !rest.contains('}') {
                open.push(Some(i));
            }
            continue;
        }
        if relation(&mut g, cluster, line) {
            continue;
        }
        if let Some((name, member)) = line.split_once(':') {
            let (id, _) = class_name(name.trim());
            if !id.is_empty() && !id.contains(char::is_whitespace) {
                let i = touch_class(&mut g, &id, cluster);
                add_member(members.entry(i).or_default(), member.trim());
            }
            continue;
        }
        let (id, _) = class_name(line);
        if !id.is_empty() && !id.contains(char::is_whitespace) {
            touch_class(&mut g, &id, cluster);
        }
    }
    for (i, m) in members {
        let node = &mut g.nodes[i];
        if !m.annotations.is_empty() {
            let ann: Vec<String> = m.annotations.iter().map(|a| format!("<<{a}>>")).collect();
            node.label = format!("{}\n{}", ann.join(" "), node.label);
        }
        node.sections = vec![m.attrs, m.methods];
    }
    let legend = legend(&g);
    if !legend.is_empty() {
        g.notes.insert(0, legend);
    }
    g
}

fn word<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// `Name`, `Name~T~`, `Name["Label"]`, `Name:::css` → (id, display label).
fn class_name(s: &str) -> (String, Option<String>) {
    let s = s.trim();
    let s = s.split_once(":::").map_or(s, |(a, _)| a).trim();
    let (s, label) = match s.split_once('[') {
        Some((a, b)) => (a.trim(), Some(decode_label(b.trim_end_matches(']')))),
        None => (s, None),
    };
    let s = s.trim_matches('`');
    let (id, generic) = match s.split_once('~') {
        Some((a, b)) => (a.to_string(), Some(b.trim_end_matches('~').to_string())),
        None => (s.to_string(), None),
    };
    let label = label.or_else(|| generic.map(|g| format!("{id}<{}>", g.replace('~', ""))));
    (id, label)
}

fn touch_class(g: &mut Graph, id: &str, cluster: Option<usize>) -> usize {
    let i = g.touch(id, cluster);
    g.nodes[i].shape = Shape::Sections;
    g.nodes[i].declared = true;
    i
}

fn add_member(m: &mut Members, raw: &str) {
    let s = raw.trim();
    if let Some(a) = s.strip_prefix("<<").and_then(|r| r.strip_suffix(">>")) {
        m.annotations.push(a.trim().to_string());
        return;
    }
    // `List~int~` is a generic: `List<int>` (after decoding, so the angle
    // brackets are not taken for HTML).
    let s = decode_label(s);
    let mut out = String::new();
    let mut open = false;
    for c in s.chars() {
        if c == '~' {
            out.push(if open { '>' } else { '<' });
            open = !open;
        } else {
            out.push(c);
        }
    }
    let text = out;
    if text.is_empty() {
        return;
    }
    if text.contains('(') {
        m.methods.push(text);
    } else {
        m.attrs.push(text);
    }
}

/// `A <|-- B`, `A "1" *-- "many" B : label`, `A ..> B`, … Returns false
/// when the line is not a relationship.
fn relation(g: &mut Graph, cluster: Option<usize>, line: &str) -> bool {
    // The operator: the first `--` or `..` outside quotes.
    let bytes = line.as_bytes();
    let mut quote = false;
    let mut op = None;
    for i in 0..bytes.len().saturating_sub(1) {
        match bytes[i] {
            b'"' => quote = !quote,
            b'-' | b'.' if !quote && bytes[i + 1] == bytes[i] => {
                op = Some(i);
                break;
            }
            _ => {}
        }
    }
    let Some(i) = op else { return false };
    let style = if bytes[i] == b'.' {
        LineStyle::Dotted
    } else {
        LineStyle::Solid
    };
    let left = line[..i].trim_end();
    let right_all = line[i + 2..].trim_start();
    let (right, label) = match right_all.split_once(':') {
        Some((r, l)) => (r.trim(), decode_label(l)),
        None => (right_all.trim(), String::new()),
    };
    let (left, start) = if let Some(l) = left.strip_suffix("<|") {
        (l, Marker::Triangle)
    } else if let Some(l) = left.strip_suffix('*') {
        (l, Marker::DiamondFilled)
    } else if let Some(l) = left.strip_suffix('<') {
        (l, Marker::Arrow)
    } else if let Some(l) = left.strip_suffix('o').filter(|l| l.ends_with([' ', '"'])) {
        (l, Marker::DiamondOpen)
    } else {
        (left, Marker::None)
    };
    let (right, end) = if let Some(r) = right.strip_prefix("|>") {
        (r, Marker::Triangle)
    } else if let Some(r) = right.strip_prefix('*') {
        (r, Marker::DiamondFilled)
    } else if let Some(r) = right.strip_prefix('>') {
        (r, Marker::Arrow)
    } else if let Some(r) = right
        .strip_prefix('o')
        .filter(|r| r.starts_with([' ', '"']))
    {
        (r, Marker::DiamondOpen)
    } else {
        (right, Marker::None)
    };
    let (left, card_l) = trailing_quote(left.trim());
    let (right, card_r) = leading_quote(right.trim());
    let (a, _) = class_name(left);
    let (b, _) = class_name(right);
    if a.is_empty()
        || b.is_empty()
        || a.contains(char::is_whitespace)
        || b.contains(char::is_whitespace)
    {
        return false;
    }
    let ia = touch_class(g, &a, cluster);
    let ib = touch_class(g, &b, cluster);
    g.add_edge(ia, ib, &label, style, start, end);
    if card_l.is_some() || card_r.is_some() {
        if let Some(e) = g.edges.last_mut() {
            e.cards = Some((card_l.unwrap_or_default(), card_r.unwrap_or_default()));
        }
    }
    true
}

fn trailing_quote(s: &str) -> (&str, Option<String>) {
    if let Some(body) = s.strip_suffix('"') {
        if let Some(open) = body.rfind('"') {
            return (body[..open].trim(), Some(body[open + 1..].to_string()));
        }
    }
    (s, None)
}

fn leading_quote(s: &str) -> (&str, Option<String>) {
    if let Some(body) = s.strip_prefix('"') {
        if let Some(close) = body.find('"') {
            return (body[close + 1..].trim(), Some(body[..close].to_string()));
        }
    }
    (s, None)
}

/// "△ inheritance · ◆ composition · …" for the relationship kinds in use.
fn legend(g: &Graph) -> String {
    let mut kinds: Vec<&str> = Vec::new();
    let mut add = |k: &'static str| {
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    };
    for e in &g.edges {
        for m in [e.start, e.end] {
            match (m, e.style) {
                (Marker::Triangle, LineStyle::Dotted) => add("△ realization"),
                (Marker::Triangle, _) => add("△ inheritance"),
                (Marker::DiamondFilled, _) => add("◆ composition"),
                (Marker::DiamondOpen, _) => add("◇ aggregation"),
                (Marker::Arrow, LineStyle::Dotted) => add("▶ dependency"),
                (Marker::Arrow, _) => add("▶ association"),
                _ => {}
            }
        }
    }
    kinds.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> Graph {
        let body: Vec<String> = src.lines().map(str::to_string).collect();
        parse(&body)
    }

    #[test]
    fn members_bodies_annotations_and_generics() {
        let g = parse_src(
            "class Animal {\n  <<interface>>\n  +String name\n  +eat() void\n}\nAnimal : +int age\nclass Square~Shape~{\n  List~int~ ids\n}\n<<service>> Repo\nclass Empty",
        );
        let a = &g.nodes[g.find("Animal").unwrap()];
        assert_eq!(a.label, "<<interface>>\nAnimal");
        assert_eq!(
            a.sections,
            vec![
                vec!["+String name".to_string(), "+int age".into()],
                vec!["+eat() void".to_string()]
            ]
        );
        let s = &g.nodes[g.find("Square").unwrap()];
        assert_eq!(s.label, "Square<Shape>");
        assert_eq!(s.sections[0], vec!["List<int> ids".to_string()]);
        assert_eq!(g.nodes[g.find("Repo").unwrap()].label, "<<service>>\nRepo");
        assert!(g.find("Empty").is_some());
    }

    #[test]
    fn relationship_kinds_cardinalities_and_labels() {
        let g = parse_src(
            "A <|-- B\nC *-- D\nE o-- F\nG --> H\nI ..> J\nK ..|> L\nM -- N\nCustomer \"1\" --> \"*\" Ticket : buys\nP <|--|> Q",
        );
        let k: Vec<(Marker, Marker, LineStyle)> =
            g.edges.iter().map(|e| (e.start, e.end, e.style)).collect();
        assert_eq!(k[0], (Marker::Triangle, Marker::None, LineStyle::Solid));
        assert_eq!(
            k[1],
            (Marker::DiamondFilled, Marker::None, LineStyle::Solid)
        );
        assert_eq!(k[2], (Marker::DiamondOpen, Marker::None, LineStyle::Solid));
        assert_eq!(k[3], (Marker::None, Marker::Arrow, LineStyle::Solid));
        assert_eq!(k[4], (Marker::None, Marker::Arrow, LineStyle::Dotted));
        assert_eq!(k[5], (Marker::None, Marker::Triangle, LineStyle::Dotted));
        assert_eq!(k[6], (Marker::None, Marker::None, LineStyle::Solid));
        assert_eq!(k[8], (Marker::Triangle, Marker::Triangle, LineStyle::Solid));
        let e = &g.edges[7];
        assert_eq!(e.label, "buys");
        assert_eq!(e.cards, Some(("1".into(), "*".into())));
        assert_eq!(
            g.notes[0],
            "△ inheritance · ◆ composition · ◇ aggregation · ▶ association · ▶ dependency · △ realization"
        );
    }

    #[test]
    fn namespaces_are_subgraphs() {
        let g = parse_src("namespace Shapes {\n  class Triangle\n  class Square {\n    int side\n  }\n}\nclass Outside");
        assert_eq!(g.clusters.len(), 1);
        assert_eq!(g.nodes[g.find("Square").unwrap()].cluster, Some(0));
        assert_eq!(g.nodes[g.find("Outside").unwrap()].cluster, None);
        assert_eq!(
            g.nodes[g.find("Square").unwrap()].sections[0],
            vec!["int side".to_string()]
        );
    }
}
