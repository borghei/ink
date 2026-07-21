//! Shared syntect assets.
//!
//! `SyntaxSet::load_defaults_newlines()` and `ThemeSet::load_defaults()`
//! deserialize syntect's bundled binary dumps — tens of milliseconds and a
//! large allocation each. They used to run inside `layout_document`, i.e. on
//! every startup, resize, theme-picker keystroke, and watch reload. Loading
//! them once behind a `OnceLock` removes that cost from every rebuild.

use std::sync::OnceLock;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

pub fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

pub fn theme_set() -> &'static ThemeSet {
    static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}
