//! Rendering tests: exact output for small diagrams (so a layout change
//! shows up as a readable diff) and properties every diagram in the corpus
//! under `tests/fixtures/mermaid/` must keep.

use super::*;
use std::time::{Duration, Instant};

fn theme() -> Theme {
    crate::theme::resolve_theme("dark")
}

/// The rendered lines as plain text, trailing blanks trimmed.
fn render(src: &str, width: usize) -> String {
    render_opts(src, width, false)
}

fn render_opts(src: &str, width: usize, ascii: bool) -> String {
    let lines = render_mermaid_with(src, &theme(), width, 0, ascii);
    let mut out: Vec<String> = lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// Compare with an expected block written with a leading newline.
fn expect(src: &str, width: usize, expected: &str) {
    let got = render(src, width);
    let want = expected.strip_prefix('\n').unwrap_or(expected).trim_end();
    assert_eq!(got, want, "\n--- got ---\n{got}\n--- want ---\n{want}\n");
}

fn corpus() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mermaid");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixture dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mmd"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

fn parsed(src: &str) -> Option<Graph> {
    parse_graph(&split_source(&text::clean_source(src))).map(|(g, _)| g)
}

// ── Flowcharts: exact output ──

#[test]
fn two_nodes_top_down() {
    expect(
        "graph TD\nA-->B",
        40,
        r"
╭─ flowchart ──────╮
│      ┌───┐       │
│      │ A │       │
│      └─┬─┘       │
│        ▼         │
│      ┌───┐       │
│      │ B │       │
│      └───┘       │
╰──────────────────╯",
    );
}

#[test]
fn shared_nodes_are_drawn_once_and_fan_out_on_one_track() {
    expect(
        "flowchart TD\nA[Start] --> B{Check}\nB -->|ok| C[Done]\nB -->|retry| A",
        50,
        r"
╭─ flowchart ──────╮
│   ┌───────┐      │
│   │ Start │      │
│   └──┬────┘      │
│      │ ▲         │
│     ┌┘ └─┐       │
│     │  retry     │
│     └─┐ ┌┘       │
│       ▼ │        │
│    ╱────┴──╲     │
│    < Check >     │
│    ╲───┬───╱     │
│        │         │
│        ok        │
│        ▼         │
│     ┌──────┐     │
│     │ Done │     │
│     └──────┘     │
╰──────────────────╯",
    );
}

#[test]
fn bottom_up_fan_in_shares_a_track() {
    expect(
        "graph BT\nA --> B & C",
        40,
        r"
╭─ flowchart ──────╮
│   ┌───┐  ┌───┐   │
│   │ B │  │ C │   │
│   └───┘  └───┘   │
│     ▲      ▲     │
│     └───┬──┘     │
│         │        │
│       ┌─┴─┐      │
│       │ A │      │
│       └───┘      │
╰──────────────────╯",
    );
}

#[test]
fn subgraph_is_a_titled_frame() {
    expect(
        "flowchart TB\nsubgraph S [Inside]\nA --> B\nend\nC --> A",
        40,
        r"
╭─ flowchart ──────╮
│     ┌───┐        │
│     │ C │        │
│     └─┬─┘        │
│   ┌───┼──────┐   │
│   │   ▼      │   │
│   │ ┌───┐    │   │
│   │ │ A │    │   │
│   │ └─┬─┘    │   │
│   │   ▼      │   │
│   │ ┌───┐    │   │
│   │ │ B │    │   │
│   │ └───┘    │   │
│   └─ Inside ─┘   │
╰──────────────────╯",
    );
}

#[test]
fn left_to_right_with_labels_shapes_and_styles() {
    expect(
        "graph LR\nA[Start] --> B{Ok?}\nB -->|yes| C[Done]\nB -.-> D((Retry))",
        60,
        r"
╭─ flowchart ─────────────────────────╮
│                           ┌──────┐  │
│            ╱─────╲      ┌▶│ Done │  │
│ ┌───────┐  │     ├─ yes ┘ └──────┘  │
│ │ Start ├─▶< Ok? >                  │
│ └───────┘  │     ├┐       ╭───────╮ │
│            ╲─────╱└┄┄┄┄┄┄▶( Retry ) │
│                           ╰───────╯ │
╰─────────────────────────────────────╯",
    );
}

#[test]
fn left_to_right_too_wide_is_drawn_top_down() {
    expect(
        "graph LR\nA[A long first label] --> B[Another long label] --> C[Third label here]",
        30,
        r"
╭─ flowchart ────────────╮
│ ┌────────────────────┐ │
│ │ A long first label │ │
│ └─────────┬──────────┘ │
│           ▼            │
│ ┌────────────────────┐ │
│ │ Another long label │ │
│ └─────────┬──────────┘ │
│           ▼            │
│  ┌──────────────────┐  │
│  │ Third label here │  │
│  └──────────────────┘  │
╰────────────────────────╯",
    );
}

#[test]
fn self_loops_and_markers() {
    expect(
        "graph TD\nA -->|again| A\nA --o B\nB <--> C\nC --x D",
        40,
        r"
╭─ flowchart ──────╮
│  ┌───┐           │
│  │ A ├─┐ again   │
│  │   │◀┘         │
│  └─┬─┘           │
│    ○             │
│  ┌───┐           │
│  │ B │           │
│  └───┘           │
│    ▲             │
│    ▼             │
│  ┌───┐           │
│  │ C │           │
│  └─┬─┘           │
│    ×             │
│  ┌───┐           │
│  │ D │           │
│  └───┘           │
╰──────────────────╯",
    );
}

#[test]
fn too_wide_even_top_down_falls_back_to_a_list() {
    let src = format!(
        "graph TD\n{}",
        (0..12)
            .map(|i| format!("Hub --> N{i}[Node number {i}]"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let out = render(&src, 40);
    assert!(out.contains("│ Hub "), "{out}");
    assert!(out.contains("──▶ Node number 0"), "{out}");
    for line in out.lines() {
        assert!(text::width(line) <= 40, "{line}");
    }
}

#[test]
fn ascii_mode_is_seven_bit() {
    let out = render_opts(
        "graph LR\nA[Start] --> B{Ok?}\nB -->|yes| C[(Done)]\nB -.-> D((Retry))\nD ==> E[[Sub]]\nsubgraph S\nE\nend\nE --o A",
        60,
        true,
    );
    assert!(out.is_ascii(), "{out}");
    assert!(out.contains("Start"));
}

#[test]
fn control_bytes_in_labels_never_reach_the_output() {
    for src in [
        "graph TD\nA[\u{1b}]2;pwned\u{7}evil] --> B[\u{1b}[31mred]",
        "graph TD\nA -->|\u{1b}[2J| B",
        "graph TD\nsubgraph \u{9b}31m title\nA\nend",
    ] {
        let lines = render_mermaid_with(src, &theme(), 60, 0, false);
        for l in &lines {
            for s in &l.spans {
                assert!(
                    !s.text
                        .chars()
                        .any(|c| c.is_control() || ('\u{80}'..='\u{9f}').contains(&c)),
                    "{:?}",
                    s.text
                );
            }
        }
        let out = render(src, 60);
        assert!(
            out.contains("evil") || out.contains("[2J") || out.contains("title"),
            "{out}"
        );
    }
}

// ── State diagrams ──

#[test]
fn state_diagram_start_end_fork_and_back_edge() {
    expect(
        "stateDiagram-v2\n[*] --> Still\nStill --> Moving : push\nMoving --> Still\nMoving --> [*]\nstate fork <<fork>>\nStill --> fork",
        40,
        r"
╭─ state diagram ───╮
│        ●          │
│        │          │
│        ▼          │
│    ┌───────┐      │
│    │ Still │      │
│    └──┬────┘      │
│       │ ▲         │
│    ┌──┴─┼─────┐   │
│   push  │     │   │
│    │ ┌──┘     │   │
│    ▼ │        ▼   │
│ ┌────┴───┐  ━━━━━ │
│ │ Moving │        │
│ └───┬────┘        │
│     │             │
│     ▼             │
│     ◉             │
╰───────────────────╯",
    );
}

// ── Class and ER diagrams ──

#[test]
fn class_diagram_compartments_and_relationship_glyphs() {
    expect(
        "classDiagram\nclass Animal {\n  <<interface>>\n  +String name\n  +eat() void\n}\nAnimal <|-- Dog\nDog *-- Tail\nDog \"1\" --> \"*\" Bone : chews",
        50,
        r"
╭─ class diagram ───────────────────────────────╮
│              ┌───────────────┐                │
│              │ <<interface>> │                │
│              │    Animal     │                │
│              ├───────────────┤                │
│              │ +String name  │                │
│              ├───────────────┤                │
│              │ +eat() void   │                │
│              └───────────────┘                │
│                      △                        │
│                      │                        │
│                      │                        │
│                   ┌──┴──┐                     │
│                   │ Dog │                     │
│                   └───┬─┘                     │
│                     ◆ │                       │
│                  ┌──┘ └────┐                  │
│                  │         1                  │
│                  │       chews                │
│                  │         *                  │
│                  │         ▼                  │
│               ┌──┴───┐  ┌──────┐              │
│               │ Tail │  │ Bone │              │
│               └──────┘  └──────┘              │
│                                               │
│ △ inheritance · ◆ composition · ▶ association │
╰───────────────────────────────────────────────╯",
    );
}

#[test]
fn er_diagram_attributes_and_cardinalities() {
    expect(
        "erDiagram\nCUSTOMER ||--o{ ORDER : places\nCUSTOMER {\n  string name PK\n}",
        50,
        r"
╭─ ER diagram ───────╮
│ ┌────────────────┐ │
│ │    CUSTOMER    │ │
│ ├────────────────┤ │
│ │ string name PK │ │
│ └───────┬────────┘ │
│         │          │
│         1          │
│       places       │
│        0..*        │
│         │          │
│     ┌───┴───┐      │
│     │ ORDER │      │
│     └───────┘      │
╰────────────────────╯",
    );
}

// ── Gantt charts ──

#[test]
fn gantt_bars_on_a_scaled_axis() {
    expect(
        "gantt\ntitle Release\ndateFormat YYYY-MM-DD\nsection Build\nDesign :done, d1, 2024-03-04, 5d\nCode :active, c1, after d1, 10d\nReview :crit, after c1, 3d\nShip :milestone, after c1, 0d",
        50,
        r"
╭─ Release ──────────────────────────────────────╮
│           03-04         03-11         03-18    │
│           ├─────────────┼─────────────┼──────┤ │
│ Build                                          │
│   Design  ██████████                           │
│   Code              ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓       │
│   Review                                ░░░░░░ │
│   Ship                                  ◆      │
╰────────────────────────────────────────────────╯",
    );
}

// ── Everything else ──

#[test]
fn unsupported_types_show_their_source_in_a_titled_box() {
    expect(
        "gitGraph\n    commit\n    branch develop",
        40,
        r"
╭─ mermaid: gitGraph (not rendered) ─╮
│ gitGraph                           │
│     commit                         │
│     branch develop                 │
╰────────────────────────────────────╯",
    );
    let out = render("somethingNew\nfoo --> bar", 40);
    assert!(
        out.starts_with("╭─ mermaid: somethingNew (not render"),
        "{out}"
    );
}

#[test]
fn mindmaps_are_trees() {
    expect(
        "mindmap\n  root((Plan))\n    Goals\n      Ship\n    Risks",
        40,
        r"
╭─ mindmap ────────╮
│ Plan             │
│ ├─ Goals         │
│ │  └─ Ship       │
│ └─ Risks         │
╰──────────────────╯",
    );
}

#[test]
fn sequence_and_pie_wrap_when_narrow_and_keep_their_form_when_not() {
    let seq = "sequenceDiagram\n    Alice->>John: Hello John, how are you?";
    expect(
        seq,
        60,
        r"
╭─ sequence diagram ───────────────────────────────────╮
│  Alice ──▶ John: Hello John, how are you?
╰──────────────────────────────────────────────────────╯",
    );
    for line in render(seq, 24).lines() {
        assert!(text::width(line) <= 24, "{line}");
    }
    let pie = "pie title Pets\n    \"Dogs\" : 3\n    \"Cats\" : 1";
    expect(
        pie,
        60,
        r"
╭─ Pets ───────────────────────────────────────────────╮
│          Dogs ██████████████████████ 75.0%
│          Cats ███████ 25.0%
╰──────────────────────────────────────────────────────╯",
    );
}

#[test]
fn corpus_is_big_enough() {
    assert!(corpus().len() >= 20, "{} diagrams", corpus().len());
}

// ── Properties over the corpus ──

const WIDTHS: [usize; 5] = [24, 40, 60, 80, 120];

#[test]
fn no_line_is_wider_than_the_width() {
    for (name, src) in corpus() {
        for w in WIDTHS {
            for ascii in [false, true] {
                for line in render_mermaid_with(&src, &theme(), w, 2, ascii) {
                    let lw = line.width();
                    assert!(
                        lw <= w + 2,
                        "{name} at {w}: {lw} columns\n{}",
                        render(&src, w)
                    );
                }
            }
        }
    }
}

#[test]
fn output_is_deterministic() {
    for (name, src) in corpus() {
        for w in [40, 100] {
            assert_eq!(render(&src, w), render(&src, w), "{name}");
        }
    }
}

#[test]
fn ascii_output_is_pure_ascii() {
    for (name, src) in corpus() {
        if !src.is_ascii() {
            continue;
        }
        for w in [40, 100] {
            let out = render_opts(&src, w, true);
            assert!(out.is_ascii(), "{name}:\n{out}");
        }
    }
}

/// Every node label that is not part of some other text is drawn exactly
/// once when the diagram is laid out (not listed).
#[test]
fn every_node_is_drawn_exactly_once() {
    let word_count = |hay: &str, needle: &str| -> usize {
        let mut n = 0;
        let mut from = 0;
        while let Some(i) = hay[from..].find(needle) {
            let at = from + i;
            let before = hay[..at].chars().next_back();
            let after = hay[at + needle.len()..].chars().next();
            let edge = |c: Option<char>| !c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            if edge(before) && edge(after) {
                n += 1;
            }
            from = at + needle.len();
        }
        n
    };
    let mut checked = 0;
    for (name, src) in corpus() {
        let Some(g) = parsed(&src) else { continue };
        // Only the drawing: frames that could not be drawn are listed as
        // notes, which name their members again.
        let Some(drawn) = lay_out(&g, 200) else {
            continue;
        };
        let out: String = drawn
            .rows
            .iter()
            .map(|r| r.iter().map(|(t, _)| t.as_str()).collect::<String>() + "\n")
            .collect();
        let others: Vec<String> = g
            .edges
            .iter()
            .map(|e| e.label.clone())
            .chain(g.clusters.iter().map(|c| c.title.clone()))
            .chain(g.notes.iter().cloned())
            .chain(
                g.nodes
                    .iter()
                    .flat_map(|n| n.sections.iter().flatten().cloned()),
            )
            .collect();
        for (i, node) in g.nodes.iter().enumerate() {
            let label = &node.label;
            if node.shape.is_glyph()
                || label.contains('\n')
                || text::width(label) > 20
                || label.is_empty()
            {
                continue;
            }
            let elsewhere = others.iter().any(|o| word_count(o, label) > 0)
                || g.nodes
                    .iter()
                    .enumerate()
                    .any(|(j, n)| j != i && word_count(&n.label, label) > 0);
            if elsewhere {
                continue;
            }
            assert_eq!(word_count(&out, label), 1, "{name}: {label:?}\n{out}");
            checked += 1;
        }
    }
    assert!(checked > 50, "only {checked} labels checked");
}

// ── Hostile input: bounded work ──

fn assert_quick(src: &str, limit: Duration) {
    let t = Instant::now();
    let lines = render_mermaid_with(src, &theme(), 100, 2, false);
    let took = t.elapsed();
    assert!(took < limit, "took {took:?}");
    for l in &lines {
        assert!(l.width() <= 102);
    }
}

#[test]
fn hostile_flowcharts_return_quickly() {
    let limit = Duration::from_secs(5);
    // 800 edges among 150 nodes: over the edge cap, listed.
    let mut s = String::from("graph TD\n");
    for i in 0..800 {
        s.push_str(&format!(
            "N{} --> N{}\n",
            (i * 37) % 150,
            (i * 91 + 7) % 150
        ));
    }
    assert_quick(&s, limit);
    // A 5000-node chain.
    let mut s = String::from("graph LR\n");
    for i in 0..5000 {
        s.push_str(&format!("C{i} --> C{}\n", i + 1));
    }
    assert_quick(&s, limit);
    // At the caps: 200 nodes, 400 labelled edges, with cycles.
    let mut s = String::from("graph TD\n");
    for i in 0..400 {
        s.push_str(&format!(
            "E{} -->|x| E{}\n",
            (i * 13) % 200,
            (i * 7 + 3) % 200
        ));
    }
    assert_quick(&s, limit);
    // A labelled chain of 199 with a back edge over all of it.
    let mut s = String::from("graph TD\n");
    for i in 0..198 {
        s.push_str(&format!("D{i} -->|l{i}| D{}\n", i + 1));
    }
    s.push_str("D198 --> D0\n");
    assert_quick(&s, limit);
    // Self-loops, a node named like its subgraph, a subgraph in itself.
    assert_quick(
        "graph TD\nsubgraph A\nA --> A\nend\nsubgraph S\nsubgraph S\nX-->X\nend\nend\nS-->S",
        limit,
    );
    // 120 nested subgraphs.
    let mut s = String::from("graph TD\n");
    for i in 0..120 {
        s.push_str(&format!("subgraph G{i}\nK{i} --> K{}\n", i + 1));
    }
    for _ in 0..120 {
        s.push_str("end\n");
    }
    assert_quick(&s, limit);
    // Unbalanced brackets and quotes, stray links.
    assert_quick(
        "graph TD\nA[[[(((\"\nB --> --> -->\n-->\n|||\nC{{{{ --> D",
        limit,
    );
}
