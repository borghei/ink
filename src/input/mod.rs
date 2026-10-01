pub mod keymap;
pub mod preset;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
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
    Help,
    LinkMode,
    LinkHint(char),
    SlideNext,
    SlidePrev,

    // Selection & clipboard
    /// Enter character-wise visual mode.
    SelectMode,
    /// Enter (or, inside visual mode, toggle) line-wise visual mode.
    SelectLineMode,
    /// Copy the selection and leave visual mode.
    Yank,
    /// Leave visual mode without copying.
    SelCancel,
    /// Move the visual-mode cursor by whole lines / columns.
    SelUp(u16),
    SelDown(u16),
    SelLeft(u16),
    SelRight(u16),
    SelWordNext,
    SelWordPrev,
    SelLineStart,
    SelLineEnd,
    SelDocStart,
    SelDocEnd,
    SelPageDown,
    SelPageUp,
    /// Label the visible code blocks for copying.
    CopyCode,
    /// Copy the section the viewport starts in, as markdown source.
    CopySection,
    /// Inside the link-hint overlay: copy the URL instead of opening it.
    HintCopyToggle,

    /// Move keyboard focus into the table of contents (opening it).
    TocFocus,
    /// Open the current file in `$VISUAL` / `$EDITOR`.
    Edit,
    /// A key for the focused table of contents.
    Toc(crate::toc::TocKey),

    /// Mouse press / drag / release at a (column, row) on screen.
    MouseDown(u16, u16),
    MouseDrag(u16, u16),
    MouseUp(u16, u16),
    /// Mouse wheel at a (column, row): the reader scrolls whatever is under
    /// the pointer (the document, or the TOC sidebar).
    WheelUp(u16, u16),
    WheelDown(u16, u16),

    None,
}

/// Which key-mapping regime `poll_action` should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal document navigation (the configured keymap).
    Normal,
    /// Search input line — keys become query text.
    Search,
    /// Link-hint overlay — letter keys pick a link, Esc cancels.
    LinkHint,
    /// Presentation mode — arrows/space move between slides.
    Slides,
    /// Visual selection — motions move the cursor, `y` copies.
    Visual,
    /// The table of contents has focus — motions move its cursor.
    Toc,
    /// Typing a filter into the focused table of contents.
    TocFilter,
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

/// A readable summary of the active keymap, grouped by action, for the help
/// overlay and the `ink keybindings` subcommand. Returns `(label, keys)`
/// pairs in a curated display order.
pub fn keymap_summary() -> Vec<(&'static str, Vec<String>)> {
    use std::collections::BTreeMap;
    let Some(km) = KEYMAP.get() else {
        return Vec::new();
    };

    let mut by_action: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for ((code, mods), action) in km.singles.iter() {
        by_action
            .entry(action_label(action))
            .or_default()
            .push(fmt_key(code, mods));
    }
    for (prefix, sub) in km.chord_prefixes.iter() {
        for (second, action) in sub.iter() {
            by_action
                .entry(action_label(action))
                .or_default()
                .push(format!(
                    "{} {}",
                    fmt_key(&prefix.0, &prefix.1),
                    fmt_key(&second.0, &second.1)
                ));
        }
    }

    // Curated order for the overlay; unknown/other actions are dropped.
    const ORDER: &[&str] = &[
        "scroll",
        "page",
        "top / bottom",
        "next / prev heading",
        "search",
        "table of contents",
        "focus contents",
        "follow link",
        "link hints",
        "select text",
        "copy code block",
        "copy section",
        "edit in $EDITOR",
        "theme picker",
        "tabs",
        "back / forward",
        "help",
        "quit",
    ];
    let mut out = Vec::new();
    for label in ORDER {
        if let Some(mut keys) = by_action.remove(label) {
            keys.sort();
            keys.dedup();
            out.push((*label, keys));
        }
    }
    out
}

fn action_label(a: &Action) -> &'static str {
    match a {
        Action::ScrollUp(1) | Action::ScrollDown(1) => "scroll",
        Action::ScrollUp(_) | Action::ScrollDown(_) => "scroll",
        Action::PageUp | Action::PageDown => "page",
        Action::Home | Action::End => "top / bottom",
        Action::NextHeading | Action::PrevHeading => "next / prev heading",
        Action::Search => "search",
        Action::ToggleToc => "table of contents",
        Action::TocFocus => "focus contents",
        Action::FollowLink => "follow link",
        Action::LinkMode => "link hints",
        Action::SelectMode | Action::SelectLineMode => "select text",
        Action::CopyCode => "copy code block",
        Action::CopySection => "copy section",
        Action::Edit => "edit in $EDITOR",
        Action::ThemePicker => "theme picker",
        Action::NextTab | Action::PrevTab => "tabs",
        Action::NavBack | Action::NavForward => "back / forward",
        Action::Help => "help",
        Action::ExitApp => "quit",
        _ => "other",
    }
}

fn fmt_key(code: &KeyCode, mods: &KeyModifiers) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if mods.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl");
    }
    if mods.contains(KeyModifiers::ALT) {
        parts.push("alt");
    }
    if mods.contains(KeyModifiers::SHIFT) {
        parts.push("shift");
    }
    let key = match code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "backtab".into(),
        KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
            if crate::glyphs::current().ascii =>
        {
            format!("{code:?}").to_lowercase()
        }
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        KeyCode::PageUp => "pgup".into(),
        KeyCode::PageDown => "pgdn".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::Backspace => "bksp".into(),
        other => format!("{other:?}").to_lowercase(),
    };
    if parts.is_empty() {
        key
    } else {
        format!("{}-{}", parts.join("-"), key)
    }
}

/// Whether a key event should drive an action.
///
/// crossterm on Windows reports both the press and the release of every key
/// (plus repeats), while Unix terminals only ever deliver presses. Acting on
/// releases made every key fire twice there. Repeats are kept: holding `j`
/// should keep scrolling.
pub fn is_actionable_key(key: &KeyEvent) -> bool {
    key.kind != KeyEventKind::Release
}

/// Events read ahead of the event loop by [`discard_late_reply`] that were
/// real input, served before anything new is read.
static PENDING: std::sync::Mutex<std::collections::VecDeque<Event>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// Drop a background-colour reply that reached the input queue after the
/// startup query gave up on it (see `theme::detect::reply_may_arrive_late`).
/// Reads only what is already buffered; anything that is not part of the
/// reply is kept and handed to the event loop in order. crossterm already
/// swallows the DA1 answer (`ESC [ ? … c`) itself; the OSC 11 answer it
/// would turn into key presses (`Alt-]`, `1`, `1`, `;`, …).
pub fn discard_late_reply() {
    let mut events = Vec::new();
    while event::poll(std::time::Duration::ZERO).unwrap_or(false) {
        match event::read() {
            Ok(e) => events.push(e),
            Err(_) => break,
        }
    }
    let kept = strip_leading_osc_reply(events);
    if let Ok(mut pending) = PENDING.lock() {
        pending.extend(kept);
    }
}

/// Remove a leading OSC 11 reply (`ESC ] 11 ; … BEL` or `… ESC \`), as
/// crossterm decodes it into key events, from `events`. A reply cut off
/// before its terminator is removed too. Anything else is left alone.
fn strip_leading_osc_reply(events: Vec<Event>) -> Vec<Event> {
    // Windows reports key releases too; they carry no information here.
    let presses: Vec<(usize, KeyCode, KeyModifiers)> = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Event::Key(k) if k.kind != KeyEventKind::Release => Some((i, k.code, k.modifiers)),
            _ => None,
        })
        .collect();
    let starts_reply = matches!(presses.first(), Some((_, KeyCode::Char(']'), m)) if m.contains(KeyModifiers::ALT));
    if !starts_reply {
        return events;
    }
    // `11;` must follow, as far as the input goes.
    let prefix: Vec<char> = presses[1..]
        .iter()
        .take(3)
        .map(|(_, c, _)| match c {
            KeyCode::Char(c) => *c,
            _ => '\0',
        })
        .collect();
    if !"11;".chars().zip(&prefix).all(|(a, b)| a == *b) {
        return events;
    }
    let end = presses.iter().skip(1).find_map(|(i, c, m)| match c {
        KeyCode::Char('g') if m.contains(KeyModifiers::CONTROL) => Some(*i),
        KeyCode::Char('\\') if m.contains(KeyModifiers::ALT) => Some(*i),
        _ => None,
    });
    match end {
        Some(i) => events.into_iter().skip(i + 1).collect(),
        // Cut off: only the reply's own characters may follow.
        None if presses[1..].iter().all(|(_, c, m)| {
            matches!(c, KeyCode::Char(c) if c.is_ascii_alphanumeric() || ":/;#?".contains(*c))
                && !m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        }) =>
        {
            events
                .into_iter()
                .filter(|e| !matches!(e, Event::Key(_)))
                .collect()
        }
        None => events,
    }
}

pub fn poll_action(timeout: std::time::Duration, mode: InputMode) -> Option<Action> {
    let queued = PENDING.lock().ok().and_then(|mut p| p.pop_front());
    if let Some(event) = queued {
        return Some(map_event(event, mode));
    }
    if event::poll(timeout).ok()? {
        let event = event::read().ok()?;
        if let Event::Key(key) = &event {
            if !is_actionable_key(key) {
                return None;
            }
        }
        Some(map_event(event, mode))
    } else {
        None
    }
}

fn map_event(event: Event, mode: InputMode) -> Action {
    match event {
        Event::Key(key) if !is_actionable_key(&key) => Action::None,
        Event::Key(key) => match mode {
            InputMode::Search => map_search_key(key),
            InputMode::LinkHint => map_link_hint_key(key),
            InputMode::Slides => map_slides_key(key),
            InputMode::Visual => map_visual_key(key),
            InputMode::Toc => map_toc_key(key),
            InputMode::TocFilter => map_toc_filter_key(key),
            InputMode::Normal => map_key(key),
        },
        Event::Mouse(mouse) => map_mouse(mouse),
        Event::Resize(w, h) => Action::Resize(w, h),
        _ => Action::None,
    }
}

fn map_link_hint_key(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::CloseSearch,
        // Labels are always lowercase, so an uppercase Y is free to mean
        // "copy this one instead of opening it".
        KeyCode::Char('Y') => Action::HintCopyToggle,
        KeyCode::Char(c) if c.is_ascii_alphabetic() => Action::LinkHint(c.to_ascii_lowercase()),
        _ => Action::None,
    }
}

/// Visual-mode keys.
///
/// Fixed, like Search and Slides mode: motions here are a self-contained
/// vim-shaped table rather than the configurable keymap, and arrow keys cover
/// anyone who does not think in `hjkl`.
fn map_visual_key(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => Action::SelCancel,
            KeyCode::Char('d') | KeyCode::Char('f') => Action::SelPageDown,
            KeyCode::Char('u') | KeyCode::Char('b') => Action::SelPageUp,
            _ => Action::None,
        };
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => Action::SelCancel,
        KeyCode::Char('y') | KeyCode::Enter => Action::Yank,
        KeyCode::Char('v') => Action::SelectMode,
        KeyCode::Char('V') => Action::SelectLineMode,
        KeyCode::Char('j') | KeyCode::Down => Action::SelDown(1),
        KeyCode::Char('k') | KeyCode::Up => Action::SelUp(1),
        KeyCode::Char('h') | KeyCode::Left => Action::SelLeft(1),
        KeyCode::Char('l') | KeyCode::Right => Action::SelRight(1),
        KeyCode::Char('w') | KeyCode::Char('e') => Action::SelWordNext,
        KeyCode::Char('b') => Action::SelWordPrev,
        KeyCode::Char('0') | KeyCode::Home => Action::SelLineStart,
        KeyCode::Char('$') | KeyCode::End => Action::SelLineEnd,
        KeyCode::Char('g') => Action::SelDocStart,
        KeyCode::Char('G') => Action::SelDocEnd,
        KeyCode::Char(' ') | KeyCode::PageDown => Action::SelPageDown,
        KeyCode::PageUp => Action::SelPageUp,
        _ => Action::None,
    }
}

/// Keys while the table of contents has focus.
///
/// Fixed, like visual mode: a small vim-shaped table plus arrows. The
/// configured `toc_focus` key (and `toggle_toc`, which closes the sidebar)
/// still work here so the key that came in also goes out.
fn map_toc_key(key: KeyEvent) -> Action {
    use crate::toc::TocKey as T;
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        let fixed = match key.code {
            KeyCode::Char('c') => Some(T::Leave),
            KeyCode::Char('d') | KeyCode::Char('f') => Some(T::PageDown),
            KeyCode::Char('u') | KeyCode::Char('b') => Some(T::PageUp),
            _ => None,
        };
        if let Some(k) = fixed {
            return Action::Toc(k);
        }
    } else {
        let fixed = match key.code {
            KeyCode::Char('j') | KeyCode::Down => Some(T::Down),
            KeyCode::Char('k') | KeyCode::Up => Some(T::Up),
            KeyCode::Char('g') | KeyCode::Home => Some(T::First),
            KeyCode::Char('G') | KeyCode::End => Some(T::Last),
            KeyCode::PageDown | KeyCode::Char(' ') => Some(T::PageDown),
            KeyCode::PageUp => Some(T::PageUp),
            KeyCode::Enter => Some(T::Jump),
            KeyCode::Esc | KeyCode::Char('q') => Some(T::Leave),
            KeyCode::Char('/') => Some(T::StartFilter),
            KeyCode::Char('h') | KeyCode::Left => Some(T::Collapse),
            KeyCode::Char('l') | KeyCode::Right => Some(T::Expand),
            _ => None,
        };
        if let Some(k) = fixed {
            return Action::Toc(k);
        }
    }
    let kb = normalize_event(key);
    match KEYMAP.get().and_then(|km| km.singles.get(&kb)) {
        Some(Action::TocFocus) => Action::Toc(T::Leave),
        Some(Action::ToggleToc) => Action::ToggleToc,
        Some(Action::Help) => Action::Help,
        _ => Action::None,
    }
}

/// Keys while typing a TOC filter: text goes into the filter, arrows move,
/// Enter jumps, Esc clears.
fn map_toc_filter_key(key: KeyEvent) -> Action {
    use crate::toc::TocKey as T;
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => Action::Toc(T::ClearFilter),
            _ => Action::None,
        };
    }
    match key.code {
        KeyCode::Esc => Action::Toc(T::ClearFilter),
        KeyCode::Enter => Action::Toc(T::Jump),
        KeyCode::Backspace => Action::Toc(T::FilterBack),
        KeyCode::Down => Action::Toc(T::Down),
        KeyCode::Up => Action::Toc(T::Up),
        KeyCode::PageDown => Action::Toc(T::PageDown),
        KeyCode::PageUp => Action::Toc(T::PageUp),
        KeyCode::Char(c) => Action::Toc(T::FilterChar(c)),
        _ => Action::None,
    }
}

fn map_slides_key(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::ExitApp;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::ExitApp,
        KeyCode::Right
        | KeyCode::Char(' ')
        | KeyCode::Char('l')
        | KeyCode::Char('n')
        | KeyCode::PageDown
        | KeyCode::Enter => Action::SlideNext,
        KeyCode::Left
        | KeyCode::Backspace
        | KeyCode::Char('h')
        | KeyCode::Char('p')
        | KeyCode::PageUp => Action::SlidePrev,
        KeyCode::Down | KeyCode::Char('j') => Action::ScrollDown(1),
        KeyCode::Up | KeyCode::Char('k') => Action::ScrollUp(1),
        KeyCode::Char('t') => Action::ThemePicker,
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
    use crossterm::event::MouseButton;
    match mouse.kind {
        MouseEventKind::ScrollUp => Action::WheelUp(mouse.column, mouse.row),
        MouseEventKind::ScrollDown => Action::WheelDown(mouse.column, mouse.row),
        MouseEventKind::Down(MouseButton::Left) => Action::MouseDown(mouse.column, mouse.row),
        MouseEventKind::Drag(MouseButton::Left) => Action::MouseDrag(mouse.column, mouse.row),
        MouseEventKind::Up(MouseButton::Left) => Action::MouseUp(mouse.column, mouse.row),
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn key(kind: KeyEventKind) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char('j'),
            modifiers: KeyModifiers::NONE,
            kind,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn press_and_repeat_are_actionable_release_is_not() {
        assert!(is_actionable_key(&key(KeyEventKind::Press)));
        assert!(is_actionable_key(&key(KeyEventKind::Repeat)));
        assert!(!is_actionable_key(&key(KeyEventKind::Release)));
    }

    #[test]
    fn a_release_maps_to_no_action_in_every_mode() {
        for mode in [
            InputMode::Normal,
            InputMode::Search,
            InputMode::LinkHint,
            InputMode::Slides,
            InputMode::Visual,
            InputMode::Toc,
            InputMode::TocFilter,
        ] {
            let ev = Event::Key(key(KeyEventKind::Release));
            assert_eq!(map_event(ev, mode), Action::None);
        }
    }

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn the_focused_toc_has_its_own_motions() {
        use crate::toc::TocKey as T;
        let none = KeyModifiers::NONE;
        let cases = [
            (KeyCode::Char('j'), none, T::Down),
            (KeyCode::Up, none, T::Up),
            (KeyCode::Char('G'), KeyModifiers::SHIFT, T::Last),
            (KeyCode::Char('d'), KeyModifiers::CONTROL, T::PageDown),
            (KeyCode::PageUp, none, T::PageUp),
            (KeyCode::Enter, none, T::Jump),
            (KeyCode::Esc, none, T::Leave),
            (KeyCode::Char('/'), none, T::StartFilter),
            (KeyCode::Char('h'), none, T::Collapse),
            (KeyCode::Right, none, T::Expand),
        ];
        for (code, mods, want) in cases {
            assert_eq!(
                map_toc_key(press(code, mods)),
                Action::Toc(want),
                "{code:?}"
            );
        }
        // While filtering, letters are text, not motions.
        assert_eq!(
            map_toc_filter_key(press(KeyCode::Char('j'), none)),
            Action::Toc(T::FilterChar('j'))
        );
        assert_eq!(
            map_toc_filter_key(press(KeyCode::Esc, none)),
            Action::Toc(T::ClearFilter)
        );
        assert_eq!(
            map_toc_filter_key(press(KeyCode::Enter, none)),
            Action::Toc(T::Jump)
        );
    }

    #[test]
    fn toc_focus_is_bound_and_rebindable() {
        use crate::input::keymap::parse_key;
        for preset in [preset::DEFAULT, preset::EMACS] {
            let (km, warnings) = build_keymap(preset, None);
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(
                km.singles.get(&parse_key("o").unwrap()),
                Some(&Action::TocFocus)
            );
        }
        let overrides = HashMap::from([("toc_focus".to_string(), vec!["ctrl-o".to_string()])]);
        let (km, warnings) = build_keymap(preset::DEFAULT, Some(&overrides));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            km.singles.get(&parse_key("ctrl-o").unwrap()),
            Some(&Action::TocFocus)
        );
        assert_eq!(km.singles.get(&parse_key("o").unwrap()), None);
    }

    #[test]
    fn the_wheel_reports_where_the_pointer_is() {
        use crossterm::event::MouseEvent;
        let wheel = |kind| MouseEvent {
            kind,
            column: 7,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            map_mouse(wheel(MouseEventKind::ScrollUp)),
            Action::WheelUp(7, 3)
        );
        assert_eq!(
            map_mouse(wheel(MouseEventKind::ScrollDown)),
            Action::WheelDown(7, 3)
        );
    }

    /// Bytes as crossterm decodes them into key events (`ESC x` → Alt-x,
    /// BEL → Ctrl-g).
    fn typed(bytes: &[u8]) -> Vec<Event> {
        let mut out = Vec::new();
        let mut alt = false;
        for &b in bytes {
            let (code, mut mods) = match b {
                0x1b => {
                    alt = true;
                    continue;
                }
                0x07 => (KeyCode::Char('g'), KeyModifiers::CONTROL),
                b if b.is_ascii_uppercase() => (KeyCode::Char(b as char), KeyModifiers::SHIFT),
                b => (KeyCode::Char(b as char), KeyModifiers::NONE),
            };
            if std::mem::take(&mut alt) {
                mods |= KeyModifiers::ALT;
            }
            out.push(Event::Key(KeyEvent::new(code, mods)));
        }
        out
    }

    #[test]
    fn a_late_osc_11_reply_is_dropped_and_real_keys_are_kept() {
        // BEL-terminated, followed by a real `j`.
        let kept = strip_leading_osc_reply(typed(b"\x1b]11;rgb:ffff/FFFF/ffff\x07j"));
        assert_eq!(kept, typed(b"j"));
        // ST-terminated.
        let kept = strip_leading_osc_reply(typed(b"\x1b]11;#1e1e2e\x1b\\q"));
        assert_eq!(kept, typed(b"q"));
        // Cut off before the terminator: all of it is reply.
        assert!(strip_leading_osc_reply(typed(b"\x1b]11;rgb:ff")).is_empty());
        assert!(strip_leading_osc_reply(typed(b"\x1b]1")).is_empty());
        // Ordinary input is left alone.
        assert_eq!(strip_leading_osc_reply(typed(b"jjq")), typed(b"jjq"));
        assert_eq!(strip_leading_osc_reply(typed(b"\x1b]x")), typed(b"\x1b]x"));
        assert_eq!(
            strip_leading_osc_reply(typed(b"q\x1b]11;")),
            typed(b"q\x1b]11;")
        );
        assert!(strip_leading_osc_reply(Vec::new()).is_empty());
    }
}
