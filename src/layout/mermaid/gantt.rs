//! Gantt charts: tasks on a time axis scaled to the available width.
//!
//! Dates (`YYYY-MM-DD`, optionally with a time; or `HH:mm` / Unix
//! timestamps per `dateFormat`), durations (`ms s m h d w M y`), `after`
//! and `until` dependencies, ids, and the `done` / `active` / `crit` /
//! `milestone` tags are understood. Each task is one row: its name in a
//! left column, then a bar — `█` done, `▓` active, `░` planned, `◆` a
//! milestone — with critical tasks in the warning colour. Work is linear in
//! the number of tasks.

use super::canvas::Class;
use super::text;
use std::collections::BTreeMap;

type Row = Vec<(String, Class)>;

#[derive(Debug, Clone, PartialEq)]
struct Task {
    name: String,
    section: Option<usize>,
    start: f64,
    end: f64,
    done: bool,
    active: bool,
    crit: bool,
    milestone: bool,
}

/// A parsed chart: title, section names, tasks.
#[derive(Debug, Default)]
struct Chart {
    title: Option<String>,
    sections: Vec<String>,
    tasks: Vec<Task>,
    /// Whether any date had a time of day (or the format is time-only).
    timed: bool,
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `HH:mm[:ss]` as a fraction of a day.
fn parse_clock(s: &str) -> Option<f64> {
    let mut parts = s.split(':');
    let h: f64 = parts.next()?.trim().parse().ok()?;
    let m: f64 = parts.next()?.trim().parse().ok()?;
    let sec: f64 = parts.next().map_or(Some(0.0), |p| p.trim().parse().ok())?;
    if !(0.0..48.0).contains(&h) || !(0.0..60.0).contains(&m) {
        return None;
    }
    Some((h * 3600.0 + m * 60.0 + sec) / 86_400.0)
}

/// A date in days since the epoch, and whether it had a time of day.
fn parse_date(s: &str, format: &str) -> Option<(f64, bool)> {
    let s = s.trim();
    if format.trim() == "X" || format.trim() == "x" {
        let v: f64 = s.parse().ok()?;
        let secs = if format.trim() == "x" { v / 1000.0 } else { v };
        return Some((secs / 86_400.0, true));
    }
    if !format.contains('Y') && format.contains('H') {
        return parse_clock(s).map(|t| (t, true));
    }
    // YYYY-MM-DD (also with / or .), then an optional time.
    let (date, time) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let mut it = date.split(['-', '/', '.']);
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if it.next().is_some()
        || !(1..=12).contains(&m)
        || !(1..=31).contains(&d)
        || !(1..=9999).contains(&y)
    {
        return None;
    }
    let day = days_from_civil(y, m, d) as f64;
    match time.map(parse_clock) {
        Some(Some(t)) => Some((day + t, true)),
        Some(None) => None,
        None => Some((day, false)),
    }
}

/// `3d`, `1.5w`, `24h`, `30m`, `500ms` in days.
fn parse_duration(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    let days = match unit {
        "ms" => n / 86_400_000.0,
        "s" => n / 86_400.0,
        "m" => n / 1440.0,
        "h" => n / 24.0,
        "d" => n,
        "w" => n * 7.0,
        "M" => n * 30.0,
        "y" => n * 365.0,
        _ => return None,
    };
    (days.is_finite() && days >= 0.0).then_some(days)
}

fn parse(title: Option<String>, body: &[String]) -> Chart {
    let mut chart = Chart {
        title,
        ..Default::default()
    };
    let mut format = String::from("YYYY-MM-DD");
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut section: Option<usize> = None;
    let mut prev_end: Option<f64> = None;
    for raw in body {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let kw = |k: &str| {
            line.strip_prefix(k)
                .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
                .map(str::trim)
        };
        if let Some(t) = kw("title") {
            chart.title = Some(text::decode_label(t));
            continue;
        }
        if let Some(f) = kw("dateFormat") {
            format = f.to_string();
            if !format.contains('Y') && format.contains('H') {
                chart.timed = true;
            }
            continue;
        }
        if let Some(s) = kw("section") {
            chart.sections.push(text::decode_label(s));
            section = Some(chart.sections.len() - 1);
            continue;
        }
        if [
            "axisFormat",
            "tickInterval",
            "excludes",
            "includes",
            "todayMarker",
            "weekday",
            "weekend",
            "inclusiveEndDates",
            "topAxis",
            "displayMode",
            "accTitle",
            "accDescr",
            "click",
        ]
        .iter()
        .any(|k| kw(k).is_some() || line.starts_with(&format!("{k}:")))
        {
            continue;
        }
        let Some((name, meta)) = line.split_once(':') else {
            continue;
        };
        let mut items: Vec<&str> = meta
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let mut task = Task {
            name: text::decode_label(name),
            section,
            start: 0.0,
            end: 0.0,
            done: false,
            active: false,
            crit: false,
            milestone: false,
        };
        while let Some(&tag) = items.first() {
            match tag {
                "done" => task.done = true,
                "active" => task.active = true,
                "crit" => task.crit = true,
                "milestone" => task.milestone = true,
                _ => break,
            }
            items.remove(0);
        }
        let looks_like_start =
            |s: &str| s.starts_with("after ") || parse_date(s, &format).is_some();
        let (id, start_s, end_s) = match items.as_slice() {
            [] => (None, None, None),
            [e] => (None, None, Some(*e)),
            [a, e] if looks_like_start(a) => (None, Some(*a), Some(*e)),
            [id, e] => (Some(*id), None, Some(*e)),
            [id, s, e, ..] => (Some(*id), Some(*s), Some(*e)),
        };
        let end_of = |ids: &BTreeMap<String, usize>, tasks: &[Task], refs: &str| -> Option<f64> {
            refs.split_whitespace()
                .filter_map(|r| ids.get(r).map(|&i| tasks[i].end))
                .reduce(f64::max)
        };
        let fallback = prev_end.unwrap_or(0.0);
        task.start = match start_s {
            Some(s) if s.starts_with("after ") => {
                end_of(&ids, &chart.tasks, &s[6..]).unwrap_or(fallback)
            }
            Some(s) => match parse_date(s, &format) {
                Some((d, timed)) => {
                    chart.timed |= timed;
                    d
                }
                None => fallback,
            },
            None => fallback,
        };
        task.end = match end_s {
            Some(e) if e.starts_with("until ") => e[6..]
                .split_whitespace()
                .filter_map(|r| ids.get(r).map(|&i| chart.tasks[i].start))
                .reduce(f64::min)
                .unwrap_or(task.start + 1.0),
            Some(e) => match parse_duration(e) {
                Some(d) => task.start + d,
                None => match parse_date(e, &format) {
                    Some((d, timed)) => {
                        chart.timed |= timed;
                        // An end date is the end instant, as in mermaid.
                        d
                    }
                    None => task.start + 1.0,
                },
            },
            None => task.start + 1.0,
        };
        if task.end < task.start {
            task.end = task.start;
        }
        if let Some(id) = id {
            ids.insert(id.to_string(), chart.tasks.len());
        }
        prev_end = Some(task.end);
        chart.tasks.push(task);
    }
    chart
}

/// Render a chart into rows `inner` columns wide at most, plus its title.
pub fn render(title: Option<String>, body: &[String], inner: usize) -> (String, Vec<Row>) {
    let chart = parse(title, body);
    let title = chart.title.clone().unwrap_or_else(|| "gantt".to_string());
    let mut rows: Vec<Row> = Vec::new();
    if chart.tasks.is_empty() {
        rows.push(vec![("(no tasks)".to_string(), Class::Label)]);
        return (title, rows);
    }
    let inner = inner.max(12);
    // Name column: indent (under a section) + name + a space.
    let longest = chart
        .tasks
        .iter()
        .map(|t| text::width(&t.name) + if t.section.is_some() { 3 } else { 1 })
        .max()
        .unwrap_or(4);
    let name_w = longest
        .min((inner * 2 / 5).max(8))
        .min(inner.saturating_sub(10));
    let chart_w = inner.saturating_sub(name_w + 1).max(1);
    let t0 = chart
        .tasks
        .iter()
        .map(|t| t.start)
        .fold(f64::INFINITY, f64::min);
    let mut t1 = chart
        .tasks
        .iter()
        .map(|t| t.end)
        .fold(f64::NEG_INFINITY, f64::max);
    if t1.is_nan() || t1 <= t0 {
        t1 = t0 + 1.0;
    }
    let span = t1 - t0;
    let col = |t: f64| -> usize {
        let c = ((t - t0) / span * chart_w as f64).floor();
        (c.max(0.0) as usize).min(chart_w - 1)
    };
    let col_end = |t: f64| -> usize {
        let c = ((t - t0) / span * chart_w as f64).ceil();
        (c.max(0.0) as usize).min(chart_w)
    };

    // Axis: tick labels over a ruler.
    let fmt_tick = |t: f64| -> String {
        let day = t.floor() as i64;
        let (y, m, d) = civil_from_days(day);
        if chart.timed && span <= 3.0 {
            let mins = ((t - t.floor()) * 1440.0).round() as i64;
            format!("{:02}:{:02}", mins / 60 % 24, mins % 60)
        } else if civil_from_days(t0.floor() as i64).0 == civil_from_days(t1.floor() as i64).0 {
            format!("{m:02}-{d:02}")
        } else {
            format!("{y:04}-{m:02}-{d:02}")
        }
    };
    // Ticks at a round interval (minutes to years) leaving room for their
    // labels.
    let label_w = text::width(&fmt_tick(t0)) + 2;
    let max_ticks = (chart_w / label_w).max(1) as f64;
    const STEPS: [f64; 22] = [
        1.0 / 1440.0,
        2.0 / 1440.0,
        5.0 / 1440.0,
        10.0 / 1440.0,
        15.0 / 1440.0,
        30.0 / 1440.0,
        1.0 / 24.0,
        2.0 / 24.0,
        3.0 / 24.0,
        6.0 / 24.0,
        12.0 / 24.0,
        1.0,
        2.0,
        7.0,
        14.0,
        30.0,
        61.0,
        91.0,
        182.0,
        365.0,
        730.0,
        3650.0,
    ];
    let step = STEPS
        .iter()
        .copied()
        .find(|st| span / st <= max_ticks)
        .unwrap_or(span / max_ticks);
    let mut labels = vec![' '; chart_w];
    let mut ruler: Vec<char> = vec!['─'; chart_w];
    let put = |labels: &mut Vec<char>, c: usize, t: f64| {
        let lbl: Vec<char> = fmt_tick(t).chars().collect();
        let at = c.min(chart_w.saturating_sub(lbl.len()));
        let free = at + lbl.len() <= chart_w
            && labels[at..at + lbl.len()].iter().all(|&ch| ch == ' ')
            && (at == 0 || labels[at - 1] == ' ')
            && labels.get(at + lbl.len()).is_none_or(|&ch| ch == ' ');
        if free {
            labels[at..at + lbl.len()].copy_from_slice(&lbl);
        }
    };
    // The start is always labelled.
    put(&mut labels, 0, t0);
    let ticks: Vec<f64> = if step >= 28.0 {
        // Month starts, every `k` months.
        let k = (step / 30.4).round().max(1.0) as i64;
        let (mut y, mut m, d) = civil_from_days(t0.floor() as i64);
        if d > 1 {
            m += 1;
        }
        let mut out = Vec::new();
        while out.len() < 64 {
            if m > 12 {
                y += (m - 1) / 12;
                m = (m - 1) % 12 + 1;
            }
            let t = days_from_civil(y, m, 1) as f64;
            if t > t1 {
                break;
            }
            out.push(t);
            m += k;
        }
        out
    } else {
        // Weeks start on Monday (the epoch was a Thursday).
        let offset = if step >= 7.0 { 4.0 } else { 0.0 };
        let mut t = ((t0 - offset) / step).ceil() * step + offset;
        let mut out = Vec::new();
        while t <= t1 + 1e-9 && out.len() < 64 {
            out.push(t);
            t += step;
        }
        out
    };
    for t in ticks {
        let c = col(t);
        ruler[c] = '┼';
        if c > 0 {
            put(&mut labels, c, t);
        }
    }
    ruler[0] = '├';
    ruler[chart_w - 1] = '┤';
    let pad = " ".repeat(name_w + 1);
    rows.push(vec![
        (pad.clone(), Class::Frame),
        (labels.iter().collect(), Class::Label),
    ]);
    rows.push(vec![
        (pad, Class::Frame),
        (ruler.iter().collect(), Class::Frame),
    ]);

    let mut current: Option<Option<usize>> = None;
    for t in &chart.tasks {
        if current != Some(t.section) {
            current = Some(t.section);
            if let Some(s) = t.section {
                rows.push(vec![(
                    text::truncate(&chart.sections[s], inner),
                    Class::Title,
                )]);
            }
        }
        let indent = if t.section.is_some() { 2 } else { 0 };
        let name = text::truncate(&t.name, name_w.saturating_sub(indent + 1).max(1));
        let name_cell = format!("{}{}", " ".repeat(indent), name);
        let name_cell = format!(
            "{name_cell}{}",
            " ".repeat((name_w + 1).saturating_sub(text::width(&name_cell)))
        );
        let class = if t.crit {
            Class::Alert
        } else if t.done {
            Class::Node
        } else if t.active {
            Class::Title
        } else {
            Class::Edge
        };
        let (a, glyph, len) = if t.milestone {
            (col(t.start), "◆", 1)
        } else {
            let a = col(t.start);
            let b = col_end(t.end).max(a + 1);
            let g = if t.done {
                "█"
            } else if t.active {
                "▓"
            } else {
                "░"
            };
            (a, g, b - a)
        };
        rows.push(vec![
            (name_cell, Class::Plain),
            (" ".repeat(a), Class::Frame),
            (glyph.repeat(len), class),
        ]);
    }
    (title, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(src: &str) -> Vec<String> {
        src.lines().map(str::to_string).collect()
    }

    #[test]
    fn civil_dates_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn dates_durations_dependencies_and_tags() {
        let c = parse(
            None,
            &body(
                "dateFormat YYYY-MM-DD\nsection A\nFirst :done, a1, 2024-01-01, 3d\nSecond :active, a2, after a1, 1w\nThird :crit, 2024-01-20, 2024-01-22\nFourth : 24h\nGate :milestone, m1, after a2, 0d\nLast :until m1",
            ),
        );
        let base = days_from_civil(2024, 1, 1) as f64;
        let t = &c.tasks;
        assert_eq!((t[0].start - base, t[0].end - base), (0.0, 3.0));
        assert!(t[0].done);
        assert_eq!((t[1].start - base, t[1].end - base), (3.0, 10.0));
        assert!(t[1].active);
        assert_eq!((t[2].start - base, t[2].end - base), (19.0, 21.0));
        assert!(t[2].crit);
        assert_eq!((t[3].start - base, t[3].end - base), (21.0, 22.0));
        assert!(t[4].milestone);
        assert_eq!(t[4].start - base, 10.0);
        assert_eq!(t[5].end - base, 10.0);
        assert_eq!(c.sections, vec!["A"]);
    }

    #[test]
    fn time_only_charts() {
        let c = parse(
            None,
            &body("dateFormat HH:mm\nWake : 06:00, 30m\nRun : after x, 1h"),
        );
        assert!((c.tasks[0].start - 0.25).abs() < 1e-9);
        assert!((c.tasks[1].start - (0.25 + 30.0 / 1440.0)).abs() < 1e-9);
        assert!(c.timed);
    }

    #[test]
    fn three_thousand_tasks_render_in_linear_time() {
        let mut src = String::from("dateFormat YYYY-MM-DD\nsection S\n");
        src.push_str("t0 :a0, 2024-01-01, 1d\n");
        for i in 1..3000 {
            src.push_str(&format!("t{i} :a{i}, after a{} a{}, 1d\n", i - 1, i / 2));
        }
        let t = std::time::Instant::now();
        let (_, rows) = render(None, &body(&src), 80);
        assert!(t.elapsed() < std::time::Duration::from_secs(3));
        assert_eq!(rows.len(), 3000 + 3);
    }
}
