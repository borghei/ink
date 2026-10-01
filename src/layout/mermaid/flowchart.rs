//! Parser for `graph` / `flowchart` diagrams.

use super::graph::{Dir, Graph, LineStyle, Marker, Shape};
use super::text::decode_label;

/// Parse a flowchart. `header` is the rest of the first line after the
/// `graph`/`flowchart` keyword (the direction); `body` the remaining lines.
pub fn parse(header: &str, body: &[String]) -> Graph {
    let dir = header
        .split_whitespace()
        .next()
        .and_then(Dir::parse)
        .unwrap_or(Dir::Down);
    let mut g = Graph::new(dir);
    let mut stack: Vec<usize> = Vec::new();
    // A header may carry a statement after `;` (`graph TD; A-->B`).
    let mut lines: Vec<String> = Vec::new();
    if let Some((_, rest)) = header.split_once(';') {
        lines.push(rest.to_string());
    }
    lines.extend(body.iter().cloned());
    for line in &lines {
        for stmt in split_statements(line) {
            statement(&mut g, &mut stack, stmt.trim());
        }
    }
    g.redirect_cluster_edges();
    g
}

/// Split a line on `;` outside quotes, brackets and `|edge labels|`.
fn split_statements(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut quote = false;
    let mut pipe = false;
    for c in line.chars() {
        match c {
            '"' => quote = !quote,
            '|' if !quote && depth == 0 => pipe = !pipe,
            '[' | '(' | '{' if !quote && !pipe => depth += 1,
            ']' | ')' | '}' if !quote && !pipe => depth = (depth - 1).max(0),
            ';' if !quote && !pipe && depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

fn keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim())
    } else {
        None
    }
}

fn statement(g: &mut Graph, stack: &mut Vec<usize>, s: &str) {
    if s.is_empty() || s.starts_with("%%") {
        return;
    }
    if let Some(rest) = keyword(s, "subgraph") {
        let (id, title) = subgraph_title(rest);
        let c = g.add_cluster(&id, &title, stack.last().copied());
        stack.push(c);
        return;
    }
    if s == "end" {
        stack.pop();
        return;
    }
    for kw in [
        "direction",
        "classDef",
        "class",
        "style",
        "linkStyle",
        "click",
        "accTitle",
        "accDescr",
    ] {
        if keyword(s, kw).is_some() || s.starts_with(&format!("{kw}:")) {
            return;
        }
    }
    chain(g, stack.last().copied(), s);
}

/// `id`, `id [Title]`, `id["Title"]`, `"Title"`, or `Several words`.
fn subgraph_title(rest: &str) -> (String, String) {
    let rest = rest.trim();
    if rest.starts_with('"') {
        let t = decode_label(rest);
        return (t.clone(), t);
    }
    if let Some(open) = rest.find('[') {
        let id = rest[..open].trim().to_string();
        let inner = rest[open + 1..].trim_end().trim_end_matches(']');
        return (id, decode_label(inner));
    }
    let id = rest.split_whitespace().next().unwrap_or("").to_string();
    (id, decode_label(rest))
}

struct Cursor<'a> {
    s: &'a [char],
    i: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn at(&self, k: usize) -> Option<char> {
        self.s.get(self.i + k).copied()
    }
    fn starts(&self, pat: &str) -> bool {
        pat.chars()
            .enumerate()
            .all(|(k, c)| self.s.get(self.i + k) == Some(&c))
    }
    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }
    fn rest(&self) -> String {
        self.s[self.i.min(self.s.len())..].iter().collect()
    }
    /// Text up to the first occurrence of `pat`, consuming the pattern too.
    fn until(&mut self, pat: &str) -> Option<String> {
        let start = self.i;
        while self.i < self.s.len() {
            if self.starts(pat) {
                let t: String = self.s[start..self.i].iter().collect();
                self.i += pat.chars().count();
                return Some(t);
            }
            self.i += 1;
        }
        self.i = start;
        None
    }
}

fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || (!c.is_ascii() && !c.is_whitespace() && !c.is_control())
}

struct NodeRef {
    id: String,
    shape: Option<(String, Shape)>,
}

struct Link {
    style: LineStyle,
    start: Marker,
    end: Marker,
    label: String,
}

fn chain(g: &mut Graph, cluster: Option<usize>, s: &str) {
    let chars: Vec<char> = s.chars().collect();
    let mut cur = Cursor { s: &chars, i: 0 };
    let mut prev = node_group(&mut cur);
    if prev.is_empty() {
        return;
    }
    let mut prev_ids: Vec<usize> = prev.drain(..).map(|n| register(g, cluster, n)).collect();
    loop {
        cur.skip_ws();
        let Some(link) = link(&mut cur) else { break };
        let next = node_group(&mut cur);
        if next.is_empty() {
            break;
        }
        let ids: Vec<usize> = next.into_iter().map(|n| register(g, cluster, n)).collect();
        for &a in &prev_ids {
            for &b in &ids {
                g.add_edge(a, b, &link.label, link.style, link.start, link.end);
            }
        }
        prev_ids = ids;
    }
}

fn register(g: &mut Graph, cluster: Option<usize>, n: NodeRef) -> usize {
    match n.shape {
        Some((label, shape)) => g.declare(&n.id, &label, shape, cluster),
        None => g.touch(&n.id, cluster),
    }
}

fn node_group(cur: &mut Cursor) -> Vec<NodeRef> {
    let mut out = Vec::new();
    loop {
        cur.skip_ws();
        let Some(n) = node(cur) else { break };
        out.push(n);
        cur.skip_ws();
        if cur.peek() == Some('&') {
            cur.i += 1;
            continue;
        }
        break;
    }
    out
}

fn node(cur: &mut Cursor) -> Option<NodeRef> {
    let start = cur.i;
    while let Some(c) = cur.peek() {
        let dash_in_id =
            c == '-' && cur.i > start && cur.at(1).is_some_and(|n| n.is_alphanumeric());
        if is_id_char(c) || dash_in_id {
            cur.i += 1;
        } else {
            break;
        }
    }
    // A lone `o`/`x` right before a link body is a link head, not a node.
    if cur.i == start {
        return None;
    }
    let id: String = cur.s[start..cur.i].iter().collect();
    let shapes: [(&str, &str, Shape); 13] = [
        ("(((", ")))", Shape::Circle),
        ("((", "))", Shape::Circle),
        ("([", "])", Shape::Stadium),
        ("[[", "]]", Shape::Subroutine),
        ("[(", ")]", Shape::Cylinder),
        ("{{", "}}", Shape::Hexagon),
        ("[/", "/]", Shape::Slant('/', '/')),
        ("[\\", "\\]", Shape::Slant('\\', '\\')),
        ("(", ")", Shape::Round),
        ("[", "]", Shape::Rect),
        ("{", "}", Shape::Diamond),
        (">", "]", Shape::Flag),
        ("@{", "}", Shape::Rect),
    ];
    let mut shape = None;
    for (open, close, sh) in shapes {
        if !cur.starts(open) {
            continue;
        }
        cur.i += open.chars().count();
        if open == "@{" {
            let body = cur.until("}").unwrap_or_else(|| {
                let r = cur.rest();
                cur.i = cur.s.len();
                r
            });
            shape = Some(shape_data(&body, &id));
            break;
        }
        let label = if cur.peek() == Some('"') {
            cur.i += 1;
            let q = cur.until("\"").unwrap_or_default();
            // Skip to the closer.
            let _ = cur.until(close);
            q
        } else if matches!(sh, Shape::Slant(..)) {
            // `[/text\]` and `[\text/]` are trapezoids.
            let other = if open == "[/" { "\\]" } else { "/]" };
            let save = cur.i;
            let a = cur.until(close).map(|t| (t, cur.i));
            cur.i = save;
            let b = cur.until(other).map(|t| (t, cur.i));
            match (a, b) {
                (Some((t, ia)), Some((u, ib))) => {
                    if ia <= ib {
                        cur.i = ia;
                        t
                    } else {
                        cur.i = ib;
                        let l = open.chars().nth(1).unwrap_or('/');
                        let r = other.chars().next().unwrap_or('\\');
                        shape = Some((decode_label(&u), Shape::Slant(l, r)));
                        break;
                    }
                }
                (Some((t, ia)), None) => {
                    cur.i = ia;
                    t
                }
                (None, Some((u, ib))) => {
                    cur.i = ib;
                    let l = open.chars().nth(1).unwrap_or('/');
                    let r = other.chars().next().unwrap_or('\\');
                    shape = Some((decode_label(&u), Shape::Slant(l, r)));
                    break;
                }
                (None, None) => {
                    let r = cur.rest();
                    cur.i = cur.s.len();
                    r
                }
            }
        } else {
            match cur.until(close) {
                Some(t) => t,
                None => {
                    let r = cur.rest();
                    cur.i = cur.s.len();
                    r
                }
            }
        };
        shape = Some((decode_label(&label), sh));
        break;
    }
    // `:::className`
    if cur.starts(":::") {
        cur.i += 3;
        while cur.peek().is_some_and(|c| is_id_char(c) || c == '-') {
            cur.i += 1;
        }
    }
    Some(NodeRef { id, shape })
}

/// `@{ shape: diam, label: "Text" }`
fn shape_data(body: &str, id: &str) -> (String, Shape) {
    let mut label = id.to_string();
    let mut shape = Shape::Rect;
    for part in body.split(',') {
        let Some((k, v)) = part.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "label" => label = decode_label(v),
            "shape" => {
                shape = match v.trim_matches('"') {
                    "rounded" | "event" => Shape::Round,
                    "stadium" | "pill" | "terminal" => Shape::Stadium,
                    "circle" | "circ" | "dbl-circ" | "double-circle" => Shape::Circle,
                    "diam" | "diamond" | "decision" | "question" => Shape::Diamond,
                    "hex" | "hexagon" | "prepare" => Shape::Hexagon,
                    "cyl" | "db" | "database" | "cylinder" => Shape::Cylinder,
                    "subproc" | "subprocess" | "subroutine" | "fr-rect" => Shape::Subroutine,
                    _ => Shape::Rect,
                }
            }
            _ => {}
        }
    }
    (label, shape)
}

/// A link token (`-->`, `-.->`, `==>`, `--o`, `<-->`, `-- text -->`, …)
/// with an optional `|label|`.
fn link(cur: &mut Cursor) -> Option<Link> {
    let save = cur.i;
    let mut start = Marker::None;
    match cur.peek() {
        Some('<') => {
            start = Marker::Arrow;
            cur.i += 1;
        }
        Some('o') if matches!(cur.at(1), Some('-' | '=' | '.')) => {
            start = Marker::Circle;
            cur.i += 1;
        }
        Some('x') if matches!(cur.at(1), Some('-' | '=' | '.')) => {
            start = Marker::Cross;
            cur.i += 1;
        }
        _ => {}
    }
    let body_start = cur.i;
    while matches!(cur.peek(), Some('-' | '=' | '.' | '~')) {
        cur.i += 1;
    }
    let body: String = cur.s[body_start..cur.i].iter().collect();
    if body.chars().count() < 2 || body.starts_with('.') {
        cur.i = save;
        return None;
    }
    let style_of = |b: &str| {
        if b.contains('~') {
            LineStyle::Invisible
        } else if b.contains('=') {
            LineStyle::Thick
        } else if b.contains('.') {
            LineStyle::Dotted
        } else {
            LineStyle::Solid
        }
    };
    let mut style = style_of(&body);
    let mut label = String::new();
    let mut end = head(cur);
    // `-- text -->`, `-. text .->`, `== text ==>`
    if end == Marker::None
        && matches!(body.as_str(), "--" | "==" | "-.")
        && cur.peek().is_some_and(char::is_whitespace)
    {
        let closers: &[&str] = match body.as_str() {
            "--" => &["-->", "---", "--o", "--x"],
            "==" => &["==>", "===", "==o", "==x"],
            _ => &[".->", ".-"],
        };
        let text_start = cur.i;
        let mut found = None;
        let mut k = cur.i;
        'scan: while k < cur.s.len() {
            for c in closers {
                let probe = Cursor { s: cur.s, i: k };
                if probe.starts(c) {
                    found = Some(k);
                    break 'scan;
                }
            }
            k += 1;
        }
        if let Some(k) = found {
            label = cur.s[text_start..k].iter().collect::<String>();
            cur.i = k;
            while matches!(cur.peek(), Some('-' | '=' | '.')) {
                cur.i += 1;
            }
            end = head(cur);
            if body == "-." {
                style = LineStyle::Dotted;
            }
        }
    }
    cur.skip_ws();
    if cur.peek() == Some('|') {
        cur.i += 1;
        if let Some(t) = cur.until("|") {
            label = t;
        }
    }
    Some(Link {
        style,
        start,
        end,
        label: decode_label(&label),
    })
}

fn head(cur: &mut Cursor) -> Marker {
    match cur.peek() {
        Some('>') => {
            cur.i += 1;
            Marker::Arrow
        }
        Some(c @ ('o' | 'x')) if !cur.at(1).is_some_and(is_id_char) => {
            cur.i += 1;
            if c == 'o' {
                Marker::Circle
            } else {
                Marker::Cross
            }
        }
        _ => Marker::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> Graph {
        let mut lines = src.lines();
        let header = lines.next().unwrap_or("");
        let header = header.split_once(' ').map_or("", |(_, r)| r);
        let body: Vec<String> = lines.map(str::to_string).collect();
        parse(header, &body)
    }

    fn edges(g: &Graph) -> Vec<(String, String, String)> {
        g.edges
            .iter()
            .map(|e| {
                (
                    g.nodes[e.from].id.clone(),
                    g.nodes[e.to].id.clone(),
                    e.label.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn shapes_and_labels() {
        let g = parse_src(
            "graph TD\nA[rect] --> B(round)\nC([stadium]) --> D{diamond}\nE((circle)) --> F>flag]\nG[[sub]] --> H[(db)]\nI{{hex}} --> J[/para/]\nK[/trap\\] --> L[\"quoted [x] (y)\"]",
        );
        let shapes: Vec<(String, String, Shape)> = g
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone(), n.shape))
            .collect();
        assert_eq!(shapes[0], ("A".into(), "rect".into(), Shape::Rect));
        assert_eq!(shapes[1], ("B".into(), "round".into(), Shape::Round));
        assert_eq!(shapes[2], ("C".into(), "stadium".into(), Shape::Stadium));
        assert_eq!(shapes[3], ("D".into(), "diamond".into(), Shape::Diamond));
        assert_eq!(shapes[4], ("E".into(), "circle".into(), Shape::Circle));
        assert_eq!(shapes[5], ("F".into(), "flag".into(), Shape::Flag));
        assert_eq!(shapes[6], ("G".into(), "sub".into(), Shape::Subroutine));
        assert_eq!(shapes[7], ("H".into(), "db".into(), Shape::Cylinder));
        assert_eq!(shapes[8], ("I".into(), "hex".into(), Shape::Hexagon));
        assert_eq!(
            shapes[9],
            ("J".into(), "para".into(), Shape::Slant('/', '/'))
        );
        assert_eq!(
            shapes[10],
            ("K".into(), "trap".into(), Shape::Slant('/', '\\'))
        );
        assert_eq!(
            shapes[11],
            ("L".into(), "quoted [x] (y)".into(), Shape::Rect)
        );
    }

    #[test]
    fn link_kinds() {
        let g = parse_src(
            "flowchart LR\nA --> B\nB --- C\nC -.-> D\nD ==> E\nE --o F\nF --x G\nG <--> H\nH -->|yes| I\nI -- some text --> J\nJ -. dotted .-> K\nK == thick ==> L\nL ~~~ M",
        );
        assert_eq!(g.dir, Dir::Right);
        let e = &g.edges;
        assert_eq!((e[0].style, e[0].end), (LineStyle::Solid, Marker::Arrow));
        assert_eq!((e[1].style, e[1].end), (LineStyle::Solid, Marker::None));
        assert_eq!((e[2].style, e[2].end), (LineStyle::Dotted, Marker::Arrow));
        assert_eq!((e[3].style, e[3].end), (LineStyle::Thick, Marker::Arrow));
        assert_eq!(e[4].end, Marker::Circle);
        assert_eq!(e[5].end, Marker::Cross);
        assert_eq!((e[6].start, e[6].end), (Marker::Arrow, Marker::Arrow));
        assert_eq!(e[7].label, "yes");
        assert_eq!(
            (e[8].label.as_str(), e[8].end),
            ("some text", Marker::Arrow)
        );
        assert_eq!(
            (e[9].label.as_str(), e[9].style),
            ("dotted", LineStyle::Dotted)
        );
        assert_eq!(
            (e[10].label.as_str(), e[10].style),
            ("thick", LineStyle::Thick)
        );
        assert_eq!(e[11].style, LineStyle::Invisible);
        assert_eq!(g.nodes.len(), 13);
    }

    #[test]
    fn chains_groups_subgraphs_and_ignored_lines() {
        let g = parse_src(
            "graph TB\n%% a comment\nclassDef red fill:#f00\nA --> B --> C\nA & B --> D & E\nsubgraph one [First One]\n  X --> Y\n  subgraph two\n    Z\n  end\nend\nstyle A fill:#f9f\nclick A callback\nlinkStyle 0 stroke:#f00\nclass A red\nA:::red --> X",
        );
        assert_eq!(
            edges(&g),
            vec![
                ("A".into(), "B".into(), "".into()),
                ("B".into(), "C".into(), "".into()),
                ("A".into(), "D".into(), "".into()),
                ("A".into(), "E".into(), "".into()),
                ("B".into(), "D".into(), "".into()),
                ("B".into(), "E".into(), "".into()),
                ("X".into(), "Y".into(), "".into()),
                ("A".into(), "X".into(), "".into()),
            ]
        );
        assert_eq!(g.clusters.len(), 2);
        assert_eq!(g.clusters[0].title, "First One");
        assert_eq!(g.clusters[1].parent, Some(0));
        let z = g.find("Z").unwrap();
        assert_eq!(g.nodes[z].cluster, Some(1));
        assert_eq!(g.nodes[g.find("X").unwrap()].cluster, Some(0));
        assert_eq!(g.nodes[g.find("A").unwrap()].cluster, None);
    }

    #[test]
    fn semicolons_and_breaks() {
        let g = parse_src("graph TD;\nA[\"one<br>two\"]-->B;B-->C;");
        assert_eq!(g.nodes[0].label, "one\ntwo");
        assert_eq!(g.edges.len(), 2);
    }

    #[test]
    fn edges_to_a_subgraph_attach_to_its_members() {
        let g = parse_src("graph TD\nsubgraph S\nA --> B\nend\nX --> S\nS --> Y");
        assert!(g.find("S").is_none());
        assert_eq!(
            edges(&g),
            vec![
                ("A".into(), "B".into(), "".into()),
                ("X".into(), "A".into(), "".into()),
                ("B".into(), "Y".into(), "".into()),
            ]
        );
    }
}
