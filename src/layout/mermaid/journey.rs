//! User journeys: tasks grouped under sections, each with a 1–5 score and
//! the actors involved. Drawn as one row per task, the score as a bar and
//! its number so it reads without colour:
//!
//! ```text
//! Go to work
//!   Make tea     █████ 5  Me
//!   Do work      █░░░░ 1  Me, Cat
//! ```
//!
//! When the actors don't fit beside the tasks, they are listed once at the
//! end instead; when even a task's name has no room, it goes on its own line
//! with its score below.

use super::canvas::{Class, Row};
use super::text;

#[derive(Debug, PartialEq)]
struct Task {
    name: String,
    /// The score as written (`5`, `x`), shown beside the bar.
    score: String,
    /// The score as a number, when it is one.
    value: Option<i64>,
    actors: Vec<String>,
    section: Option<usize>,
}

#[derive(Debug, Default, PartialEq)]
struct Journey {
    title: Option<String>,
    sections: Vec<String>,
    tasks: Vec<Task>,
}

/// The highest score; the bar has one cell per point.
const MAX_SCORE: i64 = 5;

/// [`text::keyword`], ignoring case as mermaid does.
fn keyword<'a>(line: &'a str, k: &str) -> Option<&'a str> {
    let head = line.get(..k.len())?;
    if !head.eq_ignore_ascii_case(k) {
        return None;
    }
    let rest = &line[k.len()..];
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// Parse a journey body: `title …`, `section …` and `Task name: score:
/// actor, actor` lines (the actors are optional).
fn parse(body: &[String]) -> Journey {
    let mut j = Journey::default();
    let mut section = None;
    let mut in_description = false;
    for raw in body {
        let line = raw.trim();
        let line = line.find("%%").map_or(line, |i| &line[..i]).trim();
        if in_description {
            in_description = !line.contains('}');
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = keyword(line, "title") {
            j.title = Some(text::decode_label(rest));
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("acctitle") || lower.starts_with("accdescr") {
            in_description = line.contains('{') && !line.contains('}');
            continue;
        }
        if let Some(rest) = keyword(line, "section") {
            j.sections.push(text::decode_label(rest));
            section = Some(j.sections.len() - 1);
            continue;
        }
        let mut parts = line.splitn(3, ':');
        let name = text::decode_label(parts.next().unwrap_or(""));
        let score = parts.next().map(str::trim).unwrap_or("").to_string();
        let actors = parts
            .next()
            .unwrap_or("")
            .split(',')
            .map(text::decode_label)
            .filter(|a| !a.is_empty())
            .collect();
        if name.is_empty() && score.is_empty() {
            continue;
        }
        j.tasks.push(Task {
            name,
            value: score.parse().ok(),
            score,
            actors,
            section,
        });
    }
    j
}

/// The colour class for a score: low scores warn, high ones are good.
fn score_class(value: Option<i64>) -> Class {
    match value {
        Some(v) if v >= 4 => Class::Good,
        Some(3) => Class::Fair,
        Some(_) => Class::Bad,
        None => Class::Label,
    }
}

/// `█████ 5`: one filled cell per point, clamped to 0..=5, then the score as
/// written (`?` when there is none).
fn score_cells(task: &Task, score_w: usize) -> (String, String) {
    let filled = task.value.unwrap_or(0).clamp(0, MAX_SCORE) as usize;
    let bar = format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(MAX_SCORE as usize - filled)
    );
    let shown = if task.score.is_empty() {
        "?".to_string()
    } else {
        text::truncate(&task.score, score_w)
    };
    let pad = score_w.saturating_sub(text::width(&shown));
    (bar, format!(" {shown}{}", " ".repeat(pad)))
}

/// The journey's title and rows at most `inner` columns wide. `title` is a
/// frontmatter title, which an inline `title` line replaces.
pub fn render(title: Option<String>, body: &[String], inner: usize) -> (String, Vec<Row>) {
    let j = parse(body);
    let title = j
        .title
        .clone()
        .or(title)
        .unwrap_or_else(|| "user journey".to_string());
    if j.tasks.is_empty() {
        return (title, vec![vec![("(no tasks)".to_string(), Class::Label)]]);
    }
    let inner = inner.max(12);
    let indent = if j.sections.is_empty() { 0 } else { 2 };
    let score_w = j
        .tasks
        .iter()
        .map(|t| text::width(&t.score).max(1))
        .max()
        .unwrap_or(1)
        .min(4);
    let name_max = j
        .tasks
        .iter()
        .map(|t| text::width(&t.name))
        .max()
        .unwrap_or(0);
    let actors_of = |t: &Task| t.actors.join(", ");
    let actors_max = j
        .tasks
        .iter()
        .map(|t| text::width(&actors_of(t)))
        .max()
        .unwrap_or(0);
    // name, two spaces, the 5-cell bar, " score"
    let fixed = 2 + MAX_SCORE as usize + 1 + score_w;
    let with_actors = actors_max > 0 && indent + name_max + fixed + 2 + actors_max <= inner;
    // Names are cut to fit beside the bar, but never below 8 columns: then
    // each name gets its own line.
    let name_room = inner.saturating_sub(indent + fixed);
    let name_w = if with_actors {
        name_max
    } else {
        name_max.min(name_room)
    };
    let stacked = name_room < 8 && name_max > name_room;

    let mut rows = Vec::new();
    let mut shown = 0;
    let mut headers_up_to = |rows: &mut Vec<Row>, last: usize| {
        while shown <= last && shown < j.sections.len() {
            for l in text::wrap(&j.sections[shown], inner) {
                rows.push(vec![(l, Class::Title)]);
            }
            shown += 1;
        }
    };
    for t in &j.tasks {
        if let Some(s) = t.section {
            headers_up_to(&mut rows, s);
        }
        let lead = " ".repeat(if t.section.is_some() { indent } else { 0 });
        let (bar, score) = score_cells(t, score_w);
        let class = score_class(t.value);
        if stacked {
            for l in text::wrap(&t.name, inner.saturating_sub(lead.len()).max(1)) {
                rows.push(vec![(format!("{lead}{l}"), Class::Text)]);
            }
            // The score sits a little in under its name when there's room.
            let nudge = if lead.len() + 2 + fixed - 2 <= inner {
                2
            } else {
                0
            };
            rows.push(vec![
                (format!("{lead}{}", " ".repeat(nudge)), Class::Plain),
                (bar, class),
                (score, class),
            ]);
            continue;
        }
        let name = text::truncate(&t.name, name_w);
        // Bars line up whether or not the task sits under a section.
        let pad = (indent + name_w).saturating_sub(lead.len() + text::width(&name));
        let mut row: Row = vec![
            (format!("{lead}{name}{}  ", " ".repeat(pad)), Class::Text),
            (bar, class),
            (score, class),
        ];
        if with_actors && !t.actors.is_empty() {
            row.push((format!("  {}", actors_of(t)), Class::Label));
        }
        rows.push(row);
    }
    headers_up_to(&mut rows, usize::MAX);

    // Actors that didn't fit beside the tasks: every one, once, at the end.
    if !with_actors {
        let mut all: Vec<&str> = Vec::new();
        for t in &j.tasks {
            for a in &t.actors {
                if !all.contains(&a.as_str()) {
                    all.push(a);
                }
            }
        }
        if !all.is_empty() {
            for l in text::wrap(&format!("Actors: {}", all.join(", ")), inner) {
                rows.push(vec![(l, Class::Label)]);
            }
        }
    }
    (title, rows)
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

    const DAY: &str = "title My working day\nsection Go to work\n  Make tea: 5: Me\n  Go upstairs: 3: Me\n  Do work: 1: Me, Cat\nsection Go home\n  Go downstairs: 5: Me\n  Sit down: 5: Me\n";

    #[test]
    fn tasks_scores_and_actors_are_parsed() {
        let j = parse(&lines(DAY));
        assert_eq!(j.title.as_deref(), Some("My working day"));
        assert_eq!(j.sections, ["Go to work", "Go home"]);
        assert_eq!(j.tasks.len(), 5);
        assert_eq!(j.tasks[2].name, "Do work");
        assert_eq!(j.tasks[2].value, Some(1));
        assert_eq!(j.tasks[2].actors, ["Me", "Cat"]);
        assert_eq!(j.tasks[4].section, Some(1));
    }

    #[test]
    fn the_day_is_drawn_with_bars_scores_and_actors() {
        let (title, rows) = render(None, &lines(DAY), 40);
        assert_eq!(title, "My working day");
        assert_eq!(
            plain(&rows),
            "Go to work
  Make tea       █████ 5  Me
  Go upstairs    ███░░ 3  Me
  Do work        █░░░░ 1  Me, Cat
Go home
  Go downstairs  █████ 5  Me
  Sit down       █████ 5  Me"
        );
    }

    #[test]
    fn when_narrow_the_actors_are_listed_once_at_the_end() {
        let (_, rows) = render(None, &lines(DAY), 28);
        let out = plain(&rows);
        assert!(out.contains("  Do work        █░░░░ 1\n"), "{out}");
        assert!(out.ends_with("Actors: Me, Cat"), "{out}");
    }

    #[test]
    fn names_are_cut_and_then_stacked_as_the_width_shrinks() {
        let body = lines("A task with a rather long name: 4: Me\nShort: 2\n");
        let (_, rows) = render(None, &body, 24);
        assert!(
            plain(&rows).contains("A task with a …  ████░ 4"),
            "{}",
            plain(&rows)
        );
        let (_, rows) = render(None, &body, 12);
        let out = plain(&rows);
        assert!(
            out.starts_with("A task with\na rather\nlong name\n  ████░ 4"),
            "{out}"
        );
    }

    #[test]
    fn scores_outside_one_to_five_or_not_numbers_still_show() {
        let (_, rows) = render(None, &lines("Zero: 0\nHigh: 9\nWord: x\nNone:\n"), 40);
        assert_eq!(
            plain(&rows),
            "Zero  ░░░░░ 0\nHigh  █████ 9\nWord  ░░░░░ x\nNone  ░░░░░ ?"
        );
    }

    #[test]
    fn scores_are_coloured_by_band() {
        assert_eq!(score_class(Some(5)), Class::Good);
        assert_eq!(score_class(Some(4)), Class::Good);
        assert_eq!(score_class(Some(3)), Class::Fair);
        assert_eq!(score_class(Some(1)), Class::Bad);
        assert_eq!(score_class(None), Class::Label);
    }

    #[test]
    fn tasks_before_any_section_and_empty_sections() {
        let (_, rows) = render(
            None,
            &lines("First: 3\nsection A\nsection B\n  Second: 4\n"),
            40,
        );
        assert_eq!(plain(&rows), "First     ███░░ 3\nA\nB\n  Second  ████░ 4");
    }

    #[test]
    fn an_empty_journey_says_so() {
        let (title, rows) = render(Some("front".into()), &lines("section Only\n"), 40);
        assert_eq!(title, "front");
        assert_eq!(plain(&rows), "(no tasks)");
    }

    #[test]
    fn descriptions_comments_and_keyword_case() {
        let j = parse(&lines(
            "Title T\naccDescr {\n  text: 1: x\n}\nSECTION S\n  Task: 4: Me %% note\n",
        ));
        assert_eq!(j.title.as_deref(), Some("T"));
        assert_eq!(j.tasks.len(), 1);
        assert_eq!(j.tasks[0].actors, ["Me"]);
    }

    #[test]
    fn rows_stay_within_the_width() {
        let body = lines(
            "section A section with a long enough name to wrap\n\
             お茶を入れる長いタスク名: 5: 私, 猫\n\
             An extremely long task name that goes on and on: 12345: Actor One, Actor Two, Actor Three\n\
             x: 3\n",
        );
        for inner in [12, 16, 20, 30, 50, 80] {
            let (_, rows) = render(None, &body, inner);
            for r in &rows {
                let w: usize = r.iter().map(|(t, _)| text::width(t)).sum();
                assert!(w <= inner, "{w} > {inner}: {r:?}");
            }
        }
    }
}
