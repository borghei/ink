use crate::input::Action;
use crossterm::event::{KeyCode, KeyModifiers};
use std::collections::HashMap;

/// A single keypress: code + modifier set.
pub type KeyBinding = (KeyCode, KeyModifiers);

/// Parse a key string like `q`, `ctrl-c`, `alt-down`, `shift-g`, `ctrl-shift-p`.
///
/// Modifiers (`ctrl`, `alt`, `shift`) are joined with `-`. Last segment is the key.
/// Named keys: esc, enter, tab, backtab, space, pageup, pagedown, home, end,
/// up, down, left, right, backspace, f1..f12.
pub fn parse_key(s: &str) -> Result<KeyBinding, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty key string".into());
    }

    let parts: Vec<&str> = s.split('-').collect();
    let (mods_parts, key_part) = parts.split_at(parts.len() - 1);
    let key_part = key_part[0];

    let mut mods = KeyModifiers::empty();
    for m in mods_parts {
        match m.to_lowercase().as_str() {
            "ctrl" | "control" | "c" => mods |= KeyModifiers::CONTROL,
            "alt" | "meta" | "option" | "m" => mods |= KeyModifiers::ALT,
            "shift" | "s" => mods |= KeyModifiers::SHIFT,
            other => return Err(format!("unknown modifier '{other}' in '{s}'")),
        }
    }

    let code = parse_keycode(key_part)?;

    // Single-char shifted letters: if user wrote `shift-g`, normalize to `KeyCode::Char('G')`
    // and drop the SHIFT modifier (crossterm reports uppercase chars without SHIFT on most platforms).
    let (code, mods) = normalize_shift(code, mods);

    Ok((code, mods))
}

fn parse_keycode(s: &str) -> Result<KeyCode, String> {
    let lower = s.to_lowercase();
    Ok(match lower.as_str() {
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" | "shift-tab" => KeyCode::BackTab,
        "space" => KeyCode::Char(' '),
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        f if f.starts_with('f') && f.len() >= 2 => {
            let n: u8 = f[1..]
                .parse()
                .map_err(|_| format!("invalid function key '{s}'"))?;
            if (1..=12).contains(&n) {
                KeyCode::F(n)
            } else {
                return Err(format!("function key out of range: '{s}'"));
            }
        }
        _ => {
            // Single character (preserve original case for letter shift detection)
            let mut chars = s.chars();
            let c = chars.next().ok_or_else(|| format!("empty key '{s}'"))?;
            if chars.next().is_some() {
                return Err(format!("unrecognized key '{s}'"));
            }
            KeyCode::Char(c)
        }
    })
}

fn normalize_shift(code: KeyCode, mods: KeyModifiers) -> (KeyCode, KeyModifiers) {
    if let KeyCode::Char(c) = code {
        if mods.contains(KeyModifiers::SHIFT) && c.is_ascii_alphabetic() {
            return (
                KeyCode::Char(c.to_ascii_uppercase()),
                mods - KeyModifiers::SHIFT,
            );
        }
        if c.is_ascii_uppercase() {
            // Already uppercase — strip SHIFT if present, leave char as-is.
            return (KeyCode::Char(c), mods - KeyModifiers::SHIFT);
        }
    }
    (code, mods)
}

/// Map a stable string action ID to an `Action`. Returns `None` for unknown IDs.
pub fn action_from_id(id: &str) -> Option<Action> {
    Some(match id {
        "exit_app" => Action::ExitApp,
        "close_doc" => Action::CloseDoc,
        "open_browser" => Action::OpenBrowser,
        "scroll_up" => Action::ScrollUp(1),
        "scroll_down" => Action::ScrollDown(1),
        "scroll_up_fast" => Action::ScrollUp(10),
        "scroll_down_fast" => Action::ScrollDown(10),
        "page_up" => Action::PageUp,
        "page_down" => Action::PageDown,
        "home" => Action::Home,
        "end" => Action::End,
        "next_heading" => Action::NextHeading,
        "prev_heading" => Action::PrevHeading,
        "next_tab" => Action::NextTab,
        "prev_tab" => Action::PrevTab,
        "toggle_toc" => Action::ToggleToc,
        "search" => Action::Search,
        "theme_picker" => Action::ThemePicker,
        "follow_link" => Action::FollowLink,
        "nav_back" => Action::NavBack,
        "nav_forward" => Action::NavForward,
        _ => return None,
    })
}

/// Resolve the active key map from a preset + per-action overrides.
///
/// Strategy: build a `(KeyBinding -> action_id)` map from the preset,
/// then for each action in `overrides` REMOVE all preset bindings for that action
/// and REPLACE with the override list. Last writer wins on cross-action collisions
/// (a stderr warning is printed by the caller).
pub fn build_keymap(
    preset: &[(&str, &[&str])],
    overrides: Option<&HashMap<String, Vec<String>>>,
) -> (HashMap<KeyBinding, Action>, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();
    // First, gather effective per-action key strings.
    let mut effective: HashMap<String, Vec<String>> = preset
        .iter()
        .map(|(id, keys)| {
            (
                (*id).to_string(),
                keys.iter().map(|s| (*s).to_string()).collect(),
            )
        })
        .collect();

    if let Some(over) = overrides {
        for (id, keys) in over {
            if action_from_id(id).is_none() {
                warnings.push(format!("unknown action id in config: '{id}'"));
                continue;
            }
            effective.insert(id.clone(), keys.clone());
        }
    }

    let mut map: HashMap<KeyBinding, Action> = HashMap::new();
    let mut bound_to: HashMap<KeyBinding, String> = HashMap::new();

    for (id, keys) in &effective {
        let Some(action) = action_from_id(id) else {
            continue;
        };
        for key_str in keys {
            match parse_key(key_str) {
                Ok(kb) => {
                    if let Some(prev) = bound_to.get(&kb) {
                        if prev != id {
                            warnings.push(format!(
                                "keybinding conflict: '{key_str}' bound to both '{prev}' and '{id}' — using '{id}'"
                            ));
                        }
                    }
                    bound_to.insert(kb, id.clone());
                    map.insert(kb, action.clone());
                }
                Err(e) => {
                    warnings.push(format!("invalid key '{key_str}' for '{id}': {e}"));
                }
            }
        }
    }

    (map, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_char() {
        assert_eq!(
            parse_key("q"),
            Ok((KeyCode::Char('q'), KeyModifiers::empty()))
        );
    }

    #[test]
    fn parses_ctrl_combo() {
        assert_eq!(
            parse_key("ctrl-c"),
            Ok((KeyCode::Char('c'), KeyModifiers::CONTROL))
        );
    }

    #[test]
    fn parses_alt_arrow() {
        assert_eq!(
            parse_key("alt-down"),
            Ok((KeyCode::Down, KeyModifiers::ALT))
        );
    }

    #[test]
    fn shift_letter_normalizes_to_uppercase() {
        // shift-g should equal G with no SHIFT modifier (matches crossterm event reporting).
        assert_eq!(
            parse_key("shift-g"),
            Ok((KeyCode::Char('G'), KeyModifiers::empty()))
        );
    }

    #[test]
    fn parses_named_keys() {
        assert_eq!(parse_key("esc"), Ok((KeyCode::Esc, KeyModifiers::empty())));
        assert_eq!(
            parse_key("space"),
            Ok((KeyCode::Char(' '), KeyModifiers::empty()))
        );
        assert_eq!(
            parse_key("pagedown"),
            Ok((KeyCode::PageDown, KeyModifiers::empty()))
        );
    }

    #[test]
    fn invalid_returns_err() {
        assert!(parse_key("").is_err());
        assert!(parse_key("flarg-x").is_err());
        assert!(parse_key("ctrl-flarp").is_err());
    }

    #[test]
    fn action_from_id_known() {
        assert_eq!(action_from_id("exit_app"), Some(Action::ExitApp));
        assert_eq!(action_from_id("scroll_down"), Some(Action::ScrollDown(1)));
        assert_eq!(
            action_from_id("scroll_down_fast"),
            Some(Action::ScrollDown(10))
        );
        assert_eq!(action_from_id("nope"), None);
    }
}
