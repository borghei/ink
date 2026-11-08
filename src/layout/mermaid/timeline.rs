//! Timelines: periods in order, each with its events, grouped under
//! optional sections. Drawn as a vertical list, since mermaid's horizontal
//! layout doesn't fit a terminal:
//!
//! ```text
//! 2002-2006
//!   2002 ── LinkedIn
//!   2004 ┬─ Facebook
//!        └─ Google
//! ```
//!
//! When the period column would leave too little room for the events, each
//! period goes on its own line with its events hanging below it.

use super::canvas::{Class, Row};
use super::text;

#[derive(Debug, Default, PartialEq)]
struct Period {
    name: String,
    events: Vec<String>,
    /// Index into `Timeline::sections`, if the period sits in one.
    section: Option<usize>,
}

#[derive(Debug, Default, PartialEq)]
struct Timeline {
    title: Option<String>,
    sections: Vec<String>,
    periods: Vec<Period>,
}

/// Parse a timeline body: `title …`, `section …`, `period : event : event`
/// and continuation lines `: event` that add to the period above.
fn parse(body: &[String]) -> Timeline {
    let mut t = Timeline::default();
    let mut section = None;
    for raw in body {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        if let Some(rest) = text::keyword(line, "title") {
            t.title = Some(text::decode_label(rest));
            continue;
        }
        if line.starts_with("accTitle") || line.starts_with("accDescr") {
            continue;
        }
        if let Some(rest) = text::keyword(line, "section") {
            t.sections.push(text::decode_label(rest));
            section = Some(t.sections.len() - 1);
            continue;
        }
        let mut parts = line.split(':');
        let name = parts.next().unwrap_or("").trim();
        let events = parts.map(text::decode_label).filter(|e| !e.is_empty());
        if name.is_empty() {
            // `: event` continues the period above; before any period it
            // starts one with no name.
            match t.periods.last_mut() {
                Some(p) if p.section == section => p.events.extend(events),
                _ => t.periods.push(Period {
                    name: String::new(),
                    events: events.collect(),
                    section,
                }),
            }
            continue;
        }
        t.periods.push(Period {
            name: text::decode_label(name),
            events: events.collect(),
            section,
        });
    }
    t
}

/// The narrowest the event column may get beside the period column before
/// the stacked form is used instead.
const MIN_EVENT: usize = 12;

/// The timeline's title and rows at most `inner` columns wide. `title` is a
/// frontmatter title, which an inline `title` line replaces.
pub fn render(title: Option<String>, body: &[String], inner: usize) -> (String, Vec<Row>) {
    let t = parse(body);
    let title = t
        .title
        .clone()
        .or(title)
        .unwrap_or_else(|| "timeline".to_string());
    if t.periods.is_empty() {
        return (title, vec![vec![("(no events)".to_string(), Class::Label)]]);
    }
    let inner = inner.max(8);
    let indent = if t.sections.is_empty() { 0 } else { 2 };
    // Every line of a period's name, so a `<br>` in it lines up too.
    let period_w = t
        .periods
        .iter()
        .flat_map(|p| p.name.split('\n'))
        .map(text::width)
        .max()
        .unwrap_or(0);
    // `name ┬─ event`: the name column, a space, the connector and a space.
    let side_by_side = indent + period_w + 4 + MIN_EVENT <= inner && period_w <= inner / 3;

    let mut rows = Vec::new();
    let mut current_section = None;
    for p in &t.periods {
        if p.section != current_section {
            current_section = p.section;
            if let Some(s) = p.section {
                for l in text::wrap(&t.sections[s], inner) {
                    rows.push(vec![(l, Class::Title)]);
                }
            }
        }
        if side_by_side {
            push_side_by_side(&mut rows, p, indent, period_w, inner);
        } else {
            push_stacked(&mut rows, p, indent, inner);
        }
    }
    (title, rows)
}

/// `  2004 ┬─ Facebook` / `       └─ Google`: the period's name in a fixed
/// column, its events beside it. A wrapped event continues under its own
/// text, behind a `│` while more events follow.
fn push_side_by_side(
    rows: &mut Vec<Row>,
    p: &Period,
    indent: usize,
    period_w: usize,
    inner: usize,
) {
    let lead = " ".repeat(indent);
    let names: Vec<&str> = p.name.split('\n').collect();
    let room = inner.saturating_sub(indent + period_w + 4).max(1);
    let mut lines: Vec<(String, String)> = Vec::new();
    let n = p.events.len();
    for (i, event) in p.events.iter().enumerate() {
        let (first, rest) = match (n, i) {
            (1, _) => ("──", "  "),
            (_, 0) => ("┬─", "│ "),
            _ if i + 1 == n => ("└─", "  "),
            _ => ("├─", "│ "),
        };
        for (k, l) in text::wrap(event, room).into_iter().enumerate() {
            let conn = if k == 0 { first } else { rest };
            lines.push((conn.to_string(), l));
        }
    }
    let height = lines.len().max(names.len()).max(1);
    for row in 0..height {
        let name = names.get(row).copied().unwrap_or("");
        let pad = period_w.saturating_sub(text::width(name));
        let mut r: Row = vec![(format!("{lead}{name}{}", " ".repeat(pad)), Class::Text)];
        if let Some((conn, event)) = lines.get(row) {
            r.push((format!(" {conn} "), Class::Edge));
            r.push((event.clone(), Class::Plain));
        }
        rows.push(r);
    }
}

/// The period on its own line, its events hanging below it:
/// `2004` / `├─ Facebook` / `└─ Google`.
fn push_stacked(rows: &mut Vec<Row>, p: &Period, indent: usize, inner: usize) {
    let lead = " ".repeat(indent);
    if !p.name.is_empty() {
        for l in text::wrap(&p.name, inner.saturating_sub(indent).max(1)) {
            rows.push(vec![(format!("{lead}{l}"), Class::Text)]);
        }
    }
    let room = inner.saturating_sub(indent + 3).max(1);
    let n = p.events.len();
    for (i, event) in p.events.iter().enumerate() {
        let last = i + 1 == n;
        let (first, rest) = if last {
            ("└─ ", "   ")
        } else {
            ("├─ ", "│  ")
        };
        for (k, l) in text::wrap(event, room).into_iter().enumerate() {
            let conn = if k == 0 { first } else { rest };
            rows.push(vec![
                (format!("{lead}{conn}"), Class::Edge),
                (l, Class::Plain),
            ]);
        }
    }
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

    #[test]
    fn periods_events_and_sections_are_parsed() {
        let t = parse(&lines(
            "  title Social media\n  section Early\n    2002 : LinkedIn\n    2004 : Facebook : Google\n  section Later\n    2007 : iPhone\n         : Tweets\n",
        ));
        assert_eq!(t.title.as_deref(), Some("Social media"));
        assert_eq!(t.sections, ["Early", "Later"]);
        assert_eq!(t.periods.len(), 3);
        assert_eq!(t.periods[1].events, ["Facebook", "Google"]);
        assert_eq!(t.periods[2].events, ["iPhone", "Tweets"]);
        assert_eq!(t.periods[2].section, Some(1));
    }

    #[test]
    fn an_event_before_any_period_starts_an_unnamed_one() {
        let t = parse(&lines(": orphan\n2001 : x\n"));
        assert_eq!(t.periods[0].name, "");
        assert_eq!(t.periods[0].events, ["orphan"]);
        assert_eq!(t.periods[1].name, "2001");
    }

    #[test]
    fn a_period_with_no_events_keeps_its_place() {
        let t = parse(&lines("2001\n2002 : x\n"));
        assert_eq!(t.periods[0].events, Vec::<String>::new());
        let (_, rows) = render(None, &lines("2001\n2002 : x\n"), 40);
        assert_eq!(plain(&rows), "2001\n2002 ── x");
    }

    #[test]
    fn side_by_side_with_sections() {
        let (title, rows) = render(
            None,
            &lines("title Social media\nsection Early\n2002 : LinkedIn\n2004 : Facebook : Google : MySpace\nsection Later\n2007 : iPhone\n"),
            40,
        );
        assert_eq!(title, "Social media");
        assert_eq!(
            plain(&rows),
            "Early
  2002 ── LinkedIn
  2004 ┬─ Facebook
       ├─ Google
       └─ MySpace
Later
  2007 ── iPhone"
        );
    }

    #[test]
    fn a_long_event_wraps_under_its_own_text() {
        let (_, rows) = render(
            None,
            &lines("2004 : the year a great many things happened at once : next\n"),
            30,
        );
        assert_eq!(
            plain(&rows),
            "2004 ┬─ the year a great many
     │  things happened at
     │  once
     └─ next"
        );
    }

    #[test]
    fn long_periods_stack_with_their_events_below() {
        let (_, rows) = render(
            None,
            &lines("The early years of the web : Mosaic : Netscape\n"),
            30,
        );
        assert_eq!(
            plain(&rows),
            "The early years of the web
├─ Mosaic
└─ Netscape"
        );
    }

    #[test]
    fn wide_characters_line_up() {
        let (_, rows) = render(None, &lines("平成 : a\n2000 : b\n"), 40);
        let out = plain(&rows);
        let cols: Vec<usize> = out
            .lines()
            .map(|l| text::width(&l[..l.find('─').unwrap()]))
            .collect();
        assert!(cols.iter().all(|&c| c == cols[0]), "{out}");
    }

    #[test]
    fn a_frontmatter_title_is_replaced_by_an_inline_one() {
        let (t, _) = render(Some("front".into()), &lines("2000 : x\n"), 40);
        assert_eq!(t, "front");
        let (t, _) = render(Some("front".into()), &lines("title inline\n2000 : x\n"), 40);
        assert_eq!(t, "inline");
    }

    #[test]
    fn an_empty_timeline_says_so() {
        let (title, rows) = render(None, &lines("title Nothing yet\n"), 40);
        assert_eq!(title, "Nothing yet");
        assert_eq!(plain(&rows), "(no events)");
    }

    #[test]
    fn rows_stay_within_the_width() {
        let body = lines(
            "title t\nsection A section name that is long enough to wrap around\n\
             1999-2001 : an event with quite a lot of words in it : x\n\
             平成時代の終わり : 令和 : y\n: z\n",
        );
        for inner in [8, 12, 20, 30, 50, 80] {
            let (_, rows) = render(None, &body, inner);
            for r in &rows {
                let w: usize = r.iter().map(|(t, _)| text::width(t)).sum();
                assert!(w <= inner.max(8), "{w} > {inner}: {r:?}");
            }
        }
    }
}
