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
    title: Option<String>,
    branches: Vec<String>,
    events: Vec<Event>,
}

/// A piece of a command line: a quoted string, a bare word, or `key:`.
#[derive(Debug, PartialEq)]
enum Tok {
    Str(String),
    Word(String),
    Key(String),
}

/// Split the text after a command into tokens. Strings are `"…"` or `'…'`
/// with `\` escapes, as in mermaid; `key:value` and `key: value` both give
/// a key and its value.
fn tokens(rest: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = rest.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == '"' || c == '\'' {
            chars.next();
            let mut s = String::new();
            while let Some(ch) = chars.next() {
                match ch {
                    '\\' => {
                        if let Some(esc) = chars.next() {
                            s.push(esc);
                        }
                    }
                    ch if ch == c => break,
                    ch => s.push(ch),
                }
            }
            out.push(Tok::Str(s));
            continue;
        }
        let mut word = String::new();
        while let Some(&ch) = chars.peek() {
            if ch.is_whitespace() || ch == '"' || ch == '\'' {
                break;
            }
            word.push(ch);
            chars.next();
            if ch == ':' {
                break;
            }
        }
        match word.strip_suffix(':') {
            Some(key) if !key.is_empty() => out.push(Tok::Key(key.to_ascii_lowercase())),
            _ => out.push(Tok::Word(word)),
        }
    }
    out
}

/// The options of a command: each `key: value`, every `tag:` kept in order.
/// A leading string with no key is the commit message (`commit "fix"`).
#[derive(Debug, Default)]
struct Options {
    msg: Option<String>,
    id: Option<String>,
    tags: Vec<String>,
    kind: Option<String>,
    /// The first bare word or string: a branch name.
    name: Option<String>,
}

fn options(toks: &[Tok]) -> Options {
    let mut o = Options::default();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Key(k) => {
                let value = match toks.get(i + 1) {
                    Some(Tok::Str(v) | Tok::Word(v)) => {
                        i += 1;
                        v.clone()
                    }
                    _ => String::new(),
                };
                match k.as_str() {
                    "msg" => o.msg = Some(value),
                    "id" => o.id = Some(value),
                    "tag" => o.tags.push(value),
                    "type" => o.kind = Some(value),
                    _ => {}
                }
            }
            Tok::Str(v) | Tok::Word(v) => {
                if o.name.is_none() {
                    o.name = Some(v.clone());
                }
                if o.msg.is_none() && matches!(toks[i], Tok::Str(_)) {
                    o.msg = Some(v.clone());
                }
            }
        }
        i += 1;
    }
    o
}

fn kind_of(o: &Options) -> Kind {
    match o.kind.as_deref().map(str::to_ascii_uppercase).as_deref() {
        Some("HIGHLIGHT") => Kind::Highlight,
        Some("REVERSE") => Kind::Reverse,
        _ => Kind::Normal,
    }
}

fn tag_of(o: &Options) -> Option<String> {
    let tags: Vec<String> = o
        .tags
        .iter()
        .map(|t| text::decode_label(t))
        .filter(|t| !t.is_empty())
        .collect();
    (!tags.is_empty()).then(|| tags.join(", "))
}

/// `line` without a `%%` comment, ignoring `%%` inside a quoted string.
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut prev = '\0';
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q && prev != '\\' => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '%' && line[i..].starts_with("%%") => return line[..i].trim(),
            None => {}
        }
        prev = c;
    }
    line.trim()
}

/// The command word of a line and the rest, the word lower-cased.
fn command(line: &str) -> (String, &str) {
    let end = line.find(char::is_whitespace).unwrap_or(line.len());
    (line[..end].to_ascii_lowercase(), &line[end..])
}

/// The default name of the main branch. Mermaid takes another from the
/// `mainBranchName` setting, which is not passed down here; instead, when
/// the source never mentions `main`, the first `checkout` or `merge` of a
/// branch it never creates names it.
const MAIN: &str = "main";

/// Parse a gitGraph body. Unknown branches in `checkout` and `merge`, a
/// second `branch` of an existing name and a branch merged into itself are
/// mistakes mermaid rejects; here they are skipped.
fn parse(body: &[String]) -> Graph {
    let mut g = Graph {
        title: None,
        branches: vec![MAIN.to_string()],
        events: Vec::new(),
    };
    let mut index: HashMap<String, usize> = HashMap::from([(MAIN.to_string(), 0)]);
    // Every branch the source creates, so a checkout of any other name can
    // only mean the main branch under the name its settings gave it.
    let created: std::collections::HashSet<String> = body
        .iter()
        .filter_map(|raw| {
            let (cmd, rest) = command(strip_comment(raw.trim()));
            (cmd == "branch")
                .then(|| options(&tokens(rest)).name)
                .flatten()
                .map(|n| text::decode_label(&n))
        })
        .collect();
    // A source that names `main` itself uses the default name; only one
    // that never does can have renamed it.
    let mentions_main = body.iter().any(|raw| {
        let (cmd, rest) = command(strip_comment(raw.trim()));
        matches!(cmd.as_str(), "checkout" | "switch" | "merge")
            && options(&tokens(rest)).name.as_deref() == Some(MAIN)
    });
    let mut main_renamed = mentions_main;
    let mut current = 0;
    let mut in_description = false;
    for raw in body {
        let line = strip_comment(raw.trim());
        if in_description {
            in_description = !line.contains('}');
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let (cmd, rest) = command(line);
        let o = options(&tokens(rest));
        let mut lookup = |g: &mut Graph, name: &str| -> Option<usize> {
            if let Some(&b) = index.get(name) {
                return Some(b);
            }
            if !main_renamed && !created.contains(name) && !name.is_empty() {
                main_renamed = true;
                index.remove(&g.branches[0]);
                g.branches[0] = name.to_string();
                index.insert(name.to_string(), 0);
                return Some(0);
            }
            None
        };
        match cmd.as_str() {
            "title" => {
                let t = text::decode_label(rest);
                if !t.is_empty() {
                    g.title = Some(t);
                }
            }
            "acctitle:" | "accdescr:" | "acctitle" | "accdescr" => {
                in_description = line.contains('{') && !line.contains('}');
            }
            "commit" => {
                let label = o
                    .msg
                    .as_ref()
                    .or(o.id.as_ref())
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
                let name = o
                    .name
                    .as_deref()
                    .map(text::decode_label)
                    .unwrap_or_default();
                if name.is_empty() || index.contains_key(&name) {
                    continue;
                }
                g.branches.push(name.clone());
                let branch = g.branches.len() - 1;
                index.insert(name, branch);
                g.events.push(Event::Fork {
                    branch,
                    from: current,
                });
                current = branch;
            }
            "checkout" | "switch" => {
                let name = o
                    .name
                    .as_deref()
                    .map(text::decode_label)
                    .unwrap_or_default();
                if let Some(b) = lookup(&mut g, &name) {
                    current = b;
                }
            }
            "merge" => {
                let name = o
                    .name
                    .as_deref()
                    .map(text::decode_label)
                    .unwrap_or_default();
                let Some(from) = lookup(&mut g, &name) else {
                    continue;
                };
                if from == current {
                    continue;
                }
                g.events.push(Event::Merge {
                    from,
                    into: current,
                    tag: tag_of(&o),
                    kind: kind_of(&o),
                });
            }
            "cherry-pick" => {
                let id = o.id.as_deref().map(text::decode_label).unwrap_or_default();
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
    // Branches in order of first appearance. `held` has each taken lane
    // with the last row it is held to, soonest first; `free` the lanes
    // released by then, lowest first.
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let mut order: Vec<usize> = (0..n).filter(|&b| first[b].is_some()).collect();
    order.sort_by_key(|&b| (first[b], b));
    let mut held: BinaryHeap<Reverse<(usize, usize)>> = BinaryHeap::new();
    let mut free: BinaryHeap<Reverse<usize>> = BinaryHeap::new();
    let mut lanes_used = 0;
    let mut out = vec![None; n];
    for b in order {
        let start = first[b].unwrap_or(0);
        while held.peek().is_some_and(|Reverse((end, _))| *end < start) {
            if let Some(Reverse((_, lane))) = held.pop() {
                free.push(Reverse(lane));
            }
        }
        let lane = match free.pop() {
            Some(Reverse(l)) => l,
            None => {
                lanes_used += 1;
                lanes_used - 1
            }
        };
        held.push(Reverse((last[b], lane)));
        out[b] = Some((lane, start, last[b]));
    }
    out
}

/// Columns between lanes.
const LANE: usize = 2;

/// Fewest columns left for labels beside the lanes before the graph is
/// listed instead.
const MIN_LABEL: usize = 10;

/// A branch's colour, by branch rather than lane, so a branch that reuses a
/// lane doesn't look like the one before it.
fn branch_class(branch: usize) -> Class {
    Class::Series((branch % usize::from(SERIES)) as u8)
}

fn label_class(branch: usize) -> Class {
    Class::SeriesLabel((branch % usize::from(SERIES)) as u8)
}

/// The most cells the lane drawing may take before the graph is listed.
const MAX_CELLS: usize = 2_000_000;

/// The end of `s` in at most `max` columns, with `…` in front when cut:
/// `…ature/login`. Branch names differ at the end more often than the start.
fn truncate_start(s: &str, max: usize) -> String {
    if text::width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    use unicode_segmentation::UnicodeSegmentation;
    let mut tail: Vec<&str> = Vec::new();
    let mut w = 1;
    for g in s.graphemes(true).rev() {
        let gw = text::width(g);
        if w + gw > max {
            break;
        }
        w += gw;
        tail.push(g);
    }
    tail.reverse();
    format!("…{}", tail.concat())
}

fn dot(kind: Kind) -> &'static str {
    match kind {
        Kind::Normal => "●",
        Kind::Highlight => "◆",
        Kind::Reverse => "○",
    }
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
    let title = g
        .title
        .clone()
        .or(title)
        .unwrap_or_else(|| "git graph".to_string());
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
    let rows = g.events.len();
    if lanes_w + 1 + MIN_LABEL > inner || rows.saturating_mul(lanes_w) > MAX_CELLS {
        return (title, listed(&g, inner));
    }

    // The canvas holds only the lanes; labels are added after it.
    let mut cv = Canvas::new(rows, lanes_w);
    let col = |b: usize| lanes[b].map_or(0, |(l, _, _)| l * LANE);
    // Each branch's lane, top to bottom.
    for (b, lane) in lanes.iter().enumerate() {
        if let Some((l, start, end)) = *lane {
            cv.line(
                start,
                l * LANE,
                end,
                l * LANE,
                LineStyle::Solid,
                branch_class(b),
            );
        }
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
    let mut labels: Vec<(String, Class)> = Vec::with_capacity(rows);
    for (r, ev) in g.events.iter().enumerate() {
        let (label, class) = match ev {
            Event::Commit {
                branch,
                label,
                tag,
                kind,
            } => {
                cv.glyph(r, col(*branch), dot(*kind), branch_class(*branch));
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
                    class = label_class(0);
                }
                (text, class)
            }
            Event::Fork { branch, from } => {
                cv.line(
                    r,
                    col(*from),
                    r,
                    col(*branch),
                    LineStyle::Solid,
                    branch_class(*branch),
                );
                (g.branches[*branch].clone(), label_class(*branch))
            }
            Event::Merge {
                from,
                into,
                tag,
                kind,
            } => {
                cv.line(
                    r,
                    col(*into),
                    r,
                    col(*from),
                    LineStyle::Solid,
                    branch_class(*from),
                );
                cv.glyph(r, col(*into), dot(*kind), branch_class(*into));
                (
                    with_tag(&format!("merge {}", g.branches[*from]), tag),
                    Class::Plain,
                )
            }
        };
        labels.push((label, class));
    }
    let rows = cv
        .render()
        .into_iter()
        .zip(labels)
        .zip(&open)
        .map(|((mut r, (label, class)), &open)| {
            // Keep the lanes open on this row, then the label after them.
            let mut kept = Vec::new();
            let mut w = 0;
            for (t, c) in r.drain(..) {
                if w >= open {
                    break;
                }
                let take: String = {
                    use unicode_segmentation::UnicodeSegmentation;
                    let mut out = String::new();
                    for g in t.graphemes(true) {
                        if w + text::width(g) > open {
                            break;
                        }
                        w += text::width(g);
                        out.push_str(g);
                    }
                    out
                };
                kept.push((take, c));
            }
            if !label.is_empty() {
                let room = inner.saturating_sub(open + 1);
                let one_line = label.replace('\n', " ");
                kept.push((format!(" {}", text::truncate(&one_line, room)), class));
            }
            while kept.last().is_some_and(|(t, _)| t.trim().is_empty()) {
                kept.pop();
            }
            kept
        })
        .collect();
    (title, rows)
}

/// The graph as a list, one event per line, for when the lanes don't fit:
/// `develop  from main`, `develop  add parser`, `main  merge develop`.
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
            // Under the new branch's own name, so a long name sits in the
            // name column (its end kept) rather than wrapping in the text.
            Event::Fork { branch, from } => (*branch, format!("from {}", g.branches[*from])),
            Event::Merge {
                from, into, tag, ..
            } => (
                *into,
                with_tag(&format!("merge {}", g.branches[*from]), tag),
            ),
        };
        let name = truncate_start(&g.branches[branch], name_w);
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
        let o = options(&tokens(
            r#" id: "a b" tag:v1 type: HIGHLIGHT msg: "fix: the bug" tag: 'two words'"#,
        ));
        assert_eq!(o.id.as_deref(), Some("a b"));
        assert_eq!(o.tags, ["v1", "two words"]);
        assert_eq!(o.kind.as_deref(), Some("HIGHLIGHT"));
        assert_eq!(o.msg.as_deref(), Some("fix: the bug"));
    }

    #[test]
    fn strings_take_either_quote_and_backslash_escapes() {
        assert_eq!(
            tokens(r#" "say \"hi\" now" 'it\'s' bare"#),
            [
                Tok::Str("say \"hi\" now".into()),
                Tok::Str("it's".into()),
                Tok::Word("bare".into())
            ]
        );
        let o = options(&tokens(r#" "bare message""#));
        assert_eq!(o.msg.as_deref(), Some("bare message"));
    }

    #[test]
    fn a_comment_marker_inside_a_string_is_text() {
        assert_eq!(
            strip_comment(r#"commit msg: "50%% done" %% note"#),
            r#"commit msg: "50%% done""#
        );
        assert_eq!(strip_comment("commit %% note"), "commit");
    }

    #[test]
    fn quoted_names_tags_and_a_body_title() {
        let src = "title Real title\ncommit id: 'single quoted' tag: 'v 1' tag: \"b\"\nbranch 'feat x'\ncommit\ncheckout main\nmerge 'feat x'\n";
        let (title, rows) = render(None, &lines(src), 40);
        assert_eq!(title, "Real title");
        let out = plain(&rows);
        assert!(out.starts_with("● main · single quoted [v 1, b]"), "{out}");
        assert!(out.contains("feat x"), "{out}");
        assert!(out.ends_with("merge feat x"), "{out}");
    }

    #[test]
    fn a_main_branch_with_another_name_is_found_by_its_checkout() {
        let src = "commit id: \"one\"\nbranch dev\ncommit id: \"two\"\ncheckout trunk\ncommit id: \"three\"\nmerge dev\n";
        let (_, rows) = render(None, &lines(src), 40);
        assert_eq!(
            plain(&rows),
            "● trunk · one
├─┐ dev
│ ● two
● │ three
●─┘ merge dev"
        );
    }

    #[test]
    fn a_reused_lane_keeps_the_colour_of_its_own_branch() {
        // Main ends at the first fork, and b takes main's lane.
        let src = "commit\nbranch a\ncommit\nbranch b\ncommit\ncheckout a\ncommit\n";
        let (_, rows) = render(None, &lines(src), 40);
        let dot_class = |row: &Row| row.iter().find(|(t, _)| t.contains('●')).map(|(_, c)| *c);
        let classes: Vec<_> = rows.iter().filter_map(dot_class).collect();
        assert_eq!(classes[0], branch_class(0));
        assert!(classes.contains(&branch_class(2)), "{classes:?}");
    }

    #[test]
    fn listed_branch_names_keep_their_distinguishing_end() {
        assert_eq!(truncate_start("feature/login", 8), "…e/login");
        assert_eq!(truncate_start("short", 8), "short");
        let mut src = String::from("commit\n");
        let names = [
            "feature/login",
            "feature/signup",
            "feature/logout",
            "feature/admin",
            "feature/users",
            "feature/billing",
            "feature/search",
        ];
        for name in names {
            src.push_str(&format!(
                "checkout main\nbranch {name}\ncommit msg: \"work\"\n"
            ));
        }
        // Merged only at the end, so every branch keeps a lane open.
        for name in names {
            src.push_str(&format!("checkout main\nmerge {name}\n"));
        }
        let (_, rows) = render(None, &lines(&src), 22);
        let out = plain(&rows);
        assert!(out.contains("…/login  from main\n…/login  work"), "{out}");
        assert!(out.contains("…signup  work"), "{out}");
    }

    #[test]
    fn many_commits_stay_small() {
        let src: String = (0..100_000).map(|_| "commit\n").collect();
        let (_, rows) = render(None, &lines(&src), 300);
        assert_eq!(rows.len(), 100_000);
        assert!(rows
            .iter()
            .all(|r| r.iter().map(|(t, _)| text::width(t)).sum::<usize>() <= 300));
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
            out.starts_with("main  commit\nb0    from main\nb0    commit"),
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
        for i in 0..40_000 {
            src.push_str(&format!("branch b{i}\ncommit\ncheckout main\ncommit\n"));
        }
        for i in 0..40_000 {
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
