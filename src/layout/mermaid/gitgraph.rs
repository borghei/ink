//! Git graphs: commits top to bottom, one lane per branch, the way
//! `git log --graph` draws them, whatever direction the source asks for.
//!
//! ```text
//! ● main
//! ●
//! ├─┐ develop
//! │ ● add parser
//! │ ●
//! ●─┘ merge develop
//! ● [v1.0]
//! ```
//!
//! A lane is freed when its branch has nothing more to do, and reused by the
//! next new branch. When the lanes don't leave room for labels, the graph is
//! listed event by event instead.

use super::canvas::{Canvas, Class, Row, SERIES};
use super::graph::LineStyle;
use super::text;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Normal,
    Highlight,
    Reverse,
}

#[derive(Debug, PartialEq)]
enum Event {
    /// A commit on `branch`, labelled with its message or id.
    Commit {
        branch: usize,
        label: String,
        tag: Option<String>,
        kind: Kind,
    },
    /// `branch` starts from `from`.
    Fork { branch: usize, from: usize },
    /// `from` is merged into `into`, which gets a merge commit.
    Merge {
        from: usize,
        into: usize,
        tag: Option<String>,
        kind: Kind,
    },
}

#[derive(Debug, Default)]
struct Graph {
    branches: Vec<String>,
    events: Vec<Event>,
}

/// `key: value` options after a command: `id: "a b" tag: v1 type: HIGHLIGHT`.
/// A value is quoted (`"…"`) or runs to the next whitespace.
fn options(rest: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut s = rest.trim_start();
    while !s.is_empty() {
        let Some(colon) = s.find(':') else { break };
        let key = s[..colon].trim().to_ascii_lowercase();
        let after = s[colon + 1..].trim_start();
        let (value, next) = match after.strip_prefix('"') {
            Some(q) => match q.find('"') {
                Some(end) => (&q[..end], &q[end + 1..]),
                None => (q, ""),
            },
            None => {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        // A key may only be one word; anything else before the colon is
        // dropped (`commit junk id: x`).
        let key = key
            .rsplit(char::is_whitespace)
            .next()
            .unwrap_or("")
            .to_string();
        if !key.is_empty() {
            out.insert(key, value.to_string());
        }
        s = next.trim_start();
    }
    out
}

fn kind_of(opts: &HashMap<String, String>) -> Kind {
    match opts.get("type").map(|t| t.to_ascii_uppercase()) {
        Some(t) if t == "HIGHLIGHT" => Kind::Highlight,
        Some(t) if t == "REVERSE" => Kind::Reverse,
        _ => Kind::Normal,
    }
}

fn tag_of(opts: &HashMap<String, String>) -> Option<String> {
    opts.get("tag")
        .map(|t| text::decode_label(t))
        .filter(|t| !t.is_empty())
}

/// A branch name as written: bare or quoted.
fn name_of(rest: &str) -> String {
    let rest = rest.trim();
    let name = match rest.strip_prefix('"') {
        Some(q) => q.split('"').next().unwrap_or(""),
        None => rest.split_whitespace().next().unwrap_or(""),
    };
    text::decode_label(name)
}

/// The command word of a line and the rest, the word lower-cased.
fn command(line: &str) -> (String, &str) {
    let end = line.find(char::is_whitespace).unwrap_or(line.len());
    (line[..end].to_ascii_lowercase(), &line[end..])
}

/// Parse a gitGraph body. Unknown branches in `checkout` and `merge`, a
/// second `branch` of an existing name and a branch merged into itself are
/// mistakes mermaid rejects; here they are skipped.
fn parse(body: &[String]) -> Graph {
    let mut g = Graph {
        branches: vec!["main".to_string()],
        events: Vec::new(),
    };
    let mut current = 0;
    let index = |g: &Graph, name: &str| g.branches.iter().position(|b| b == name);
    for raw in body {
        let line = raw.trim();
        let line = line.find("%%").map_or(line, |i| &line[..i]).trim();
        if line.is_empty() {
            continue;
        }
        let (cmd, rest) = command(line);
        match cmd.as_str() {
            "commit" => {
                let o = options(rest);
                let label = o
                    .get("msg")
                    .or_else(|| o.get("id"))
                    .map(|l| text::decode_label(l))
                    .unwrap_or_default();
                g.events.push(Event::Commit {
                    branch: current,
                    label,
                    tag: tag_of(&o),
                    kind: kind_of(&o),
                });
            }
            "branch" => {
                let name = name_of(rest);
                if name.is_empty() || index(&g, &name).is_some() {
                    continue;
                }
                g.branches.push(name);
                let branch = g.branches.len() - 1;
                g.events.push(Event::Fork {
                    branch,
                    from: current,
                });
                current = branch;
            }
            "checkout" | "switch" => {
                if let Some(b) = index(&g, &name_of(rest)) {
                    current = b;
                }
            }
            "merge" => {
                let Some(from) = index(&g, &name_of(rest)) else {
                    continue;
                };
                if from == current {
                    continue;
                }
                // The options follow the branch name.
                let after_name = rest.trim_start();
                let after_name = match after_name.strip_prefix('"') {
                    Some(q) => q.split_once('"').map_or("", |(_, r)| r),
                    None => after_name
                        .split_once(char::is_whitespace)
                        .map_or("", |(_, r)| r),
                };
                let o = options(after_name);
                g.events.push(Event::Merge {
                    from,
                    into: current,
                    tag: tag_of(&o),
                    kind: kind_of(&o),
                });
            }
            "cherry-pick" => {
                let o = options(rest);
                let id = o
                    .get("id")
                    .map(|i| text::decode_label(i))
                    .unwrap_or_default();
                g.events.push(Event::Commit {
                    branch: current,
                    label: format!("cherry-pick {id}").trim_end().to_string(),
                    tag: tag_of(&o),
                    kind: Kind::Normal,
                });
            }
            _ => {}
        }
    }
    g
}

impl Event {
    /// The branches a row involves: they must have a lane on that row.
    fn branches(&self) -> [Option<usize>; 2] {
        match *self {
            Event::Commit { branch, .. } => [Some(branch), None],
            Event::Fork { branch, from } => [Some(branch), Some(from)],
            Event::Merge { from, into, .. } => [Some(from), Some(into)],
        }
    }
}

/// Each branch's lane, first row and last row. A branch holds its lane from
/// its fork (row 0 for main) to the last row that involves it; a new branch
/// takes the lowest lane that is free by then. `None` for a branch that
/// never appears.
fn lanes(g: &Graph) -> Vec<Option<(usize, usize, usize)>> {
    let n = g.branches.len();
    let mut first = vec![None; n];
    let mut last = vec![0; n];
    for (row, ev) in g.events.iter().enumerate() {
        for b in ev.branches().into_iter().flatten() {
            first[b].get_or_insert(row);
            last[b] = row;
        }
    }
    // Main is there from the top, whatever its first event.
    if first[0].is_some() {
        first[0] = Some(0);
    }
    // Lanes in order of first appearance; `busy[l]` is the last row lane
    // `l` is held to.
    let mut order: Vec<usize> = (0..n).filter(|&b| first[b].is_some()).collect();
    order.sort_by_key(|&b| (first[b], b));
    let mut busy: Vec<usize> = Vec::new();
    let mut out = vec![None; n];
    for b in order {
        let start = first[b].unwrap_or(0);
        let lane = match busy.iter().position(|&end| end < start) {
            Some(l) => l,
            None => {
                busy.push(0);
                busy.len() - 1
            }
        };
        busy[lane] = last[b];
        out[b] = Some((lane, start, last[b]));
    }
    out
}

/// Columns between lanes.
const LANE: usize = 2;

/// Fewest columns left for labels beside the lanes before the graph is
/// listed instead.
const MIN_LABEL: usize = 10;

fn lane_class(lane: usize) -> Class {
    Class::Series((lane % usize::from(SERIES)) as u8)
}

fn label_class(lane: usize) -> Class {
    Class::SeriesLabel((lane % usize::from(SERIES)) as u8)
}

fn with_tag(label: &str, tag: &Option<String>) -> String {
    match tag {
        Some(t) if label.is_empty() => format!("[{t}]"),
        Some(t) => format!("{label} [{t}]"),
        None => label.to_string(),
    }
}

/// The graph's rows at most `inner` columns wide. `title` is a frontmatter
/// title.
pub fn render(title: Option<String>, body: &[String], inner: usize) -> (String, Vec<Row>) {
    let g = parse(body);
    let title = title.unwrap_or_else(|| "git graph".to_string());
    if g.events.is_empty() {
        return (
            title,
            vec![vec![("(no commits)".to_string(), Class::Label)]],
        );
    }
    let inner = inner.max(8);
    let lanes = lanes(&g);
    let width = lanes
        .iter()
        .flatten()
        .map(|&(l, _, _)| l + 1)
        .max()
        .unwrap_or(1);
    let lanes_w = (width - 1) * LANE + 1;
    if lanes_w + 1 + MIN_LABEL > inner {
        return (title, listed(&g, inner));
    }

    let rows = g.events.len();
    let mut cv = Canvas::new(rows, inner);
    let col = |b: usize| lanes[b].map_or(0, |(l, _, _)| l * LANE);
    // Each branch's lane, top to bottom.
    for &(l, start, end) in lanes.iter().flatten() {
        cv.line(
            start,
            l * LANE,
            end,
            l * LANE,
            LineStyle::Solid,
            lane_class(l),
        );
    }
    // Each row's label starts just after the rightmost lane open on that
    // row, as `git log --graph` does, not after the widest row's lanes.
    let mut open = vec![0usize; rows];
    for &(l, start, end) in lanes.iter().flatten() {
        for o in &mut open[start..=end] {
            *o = (*o).max(l * LANE + 1);
        }
    }
    let mut main_named = false;
    for (r, ev) in g.events.iter().enumerate() {
        let label_at = open[r] + 1;
        let (label, class) = match ev {
            Event::Commit {
                branch,
                label,
                tag,
                kind,
            } => {
                let l = lanes[*branch].map_or(0, |(l, _, _)| l);
                let dot = match kind {
                    Kind::Normal => "●",
                    Kind::Highlight => "◆",
                    Kind::Reverse => "○",
                };
                cv.glyph(r, col(*branch), dot, lane_class(l));
                // The main branch has no fork row to carry its name, so its
                // first commit carries it, before any label of its own.
                let mut text = with_tag(label, tag);
                let mut class = Class::Plain;
                if *branch == 0 && !main_named {
                    main_named = true;
                    let name = &g.branches[0];
                    text = if text.is_empty() {
                        name.clone()
                    } else {
                        format!("{name} · {text}")
                    };
                    class = label_class(l);
                }
                (text, class)
            }
            Event::Fork { branch, from } => {
                let l = lanes[*branch].map_or(0, |(l, _, _)| l);
                cv.line(
                    r,
                    col(*from),
                    r,
                    col(*branch),
                    LineStyle::Solid,
                    lane_class(l),
                );
                (g.branches[*branch].clone(), label_class(l))
            }
            Event::Merge {
                from,
                into,
                tag,
                kind,
            } => {
                let l = lanes[*into].map_or(0, |(l, _, _)| l);
                let lf = lanes[*from].map_or(0, |(l, _, _)| l);
                cv.line(
                    r,
                    col(*into),
                    r,
                    col(*from),
                    LineStyle::Solid,
                    lane_class(lf),
                );
                let dot = if *kind == Kind::Highlight {
                    "◆"
                } else {
                    "●"
                };
                cv.glyph(r, col(*into), dot, lane_class(l));
                (
                    with_tag(&format!("merge {}", g.branches[*from]), tag),
                    Class::Plain,
                )
            }
        };
        if !label.is_empty() {
            let one_line = label.replace('\n', " ");
            cv.text(
                r,
                label_at,
                &text::truncate(&one_line, inner.saturating_sub(label_at)),
                inner,
                class,
            );
        }
    }
    let rows = cv
        .render()
        .into_iter()
        .map(|mut r| {
            // Drop the blank tail the canvas keeps so rows aren't padded
            // out to the full width.
            while r.last().is_some_and(|(t, _)| t.trim().is_empty()) {
                r.pop();
            }
            if let Some((t, _)) = r.last_mut() {
                let trimmed = t.trim_end().len();
                t.truncate(trimmed);
            }
            r
        })
        .collect();
    (title, rows)
}

/// The graph as a list, one event per line, for when the lanes don't fit:
/// `develop  add parser`, `main  merge develop`.
fn listed(g: &Graph, inner: usize) -> Vec<Row> {
    let name_w = g
        .branches
        .iter()
        .map(|b| text::width(b))
        .max()
        .unwrap_or(0)
        .min(inner / 3);
    let mut rows = Vec::new();
    for ev in &g.events {
        let (branch, what) = match ev {
            Event::Commit {
                branch, label, tag, ..
            } => {
                let label = if label.is_empty() { "commit" } else { label };
                (*branch, with_tag(label, tag))
            }
            Event::Fork { branch, from } => (*from, format!("branch {}", g.branches[*branch])),
            Event::Merge {
                from, into, tag, ..
            } => (
                *into,
                with_tag(&format!("merge {}", g.branches[*from]), tag),
            ),
        };
        let name = text::truncate(&g.branches[branch], name_w);
        let pad = name_w.saturating_sub(text::width(&name));
        let room = inner.saturating_sub(name_w + 2).max(1);
        for (k, l) in text::wrap(&what.replace('\n', " "), room)
            .into_iter()
            .enumerate()
        {
            let lead = if k == 0 {
                format!("{name}{}  ", " ".repeat(pad))
            } else {
                " ".repeat(name_w + 2)
            };
            rows.push(vec![(lead, label_class(branch)), (l, Class::Plain)]);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(src: &str) -> Vec<String> {
        src.lines().map(str::to_string).collect()
    }

    fn plain(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| {
                r.iter()
                    .map(|(t, _)| t.as_str())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    const BASIC: &str = "commit\ncommit\nbranch develop\ncheckout develop\ncommit msg: \"add parser\"\ncommit\ncheckout main\nmerge develop\ncommit tag: \"v1.0\"\n";

    #[test]
    fn options_take_quoted_and_bare_values() {
        let o = options(r#" id: "a b" tag:v1 type: HIGHLIGHT msg: "fix: the bug""#);
        assert_eq!(o["id"], "a b");
        assert_eq!(o["tag"], "v1");
        assert_eq!(o["type"], "HIGHLIGHT");
        assert_eq!(o["msg"], "fix: the bug");
    }

    #[test]
    fn a_branch_forks_and_merges_back() {
        let (title, rows) = render(None, &lines(BASIC), 40);
        assert_eq!(title, "git graph");
        assert_eq!(
            plain(&rows),
            "● main
●
├─┐ develop
│ ● add parser
│ ●
●─┘ merge develop
● [v1.0]"
        );
    }

    #[test]
    fn a_finished_branch_frees_its_lane_for_the_next() {
        let src = "commit\nbranch a\ncommit\ncheckout main\nmerge a\nbranch b\ncommit\ncheckout main\nmerge b\n";
        let (_, rows) = render(None, &lines(src), 40);
        assert_eq!(
            plain(&rows),
            "● main
├─┐ a
│ ●
●─┘ merge a
├─┐ b
│ ●
●─┘ merge b"
        );
    }

    #[test]
    fn branches_side_by_side_cross_the_lanes_between() {
        let src = "commit\nbranch a\ncommit\ncheckout main\nbranch b\ncommit\ncheckout a\ncommit\ncheckout main\nmerge a\nmerge b\n";
        let (_, rows) = render(None, &lines(src), 40);
        let out = plain(&rows);
        assert_eq!(
            out,
            "● main
├─┐ a
│ ●
├─┼─┐ b
│ │ ●
│ ● │
●─┘ │ merge a
●───┘ merge b"
        );
    }

    #[test]
    fn commit_types_tags_and_cherry_picks() {
        let src = "commit id: \"A\" type: HIGHLIGHT\ncommit type: REVERSE tag: \"t\"\nbranch x\ncommit id: \"B\"\ncheckout main\ncherry-pick id: \"B\"\n";
        let (_, rows) = render(None, &lines(src), 40);
        let out = plain(&rows);
        assert!(out.starts_with("◆ main · A\n○ [t]"), "{out}");
        assert!(out.ends_with("● cherry-pick B"), "{out}");
    }

    #[test]
    fn mistakes_are_skipped_without_panicking() {
        let src = "checkout nowhere\nmerge nowhere\nmerge main\nbranch main\nbranch\ncommit\nbranch a\nbranch a\ncommit\n";
        let (_, rows) = render(None, &lines(src), 40);
        // Main has nothing after the fork, so its lane ends there.
        assert_eq!(plain(&rows), "● main\n└─┐ a\n  ●");
    }

    #[test]
    fn too_many_lanes_for_the_width_are_listed() {
        let mut src = String::from("commit\n");
        for i in 0..8 {
            src.push_str(&format!("checkout main\nbranch b{i}\ncommit\n"));
        }
        for i in 0..8 {
            src.push_str(&format!("checkout main\nmerge b{i}\n"));
        }
        let (_, rows) = render(None, &lines(&src), 24);
        let out = plain(&rows);
        assert!(
            out.starts_with("main  commit\nmain  branch b0\nb0    commit"),
            "{out}"
        );
        assert!(out.contains("main  merge b7"), "{out}");
    }

    #[test]
    fn an_empty_graph_says_so() {
        let (_, rows) = render(Some("History".into()), &lines("%% nothing\n"), 40);
        assert_eq!(plain(&rows), "(no commits)");
    }

    #[test]
    fn rows_stay_within_the_width() {
        let src = "commit msg: \"a very long commit message that should be cut at the frame edge\"\nbranch \"feature/日本語ブランチ\"\ncommit tag: \"v1.0.0-rc1\"\ncheckout main\nmerge \"feature/日本語ブランチ\" tag: \"release\"\n";
        for inner in [8, 12, 16, 20, 30, 60] {
            let (_, rows) = render(None, &lines(src), inner);
            for r in &rows {
                let w: usize = r.iter().map(|(t, _)| text::width(t)).sum();
                assert!(w <= inner.max(8), "{w} > {inner}: {r:?}");
            }
        }
    }

    #[test]
    fn hostile_input_stays_quick() {
        let mut src = String::new();
        for i in 0..5_000 {
            src.push_str(&format!("branch b{i}\ncommit\ncheckout main\ncommit\n"));
        }
        for i in 0..5_000 {
            src.push_str(&format!("merge b{i}\n"));
        }
        let start = std::time::Instant::now();
        render(None, &lines(&src), 80);
        let linear: String = (0..20_000).map(|_| "commit\n").collect();
        render(None, &lines(&linear), 80);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
    }
}
