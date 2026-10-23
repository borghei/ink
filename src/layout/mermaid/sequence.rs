//! Sequence diagrams: a box per participant over a vertical lifeline, and
//! messages as arrows between lifelines, top to bottom in source order.
//!
//! Solid (`─`) and dotted (`┄`) lines end in an arrowhead `▶`, a cross `×`,
//! an open head `▷` or nothing; a message to oneself is a loop on the right
//! of the lifeline. Notes are boxes beside or across lifelines, activations
//! a heavy lifeline (`┃`), and `loop` / `alt` / `opt` / `par` / `critical` /
//! `break` / `rect` blocks titled frames with dotted dividers for `else` /
//! `and` / `option`. Participant boxes repeat at the bottom, as mermaid
//! draws them.
//!
//! Lifelines are spaced so every label fits between the participants it
//! joins. Labels and names wrap tighter until the diagram fits the width;
//! when nothing fits, [`layout`] gives up and the caller lists the messages
//! instead. Work is linear in the statements times the participants.

use super::canvas::{Canvas, Class};
use super::graph::LineStyle;
use super::text;
use std::collections::HashMap;

type Row = Vec<(String, Class)>;

/// More participants than this cannot be drawn at any terminal width.
const MAX_PARTICIPANTS: usize = 64;
/// Bound on the canvas, so a hostile diagram cannot exhaust memory.
const MAX_CELLS: usize = 2_000_000;

/// What the end of a message line looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    None,
    Arrow,
    Cross,
    Open,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub label: String,
    pub actor: bool,
}

/// Where a note sits relative to its participant(s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Left,
    Right,
    Over,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Message {
        from: usize,
        to: usize,
        dotted: bool,
        head: Head,
        /// `<<->>`: a head at both ends.
        both: bool,
        label: String,
        /// The `autonumber` number, when numbering is on.
        number: Option<u64>,
        /// `+` on the target: it becomes active.
        activate: bool,
        /// `-` on the target: the sender stops being active.
        deactivate: bool,
    },
    Note {
        place: Place,
        /// The participants it spans (`lo == hi` unless `over A,B`).
        lo: usize,
        hi: usize,
        text: String,
    },
    Activate(usize),
    Deactivate(usize),
    /// A block opens: `loop`, `alt`, `opt`, `par`, `critical`, `break`, `rect`.
    Open {
        kind: String,
        label: String,
    },
    /// `else`, `and` or `option` inside the innermost block.
    Divider {
        kind: String,
        label: String,
    },
    Close,
}

#[derive(Debug, Default)]
pub struct Diagram {
    pub title: Option<String>,
    pub participants: Vec<Participant>,
    pub events: Vec<Event>,
}

impl Diagram {
    /// The participant `id` names, created on first mention.
    fn participant(&mut self, ids: &mut HashMap<String, usize>, id: &str) -> Option<usize> {
        let id = id.trim();
        if id.is_empty() {
            return None;
        }
        if let Some(&i) = ids.get(id) {
            return Some(i);
        }
        self.participants.push(Participant {
            label: text::decode_label(id),
            actor: false,
        });
        ids.insert(id.to_string(), self.participants.len() - 1);
        Some(self.participants.len() - 1)
    }
}

/// Message arrows, longest first so `-->>` is not read as `-->`. The cross
/// forms are tried only when none of these occurs (`-x` also turns up
/// inside names).
const ARROWS: &[(&str, bool, Head, bool)] = &[
    ("<<-->>", true, Head::Arrow, true),
    ("<<->>", false, Head::Arrow, true),
    ("-->>", true, Head::Arrow, false),
    ("->>", false, Head::Arrow, false),
    ("--)", true, Head::Open, false),
    ("-)", false, Head::Open, false),
    ("-->", true, Head::None, false),
    ("->", false, Head::None, false),
];
const CROSSES: &[(&str, bool, Head, bool)] = &[
    ("--x", true, Head::Cross, false),
    ("-x", false, Head::Cross, false),
];

/// A message arrow: (dotted, head, head at both ends).
type Arrow = (bool, Head, bool);

/// Split `A->>+B: text` into (from, arrow, to with its `+`/`-`, label).
fn split_message(line: &str) -> Option<(&str, Arrow, &str, &str)> {
    let (head, label) = line.split_once(':').unwrap_or((line, ""));
    let find = |set: &[(&str, bool, Head, bool)]| {
        head.char_indices().find_map(|(p, _)| {
            set.iter()
                .find(|(tok, ..)| head[p..].starts_with(tok))
                .map(|&(tok, dotted, h, both)| (p, tok.len(), (dotted, h, both)))
        })
    };
    let (p, len, arrow) = find(ARROWS).or_else(|| find(CROSSES))?;
    let from = head[..p].trim();
    let to = head[p + len..].trim();
    if from.is_empty() || to.is_empty() {
        return None;
    }
    Some((from, arrow, to, label))
}

/// Parse the body of a `sequenceDiagram`. Statements that are not drawn
/// (`title` aside: `box`, `create`, `destroy`, `link`, accessibility
/// metadata) and lines that do not parse are skipped.
pub fn parse(title: Option<String>, body: &[String]) -> Diagram {
    let mut d = Diagram {
        title,
        ..Default::default()
    };
    let mut ids: HashMap<String, usize> = HashMap::new();
    // Open groups: true for a drawn block, false for a `box` group (whose
    // `end` closes nothing that is drawn).
    let mut open: Vec<bool> = Vec::new();
    // As in mermaid: the count advances on every message, numbered or not;
    // `autonumber N [step]` sets it, and a bare `autonumber` continues it.
    let (mut seq, mut step, mut numbering) = (1u64, 1u64, false);
    let mut in_descr = false;
    for raw in body {
        let line = raw.trim().trim_end_matches(';').trim();
        if in_descr {
            in_descr = !line.contains('}');
            continue;
        }
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let (word, rest) = split_word(line);
        let kw = word.to_ascii_lowercase();
        match kw.as_str() {
            "participant" | "actor" => declare(&mut d, &mut ids, rest, kw == "actor"),
            "create" => {
                let (w, r) = split_word(rest);
                let w = w.to_ascii_lowercase();
                if w == "participant" || w == "actor" {
                    declare(&mut d, &mut ids, r, w == "actor");
                }
            }
            "note" => note(&mut d, &mut ids, rest),
            "activate" | "deactivate" => {
                if let Some(i) = d.participant(&mut ids, rest) {
                    d.events.push(if kw == "activate" {
                        Event::Activate(i)
                    } else {
                        Event::Deactivate(i)
                    });
                }
            }
            "autonumber" => {
                numbering = rest.trim() != "off";
                let mut nums = rest.split_whitespace().map(|n| n.parse::<u64>().ok());
                if let Some(start) = nums.next().flatten() {
                    seq = start;
                }
                if let Some(s) = nums.next().flatten() {
                    step = s;
                }
            }
            "loop" | "alt" | "opt" | "par" | "par_over" | "critical" | "break" | "rect" => {
                open.push(true);
                d.events.push(Event::Open {
                    kind: kw.trim_end_matches("_over").to_string(),
                    label: text::decode_label(rest),
                });
            }
            "else" | "and" | "option" => {
                if open.last() == Some(&true) {
                    d.events.push(Event::Divider {
                        kind: kw,
                        label: text::decode_label(rest),
                    });
                }
            }
            "box" => open.push(false),
            "end" => {
                if open.pop() == Some(true) {
                    d.events.push(Event::Close);
                }
            }
            "title" if !rest.is_empty() => d.title = Some(text::decode_label(rest)),
            _ if kw.starts_with("title:") => {
                let t = text::decode_label(&line["title:".len()..]);
                if !t.is_empty() {
                    d.title = Some(t);
                }
            }
            _ if kw.starts_with("accdescr") => {
                in_descr = line.contains('{') && !line.contains('}');
            }
            _ => {
                let Some((from, (dotted, head, both), to, label)) = split_message(line) else {
                    continue;
                };
                let (to, activate, deactivate) = match to.as_bytes()[0] {
                    b'+' => (&to[1..], true, false),
                    b'-' => (&to[1..], false, true),
                    _ => (to, false, false),
                };
                let (Some(from), Some(to)) =
                    (d.participant(&mut ids, from), d.participant(&mut ids, to))
                else {
                    continue;
                };
                let number = numbering.then_some(seq);
                seq = seq.saturating_add(step);
                d.events.push(Event::Message {
                    from,
                    to,
                    dotted,
                    head,
                    both,
                    label: text::decode_label(label),
                    number,
                    activate,
                    deactivate,
                });
            }
        }
    }
    for still_open in open.into_iter().rev() {
        if still_open {
            d.events.push(Event::Close);
        }
    }
    d
}

fn split_word(s: &str) -> (&str, &str) {
    s.split_once(char::is_whitespace)
        .map_or((s, ""), |(w, r)| (w, r.trim()))
}

/// `participant A`, `participant A as Alice`, `participant A@{ "type": "actor" }`.
fn declare(d: &mut Diagram, ids: &mut HashMap<String, usize>, rest: &str, actor: bool) {
    let (rest, config) = match rest.find("@{") {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (id, alias) = match rest.split_once(" as ") {
        Some((id, alias)) => (id, Some(alias)),
        None => (rest, None),
    };
    let Some(i) = d.participant(ids, id) else {
        return;
    };
    let p = &mut d.participants[i];
    if let Some(alias) = alias.map(text::decode_label).filter(|a| !a.is_empty()) {
        p.label = alias;
    }
    p.actor |= actor || config.contains("actor");
}

/// `right of A: text`, `left of A: text`, `over A: text`, `over A,B: text`.
fn note(d: &mut Diagram, ids: &mut HashMap<String, usize>, rest: &str) {
    let Some((pos, body)) = rest.split_once(':') else {
        return;
    };
    let (w, names) = split_word(pos.trim());
    let (place, names) = match w.to_ascii_lowercase().as_str() {
        "over" => (Place::Over, names),
        "right" | "left" => {
            let (of, names) = split_word(names);
            if !of.eq_ignore_ascii_case("of") {
                return;
            }
            let place = if w.eq_ignore_ascii_case("right") {
                Place::Right
            } else {
                Place::Left
            };
            (place, names)
        }
        _ => return,
    };
    let (a, b) = names.split_once(',').unwrap_or((names, names));
    let (Some(a), Some(b)) = (d.participant(ids, a), d.participant(ids, b)) else {
        return;
    };
    let (lo, hi) = if place == Place::Over {
        (a.min(b), a.max(b))
    } else {
        (a, a)
    };
    d.events.push(Event::Note {
        place,
        lo,
        hi,
        text: text::decode_label(body),
    });
}

/// The diagram as rows at most `avail` columns wide, or `None` when it
/// cannot fit (the caller then lists it). Each attempt wraps labels and
/// names tighter: (label wrap, name wrap, space between boxes).
pub fn layout(d: &Diagram, avail: usize) -> Option<Vec<Row>> {
    if d.participants.is_empty() {
        return Some(Vec::new());
    }
    if d.participants.len() > MAX_PARTICIPANTS {
        return None;
    }
    const ATTEMPTS: [(usize, usize, usize); 6] = [
        (48, 24, 2),
        (32, 20, 2),
        (24, 16, 2),
        (16, 12, 1),
        (12, 8, 1),
        (8, 6, 1),
    ];
    ATTEMPTS
        .iter()
        .find_map(|&(wrap, name_wrap, sep)| attempt(d, avail, wrap, name_wrap, sep))
}

/// A block's frame, as (participant, columns left of its lifeline) and
/// (participant, columns right of its lifeline).
type Frame = ((usize, usize), (usize, usize));

/// A block being measured: its event index, the extent of what it holds so
/// far, and the width its title and dividers need.
struct Measure {
    at: usize,
    left: Option<(usize, usize)>,
    right: Option<(usize, usize)>,
    need: usize,
}

/// Words this wide or narrower are never broken: a diagram that only fits
/// by splitting "Payment" into "Paymen" / "t" is listed instead.
const WHOLE_WORD: usize = 20;

/// [`text::wrap`] at `max` columns, widened to the longest word (up to
/// [`WHOLE_WORD`]) so words stay whole.
fn wrap_words(s: &str, max: usize) -> Vec<String> {
    let longest = s.split([' ', '\n']).map(text::width).max().unwrap_or(0);
    text::wrap(s, max.max(longest.min(WHOLE_WORD)))
}

/// `loop [Every minute]`, or just `loop`.
fn block_title(kind: &str, label: &str, wrap: usize) -> String {
    if label.is_empty() {
        kind.to_string()
    } else {
        format!(
            "{kind} [{}]",
            text::truncate(&label.replace('\n', " "), wrap)
        )
    }
}

fn attempt(
    d: &Diagram,
    avail: usize,
    wrap: usize,
    name_wrap: usize,
    sep: usize,
) -> Option<Vec<Row>> {
    let n = d.participants.len();
    let names: Vec<Vec<String>> = d
        .participants
        .iter()
        .map(|p| wrap_words(&p.label, name_wrap))
        .collect();
    let box_w: Vec<usize> = names
        .iter()
        .map(|l| l.iter().map(|s| text::width(s)).max().unwrap_or(0) + 4)
        .collect();
    let half_l = |i: usize| box_w[i] / 2;
    let half_r = |i: usize| box_w[i] - 1 - box_w[i] / 2;

    // Text per event, wrapped: message labels, note text, block titles.
    let texts: Vec<Vec<String>> = d
        .events
        .iter()
        .map(|e| match e {
            Event::Message { label, number, .. } => {
                let label = match number {
                    Some(k) if label.is_empty() => k.to_string(),
                    Some(k) => format!("{k}. {label}"),
                    None => label.clone(),
                };
                if label.is_empty() {
                    Vec::new()
                } else {
                    wrap_words(&label, wrap)
                }
            }
            Event::Note { text: t, .. } => wrap_words(t, wrap),
            Event::Open { kind, label } | Event::Divider { kind, label } => {
                vec![block_title(kind, label, wrap)]
            }
            _ => Vec::new(),
        })
        .collect();
    let text_w = |k: usize| texts[k].iter().map(|s| text::width(s)).max().unwrap_or(0);

    // Horizontal pass. Everything drawn is anchored to a lifeline: how far
    // it reaches left and right of it. Reaches and labels set the gaps
    // between lifelines; anything anchored to one lifeline then stays clear
    // of its neighbours, which is what lets a frame enclose its contents by
    // looking only at the outermost participants it holds.
    let mut reach_l = vec![0usize; n];
    let mut reach_r = vec![0usize; n];
    // (left participant, right participant, distance needed between them).
    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    let mut frames: Vec<Option<Frame>> = vec![None; d.events.len()];
    let mut stack: Vec<Measure> = Vec::new();
    for (k, e) in d.events.iter().enumerate() {
        let tw = text_w(k);
        let extent: Option<Frame> = match *e {
            Event::Message { from, to, .. } if from == to => {
                Some(((from, 0), (from, (tw + 1).max(3))))
            }
            Event::Message { from, to, .. } => {
                let (a, b) = (from.min(to), from.max(to));
                spans.push((a, b, (tw + 3).max(4)));
                Some(((a, 0), (b, 0)))
            }
            Event::Note {
                place: Place::Right,
                lo,
                ..
            } => Some(((lo, 0), (lo, tw + 5))),
            Event::Note {
                place: Place::Left,
                lo,
                ..
            } => Some(((lo, tw + 5), (lo, 0))),
            Event::Note { lo, hi, .. } if lo == hi => {
                let w = tw + 4;
                Some(((lo, w / 2), (lo, w - 1 - w / 2)))
            }
            Event::Note { lo, hi, .. } => {
                spans.push((lo, hi, tw.saturating_sub(1)));
                Some(((lo, 2), (hi, 2)))
            }
            Event::Open { .. } => {
                stack.push(Measure {
                    at: k,
                    left: None,
                    right: None,
                    need: tw + 6,
                });
                None
            }
            Event::Divider { .. } => {
                if let Some(m) = stack.last_mut() {
                    m.need = m.need.max(tw + 6);
                }
                None
            }
            Event::Close => stack.pop().map(|m| {
                let (mut l, mut r) = (m.left.unwrap_or((0, 0)), m.right.unwrap_or((0, 0)));
                l.1 += 2;
                r.1 += 2;
                if l.0 == r.0 {
                    let w = l.1 + r.1 + 1;
                    r.1 += m.need.saturating_sub(w);
                } else {
                    spans.push((l.0, r.0, m.need.saturating_sub(l.1 + r.1 + 1)));
                }
                frames[m.at] = Some((l, r));
                (l, r)
            }),
            Event::Activate(_) | Event::Deactivate(_) => None,
        };
        if let Some((l, r)) = extent {
            reach_l[l.0] = reach_l[l.0].max(l.1);
            reach_r[r.0] = reach_r[r.0].max(r.1);
            if let Some(m) = stack.last_mut() {
                m.left = Some(match m.left {
                    Some(o) if o.0 < l.0 || (o.0 == l.0 && o.1 >= l.1) => o,
                    _ => l,
                });
                m.right = Some(match m.right {
                    Some(o) if o.0 > r.0 || (o.0 == r.0 && o.1 >= r.1) => o,
                    _ => r,
                });
            }
        }
    }

    let mut gaps: Vec<usize> = (0..n - 1)
        .map(|i| (half_r(i) + half_l(i + 1) + sep + 1).max(reach_r[i] + reach_l[i + 1] + 2))
        .collect();
    // Shorter spans first, so a long label spreads over gaps its inner
    // messages have already widened.
    spans.sort_unstable_by_key(|&(a, b, need)| (b - a, a, need));
    for (a, b, need) in spans {
        let have: usize = gaps[a..b].iter().sum();
        if have < need {
            let (short, k) = (need - have, b - a);
            for (t, g) in gaps[a..b].iter_mut().enumerate() {
                *g += short / k + usize::from(t < short % k);
            }
        }
    }
    let mut c = vec![half_l(0).max(reach_l[0]); n];
    for i in 1..n {
        c[i] = c[i - 1] + gaps[i - 1];
    }
    let cols = c[n - 1] + half_r(n - 1).max(reach_r[n - 1]) + 1;
    if cols > avail {
        return None;
    }

    // Vertical pass: rows per event.
    let height = |k: usize| match &d.events[k] {
        Event::Message { from, to, .. } => texts[k].len() + if from == to { 2 } else { 1 },
        Event::Note { .. } => texts[k].len() + 2,
        Event::Open { .. } | Event::Divider { .. } | Event::Close => 1,
        Event::Activate(_) | Event::Deactivate(_) => 0,
    };
    let head_h = names.iter().map(Vec::len).max().unwrap_or(1) + 2;
    let body: usize = (0..d.events.len()).map(height).sum();
    let foot = head_h + 1 + body + 1;
    let rows = foot + head_h;
    if rows.saturating_mul(cols) > MAX_CELLS {
        return None;
    }

    let mut cv = Canvas::new(rows, cols);
    // Activation intervals (first row, last row) per participant.
    let mut active: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    let mut opened: Vec<Vec<usize>> = vec![Vec::new(); n];
    // Frames being drawn: (left column, right column, top row).
    let mut drawing: Vec<(usize, usize, usize)> = Vec::new();
    let mut r = head_h + 1;
    for (k, e) in d.events.iter().enumerate() {
        let lines = &texts[k];
        match *e {
            Event::Message {
                from,
                to,
                dotted,
                head,
                both,
                activate,
                deactivate,
                ..
            } => {
                let style = if dotted {
                    LineStyle::Dotted
                } else {
                    LineStyle::Solid
                };
                let y = r + lines.len();
                let last = if from == to {
                    // A loop on the right: out, down, back in.
                    let x = c[from];
                    for (t, l) in lines.iter().enumerate() {
                        cv.text(r + t, x + 2, l, cols, Class::Label);
                    }
                    cv.path(
                        &[(y, x + 1), (y, x + 3), (y + 1, x + 3), (y + 1, x + 1)],
                        style,
                        Class::Edge,
                    );
                    if let Some(g) = head_glyph(head, false) {
                        cv.glyph(y + 1, x + 1, g, Class::Edge);
                    }
                    y + 1
                } else {
                    let (a, b) = (c[from.min(to)], c[from.max(to)]);
                    // Centred between the two lifelines. A message that
                    // crosses others has its label in the gap beside the
                    // sender when it fits there: centred over the span, it
                    // would sit in some other gap and read as that one's.
                    let gap = if to > from {
                        (a, c[from + 1])
                    } else {
                        (c[from - 1], b)
                    };
                    let (la, lb) = if text_w(k) + 3 <= gap.1 - gap.0 {
                        gap
                    } else {
                        (a, b)
                    };
                    for (t, l) in lines.iter().enumerate() {
                        let x = la + 1 + (lb - la - 1).saturating_sub(text::width(l)) / 2;
                        cv.text(r + t, x, l, lb, Class::Label);
                    }
                    cv.line(y, a + 1, y, b - 1, style, Class::Edge);
                    let rightward = to > from;
                    let (start, end) = if rightward {
                        (a + 1, b - 1)
                    } else {
                        (b - 1, a + 1)
                    };
                    if let Some(g) = head_glyph(head, rightward) {
                        cv.glyph(y, end, g, Class::Edge);
                    }
                    if both {
                        if let Some(g) = head_glyph(head, !rightward) {
                            cv.glyph(y, start, g, Class::Edge);
                        }
                    }
                    y
                };
                if activate {
                    opened[to].push(last);
                }
                if deactivate {
                    if let Some(s) = opened[from].pop() {
                        active[from].push((s, last));
                    }
                }
            }
            Event::Note { place, lo, hi, .. } => {
                let w = lines.iter().map(|s| text::width(s)).max().unwrap_or(0) + 4;
                let (x0, x1) = match place {
                    Place::Right => (c[lo] + 2, c[lo] + 1 + w),
                    Place::Left => (c[lo] - 1 - w, c[lo] - 2),
                    Place::Over if lo == hi => (c[lo] - w / 2, c[lo] - w / 2 + w - 1),
                    Place::Over => (c[lo] - 2, c[hi] + 2),
                };
                let y1 = r + lines.len() + 1;
                draw_box(&mut cv, r, x0, y1, x1, Class::Frame, false);
                for (t, l) in lines.iter().enumerate() {
                    let x = x0 + 2 + (x1 - x0 - 3).saturating_sub(text::width(l)) / 2;
                    cv.text(r + 1 + t, x, l, x1 - 1, Class::Plain);
                }
            }
            Event::Activate(i) => opened[i].push(r),
            Event::Deactivate(i) => {
                if let Some(s) = opened[i].pop() {
                    active[i].push((s, r.saturating_sub(1).max(s)));
                }
            }
            Event::Open { ref kind, .. } => {
                if let Some(((pl, ol), (pr, or))) = frames[k] {
                    let (x0, x1) = (c[pl] - ol, c[pr] + or);
                    cv.line(r, x0, r, x1, LineStyle::Solid, Class::Frame);
                    frame_title(&mut cv, r, x0, x1, kind, &lines[0]);
                    drawing.push((x0, x1, r));
                }
            }
            Event::Divider { ref kind, .. } => {
                if let Some(&(x0, x1, _)) = drawing.last() {
                    cv.line(r, x0, r, x1, LineStyle::Dotted, Class::Frame);
                    frame_title(&mut cv, r, x0, x1, kind, &lines[0]);
                }
            }
            Event::Close => {
                if let Some((x0, x1, top)) = drawing.pop() {
                    cv.line(r, x0, r, x1, LineStyle::Solid, Class::Frame);
                    cv.line(top, x0, r, x0, LineStyle::Solid, Class::Frame);
                    cv.line(top, x1, r, x1, LineStyle::Solid, Class::Frame);
                }
            }
        }
        r += height(k);
    }
    for (i, starts) in opened.into_iter().enumerate() {
        for s in starts {
            active[i].push((s, foot - 2));
        }
    }

    // Lifelines, heavy where active, from the top boxes to the bottom ones.
    for i in 0..n {
        let style = |y: usize| {
            if active[i].iter().any(|&(a, b)| (a..=b).contains(&y)) {
                LineStyle::Thick
            } else {
                LineStyle::Solid
            }
        };
        for y in head_h - 1..foot {
            cv.add_bits(y, c[i], super::canvas::DOWN, style(y), Class::Frame);
            cv.add_bits(y + 1, c[i], super::canvas::UP, style(y + 1), Class::Frame);
        }
    }
    // Participant boxes, bottom-aligned on top and top-aligned below.
    for (i, p) in d.participants.iter().enumerate() {
        let (x0, x1) = (c[i] - half_l(i), c[i] + half_r(i));
        let h = names[i].len() + 2;
        for (top, joint) in [(head_h - h, "┬"), (foot, "┴")] {
            let bottom = top + h - 1;
            draw_box(&mut cv, top, x0, bottom, x1, Class::Node, p.actor);
            let y = if joint == "┬" { bottom } else { top };
            cv.glyph(y, c[i], joint, Class::Node);
            for (t, l) in names[i].iter().enumerate() {
                let x = x0 + 2 + (box_w[i] - 4).saturating_sub(text::width(l)) / 2;
                cv.text(top + 1 + t, x, l, x1 - 1, Class::Text);
            }
        }
    }
    Some(cv.render())
}

/// The arrowhead for a message drawn rightward or leftward.
fn head_glyph(head: Head, rightward: bool) -> Option<&'static str> {
    match (head, rightward) {
        (Head::None, _) => None,
        (Head::Arrow, true) => Some("▶"),
        (Head::Arrow, false) => Some("◀"),
        (Head::Open, true) => Some("▷"),
        (Head::Open, false) => Some("◁"),
        (Head::Cross, _) => Some("×"),
    }
}

/// A box drawn over whatever is there (lifelines pass behind it), rounded
/// for an actor.
fn draw_box(
    cv: &mut Canvas,
    y0: usize,
    x0: usize,
    y1: usize,
    x1: usize,
    class: Class,
    round: bool,
) {
    let (tl, tr, bl, br) = if round {
        ("╭", "╮", "╰", "╯")
    } else {
        ("┌", "┐", "└", "┘")
    };
    for y in y0..=y1 {
        for x in x0..=x1 {
            let g = match (y == y0, y == y1, x == x0, x == x1) {
                (true, _, true, _) => tl,
                (true, _, _, true) => tr,
                (_, true, true, _) => bl,
                (_, true, _, true) => br,
                (true, ..) | (_, true, ..) => "─",
                (_, _, true, _) | (_, _, _, true) => "│",
                _ => " ",
            };
            cv.glyph(y, x, g, class);
        }
    }
}

/// A block's title on its top edge or a divider: ` loop [label] `, the
/// keyword in the title style, cut to the frame.
fn frame_title(cv: &mut Canvas, y: usize, x0: usize, x1: usize, kind: &str, title: &str) {
    let room = (x1 - x0 + 1).saturating_sub(6);
    let title = text::truncate(title, room);
    if title.is_empty() {
        return;
    }
    let limit = x1 - 1;
    cv.text(y, x0 + 2, &format!(" {title} "), limit, Class::Label);
    if title.starts_with(kind) {
        cv.text(y, x0 + 3, kind, limit, Class::Title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> Diagram {
        let body: Vec<String> = src.lines().map(str::to_string).collect();
        parse(None, &body)
    }

    fn labels(d: &Diagram) -> Vec<(&str, bool)> {
        d.participants
            .iter()
            .map(|p| (p.label.as_str(), p.actor))
            .collect()
    }

    #[test]
    fn participants_are_declared_aliased_or_implicit_in_order() {
        let d = parse_src(
            "participant B as Bob\nactor U as User\nA->>B: hi\nB->>C: hey\nparticipant A as Alice",
        );
        assert_eq!(
            labels(&d),
            vec![
                ("Bob", false),
                ("User", true),
                ("Alice", false),
                ("C", false)
            ]
        );
    }

    #[test]
    fn every_arrow_form_parses() {
        let d = parse_src(
            "A->B: a\nA-->B: b\nA->>B: c\nA-->>B: d\nA-xB: e\nA--xB: f\nA-)B: g\nA--)B: h\nA<<->>B: i\nA<<-->>B: j",
        );
        let got: Vec<(bool, Head, bool)> = d
            .events
            .iter()
            .map(|e| match e {
                Event::Message {
                    dotted, head, both, ..
                } => (*dotted, *head, *both),
                _ => panic!("{e:?}"),
            })
            .collect();
        use Head::*;
        assert_eq!(
            got,
            vec![
                (false, None, false),
                (true, None, false),
                (false, Arrow, false),
                (true, Arrow, false),
                (false, Cross, false),
                (true, Cross, false),
                (false, Open, false),
                (true, Open, false),
                (false, Arrow, true),
                (true, Arrow, true),
            ]
        );
        assert_eq!(d.participants.len(), 2);
    }

    #[test]
    fn hyphenated_names_and_arrows_in_labels() {
        let d = parse_src("Max-xi->>API-Gateway: a -> b: c");
        assert_eq!(labels(&d), vec![("Max-xi", false), ("API-Gateway", false)]);
        match &d.events[0] {
            Event::Message { label, .. } => assert_eq!(label, "a -> b: c"),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn autonumber_counts_every_message_like_mermaid() {
        let numbers = |src: &str| -> Vec<Option<u64>> {
            parse_src(src)
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::Message { number, .. } => Some(*number),
                    _ => None,
                })
                .collect()
        };
        // A bare `autonumber` continues the count, which runs while off.
        assert_eq!(
            numbers("A->>B: a\nA->>B: b\nautonumber\nA->>B: c\nautonumber off\nA->>B: d\nautonumber\nA->>B: e"),
            vec![None, None, Some(3), None, Some(5)]
        );
        // An explicit start resets it; the step carries over.
        assert_eq!(
            numbers("autonumber 10 5\nA->>B: a\nautonumber 1\nA->>B: b\nA->>B: c"),
            vec![Some(10), Some(1), Some(6)]
        );
    }

    #[test]
    fn activation_shorthand_and_autonumber() {
        let d = parse_src("autonumber 10 5\nA->>+B: x\nB-->>-A: y\nautonumber off\nA->>B: z");
        let got: Vec<(Option<u64>, bool, bool)> = d
            .events
            .iter()
            .map(|e| match e {
                Event::Message {
                    number,
                    activate,
                    deactivate,
                    ..
                } => (*number, *activate, *deactivate),
                _ => panic!("{e:?}"),
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (Some(10), true, false),
                (Some(15), false, true),
                (None, false, false)
            ]
        );
    }

    #[test]
    fn blocks_nest_and_boxes_are_not_blocks() {
        let d = parse_src(
            "box Aqua Team\nparticipant A\nend\nloop L\nalt x\nA->>B: m\nelse y\nA->>B: n\nend\nend\nelse stray\nend\nopt unclosed",
        );
        let kinds: Vec<String> = d
            .events
            .iter()
            .map(|e| match e {
                Event::Open { kind, .. } => format!("open {kind}"),
                Event::Divider { kind, .. } => format!("div {kind}"),
                Event::Close => "close".into(),
                Event::Message { .. } => "msg".into(),
                e => format!("{e:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "open loop",
                "open alt",
                "msg",
                "div else",
                "msg",
                "close",
                "close",
                "open opt",
                "close"
            ]
        );
    }

    #[test]
    fn notes_parse_every_placement() {
        let d = parse_src(
            "Note right of A: r\nnote left of B: l\nNote over A: o\nNote over B,A: span\nNote sideways A: no",
        );
        let got: Vec<(Place, usize, usize)> = d
            .events
            .iter()
            .map(|e| match e {
                Event::Note { place, lo, hi, .. } => (*place, *lo, *hi),
                _ => panic!("{e:?}"),
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (Place::Right, 0, 0),
                (Place::Left, 1, 1),
                (Place::Over, 0, 0),
                (Place::Over, 0, 1)
            ]
        );
    }

    #[test]
    fn unsupported_statements_are_skipped() {
        let d = parse_src(
            "title: Hello\n%% comment\naccTitle: t\naccDescr {\nA->>B: hidden\n}\nlink A: x @ http://x\ncreate participant C\ndestroy C\nrubbish ~~~\nA->>B: shown",
        );
        assert_eq!(d.title.as_deref(), Some("Hello"));
        assert_eq!(labels(&d), vec![("C", false), ("A", false), ("B", false)]);
        assert_eq!(d.events.len(), 1);
    }
}
