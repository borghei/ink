pub mod keymap;
pub mod preset;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::config::KeybindingsConfig;
use crate::input::keymap::{build_keymap, KeyBinding, ResolvedKeymap};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    ExitApp,
    CloseDoc,
    OpenBrowser,
    ScrollUp(u16),
    ScrollDown(u16),
    PageUp,
    PageDown,
    Home,
    End,
    ToggleToc,
    Search,
    SearchNext,
    SearchPrev,
    CloseSearch,
    SearchInput(char),
    SearchBackspace,
    SearchConfirm,
    Resize(u16, u16),
    NextHeading,
    PrevHeading,
    NextTab,
    PrevTab,
    FollowLink,
    NavBack,
    NavForward,
    ThemePicker,
    None,
}

/// Process-wide resolved keymap. Initialized once at startup via `init_keymap`.
static KEYMAP: OnceLock<ResolvedKeymap> = OnceLock::new();

/// Pending chord prefix — set when a key matches a chord prefix, cleared on the next key.
/// Mutex<Option> over thread_local because the input loop is on the main thread; this also
/// keeps the API simple for `current_pending_chord()` if we ever want to display it.
static PENDING_CHORD: Mutex<Option<KeyBinding>> = Mutex::new(None);

/// Initialize the global keymap from config. Must be called before `poll_action`.
/// Prints any keybinding warnings to stderr.
pub fn init_keymap(cfg: Option<&KeybindingsConfig>) {
    let preset_name = cfg.and_then(|k| k.preset.as_deref()).unwrap_or("default");
    let preset = preset::lookup(preset_name).unwrap_or_else(|| {
        eprintln!("ink: unknown keybinding preset '{preset_name}', falling back to default");
        preset::DEFAULT
    });
    let overrides = cfg.and_then(|k| k.bindings.as_ref());
    let (map, warnings) = build_keymap(preset, overrides);
    for w in warnings {
        eprintln!("ink: {w}");
    }
    let _ = KEYMAP.set(map);
}

/// Read-only access to the resolved keymap, for `ink keybindings` subcommand and tests.
pub fn current_keymap() -> Option<&'static HashMap<KeyBinding, Action>> {
    KEYMAP.get().map(|r| &r.singles)
}

/// Iterate over chord bindings as flat (prefix, second_key, action) tuples — for `ink keybindings`.
pub fn current_chords() -> Vec<(KeyBinding, KeyBinding, Action)> {
    let Some(km) = KEYMAP.get() else {
        return Vec::new();
    };
    km.chord_prefixes
        .iter()
        .flat_map(|(prefix, sub)| {
            sub.iter()
                .map(move |(second, action)| (*prefix, *second, action.clone()))
        })
        .collect()
}

pub fn poll_action(timeout: std::time::Duration, search_active: bool) -> Option<Action> {
    if event::poll(timeout).ok()? {
        let event = event::read().ok()?;
        Some(map_event(event, search_active))
    } else {
        None
    }
}

fn map_event(event: Event, search_active: bool) -> Action {
    match event {
        Event::Key(key) => {
            if search_active {
                map_search_key(key)
            } else {
                map_key(key)
            }
        }
        Event::Mouse(mouse) => map_mouse(mouse),
        Event::Resize(w, h) => Action::Resize(w, h),
        _ => Action::None,
    }
}

fn map_search_key(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::CloseSearch;
    }
    match key.code {
        KeyCode::Esc => Action::CloseSearch,
        KeyCode::Enter => Action::SearchConfirm,
        KeyCode::Backspace => Action::SearchBackspace,
        KeyCode::Char(c) => Action::SearchInput(c),
        KeyCode::Down => Action::SearchNext,
        KeyCode::Up => Action::SearchPrev,
        _ => Action::None,
    }
}

fn map_key(key: KeyEvent) -> Action {
    // Normalize to (KeyCode, KeyModifiers) — strip SHIFT on already-uppercase letters
    // so `shift-g` (parsed to ('G', empty)) matches a real `Shift-G` press.
    let kb = normalize_event(key);
    let Some(km) = KEYMAP.get() else {
        return Action::None;
    };

    // Chord state: if a prefix is pending, try to complete it with this key.
    // On match → fire the chord action; on miss → drop the pending prefix and
    // dispatch this key normally (so the user isn't stuck if they pressed C-x by accident).
    let pending = PENDING_CHORD.lock().ok().and_then(|mut g| g.take());
    if let Some(prefix) = pending {
        if let Some(sub) = km.chord_prefixes.get(&prefix) {
            if let Some(action) = sub.get(&kb) {
                return action.clone();
            }
        }
        // Fall through: re-dispatch `kb` as a fresh keypress.
    }

    // No pending prefix. If this key starts a chord, stash and wait for the next key.
    // Single bindings take precedence — this matches Emacs (single key bound = wins).
    if let Some(action) = km.singles.get(&kb) {
        return action.clone();
    }
    if km.chord_prefixes.contains_key(&kb) {
        if let Ok(mut g) = PENDING_CHORD.lock() {
            *g = Some(kb);
        }
        return Action::None;
    }

    Action::None
}

fn normalize_event(key: KeyEvent) -> (KeyCode, KeyModifiers) {
    let code = key.code;
    let mut mods = key.modifiers;
    if let KeyCode::Char(c) = code {
        if c.is_ascii_uppercase() {
            mods.remove(KeyModifiers::SHIFT);
        }
    }
    (code, mods)
}

fn map_mouse(mouse: MouseEvent) -> Action {
    match mouse.kind {
        MouseEventKind::ScrollUp => Action::ScrollUp(3),
        MouseEventKind::ScrollDown => Action::ScrollDown(3),
        _ => Action::None,
    }
}
