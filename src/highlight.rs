//! Shared syntect assets.
//!
//! `SyntaxSet::load_defaults_newlines()` and `ThemeSet::load_defaults()`
//! deserialize syntect's bundled binary dumps — tens of milliseconds and a
//! large allocation each. They used to run inside `layout_document`, i.e. on
//! every startup, resize, theme-picker keystroke, and watch reload. Loading
//! them once behind a `OnceLock` removes that cost from every rebuild.

use std::str::FromStr;
use std::sync::OnceLock;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSet, ThemeSettings,
};
use syntect::parsing::SyntaxSet;

/// Name of the code theme drawn in the terminal's own 16-colour palette.
pub const ANSI_CODE_THEME: &str = "ansi";

pub fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

pub fn theme_set() -> &'static ThemeSet {
    static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
    THEME_SET.get_or_init(|| {
        let mut set = ThemeSet::load_defaults();
        set.themes.insert(ANSI_CODE_THEME.to_string(), ansi_theme());
        set
    })
}

/// A syntect colour that stands for ANSI palette slot `idx` rather than an
/// RGB value (the encoding bat uses): alpha 0, the index in `r`.
const fn ansi(idx: u8) -> Color {
    Color {
        r: idx,
        g: 0,
        b: 0,
        a: 0,
    }
}

/// Alpha 1 marks "the terminal's default colour".
const DEFAULT: Color = Color {
    r: 0,
    g: 0,
    b: 0,
    a: 1,
};

/// The theme-colour string for a highlighted token: `ansi:N`, `None` for the
/// default colour (the caller falls back to the theme's text colour), or hex.
pub fn token_color(c: Color) -> Option<String> {
    match c.a {
        0 => Some(format!("ansi:{}", c.r & 15)),
        1 => None,
        _ => Some(format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)),
    }
}

fn ansi_theme() -> Theme {
    // (scopes, palette slot, italic)
    let rules: &[(&str, Color, bool)] = &[
        ("comment, punctuation.definition.comment", ansi(8), true),
        (
            "meta.structure.dictionary.key, meta.structure.dictionary.key string, meta.structure.dictionary.key punctuation.definition.string, meta.mapping.key, meta.mapping.key string, meta.mapping.key punctuation.definition.string, support.type.property-name",
            ansi(4),
            false,
        ),
        ("string, punctuation.definition.string", ansi(2), false),
        ("constant.character.escape", ansi(6), false),
        (
            "constant.numeric, constant.language, constant.character",
            ansi(3),
            false,
        ),
        (
            "constant.other.color, punctuation.definition.constant.css",
            ansi(5),
            false,
        ),
        (
            "keyword, storage.modifier, storage.type.function",
            ansi(5),
            false,
        ),
        ("keyword.operator", DEFAULT, false),
        (
            "storage.type, support.type, entity.name.type, entity.name.class",
            ansi(6),
            false,
        ),
        (
            "entity.name.function, support.function, variable.function",
            ansi(4),
            false,
        ),
        ("variable.parameter", ansi(6), false),
        ("entity.name.tag", ansi(4), false),
        ("entity.other.attribute-name", ansi(3), false),
        ("support.constant.property-value", ansi(3), false),
        ("variable.language, support.constant", ansi(1), false),
        ("markup.heading", ansi(4), false),
        ("meta.diff.header.from-file", ansi(1), false),
        ("meta.diff.header.to-file", ansi(2), false),
        (
            "punctuation.definition.from-file, punctuation.definition.to-file, punctuation.definition.inserted, punctuation.definition.deleted",
            ansi(5),
            false,
        ),
        (
            "meta.diff.range, punctuation.definition.range.diff",
            ansi(6),
            false,
        ),
        ("markup.inserted", ansi(2), false),
        ("markup.deleted", ansi(1), false),
        ("invalid", ansi(1), false),
    ];
    Theme {
        name: Some(ANSI_CODE_THEME.to_string()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(DEFAULT),
            ..ThemeSettings::default()
        },
        scopes: rules
            .iter()
            .filter_map(|&(scope, color, italic)| {
                Some(ThemeItem {
                    scope: ScopeSelectors::from_str(scope).ok()?,
                    style: StyleModifier {
                        foreground: Some(color),
                        background: None,
                        font_style: italic.then_some(FontStyle::ITALIC),
                    },
                })
            })
            .collect(),
    }
}
