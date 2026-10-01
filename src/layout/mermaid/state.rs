//! Parser for `stateDiagram` / `stateDiagram-v2`.
//!
//! States become nodes, transitions edges, composite states subgraphs
//! (drawn as frames). `[*]` is a start (`●`) or end (`◉`) node, one of each
//! per composite state. Notes are listed under the diagram.

use super::graph::{Dir, Graph, LineStyle, Marker, Shape};
use super::text::decode_label;

pub fn parse(body: &[String]) -> Graph {
    let mut g = Graph::new(Dir::Down);
    let mut stack: Vec<usize> = Vec::new();
    let mut note: Option<(String, Vec<String>)> = None;
    for raw in body {
        let line = raw.trim();
        if let Some((target, lines)) = note.as_mut() {
            if line == "end note" {
                let text = lines.join(" ");
                g.notes.push(format!("note on {target}: {text}"));
                note = None;
            } else if !line.is_empty() {
                lines.push(decode_label(line));
            }
            continue;
        }
        if line.is_empty() || line.starts_with("%%") || line == "--" {
            continue;
        }
        let cluster = stack.last().copied();
        if line == "}" {
            stack.pop();
            continue;
        }
        if let Some(rest) = word(line, "direction") {
            if stack.is_empty() {
                if let Some(d) = Dir::parse(rest) {
                    g.dir = d;
                }
            }
            continue;
        }
        if let Some(rest) = word(line, "note") {
            // `note right of X : text` or a block up to `end note`.
            let (head, text) = match rest.split_once(':') {
                Some((h, t)) => (h, Some(t)),
                None => (rest, None),
            };
            let target = head
                .trim()
                .trim_start_matches("right of")
                .trim_start_matches("left of")
                .trim()
                .to_string();
            let target = g
                .find(&target)
                .map_or(target.clone(), |i| g.nodes[i].label.replace('\n', " "));
            match text {
                Some(t) => g
                    .notes
                    .push(format!("note on {target}: {}", decode_label(t))),
                None => note = Some((target, Vec::new())),
            }
            continue;
        }
        if [
            "classDef", "class", "style", "hide", "scale", "accTitle", "accDescr", "title",
        ]
        .iter()
        .any(|k| word(line, k).is_some() || line.starts_with(&format!("{k}:")))
        {
            continue;
        }
        if let Some(rest) = word(line, "state") {
            state_statement(&mut g, &mut stack, cluster, rest);
            continue;
        }
        if let Some((left, right)) = line.split_once("-->") {
            let (right, label) = match right.split_once(':') {
                Some((r, l)) => (r, decode_label(l)),
                None => (right, String::new()),
            };
            let a = endpoint(&mut g, cluster, left, true);
            let b = endpoint(&mut g, cluster, right, false);
            if let (Some(a), Some(b)) = (a, b) {
                g.add_edge(a, b, &label, LineStyle::Solid, Marker::None, Marker::Arrow);
            }
            continue;
        }
        if let Some((id, desc)) = line.split_once(':') {
            let id = strip_class(id);
            if !id.is_empty() && !id.contains(char::is_whitespace) {
                describe(&mut g, cluster, id, &decode_label(desc));
            }
            continue;
        }
        let id = strip_class(line);
        if !id.is_empty() && !id.contains(char::is_whitespace) {
            g.touch(id, cluster);
        }
    }
    g.redirect_cluster_edges();
    g
}

fn word<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

fn strip_class(s: &str) -> &str {
    let s = s.trim();
    s.split_once(":::").map_or(s, |(a, _)| a).trim()
}

/// A transition end: a state id, or `[*]` (a start node when it is the
/// source, an end node when it is the target; one of each per scope).
fn endpoint(g: &mut Graph, cluster: Option<usize>, s: &str, source: bool) -> Option<usize> {
    let id = strip_class(s);
    if id.is_empty() {
        return None;
    }
    if id == "[*]" {
        let scope = cluster.map_or(String::from("root"), |c| c.to_string());
        let (key, shape) = if source {
            (format!("[*]start:{scope}"), Shape::Start)
        } else {
            (format!("[*]end:{scope}"), Shape::End)
        };
        return Some(g.declare(&key, "", shape, cluster));
    }
    Some(g.touch(id, cluster))
}

/// `id : description` — the description is what the state box shows.
fn describe(g: &mut Graph, cluster: Option<usize>, id: &str, desc: &str) {
    let i = g.touch(id, cluster);
    let node = &mut g.nodes[i];
    if node.label == node.id {
        node.label = desc.to_string();
    } else {
        node.label = format!("{}\n{desc}", node.label);
    }
    node.declared = true;
}

/// The rest of a `state …` line.
fn state_statement(g: &mut Graph, stack: &mut Vec<usize>, cluster: Option<usize>, rest: &str) {
    let opens = rest.ends_with('{');
    let rest = rest.trim_end_matches('{').trim();
    // `"Description" as id` or `id as "Description"`
    let (id, desc) = if let Some((a, b)) = rest.split_once(" as ") {
        let (a, b) = (a.trim(), b.trim());
        if a.starts_with('"') {
            (strip_class(b).to_string(), Some(decode_label(a)))
        } else {
            (strip_class(a).to_string(), Some(decode_label(b)))
        }
    } else {
        let (id, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let tail = tail.trim();
        let id = strip_class(id).to_string();
        match tail {
            "<<choice>>" => {
                g.declare(&id, "", Shape::Choice, cluster);
                return;
            }
            "<<fork>>" | "<<join>>" => {
                g.declare(&id, "", Shape::Bar, cluster);
                return;
            }
            _ => {}
        }
        let desc = tail.strip_prefix(':').map(decode_label);
        (id, desc)
    };
    if id.is_empty() {
        return;
    }
    if opens {
        // A composite state: a frame titled with its description.
        let title = match (&desc, g.find(&id)) {
            (Some(d), _) => d.clone(),
            (None, Some(i)) => g.nodes[i].label.clone(),
            (None, None) => id.clone(),
        };
        if let Some(i) = g.find(&id) {
            g.nodes[i].declared = false;
        }
        let c = g.add_cluster(&id, &title, cluster);
        stack.push(c);
        return;
    }
    let i = g.touch(&id, cluster);
    if let Some(d) = desc {
        g.nodes[i].label = d;
        g.nodes[i].declared = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> Graph {
        let body: Vec<String> = src.lines().map(str::to_string).collect();
        parse(&body)
    }

    #[test]
    fn transitions_start_end_and_labels() {
        let g = parse_src("[*] --> Still\nStill --> Moving : push\nMoving --> [*]");
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.nodes[0].shape, Shape::Start);
        assert_eq!(g.nodes[3].shape, Shape::End);
        assert_eq!(g.edges[1].label, "push");
    }

    #[test]
    fn aliases_descriptions_and_pseudo_states() {
        let g = parse_src(
            "state \"A long name\" as s1\ns2 : Described\nstate fork_state <<fork>>\nstate c <<choice>>\nstate j <<join>>\nnote right of s1 : a note\nnote left of s2\n  two\n  lines\nend note",
        );
        assert_eq!(g.nodes[g.find("s1").unwrap()].label, "A long name");
        assert_eq!(g.nodes[g.find("s2").unwrap()].label, "Described");
        assert_eq!(g.nodes[g.find("fork_state").unwrap()].shape, Shape::Bar);
        assert_eq!(g.nodes[g.find("c").unwrap()].shape, Shape::Choice);
        assert_eq!(
            g.notes,
            vec![
                "note on A long name: a note",
                "note on Described: two lines"
            ]
        );
    }

    #[test]
    fn composite_states_are_subgraphs_with_their_own_start() {
        let g = parse_src(
            "[*] --> First\nstate First {\n  [*] --> second\n  second --> [*]\n}\nFirst --> Last",
        );
        assert_eq!(g.clusters.len(), 1);
        assert_eq!(g.clusters[0].title, "First");
        assert!(
            g.find("First").is_none(),
            "the composite is a frame, not a node"
        );
        // [*] --> First lands on the inner start; First --> Last leaves from the inner end.
        let label = |i: usize| (g.nodes[i].id.clone(), g.nodes[i].shape);
        let e: Vec<_> = g
            .edges
            .iter()
            .map(|e| (label(e.from), label(e.to)))
            .collect();
        assert_eq!(e[0].1 .1, Shape::Start);
        assert_eq!(e.last().unwrap().0 .1, Shape::End);
        assert_eq!(e.last().unwrap().1 .0, "Last");
    }
}
