//! The decorative glyphs ink draws: box borders, bullets, bars, markers.
//!
//! Two sets: [`UNICODE`] (the default) and [`ASCII`], for terminals whose
//! font or encoding cannot show box-drawing and symbol characters — the
//! Linux virtual console, legacy Windows console raster fonts, a non-UTF-8
//! locale, screen readers. The set is chosen once at startup ([`select`])
//! and read everywhere through [`current`].
//!
//! Where a glyph's width feeds layout arithmetic, both sets give it the same
//! display width, so switching sets never changes line lengths; the few that
//! differ (heading markers, the ellipsis) are measured where they are used.

use std::sync::OnceLock;

/// One complete glyph set.
#[derive(Debug, PartialEq, Eq)]
pub struct Glyphs {
    /// True for the 7-bit set.
    pub ascii: bool,

    // ── Boxes: code blocks, tables, popups, mermaid frames ──
    pub h: &'static str,
    pub v: &'static str,
    pub tl: &'static str,
    pub tr: &'static str,
    pub bl: &'static str,
    pub br: &'static str,
    /// `┬`: a column rule meeting the top border.
    pub tee_down: &'static str,
    /// `┴`: a column rule meeting the bottom border.
    pub tee_up: &'static str,
    /// `├`: the header separator meeting the left border.
    pub tee_right: &'static str,
    /// `┤`: the header separator meeting the right border.
    pub tee_left: &'static str,
    pub cross: &'static str,
    /// Light dashed rule (stacked-table record separators, horizontal rules).
    pub dashed: &'static str,

    // ── Document body ──
    /// Heading markers for levels 1–3 (levels 4–6 are indented only).
    pub heading: [&'static str; 3],
    /// Unordered-list marker, with its indent (4 columns).
    pub bullet: &'static str,
    /// Task-list markers, with their indent (4 columns).
    pub task_done: &'static str,
    pub task_pending: &'static str,
    /// Blockquote bar, with its indent (4 columns).
    pub quote_bar: &'static str,
    /// Centre ornament of a horizontal rule (5 columns).
    pub hr_mid: &'static str,
    /// Truncation marker.
    pub ellipsis: &'static str,
    /// Prefix for an image shown as text.
    pub image: &'static str,
    /// Admonition icons: note, tip, important, warning, caution, other.
    pub admonition: [&'static str; 6],

    // ── Chrome: bars, overlays, the file browser ──
    /// Progress bar fill (top bar).
    pub progress: &'static str,
    /// Progress bar remainder (drawn invisible when colours are available).
    pub progress_rest: &'static str,
    /// The row between the document and the status bar.
    pub separator: &'static str,
    /// Selected-item marker in lists (TOC, theme picker, browser).
    pub pointer: &'static str,
    /// "This one" / success tick.
    pub check: &'static str,
    /// Separator between status-bar items.
    pub dot: &'static str,
    /// Dash in prose (titles, headers).
    pub dash: &'static str,
    /// The scroll keys as shown in the status bar.
    pub scroll_keys: &'static str,
    /// Text cursor / accent block.
    pub block: &'static str,

    // ── Mermaid ──
    pub arrow_down: &'static str,
    /// A horizontal arrow (3 columns).
    pub arrow_right: &'static str,
    /// Gantt / chart bar cell.
    pub bar: &'static str,
    pub note: &'static str,
}

pub const UNICODE: Glyphs = Glyphs {
    ascii: false,
    h: "─",
    v: "│",
    tl: "╭",
    tr: "╮",
    bl: "╰",
    br: "╯",
    tee_down: "┬",
    tee_up: "┴",
    tee_right: "├",
    tee_left: "┤",
    cross: "┼",
    dashed: "╌",
    heading: ["█ ", "▌ ", "▎ "],
    bullet: "  ◦ ",
    task_done: "  ✓ ",
    task_pending: "  ○ ",
    quote_bar: "  │ ",
    hr_mid: "  ◆  ",
    ellipsis: "…",
    image: "🖼",
    admonition: ["ℹ ", "💡 ", "❗ ", "⚠ ", "🔴 ", "▎ "],
    progress: "▔",
    progress_rest: "▔",
    separator: "▁",
    pointer: "▸",
    check: "✓",
    dot: "·",
    dash: "—",
    scroll_keys: "↑↓/jk",
    block: "█",
    arrow_down: "▼",
    arrow_right: "──▶",
    bar: "█",
    note: "📝",
};

pub const ASCII: Glyphs = Glyphs {
    ascii: true,
    h: "-",
    v: "|",
    tl: "+",
    tr: "+",
    bl: "+",
    br: "+",
    tee_down: "+",
    tee_up: "+",
    tee_right: "+",
    tee_left: "+",
    cross: "+",
    dashed: "-",
    heading: ["# ", "## ", "### "],
    bullet: "  * ",
    task_done: "[x] ",
    task_pending: "[ ] ",
    quote_bar: "  | ",
    hr_mid: "-----",
    ellipsis: "...",
    image: "[img]",
    admonition: ["(i) ", "(*) ", "(!) ", "(!) ", "(x) ", "| "],
    progress: "=",
    progress_rest: "-",
    separator: " ",
    pointer: ">",
    check: "*",
    dot: "-",
    dash: "-",
    scroll_keys: "j/k",
    block: "#",
    arrow_down: "v",
    arrow_right: "-->",
    bar: "#",
    note: "note:",
};

static CURRENT: OnceLock<&'static Glyphs> = OnceLock::new();

/// Why the set was chosen, for `ink doctor`.
static REASON: OnceLock<&'static str> = OnceLock::new();

/// The glyph set in use. Unicode unless [`select`] chose otherwise.
pub fn current() -> &'static Glyphs {
    CURRENT.get_or_init(|| &UNICODE)
}

/// Why [`current`] is what it is.
pub fn reason() -> &'static str {
    REASON.get().copied().unwrap_or("default")
}

/// Choose the glyph set for this process (first call wins). See
/// [`choose`] for the rule.
pub fn select(flag: bool, config: Option<bool>) {
    let term = std::env::var("TERM").unwrap_or_default();
    let utf8 = if cfg!(unix) {
        crate::platform::locale_is_utf8()
    } else {
        None
    };
    let (ascii, why) = choose(flag, config, &term, utf8);
    let _ = CURRENT.set(if ascii { &ASCII } else { &UNICODE });
    let _ = REASON.set(why);
}

/// Pure selection rule: `--ascii` wins; then `[behavior] ascii` either way;
/// then ASCII on the Linux console or under a locale known not to be UTF-8
/// (`utf8 == Some(false)`; an unset locale is not evidence).
pub fn choose(
    flag: bool,
    config: Option<bool>,
    term: &str,
    utf8: Option<bool>,
) -> (bool, &'static str) {
    if flag {
        return (true, "--ascii");
    }
    if let Some(v) = config {
        return (v, "config behavior.ascii");
    }
    if term == "linux" {
        return (true, "TERM=linux (console font)");
    }
    if utf8 == Some(false) {
        return (true, "locale is not UTF-8");
    }
    (false, "default")
}

/// Replace the drawing characters ink's own renderers emit (boxes, arrows,
/// bars) with ASCII stand-ins of the same width. Text without any of them is
/// returned borrowed.
pub fn asciify(text: &str) -> std::borrow::Cow<'_, str> {
    fn stand_in(c: char) -> Option<&'static str> {
        Some(match c {
            '─' | '╌' | '━' => "-",
            '│' | '┃' => "|",
            '╭' | '╮' | '╰' | '╯' | '┌' | '┐' | '└' | '┘' | '┬' | '┴' | '├' | '┤' | '┼' => {
                "+"
            }
            '▼' => "v",
            '▲' => "^",
            '▶' => ">",
            '◀' => "<",
            '█' | '▌' | '▎' => "#",
            '◆' | '•' | '◦' => "*",
            '📝' => "note:",
            _ => return None,
        })
    }
    if !text.chars().any(|c| stand_in(c).is_some()) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match stand_in(c) {
            Some(s) => out.push_str(s),
            None => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Every glyph of a set, for checks over the whole set.
#[cfg(test)]
fn all(g: &Glyphs) -> Vec<&'static str> {
    let mut v = vec![
        g.h,
        g.v,
        g.tl,
        g.tr,
        g.bl,
        g.br,
        g.tee_down,
        g.tee_up,
        g.tee_right,
        g.tee_left,
        g.cross,
        g.dashed,
        g.bullet,
        g.task_done,
        g.task_pending,
        g.quote_bar,
        g.hr_mid,
        g.ellipsis,
        g.image,
        g.progress,
        g.progress_rest,
        g.separator,
        g.pointer,
        g.check,
        g.dot,
        g.dash,
        g.scroll_keys,
        g.block,
        g.arrow_down,
        g.arrow_right,
        g.bar,
        g.note,
    ];
    v.extend(g.heading);
    v.extend(g.admonition);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn ascii_set_is_seven_bit() {
        for glyph in all(&ASCII) {
            assert!(glyph.is_ascii(), "{glyph:?}");
        }
    }

    #[test]
    fn layout_critical_glyphs_keep_their_width() {
        let pick = |g: &Glyphs| {
            [
                g.h,
                g.v,
                g.tl,
                g.tr,
                g.bl,
                g.br,
                g.tee_down,
                g.tee_up,
                g.tee_right,
                g.tee_left,
                g.cross,
                g.dashed,
                g.bullet,
                g.task_done,
                g.task_pending,
                g.quote_bar,
                g.hr_mid,
                g.progress,
                g.progress_rest,
                g.separator,
                g.pointer,
                g.block,
                g.arrow_down,
                g.arrow_right,
                g.bar,
            ]
        };
        for (u, a) in pick(&UNICODE).iter().zip(pick(&ASCII).iter()) {
            assert_eq!(u.width(), a.width(), "{u:?} vs {a:?}");
        }
    }

    #[test]
    fn selection_rule() {
        assert!(choose(true, Some(false), "xterm", Some(true)).0);
        assert!(!choose(false, Some(false), "linux", Some(false)).0);
        assert!(choose(false, Some(true), "xterm", Some(true)).0);
        assert!(choose(false, None, "linux", None).0);
        assert!(choose(false, None, "xterm", Some(false)).0);
        assert!(!choose(false, None, "xterm-256color", None).0);
        assert!(!choose(false, None, "xterm-256color", Some(true)).0);
    }

    #[test]
    fn asciify_translates_drawing_characters_only() {
        assert_eq!(asciify("╭─ flow ─╮"), "+- flow -+");
        assert_eq!(
            asciify("│     [ A ]  ▼ ──▶ █"),
            "|     [ A ]  v -->>> #".replace(">>>", ">")
        );
        assert!(matches!(
            asciify("plain é text"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn default_is_unicode() {
        // Nothing in the unit-test process calls `select`.
        assert!(!current().ascii);
    }
}
