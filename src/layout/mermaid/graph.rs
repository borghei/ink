//! The graph model every node-and-edge diagram (flowchart, state, class,
//! ER) is parsed into, and that the layered layout engine draws.

use std::collections::BTreeMap;

/// Which way the ranks run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Top to bottom (`TD`, `TB`).
    Down,
    /// Bottom to top (`BT`).
    Up,
    /// Left to right (`LR`).
    Right,
    /// Right to left (`RL`).
    Left,
}

impl Dir {
    /// Parse a mermaid direction token (`TD`, `TB`, `BT`, `LR`, `RL`,
    /// and the arrow forms `v ^ > <`).
    pub fn parse(tok: &str) -> Option<Dir> {
        Some(
            match tok
                .trim()
                .trim_end_matches(';')
                .to_ascii_uppercase()
                .as_str()
            {
                "TD" | "TB" | "V" => Dir::Down,
                "BT" | "^" => Dir::Up,
                "LR" | ">" => Dir::Right,
                "RL" | "<" => Dir::Left,
                _ => return None,
            },
        )
    }

    /// Ranks advance horizontally.
    pub fn horizontal(self) -> bool {
        matches!(self, Dir::Right | Dir::Left)
    }

    /// Ranks are laid out in reverse (`BT`, `RL`).
    pub fn reversed(self) -> bool {
        matches!(self, Dir::Up | Dir::Left)
    }
}

/// How a node is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Round,
    Stadium,
    Circle,
    Diamond,
    Hexagon,
    Subroutine,
    Cylinder,
    /// `>text]`
    Flag,
    /// Parallelograms and trapezoids, with their left and right side glyphs.
    Slant(char, char),
    /// A box split into compartments (class and ER entities): the first
    /// section is the centred title, the others left-aligned rows.
    Sections,
    /// State-diagram start `[*]`: a single `●`.
    Start,
    /// State-diagram end `[*]`: a single `◉`.
    End,
    /// `<<choice>>`: a single `◇`.
    Choice,
    /// `<<fork>>` / `<<join>>`: a short bar.
    Bar,
}

impl Shape {
    /// Drawn as a bare glyph rather than a bordered box.
    pub fn is_glyph(self) -> bool {
        matches!(self, Shape::Start | Shape::End | Shape::Choice | Shape::Bar)
    }
}

/// Line style of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LineStyle {
    Solid,
    Dotted,
    Thick,
    /// `~~~`: affects layout, not drawn.
    Invisible,
}

/// What an edge end looks like where it meets its node.
#[allow(dead_code)] // relationship markers arrive with class diagrams
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Marker {
    None,
    Arrow,
    Circle,
    Cross,
    /// Hollow triangle: inheritance / realization.
    Triangle,
    /// Filled diamond: composition.
    DiamondFilled,
    /// Hollow diamond: aggregation.
    DiamondOpen,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    /// Display text; `\n` separates lines.
    pub label: String,
    pub shape: Shape,
    /// For [`Shape::Sections`]: the compartments after the title section
    /// (the title is `label`). Each is a list of rows.
    pub sections: Vec<Vec<String>>,
    /// Innermost subgraph this node belongs to.
    pub cluster: Option<usize>,
    /// The node's shape or label was given explicitly (not just mentioned).
    pub declared: bool,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub style: LineStyle,
    /// Marker at the `from` end.
    pub start: Marker,
    /// Marker at the `to` end.
    pub end: Marker,
    /// Text cardinalities at the (`from`, `to`) ends (class and ER).
    pub cards: Option<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Cluster {
    pub id: String,
    pub title: String,
    pub parent: Option<usize>,
}

/// A parsed diagram ready for layout.
#[derive(Debug, Clone)]
pub struct Graph {
    pub dir: Dir,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub clusters: Vec<Cluster>,
    /// Lines shown under the diagram (notes, legends).
    pub notes: Vec<String>,
    index: BTreeMap<String, usize>,
}

impl Graph {
    pub fn new(dir: Dir) -> Self {
        Graph {
            dir,
            nodes: Vec::new(),
            edges: Vec::new(),
            clusters: Vec::new(),
            notes: Vec::new(),
            index: BTreeMap::new(),
        }
    }

    pub fn find(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    /// The node with `id`, created (labelled with its id) on first mention.
    /// `cluster` is the subgraph the mention appears in: a node belongs to
    /// the first subgraph it is mentioned inside.
    pub fn touch(&mut self, id: &str, cluster: Option<usize>) -> usize {
        if let Some(&i) = self.index.get(id) {
            if self.nodes[i].cluster.is_none() {
                self.nodes[i].cluster = cluster;
            }
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(Node {
            id: id.to_string(),
            label: id.to_string(),
            shape: Shape::Rect,
            sections: Vec::new(),
            cluster,
            declared: false,
        });
        self.index.insert(id.to_string(), i);
        i
    }

    /// Mention `id` and set its label and shape.
    pub fn declare(
        &mut self,
        id: &str,
        label: &str,
        shape: Shape,
        cluster: Option<usize>,
    ) -> usize {
        let i = self.touch(id, cluster);
        self.nodes[i].label = label.to_string();
        self.nodes[i].shape = shape;
        self.nodes[i].declared = true;
        i
    }

    pub fn add_edge(
        &mut self,
        from: usize,
        to: usize,
        label: &str,
        style: LineStyle,
        start: Marker,
        end: Marker,
    ) {
        self.edges.push(Edge {
            from,
            to,
            label: label.to_string(),
            style,
            start,
            end,
            cards: None,
        });
    }

    pub fn add_cluster(&mut self, id: &str, title: &str, parent: Option<usize>) -> usize {
        self.clusters.push(Cluster {
            id: id.to_string(),
            title: title.to_string(),
            parent,
        });
        self.clusters.len() - 1
    }

    /// Ancestors of a cluster from the outermost down to `c` itself.
    pub fn cluster_path(&self, c: Option<usize>) -> Vec<usize> {
        let mut path = Vec::new();
        let mut cur = c;
        while let Some(i) = cur {
            if path.len() > 64 || path.contains(&i) {
                break;
            }
            path.push(i);
            cur = self.clusters.get(i).and_then(|k| k.parent);
        }
        path.reverse();
        path
    }

    /// Edges that name a subgraph (`A --> sub1`) attach to the subgraph's
    /// first member (or, for edges leaving it, its last): there is no node
    /// to draw for a subgraph id. Nodes only ever mentioned by such an id
    /// are dropped.
    pub fn redirect_cluster_edges(&mut self) {
        let mut redirect: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        for (ci, cl) in self.clusters.iter().enumerate() {
            let Some(n) = self.find(&cl.id) else { continue };
            if self.nodes[n].declared {
                continue;
            }
            let members: Vec<usize> = (0..self.nodes.len())
                .filter(|&m| m != n && self.cluster_path(self.nodes[m].cluster).contains(&ci))
                .collect();
            if let (Some(&first), Some(&last)) = (members.first(), members.last()) {
                redirect.insert(n, (first, last));
            }
        }
        if redirect.is_empty() {
            return;
        }
        for e in &mut self.edges {
            if let Some(&(_, last)) = redirect.get(&e.from) {
                e.from = last;
            }
            if let Some(&(first, _)) = redirect.get(&e.to) {
                e.to = first;
            }
        }
        self.remove_nodes(&redirect.keys().copied().collect::<Vec<_>>());
    }

    /// Remove nodes (and any edges touching them), renumbering the rest.
    pub fn remove_nodes(&mut self, gone: &[usize]) {
        if gone.is_empty() {
            return;
        }
        let mut map = vec![usize::MAX; self.nodes.len()];
        let mut kept = Vec::new();
        for (i, n) in self.nodes.drain(..).enumerate() {
            if !gone.contains(&i) {
                map[i] = kept.len();
                kept.push(n);
            }
        }
        self.nodes = kept;
        self.edges
            .retain(|e| map[e.from] != usize::MAX && map[e.to] != usize::MAX);
        for e in &mut self.edges {
            e.from = map[e.from];
            e.to = map[e.to];
        }
        self.index = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();
    }
}
