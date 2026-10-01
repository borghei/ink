//! Terminal color capabilities, detected once from the environment.
//!
//! Two independent questions:
//!
//! * **Depth** — how many colours the terminal can show: 24-bit, the
//!   256-colour palette, or the 16 ANSI colours ([`detect_depth`]). Theme
//!   colours are RGB; below truecolor they are mapped to the nearest palette
//!   entry. In 16-colour mode they become the *named* ANSI colours, so the
//!   user's own terminal scheme decides the exact shades.
//! * **Enabled** — whether to emit colour at all. [`color_enabled`] decides,
//!   for non-TUI output (`--plain`, `ink diff`), with this precedence:
//!
//!   1. an explicit `--color always|never` beats everything;
//!   2. `NO_COLOR` (non-empty) turns color off;
//!   3. `CLICOLOR_FORCE` (non-empty, not `0`) or `FORCE_COLOR` (non-empty)
//!      turns it on;
//!   4. `TERM=dumb` turns it off;
//!   5. otherwise color is on only when stdout is a terminal.
//!
//!   The reader asks the same question as if stdout were a terminal (it
//!   always is); with colour off it draws with attributes only — bold,
//!   underline, reverse video — see [`set_tui_level`].

use ratatui::style::Color;
use std::sync::OnceLock;

/// How much colour to emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorLevel {
    /// No colour: attributes only.
    None,
    /// The 16 ANSI colours (`TERM=linux`, `xterm`, `vt100`, …).
    Ansi16,
    /// The xterm 256-colour palette.
    Ansi256,
    /// 24-bit RGB.
    TrueColor,
}

impl ColorLevel {
    /// Human-readable name for `ink doctor`.
    pub fn describe(self) -> &'static str {
        match self {
            ColorLevel::None => "none",
            ColorLevel::Ansi16 => "16 colours (ANSI)",
            ColorLevel::Ansi256 => "256 colours",
            ColorLevel::TrueColor => "truecolour (24-bit)",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TermCaps {
    /// `NO_COLOR` was set to a non-empty value.
    pub no_color: bool,
    /// What the terminal can display (never [`ColorLevel::None`]).
    pub depth: ColorLevel,
}

/// Non-empty value of an environment variable.
fn env_set(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl TermCaps {
    fn detect() -> Self {
        Self {
            no_color: env_set("NO_COLOR").is_some(),
            depth: depth_from_env().0,
        }
    }
}

fn depth_from_env() -> (ColorLevel, String) {
    detect_depth(&DepthEnv {
        colorterm: &std::env::var("COLORTERM").unwrap_or_default(),
        term: &std::env::var("TERM").unwrap_or_default(),
        term_program: &std::env::var("TERM_PROGRAM").unwrap_or_default(),
        wt_session: std::env::var_os("WT_SESSION").is_some(),
        tmux: std::env::var_os("TMUX").is_some(),
    })
}

/// The inputs to [`detect_depth`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DepthEnv<'a> {
    pub colorterm: &'a str,
    pub term: &'a str,
    pub term_program: &'a str,
    pub wt_session: bool,
    pub tmux: bool,
}

/// `TERM` values that mean 16 colours (or fewer) whatever else is set.
const ANSI16_TERMS: &[&str] = &[
    "linux",
    "xterm",
    "xterm-color",
    "vt100",
    "vt102",
    "vt220",
    "ansi",
    "screen",
    "cygwin",
    "dumb",
    "rxvt",
    "rxvt-unicode",
    "eterm",
    "cons25",
];

/// `TERM` values without `256color` in the name whose terminals still do 256.
const BARE_256_TERMS: &[&str] = &[
    "tmux",
    "foot",
    "contour",
    "rio",
    "putty",
    "konsole",
    "mintty",
    "st",
    "ms-terminal",
];

/// Pure colour-depth detection, as (level, the signal that decided it).
///
/// Conservative where it matters: almost every modern emulator reports
/// `xterm-256color`, which stays at 256; only a `TERM` that names a 16-colour
/// terminal, or an unknown one with no other colour signal, drops to 16.
pub fn detect_depth(env: &DepthEnv) -> (ColorLevel, String) {
    let DepthEnv {
        colorterm,
        term,
        term_program,
        wt_session,
        tmux,
    } = *env;
    if let Some(signal) = truecolor_signal(colorterm, term, term_program, wt_session) {
        return (ColorLevel::TrueColor, signal);
    }
    if term.contains("256color") || term.contains("256-color") {
        return (ColorLevel::Ansi256, format!("TERM={term}"));
    }
    // The Linux console can't do more, whatever the variables claim.
    if term == "linux" {
        return (ColorLevel::Ansi16, "TERM=linux".into());
    }
    // tmux renders 256-colour output for its outer terminal even when the
    // pane's TERM is plain `screen`.
    if tmux {
        return (ColorLevel::Ansi256, "TMUX (tmux re-renders colour)".into());
    }
    // A non-truecolor COLORTERM (`gnome-terminal`, `rxvt-xpm`, `1`) still
    // marks an emulator that predates the variable's truecolor meaning but
    // does 256; so does Terminal.app.
    if !colorterm.is_empty() {
        return (ColorLevel::Ansi256, format!("COLORTERM={colorterm}"));
    }
    if term_program == "Apple_Terminal" {
        return (ColorLevel::Ansi256, "TERM_PROGRAM=Apple_Terminal".into());
    }
    // No TERM at all: Windows consoles (which take 256-colour sequences) or a
    // stripped environment. Not enough evidence to downgrade.
    if term.is_empty() {
        return (ColorLevel::Ansi256, "TERM unset; assuming 256".into());
    }
    if ANSI16_TERMS.contains(&term) {
        return (ColorLevel::Ansi16, format!("TERM={term}"));
    }
    if BARE_256_TERMS.contains(&term) {
        return (ColorLevel::Ansi256, format!("TERM={term}"));
    }
    (
        ColorLevel::Ansi16,
        format!("TERM={term} (no 256color, no COLORTERM)"),
    )
}

/// Pure truecolor detection. Windows Terminal (`WT_SESSION`) and several
/// common terminals support 24-bit color without setting `COLORTERM`.
#[cfg(test)]
fn is_truecolor(colorterm: &str, term: &str, term_program: &str, wt_session: bool) -> bool {
    truecolor_signal(colorterm, term, term_program, wt_session).is_some()
}

/// The environment signal that advertises truecolor, if any.
fn truecolor_signal(
    colorterm: &str,
    term: &str,
    term_program: &str,
    wt_session: bool,
) -> Option<String> {
    if colorterm.eq_ignore_ascii_case("truecolor") || colorterm.eq_ignore_ascii_case("24bit") {
        return Some(format!("COLORTERM={colorterm}"));
    }
    if [
        "truecolor",
        "direct",
        "kitty",
        "alacritty",
        "ghostty",
        "wezterm",
    ]
    .iter()
    .any(|t| term.contains(t))
    {
        return Some(format!("TERM={term}"));
    }
    if wt_session {
        return Some("WT_SESSION (Windows Terminal)".into());
    }
    if matches!(term_program, "vscode" | "ghostty" | "WezTerm" | "iTerm.app") {
        return Some(format!("TERM_PROGRAM={term_program}"));
    }
    None
}

/// The colour depth ink renders at and the signal that decided it, for
/// `ink doctor`.
pub fn depth_report() -> (&'static str, String) {
    let (level, signal) = depth_from_env();
    (level.describe(), signal)
}

/// The reader's colour level, set once at startup by the CLI.
static TUI_LEVEL: OnceLock<ColorLevel> = OnceLock::new();

/// Fix the reader's colour level: the terminal's depth, or
/// [`ColorLevel::None`] when colour is turned off (`--color=never`,
/// `NO_COLOR`, `TERM=dumb`). Later calls are ignored.
pub fn set_tui_level(enabled: bool) {
    let _ = TUI_LEVEL.set(if enabled {
        caps().depth
    } else {
        ColorLevel::None
    });
}

/// The level the reader draws at. Without [`set_tui_level`] (library use,
/// tests): the terminal's depth, or none under `NO_COLOR`.
pub fn tui_level() -> ColorLevel {
    *TUI_LEVEL.get_or_init(|| {
        let c = caps();
        if c.no_color {
            ColorLevel::None
        } else {
            c.depth
        }
    })
}

/// The `--color` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ColorChoice {
    /// Color when stdout is a terminal (and the environment allows it).
    #[default]
    Auto,
    /// Always emit color and hyperlinks.
    Always,
    /// Never emit escape sequences.
    Never,
}

/// Should non-TUI output carry escape sequences? See the module docs for
/// the precedence.
pub fn color_enabled(choice: ColorChoice, stdout_tty: bool) -> bool {
    color_enabled_why(choice, stdout_tty).0
}

/// [`color_enabled`] plus the signal that decided it (for `ink doctor`).
pub fn color_enabled_why(choice: ColorChoice, stdout_tty: bool) -> (bool, &'static str) {
    resolve_color_why(
        choice,
        &env_set,
        std::env::var("TERM").unwrap_or_default().as_str(),
        stdout_tty,
    )
}

/// Pure form of [`color_enabled`]: `env` returns a variable's non-empty value.
#[cfg(test)]
fn resolve_color(
    choice: ColorChoice,
    env: &dyn Fn(&str) -> Option<String>,
    term: &str,
    stdout_tty: bool,
) -> bool {
    resolve_color_why(choice, env, term, stdout_tty).0
}

fn resolve_color_why(
    choice: ColorChoice,
    env: &dyn Fn(&str) -> Option<String>,
    term: &str,
    stdout_tty: bool,
) -> (bool, &'static str) {
    match choice {
        ColorChoice::Always => return (true, "--color=always"),
        ColorChoice::Never => return (false, "--color=never"),
        ColorChoice::Auto => {}
    }
    if env("NO_COLOR").is_some() {
        return (false, "NO_COLOR");
    }
    if env("CLICOLOR_FORCE").is_some_and(|v| v != "0") {
        return (true, "CLICOLOR_FORCE");
    }
    if env("FORCE_COLOR").is_some() {
        return (true, "FORCE_COLOR");
    }
    if term == "dumb" {
        return (false, "TERM=dumb");
    }
    if stdout_tty {
        (true, "stdout is a terminal")
    } else {
        (false, "stdout is not a terminal")
    }
}

/// Cached capabilities for the current process.
pub fn caps() -> TermCaps {
    static CAPS: OnceLock<TermCaps> = OnceLock::new();
    *CAPS.get_or_init(TermCaps::detect)
}

/// Map an RGB colour to what the terminal can show at `level`: unchanged at
/// truecolor, the nearest 256-palette index, the nearest named ANSI colour,
/// or [`Color::Reset`] (the terminal's own default) with colour off.
pub fn rgb_at_level(r: u8, g: u8, b: u8, level: ColorLevel) -> Color {
    match level {
        ColorLevel::TrueColor => Color::Rgb(r, g, b),
        ColorLevel::Ansi256 => Color::Indexed(rgb_to_256(r, g, b)),
        ColorLevel::Ansi16 => ansi16_color(rgb_to_ansi16(r, g, b)),
        ColorLevel::None => Color::Reset,
    }
}

/// xterm's default values for the 16 ANSI colours, used only to measure
/// which one a theme colour is closest to — the terminal draws its own.
pub const XTERM_16: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

/// Index (0–15) of the ANSI colour nearest to an RGB triple.
///
/// Plain RGB distance to the xterm palette sends every pastel theme colour to
/// grey (a soft pink is closer to `(127,127,127)` than to pure red), which
/// would flatten a theme to monochrome. So: near-neutral colours (chroma
/// below 64) take the nearest of the four greys by palette distance; anything
/// with real colour keeps its hue — the nearest of the six ANSI hues — and
/// is bright when it is light or fully saturated.
pub fn rgb_to_ansi16(r: u8, g: u8, b: u8) -> u8 {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let chroma = max - min;
    if chroma < 64 {
        let dist = |i: usize| {
            let (pr, pg, pb) = XTERM_16[i];
            let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
            d(r, pr) + d(g, pg) + d(b, pb)
        };
        return [0usize, 8, 7, 15]
            .into_iter()
            .min_by_key(|&i| dist(i))
            .unwrap_or(7) as u8;
    }
    // Hue in degrees, 0..360.
    let (rf, gf, bf, c) = (r as f32, g as f32, b as f32, chroma as f32);
    let hue = if max == r {
        60.0 * (((gf - bf) / c).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((bf - rf) / c + 2.0)
    } else {
        60.0 * ((rf - gf) / c + 4.0)
    };
    // Sectors tuned to how terminal palettes read: orange counts as yellow,
    // yellow-green as green.
    let base = match hue {
        h if !(20.0..345.0).contains(&h) => 1, // red
        h if h < 75.0 => 3,                    // yellow
        h if h < 160.0 => 2,                   // green
        h if h < 205.0 => 6,                   // cyan
        h if h < 260.0 => 4,                   // blue
        _ => 5,                                // magenta
    };
    let lightness = (max as u16 + min as u16) / 2;
    let bright = max >= 240 || lightness > 150;
    if bright {
        base + 8
    } else {
        base
    }
}

/// The named colour for ANSI index 0–15, so the terminal's own palette (the
/// user's scheme) applies. Out-of-range indices fall back to the default.
pub fn ansi16_color(idx: u8) -> Color {
    match idx {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        _ => Color::Reset,
    }
}

/// The SGR parameters selecting `rgb` as foreground (`fg`) or background at
/// `level`, without the `ESC [` / `m` around them. Empty with colour off.
pub fn sgr_params((r, g, b): (u8, u8, u8), fg: bool, level: ColorLevel) -> String {
    match level {
        ColorLevel::TrueColor => format!("{};2;{r};{g};{b}", if fg { 38 } else { 48 }),
        ColorLevel::Ansi256 => format!("{};5;{}", if fg { 38 } else { 48 }, rgb_to_256(r, g, b)),
        ColorLevel::Ansi16 => {
            let i = rgb_to_ansi16(r, g, b) as u16;
            // 30–37 / 40–47 for the normal eight, 90–97 / 100–107 bright.
            let base = match (fg, i < 8) {
                (true, true) => 30,
                (true, false) => 90 - 8,
                (false, true) => 40,
                (false, false) => 100 - 8,
            };
            (base + i).to_string()
        }
        ColorLevel::None => String::new(),
    }
}

/// Map an RGB triple to the nearest xterm-256 palette index.
pub fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    // Grayscale ramp (232..=255) when the channels are close together.
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max - min < 8 {
        // 24 gray levels from 8 to 238.
        if r < 8 {
            return 16;
        }
        if r > 248 {
            return 231;
        }
        return 232 + ((r as u16 - 8) * 23 / 240) as u8;
    }
    // 6x6x6 color cube (16..=231).
    let comp = |v: u8| -> u16 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            ((v as u16 - 35) / 40).min(5)
        }
    };
    (16 + 36 * comp(r) + 6 * comp(g) + comp(b)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            vars.iter()
                .find(|(k, v)| *k == name && !v.is_empty())
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn color_precedence() {
        use ColorChoice::*;
        let none = env_of(&[]);
        // Auto follows the TTY.
        assert!(resolve_color(Auto, &none, "xterm", true));
        assert!(!resolve_color(Auto, &none, "xterm", false));
        // The explicit flag beats everything.
        let no_color = env_of(&[("NO_COLOR", "1")]);
        assert!(resolve_color(Always, &no_color, "dumb", false));
        assert!(!resolve_color(
            Never,
            &env_of(&[("FORCE_COLOR", "1")]),
            "xterm",
            true
        ));
        // NO_COLOR beats the force vars and the TTY; an empty one is unset.
        let both = env_of(&[("NO_COLOR", "1"), ("CLICOLOR_FORCE", "1")]);
        assert!(!resolve_color(Auto, &both, "xterm", true));
        assert!(resolve_color(
            Auto,
            &env_of(&[("NO_COLOR", "")]),
            "xterm",
            true
        ));
        // Force vars beat TERM=dumb and a pipe.
        assert!(resolve_color(
            Auto,
            &env_of(&[("CLICOLOR_FORCE", "1")]),
            "dumb",
            false
        ));
        assert!(resolve_color(
            Auto,
            &env_of(&[("FORCE_COLOR", "3")]),
            "xterm",
            false
        ));
        assert!(!resolve_color(
            Auto,
            &env_of(&[("CLICOLOR_FORCE", "0")]),
            "xterm",
            false
        ));
        // TERM=dumb is off even on a TTY.
        assert!(!resolve_color(Auto, &none, "dumb", true));
    }

    #[test]
    fn truecolor_detection_covers_terminals_without_colorterm() {
        assert!(is_truecolor("truecolor", "xterm-256color", "", false));
        assert!(
            is_truecolor("", "xterm-256color", "", true),
            "Windows Terminal"
        );
        for program in ["vscode", "ghostty", "WezTerm", "iTerm.app"] {
            assert!(
                is_truecolor("", "xterm-256color", program, false),
                "{program}"
            );
        }
        assert!(!is_truecolor("", "xterm-256color", "Apple_Terminal", false));
    }

    fn depth(term: &str, colorterm: &str, term_program: &str) -> ColorLevel {
        detect_depth(&DepthEnv {
            colorterm,
            term,
            term_program,
            ..Default::default()
        })
        .0
    }

    #[test]
    fn depth_detection_table() {
        use ColorLevel::*;
        let table: &[(&str, &str, &str, ColorLevel)] = &[
            // (TERM, COLORTERM, TERM_PROGRAM, expected)
            ("xterm-256color", "truecolor", "", TrueColor),
            ("xterm-256color", "", "iTerm.app", TrueColor),
            ("xterm-kitty", "", "", TrueColor),
            ("xterm-ghostty", "", "", TrueColor),
            ("xterm-direct", "", "", TrueColor),
            // The common case stays exactly where it was.
            ("xterm-256color", "", "", Ansi256),
            ("screen-256color", "", "", Ansi256),
            ("tmux-256color", "", "", Ansi256),
            ("xterm-256color", "", "Apple_Terminal", Ansi256),
            ("putty", "", "", Ansi256),
            ("tmux", "", "", Ansi256),
            // Named 16-colour terminals.
            ("linux", "", "", Ansi16),
            ("linux", "gnome-terminal", "", Ansi16),
            ("xterm", "", "", Ansi16),
            ("vt100", "", "", Ansi16),
            ("ansi", "", "", Ansi16),
            ("screen", "", "", Ansi16),
            ("cygwin", "", "", Ansi16),
            ("rxvt-unicode", "", "", Ansi16),
            // An unknown TERM with no 256color and no COLORTERM.
            ("sun-color", "", "", Ansi16),
            // …but any COLORTERM, or Terminal.app, is evidence of 256.
            ("xterm", "gnome-terminal", "", Ansi256),
            ("xterm", "", "Apple_Terminal", Ansi256),
            // No TERM at all (Windows consoles): not enough to downgrade.
            ("", "", "", Ansi256),
        ];
        for &(term, colorterm, program, want) in table {
            assert_eq!(
                depth(term, colorterm, program),
                want,
                "TERM={term} COLORTERM={colorterm} TERM_PROGRAM={program}"
            );
        }
        // Windows Terminal and tmux.
        let wt = DepthEnv {
            wt_session: true,
            ..Default::default()
        };
        assert_eq!(detect_depth(&wt).0, TrueColor);
        let tmux_screen = DepthEnv {
            term: "screen",
            tmux: true,
            ..Default::default()
        };
        assert_eq!(detect_depth(&tmux_screen).0, Ansi256);
    }

    #[test]
    fn nearest_ansi_colour() {
        // Exact palette entries map to themselves.
        for (i, &(r, g, b)) in XTERM_16.iter().enumerate() {
            assert_eq!(rgb_to_ansi16(r, g, b), i as u8);
        }
        // Theme-like colours land on the obvious name.
        assert_eq!(rgb_to_ansi16(0x1a, 0x1b, 0x26), 0, "dark bg -> black");
        assert_eq!(rgb_to_ansi16(0xff, 0xff, 0xff), 15, "light bg -> white");
        assert_eq!(rgb_to_ansi16(0xc0, 0xca, 0xf5), 7, "pale fg -> grey");
        assert_eq!(rgb_to_ansi16(0x7a, 0xa2, 0xf7), 12, "blue heading");
        assert_eq!(rgb_to_ansi16(0xf7, 0x76, 0x8e), 9, "red-pink");
        assert_eq!(rgb_to_ansi16(0x9e, 0xce, 0x6a), 10, "pastel green");
        assert_eq!(rgb_to_ansi16(0xe0, 0xaf, 0x68), 11, "amber");
        assert_eq!(rgb_to_ansi16(0xbb, 0x9a, 0xf7), 13, "lavender");
        assert_eq!(rgb_to_ansi16(0x7d, 0xcf, 0xff), 14, "sky");
        // Light-theme accents stay normal intensity (readable on white).
        assert_eq!(rgb_to_ansi16(0x09, 0x69, 0xda), 4, "github blue");
        assert_eq!(rgb_to_ansi16(0xcf, 0x22, 0x2e), 1, "github red");
        assert_eq!(rgb_to_ansi16(0x24, 0x29, 0x2f), 0, "light-theme text");
        assert_eq!(rgb_to_ansi16(0x56, 0x5f, 0x89), 8, "muted -> bright black");
        // Named colours, so the user's palette applies.
        assert_eq!(rgb_at_level(255, 0, 0, ColorLevel::Ansi16), Color::LightRed);
        assert_eq!(rgb_at_level(0, 0, 0, ColorLevel::Ansi16), Color::Black);
        assert_eq!(rgb_at_level(1, 2, 3, ColorLevel::None), Color::Reset);
        assert_eq!(
            rgb_at_level(1, 2, 3, ColorLevel::TrueColor),
            Color::Rgb(1, 2, 3)
        );
    }

    #[test]
    fn sgr_codes_per_level() {
        use ColorLevel::*;
        assert_eq!(sgr_params((1, 2, 3), true, TrueColor), "38;2;1;2;3");
        assert_eq!(sgr_params((1, 2, 3), false, TrueColor), "48;2;1;2;3");
        assert_eq!(sgr_params((255, 0, 0), true, Ansi256), "38;5;196");
        assert_eq!(sgr_params((205, 0, 0), true, Ansi16), "31");
        assert_eq!(sgr_params((205, 0, 0), false, Ansi16), "41");
        assert_eq!(sgr_params((255, 255, 255), true, Ansi16), "97");
        assert_eq!(sgr_params((255, 255, 255), false, Ansi16), "107");
        assert_eq!(sgr_params((1, 2, 3), true, None), "");
    }

    #[test]
    fn quantize_stays_in_palette() {
        for (r, g, b) in [(0, 0, 0), (255, 255, 255), (128, 64, 200), (10, 10, 10)] {
            let idx = rgb_to_256(r, g, b);
            assert!(idx >= 16, "index {idx} below color range");
        }
    }
}
