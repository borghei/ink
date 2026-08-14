//! Built-in keybinding presets. Each preset is a list of (action_id, key_strings).
//!
//! Preset selection happens in config: `[keybindings] preset = "emacs"`.
//! User overrides under `[keybindings.bindings]` REPLACE the preset's keys for that action.

pub type Preset = &'static [(&'static str, &'static [&'static str])];

/// Default ink bindings (vim-flavored). What ships if no config exists.
pub const DEFAULT: Preset = &[
    ("exit_app", &["q", "esc", "ctrl-c"]),
    ("open_browser", &["shift-b"]),
    ("scroll_down", &["down", "j"]),
    ("scroll_up", &["up", "k"]),
    ("scroll_down_fast", &["alt-down"]),
    ("scroll_up_fast", &["alt-up"]),
    ("page_down", &["space", "pagedown", "ctrl-d", "ctrl-f"]),
    ("page_up", &["pageup", "ctrl-u", "ctrl-b"]),
    ("home", &["home"]),
    ("end", &["end", "shift-g"]),
    ("toggle_toc", &["t"]),
    ("search", &["/"]),
    ("next_heading", &["n"]),
    ("prev_heading", &["shift-n"]),
    ("theme_picker", &["shift-t"]),
    ("next_tab", &["tab"]),
    ("prev_tab", &["backtab"]),
    ("follow_link", &["enter"]),
    ("link_mode", &["f"]),
    ("help", &["?"]),
    ("nav_back", &["[", "alt-left"]),
    ("nav_forward", &["]", "alt-right"]),
    ("select_mode", &["v"]),
    ("select_line_mode", &["shift-v"]),
    ("copy_code", &["c"]),
    ("copy_section", &["shift-y"]),
];

/// Vim preset is just an alias for the default.
pub const VIM: Preset = DEFAULT;

/// Emacs preset: C-n/p for line nav, C-v/M-v for page nav, C-a/C-e for home/end,
/// C-s for search, C-f/C-b for next/prev heading (closest semantic to char-nav).
/// `ctrl-x ctrl-c` chord exits ink (alongside q/esc).
pub const EMACS: Preset = &[
    ("exit_app", &["q", "esc", "ctrl-c", "ctrl-x ctrl-c"]),
    ("open_browser", &["shift-b"]),
    ("scroll_down", &["ctrl-n", "down", "j"]),
    ("scroll_up", &["ctrl-p", "up", "k"]),
    ("scroll_down_fast", &["alt-down"]),
    ("scroll_up_fast", &["alt-up"]),
    ("page_down", &["ctrl-v", "space", "pagedown"]),
    ("page_up", &["alt-v", "pageup", "ctrl-u"]),
    ("home", &["ctrl-a", "home"]),
    ("end", &["ctrl-e", "end", "shift-g"]),
    ("toggle_toc", &["t"]),
    ("search", &["ctrl-s", "/"]),
    ("next_heading", &["ctrl-f", "n"]),
    ("prev_heading", &["shift-n"]),
    ("theme_picker", &["shift-t"]),
    ("next_tab", &["tab"]),
    ("prev_tab", &["backtab"]),
    ("follow_link", &["enter"]),
    ("link_mode", &["f"]),
    ("help", &["?"]),
    ("nav_back", &["[", "alt-left"]),
    ("nav_forward", &["]", "alt-right"]),
    ("select_mode", &["v"]),
    ("select_line_mode", &["shift-v"]),
    ("copy_code", &["c"]),
    ("copy_section", &["shift-y"]),
];

pub fn lookup(name: &str) -> Option<Preset> {
    match name.to_lowercase().as_str() {
        "default" => Some(DEFAULT),
        "vim" => Some(VIM),
        "emacs" => Some(EMACS),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::keymap::{build_keymap, parse_key};
    use crate::input::Action;

    #[test]
    fn default_preset_has_q_for_exit() {
        let (map, warnings) = build_keymap(DEFAULT, None);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        let q = parse_key("q").unwrap();
        assert_eq!(map.singles.get(&q), Some(&Action::ExitApp));
    }

    #[test]
    fn emacs_preset_has_ctrl_n_for_scroll_down() {
        let (map, _) = build_keymap(EMACS, None);
        let cn = parse_key("ctrl-n").unwrap();
        assert_eq!(map.singles.get(&cn), Some(&Action::ScrollDown(1)));
    }

    #[test]
    fn emacs_preset_has_ctrl_v_for_page_down() {
        let (map, _) = build_keymap(EMACS, None);
        let cv = parse_key("ctrl-v").unwrap();
        assert_eq!(map.singles.get(&cv), Some(&Action::PageDown));
    }

    #[test]
    fn lookup_resolves_known_presets() {
        assert!(lookup("default").is_some());
        assert!(lookup("vim").is_some());
        assert!(lookup("emacs").is_some());
        assert!(lookup("EMACS").is_some());
        assert!(lookup("zorblat").is_none());
    }
}
