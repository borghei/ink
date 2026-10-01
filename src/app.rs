use crate::clipboard::{self, ClipboardMode, CopyOutcome};
use crate::input::{self, Action};
use crate::layout;
use crate::parser::frontmatter;
use crate::render;
use crate::search::SearchState;
use crate::selection::{self, Pos, SelMode, Selection};
use crate::stats;
use crate::theme;
use crate::toc::TocState;
use crate::Args;
use anyhow::Result;
use comrak::{parse_document, Arena};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

struct Tab {
    filename: String,
    /// The document as loaded. Shared, not copied, with every history entry
    /// made from this tab: an in-page jump records a pointer.
    source: Arc<str>,
    /// Modification time of `filename` when `source` was read from it; `None`
    /// for stdin and URL documents. Going back to a file that has changed
    /// since re-reads it.
    mtime: Option<SystemTime>,
    /// The markdown actually parsed: frontmatter stripped, wikilinks expanded.
    /// Heading `source_line`s index into this, so `Y` slices sections from it.
    content: String,
    /// Lines of `source` above `content` (a stripped frontmatter block), so a
    /// `content` line maps back to a line of the file; `None` when that
    /// mapping is not known to hold.
    line_offset: Option<usize>,
    styled_lines: Vec<crate::layout::StyledLine>,
    ratatui_lines: Vec<Line<'static>>,
    /// Per-line rendered text. Selection columns, clipboard extraction, and
    /// hint placement all measure against these.
    plain: Vec<String>,
    /// Fenced code blocks and their raw source, for `c` (copy code block).
    code_blocks: Vec<crate::layout::CodeBlockSpec>,
    /// Per-line text, lowercased once, for allocation-free search scans.
    lowered: Vec<String>,
    scroll_offset: usize,
    toc: TocState,
    /// Every top-level heading's anchor text (`LayoutHeading::anchor`) and
    /// display line, in order, for `#fragment` links. Unlike the TOC this
    /// keeps headings whose display text is empty (``## `--width` ``).
    anchors: Vec<(String, usize)>,
    word_count: usize,
    reading_time: usize,
    /// Terminal width + theme generation this tab was laid out for. Used to
    /// rebuild a tab lazily (only when it becomes visible under new conditions)
    /// instead of rebuilding every tab on every resize / theme change.
    built_width: u16,
    built_gen: u32,
    /// Graphics-protocol images placed in the text flow (empty in half-block
    /// mode). Painted over their reserved blank rows by the draw loop.
    images: Vec<ImagePlacement>,
}

/// A decoded image positioned in the document for graphics-protocol rendering.
struct ImagePlacement {
    /// First document line the image occupies (its reserved blank rows).
    line_index: usize,
    /// Left column offset (content margin) within the document area.
    col_offset: u16,
    /// Reserved height in rows.
    rows: u16,
    /// `None` when the protocol encode failed — the draw loop then writes a
    /// visible notice into the reserved rows instead of leaving a silent gap.
    protocol: Option<ratatui_image::sliced::SlicedProtocol>,
}

/// A place to return to with `[` / `]`.
///
/// Carries the source that was on screen (shared with the tab, not copied),
/// so going back to stdin or a URL document (or a file that has since moved)
/// rebuilds what was shown instead of trying to re-read the name as a local
/// path. A local file that has changed since is re-read instead.
#[derive(Debug, Clone, PartialEq)]
struct NavEntry {
    filename: String,
    source: Arc<str>,
    mtime: Option<SystemTime>,
    scroll_offset: usize,
}

impl NavEntry {
    fn of(tab: &Tab) -> Self {
        Self {
            filename: tab.filename.clone(),
            source: Arc::clone(&tab.source),
            mtime: tab.mtime,
            scroll_offset: tab.scroll_offset,
        }
    }
}

/// How many places `[` can go back to; the oldest are dropped beyond it.
const NAV_HISTORY_CAP: usize = 256;

/// Back/forward stacks shared by every way of following a link.
#[derive(Debug, Default)]
struct NavHistory {
    back: Vec<NavEntry>,
    forward: Vec<NavEntry>,
}

impl NavHistory {
    /// Remember `here` before moving somewhere new; a new branch drops the
    /// forward stack, as in a browser.
    fn record(&mut self, here: NavEntry) {
        self.push_back(here);
        self.forward.clear();
    }

    /// Step back from `here`, returning where to go.
    fn go_back(&mut self, here: NavEntry) -> Option<NavEntry> {
        let to = self.back.pop()?;
        self.forward.push(here);
        Some(to)
    }

    /// Step forward from `here`, returning where to go.
    fn go_forward(&mut self, here: NavEntry) -> Option<NavEntry> {
        let to = self.forward.pop()?;
        self.push_back(here);
        Some(to)
    }

    fn push_back(&mut self, here: NavEntry) {
        if self.back.len() >= NAV_HISTORY_CAP {
            self.back.remove(0);
        }
        self.back.push(here);
    }
}

/// A labeled link in the current viewport for hint-mode selection.
struct LinkHint {
    label: char,
    url: String,
    /// The link's visible text, whitespace-collapsed. Empty when it would
    /// only repeat the URL (autolinks, bare URLs).
    caption: String,
}

/// What the letters in the link-hint overlay do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HintKind {
    /// Open the link (the `f` default).
    Open,
    /// Copy the URL to the clipboard (`Y` inside the overlay).
    CopyUrl,
}

/// A labeled code block in the current viewport, for `c` (copy code block).
struct CodeHint {
    label: char,
    /// Screen row (relative to the document area) to paint the label on.
    row: u16,
    /// Column to paint it at — the right end of the block's top border.
    col: u16,
    lang: String,
    source: String,
}

/// Available themes for the theme picker.
const THEME_LIST: &[&str] = &[
    "dark",
    "light",
    "dracula",
    "catppuccin",
    "nord",
    "tokyo-night",
    "gruvbox",
    "solarized",
];

/// How the document viewer exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppExit {
    /// User wants to terminate ink entirely.
    Quit,
    /// User wants to return to the file browser to pick another file.
    BackToBrowser,
}

/// Put the terminal back the way the shell expects it: cooked mode, main
/// screen, no mouse reporting, visible cursor.
///
/// The single restore sequence shared by normal exit, the panic hook and the
/// signal path, in both the reader and the file browser. Every step is
/// attempted even if an earlier one fails; the first error is returned.
pub(crate) fn restore_terminal() -> io::Result<()> {
    let raw = disable_raw_mode();
    let screen = execute!(
        io::stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
    raw.and(screen)
}

/// Take the terminal over for the TUI: raw mode, alternate screen, and mouse
/// reporting when `mouse_capture` is on. The counterpart of
/// [`restore_terminal`], shared by startup (reader and file browser) and the
/// return from `$EDITOR`.
pub(crate) fn enter_tui(mouse_capture: bool) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    if mouse_capture {
        execute!(io::stdout(), EnableMouseCapture)?;
    }
    Ok(())
}

/// Restore the terminal before a panic reaches the default handler.
///
/// The normal restore path runs after `run_inner` returns, which a panic skips
/// entirely — and release builds set `panic = "abort"`, so nothing unwinds.
/// Without this hook, any panic in the render loop would leave the user in raw
/// mode inside the alternate screen, with the panic message itself unreadable.
pub(crate) fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore_terminal();
            default_hook(info);
        }));
    });
    install_signal_handlers();
}

/// Termination signals (SIGTERM, SIGHUP from a closed SSH session, a SIGINT
/// sent with `kill` — raw mode turns Ctrl-C into a key) only set a flag. The
/// event loops poll every 50ms, see it via `termination_requested`, and return
/// through the normal restore path; `exit_if_signalled` then exits with the
/// conventional 128+signal status. The default action would kill the process
/// mid-frame and leave the shell raw inside the alternate screen.
#[cfg(unix)]
mod term_signal {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, OnceLock};

    use signal_hook::SigId;
    use std::sync::Mutex;

    struct Flags {
        which: Arc<AtomicUsize>,
        any: Arc<AtomicBool>,
        /// The SIGINT registrations, so they can be paused while an
        /// external editor owns the terminal.
        interrupt: Mutex<Vec<SigId>>,
    }

    static FLAGS: OnceLock<Flags> = OnceLock::new();

    fn register(sig: i32, flags: &Flags) -> Vec<SigId> {
        // A second signal while the first is still pending (the loop is
        // stuck) terminates at once instead of being swallowed.
        [
            signal_hook::flag::register_conditional_shutdown(
                sig,
                128 + sig,
                Arc::clone(&flags.any),
            ),
            signal_hook::flag::register(sig, Arc::clone(&flags.any)),
            signal_hook::flag::register_usize(sig, Arc::clone(&flags.which), sig as usize),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    pub fn install() {
        FLAGS.get_or_init(|| {
            use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
            let flags = Flags {
                which: Arc::new(AtomicUsize::new(0)),
                any: Arc::new(AtomicBool::new(false)),
                interrupt: Mutex::new(Vec::new()),
            };
            for sig in [SIGTERM, SIGHUP] {
                register(sig, &flags);
            }
            let ids = register(SIGINT, &flags);
            if let Ok(mut held) = flags.interrupt.lock() {
                *held = ids;
            }
            let _ = std::thread::Builder::new()
                .name("ink-signal-watchdog".into())
                .spawn(watchdog);
            flags
        });
    }

    /// Set while an external editor owns the terminal.
    static SUSPENDED: AtomicBool = AtomicBool::new(false);

    /// How long a signal may stay pending before the watchdog gives up on
    /// the event loop.
    const GRACE: std::time::Duration = std::time::Duration::from_secs(1);

    /// The event loop notices a signal within one 50 ms poll — unless it is
    /// stuck where it cannot look. crossterm's event reader spins forever on
    /// a terminal that has hung up (a closed SSH session, a killed tmux):
    /// its read loop never sees `WouldBlock` again. The SIGHUP that came
    /// with the hangup then sat unread and ink burned a core indefinitely.
    /// If a signal is still pending after `GRACE`, restore what can be
    /// restored and exit with the status the loop would have used.
    fn watchdog() {
        let tick = std::time::Duration::from_millis(100);
        let mut pending_for = std::time::Duration::ZERO;
        loop {
            std::thread::sleep(tick);
            if pending().is_none() || SUSPENDED.load(Ordering::Relaxed) {
                pending_for = std::time::Duration::ZERO;
                continue;
            }
            pending_for += tick;
            if pending_for >= GRACE {
                // The restore writes to stdout, whose lock the stuck thread
                // might hold: give it a moment, never wait on it.
                let (done, wait) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = super::restore_terminal();
                    let _ = done.send(());
                });
                let _ = wait.recv_timeout(std::time::Duration::from_millis(300));
                std::process::exit(128 + pending().unwrap_or(0));
            }
        }
    }

    /// While an external editor runs in cooked mode, Ctrl-C sends SIGINT to
    /// the whole foreground process group — ink included. It is meant for
    /// the editor: ignore it until `resume`. SIGTERM and SIGHUP still end
    /// ink, but only once the editor has returned (the watchdog waits too),
    /// so ink never exits underneath a running editor.
    pub fn suspend() {
        SUSPENDED.store(true, Ordering::Relaxed);
        if let Some(flags) = FLAGS.get() {
            if let Ok(mut held) = flags.interrupt.lock() {
                for id in held.drain(..) {
                    signal_hook::low_level::unregister(id);
                }
            }
        }
    }

    pub fn resume() {
        if let Some(flags) = FLAGS.get() {
            let ids = register(signal_hook::consts::SIGINT, flags);
            if let Ok(mut held) = flags.interrupt.lock() {
                held.extend(ids);
            }
        }
        SUSPENDED.store(false, Ordering::Relaxed);
    }

    pub fn pending() -> Option<i32> {
        let flags = FLAGS.get()?;
        if !flags.any.load(Ordering::Relaxed) {
            return None;
        }
        match flags.which.load(Ordering::Relaxed) {
            0 => None,
            sig => Some(sig as i32),
        }
    }
}

fn install_signal_handlers() {
    #[cfg(unix)]
    term_signal::install();
}

/// A termination signal arrived; event loops should return so the terminal
/// gets restored. Always false on Windows.
pub(crate) fn termination_requested() -> bool {
    #[cfg(unix)]
    {
        term_signal::pending().is_some()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// After the terminal is restored: if a termination signal ended the event
/// loop, exit with 128+signal like the default action would have.
pub(crate) fn exit_if_signalled() {
    #[cfg(unix)]
    if let Some(sig) = term_signal::pending() {
        std::process::exit(128 + sig);
    }
}

pub fn run(source: String, args: Args) -> Result<AppExit> {
    let mouse_capture = args.mouse_capture;

    // Probe the terminal for a graphics protocol (Kitty/iTerm2/Sixel) BEFORE
    // entering the alternate screen — the query talks over stdio. Falls back to
    // half-blocks on any non-graphics terminal or when images are disabled.
    let graphics = if args.images == crate::image::ImageMode::Off {
        crate::graphics::Graphics::halfblocks()
    } else {
        crate::graphics::Graphics::detect(args.image_protocol)
    };

    install_panic_hook();
    enter_tui(mouse_capture)?;
    // A background-colour reply that missed the startup query must not be
    // read as key presses.
    if theme::detect::reply_may_arrive_late() {
        input::discard_late_reply();
    }
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_inner(&mut terminal, source, args, &graphics);

    restore_terminal()?;
    exit_if_signalled();

    result
}

fn run_inner(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    source: String,
    mut args: Args,
    graphics: &crate::graphics::Graphics,
) -> Result<AppExit> {
    let size = terminal.size()?;

    let mut tabs: Vec<Tab> = Vec::new();
    let mut active_tab: usize = 0;

    let mut search = SearchState::new();
    let mut nav = NavHistory::default();
    let mut theme_picker_open = false;
    let mut theme_picker_index: usize = 0;
    let mut help_open = false;
    // Active link-hint overlay: labeled links currently on screen.
    let mut link_hints: Vec<LinkHint> = Vec::new();
    let mut hint_kind = HintKind::Open;
    // Active code-block hint overlay.
    let mut code_hints: Vec<CodeHint> = Vec::new();
    // Active text selection (visual mode or an in-progress mouse drag).
    let mut sel: Option<Selection> = None;
    let mut visual_mode = false;
    // A left button is down and the pointer has moved since it went down.
    let mut dragging = false;
    let mut drag_moved = false;
    // (when, column, row, consecutive clicks) — crossterm reports no click
    // count, so double/triple clicks are timed here.
    let mut last_click: Option<(Instant, u16, u16, u8)> = None;
    // Transient status message ("copied 84 chars") and when it was set.
    let mut flash: Option<(String, Instant)> = None;
    // Where the document was last drawn, for translating mouse coordinates.
    let mut doc_rect = Rect::new(0, 0, 0, 0);
    // Where the TOC's heading list was last drawn (empty when hidden), for
    // clicks and wheel scrolls over the sidebar.
    let mut toc_rect = Rect::new(0, 0, 0, 0);
    // Bumped on every theme change so tabs know their cached layout is stale.
    let mut theme_gen: u32 = 0;

    let filename = args.inputs.first().map(|s| s.as_str()).unwrap_or("stdin");
    let init_width = effective_width(size.width, args.toc);
    if args.slides {
        // Presentation mode: one tab per slide, navigated with ←/→/Space.
        // Frontmatter is stripped from the whole deck first — otherwise the
        // YAML header becomes slide 1.
        let deck = if args.frontmatter {
            source.clone()
        } else {
            crate::parser::frontmatter::strip_frontmatter(&source).1
        };
        for slide in crate::slides::split_slides(&deck) {
            tabs.push(build_tab(
                slide, filename, &args, init_width, theme_gen, graphics,
            ));
        }
    } else {
        tabs.push(build_tab(
            source, filename, &args, init_width, theme_gen, graphics,
        ));
        for input in args.inputs.iter().skip(1) {
            if let Ok(src) = std::fs::read_to_string(input) {
                tabs.push(build_tab(
                    src, input, &args, init_width, theme_gen, graphics,
                ));
            }
        }
    }

    // --watch: spawn a file watcher for the current document if a real path is in play.
    let watcher: Option<crate::watch::FileWatcher> = if args.watch {
        match args.inputs.first() {
            Some(input) if is_local_file(input) => {
                let path = std::path::PathBuf::from(input);
                match crate::watch::FileWatcher::new(&path) {
                    Ok(w) => Some(w),
                    Err(e) => {
                        eprintln!("ink: failed to start file watcher: {e}");
                        None
                    }
                }
            }
            _ => {
                eprintln!("ink: --watch requires a local file path, ignoring");
                None
            }
        }
    } else {
        None
    };

    // Find current theme index
    for (i, t) in THEME_LIST.iter().enumerate() {
        if *t == args.theme {
            theme_picker_index = i;
            break;
        }
    }

    // True when --watch is active and the latest read of the watched file failed
    // (file was deleted / renamed away). The doc keeps showing the last good content
    // and the bottom bar gets a "[file missing]" indicator.
    let mut file_missing = false;

    // Redraw only when something changed. An idle reader with no input pending
    // does no per-frame work at all (previously it redrew ~20×/second).
    let mut dirty = true;
    // Set on every Resize event; the rebuild runs once events stop for 80ms.
    let mut resize_pending: Option<std::time::Instant> = None;

    loop {
        if termination_requested() {
            return Ok(AppExit::Quit);
        }
        let viewport_height = terminal.size()?.height.saturating_sub(3); // top + separator + bottom

        // --watch: if the current doc is the watched file and it changed, rebuild it.
        if let (Some(w), Some(input)) = (watcher.as_ref(), args.inputs.first()) {
            let path = std::path::Path::new(input);
            if tabs[active_tab].filename == *input && w.check(path) {
                match std::fs::read_to_string(path) {
                    Ok(new_source) if args.slides => {
                        let deck = if args.frontmatter {
                            new_source
                        } else {
                            crate::parser::frontmatter::strip_frontmatter(&new_source).1
                        };
                        let toc_visible = tabs[active_tab].toc.visible;
                        let term_w = effective_width(terminal.size()?.width, toc_visible);
                        let slides = crate::slides::split_slides(&deck);
                        if !slides.is_empty() {
                            tabs = slides
                                .into_iter()
                                .map(|sl| build_tab(sl, input, &args, term_w, theme_gen, graphics))
                                .collect();
                            active_tab = active_tab.min(tabs.len() - 1);
                            tabs[active_tab].toc.visible = toc_visible;
                        }
                        search.update_matches(&tabs[active_tab].lowered);
                        file_missing = false;
                        dirty = true;
                    }
                    Ok(new_source) => {
                        let scroll = tabs[active_tab].scroll_offset;
                        let term_w =
                            effective_width(terminal.size()?.width, tabs[active_tab].toc.visible);
                        let mut new_tab =
                            build_tab(new_source, input, &args, term_w, theme_gen, graphics);
                        let new_max = new_tab
                            .ratatui_lines
                            .len()
                            .saturating_sub(viewport_height as usize);
                        new_tab.scroll_offset = scroll.min(new_max);
                        new_tab.toc.visible = tabs[active_tab].toc.visible;
                        new_tab
                            .toc
                            .inherit(std::mem::take(&mut tabs[active_tab].toc.nav));
                        new_tab.toc.update_selection(new_tab.scroll_offset);
                        tabs[active_tab] = new_tab;
                        search.update_matches(&tabs[active_tab].lowered);
                        if file_missing {
                            file_missing = false;
                        }
                        dirty = true;
                    }
                    Err(_) => {
                        // File was deleted or renamed away. Keep the last-rendered content
                        // and surface the state in the status bar.
                        if !file_missing {
                            file_missing = true;
                            dirty = true;
                        }
                    }
                }
            }
        }

        // Debounced resize: rebuild once the flurry of events has settled.
        if let Some(t0) = resize_pending {
            if t0.elapsed() >= Duration::from_millis(80) {
                resize_pending = None;
                rebuild_tab(
                    &mut tabs[active_tab],
                    &args,
                    terminal.size()?.width,
                    theme_gen,
                    graphics,
                );
                search.update_matches(&tabs[active_tab].lowered);
                dirty = true;
            }
        }

        // A copy message is worth two seconds of the status bar, no longer.
        if let Some((_, at)) = flash {
            if at.elapsed() >= Duration::from_secs(2) {
                flash = None;
                dirty = true;
            }
        }

        // The heading list sits under the sidebar's title row.
        let toc_rows = viewport_height.saturating_sub(1) as usize;
        if tabs[active_tab].toc.visible {
            tabs[active_tab].toc.follow(toc_rows);
        }

        let tab = &tabs[active_tab];
        let total_lines = tab.ratatui_lines.len();
        let max_scroll = total_lines.saturating_sub(viewport_height as usize);

        if dirty {
            terminal.draw(|frame| {
                let size = frame.area();
                let tab = &tabs[active_tab];

                // Fill entire frame with theme background color
                let t = theme::resolve_theme(&args.theme);
                if let Some(ref bg_hex) = t.colors.bg {
                    let bg_color = theme::hex_to_color(bg_hex);
                    let bg_block =
                        ratatui::widgets::Block::default().style(Style::default().bg(bg_color));
                    frame.render_widget(bg_block, size);
                }

                // Guard against a terminal too small to lay out safely.
                if size.width < 20 || size.height < 6 {
                    let msg = Paragraph::new("terminal too small")
                        .alignment(Alignment::Center)
                        .style(Style::default().fg(theme::hex_to_color(&t.colors.fg)));
                    frame.render_widget(msg, size);
                    return;
                }

                let vertical = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1), // top bar: progress
                        Constraint::Min(1),    // main content
                        Constraint::Length(2), // separator + bottom bar
                    ])
                    .split(size);

                let top_bar_area = vertical[0];
                let main_area = vertical[1];
                let bottom_area = vertical[2];

                // Split bottom into separator line + bar
                let bottom_split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Length(1)])
                    .split(bottom_area);
                let separator_area = bottom_split[0];
                let bottom_bar_area = bottom_split[1];

                // Separator: ▔ chars in bar-bg on content-bg = half-row visual gap
                let bar_bg = theme::hex_to_color(&t.colors.status_bar_bg);
                let content_bg = t.colors.bg.as_ref().map(|b| theme::hex_to_color(b));
                let sep_style = if let Some(cbg) = content_bg {
                    Style::default().fg(bar_bg).bg(cbg)
                } else {
                    Style::default().fg(bar_bg)
                };
                let sep_line = Line::from(Span::styled(
                    crate::glyphs::current()
                        .separator
                        .repeat(separator_area.width as usize),
                    sep_style,
                ));
                frame.render_widget(Paragraph::new(vec![sep_line]), separator_area);

                // Top bar: filename + progress
                let tab_info = if tabs.len() > 1 {
                    Some((active_tab, tabs.len()))
                } else {
                    None
                };
                render::render_top_bar(
                    frame,
                    top_bar_area,
                    &tab.filename,
                    tab.scroll_offset,
                    total_lines,
                    viewport_height as usize,
                    &t,
                    tab_info,
                );

                let (toc_area, doc_area) = if tab.toc.visible && main_area.width > 40 {
                    let horizontal = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Length(tab.toc.width), Constraint::Min(1)])
                        .split(main_area);
                    (Some(horizontal[0]), horizontal[1])
                } else {
                    (None, main_area)
                };

                if let Some(toc_area) = toc_area {
                    render::render_toc(frame, toc_area, &tab.toc, &t);
                }
                toc_rect = toc_area.map_or(Rect::default(), render::toc_list_area);

                // Remembered for the next mouse event: screen coordinates only
                // mean something relative to where the document was drawn.
                doc_rect = doc_area;

                // Render document (with search highlights and selection)
                render::render_document_with_search(
                    frame,
                    doc_area,
                    &tab.ratatui_lines,
                    tab.scroll_offset,
                    total_lines,
                    &search,
                    sel.as_ref(),
                    &tab.plain,
                    &t,
                );

                // Paint graphics-protocol images over their reserved blank rows.
                // SlicedImage self-clips to doc_area, so partially-scrolled images
                // (position.y negative or past the bottom) render correctly.
                for img in &tab.images {
                    let y = img.line_index as i64 - tab.scroll_offset as i64;
                    // Skip images fully above or below the viewport.
                    if y + img.rows as i64 <= 0 || y >= doc_area.height as i64 {
                        continue;
                    }
                    match &img.protocol {
                        Some(proto) => {
                            let pos = ratatui_image::sliced::SignedPosition::from((
                                img.col_offset as i16,
                                y as i16,
                            ));
                            frame.render_widget(
                                ratatui_image::sliced::SlicedImage::new(proto, pos),
                                doc_area,
                            );
                        }
                        None if (0..doc_area.height as i64).contains(&y) => {
                            // Encode failed: say so in the reserved space — a
                            // silent blank gap reads as a rendering bug.
                            frame.buffer_mut().set_string(
                                doc_area.x + img.col_offset,
                                doc_area.y + y as u16,
                                format!(
                                    "{} (image could not be encoded for this terminal)",
                                    crate::glyphs::current().image
                                ),
                                Style::default()
                                    .fg(theme::hex_to_color(&t.colors.link_url))
                                    .add_modifier(Modifier::ITALIC),
                            );
                        }
                        None => {}
                    }
                }

                // Theme picker overlay
                if theme_picker_open {
                    render::render_theme_picker(
                        frame,
                        main_area,
                        THEME_LIST,
                        theme_picker_index,
                        &args.theme,
                        &t,
                    );
                }

                // Link-hint overlay
                if !link_hints.is_empty() {
                    let hints: Vec<(char, String, String)> = link_hints
                        .iter()
                        .map(|h| (h.label, h.caption.clone(), h.url.clone()))
                        .collect();
                    let dash = crate::glyphs::current().dash;
                    let title = match hint_kind {
                        HintKind::Open => format!(
                            "Follow link {dash} press a letter, Y to copy instead, Esc to cancel"
                        ),
                        HintKind::CopyUrl => {
                            format!("Copy link URL {dash} press a letter, Esc to cancel")
                        }
                    };
                    render::render_link_hints(frame, main_area, &hints, &title, &t);
                }

                // Code-block hint labels, painted on the blocks themselves.
                if !code_hints.is_empty() {
                    let labels: Vec<(char, u16, u16)> =
                        code_hints.iter().map(|h| (h.label, h.row, h.col)).collect();
                    render::render_code_hints(frame, doc_area, &labels, &t);
                }

                // Help overlay
                if help_open {
                    render::render_help(frame, main_area, &t);
                }

                // Bottom bar: copy result OR search input OR keybindings + stats
                if let Some((ref msg, _)) = flash {
                    render::render_flash_bar(frame, bottom_bar_area, msg, &t);
                } else if search.active {
                    render::render_search_bar(frame, bottom_bar_area, &search, &t);
                } else if tab.toc.nav.focused {
                    render::render_toc_bar(
                        frame,
                        bottom_bar_area,
                        tab.toc.nav.filter.is_some(),
                        &t,
                    );
                } else {
                    render::render_bottom_bar(
                        frame,
                        bottom_bar_area,
                        &t,
                        &tab.filename,
                        tab.word_count,
                        tab.reading_time,
                        tabs.len() > 1,
                        tab_info,
                        file_missing,
                    );
                }
            })?;
            dirty = false;
        }

        // Handle input
        let input_mode = if search.active {
            input::InputMode::Search
        } else if !link_hints.is_empty() || !code_hints.is_empty() {
            input::InputMode::LinkHint
        } else if visual_mode {
            input::InputMode::Visual
        } else if tabs[active_tab].toc.nav.focused && !help_open && !theme_picker_open {
            if tabs[active_tab].toc.nav.filter.is_some() {
                input::InputMode::TocFilter
            } else {
                input::InputMode::Toc
            }
        } else if args.slides && !theme_picker_open && !help_open {
            input::InputMode::Slides
        } else {
            input::InputMode::Normal
        };
        if let Some(action) = input::poll_action(Duration::from_millis(50), input_mode) {
            // Any recognized action changes state → redraw on the next iteration.
            dirty = true;

            // The wheel scrolls what is under the pointer: the sidebar, or
            // (as an ordinary scroll) the document.
            let action = match action {
                Action::WheelUp(col, row) | Action::WheelDown(col, row)
                    if toc_rect.contains(Position::new(col, row))
                        && !help_open
                        && !theme_picker_open =>
                {
                    let delta = if matches!(action, Action::WheelUp(..)) {
                        -3
                    } else {
                        3
                    };
                    tabs[active_tab].toc.scroll_by(delta, toc_rows);
                    continue;
                }
                Action::WheelUp(..) => Action::ScrollUp(3),
                Action::WheelDown(..) => Action::ScrollDown(3),
                other => other,
            };

            // Help overlay is modal: any key closes it.
            if help_open {
                if action != Action::None {
                    help_open = false;
                }
                continue;
            }

            // Code-block hint overlay is modal: a label copies that block.
            if !code_hints.is_empty() {
                if let Action::LinkHint(c) = action {
                    if let Some(hint) = code_hints.iter().find(|h| h.label == c) {
                        let what = if hint.lang.is_empty() {
                            "code block".to_string()
                        } else {
                            format!("code block ({})", hint.lang)
                        };
                        flash = Some((
                            copy_text(&hint.source, args.clipboard, &what),
                            Instant::now(),
                        ));
                    }
                }
                code_hints.clear();
                continue;
            }

            // Link-hint overlay is modal: a label opens (or copies) that link,
            // Y switches between the two, Esc cancels.
            if !link_hints.is_empty() {
                match action {
                    Action::HintCopyToggle => {
                        hint_kind = HintKind::CopyUrl;
                    }
                    Action::LinkHint(c) => {
                        if let Some(hint) = link_hints.iter().find(|h| h.label == c) {
                            match hint_kind {
                                HintKind::Open => {
                                    if let Some(msg) = open_link(
                                        &hint.url, &mut tabs, active_tab, &mut nav, &args,
                                        terminal, theme_gen, graphics,
                                    ) {
                                        flash = Some((msg, Instant::now()));
                                    }
                                }
                                HintKind::CopyUrl => {
                                    flash = Some((
                                        copy_text(&hint.url, args.clipboard, "link"),
                                        Instant::now(),
                                    ));
                                }
                            }
                        }
                        link_hints.clear();
                    }
                    Action::CloseSearch => link_hints.clear(),
                    _ => link_hints.clear(),
                }
                continue;
            }
            // Theme picker mode
            if theme_picker_open {
                match action {
                    Action::ExitApp | Action::CloseSearch => {
                        // Closing keeps the previewed theme — persist it.
                        theme_picker_open = false;
                        let _ = crate::config::set_theme(THEME_LIST[theme_picker_index]);
                    }
                    Action::ScrollDown(_) => {
                        theme_picker_index = (theme_picker_index + 1) % THEME_LIST.len();
                        // Live preview: rebuild the visible tab now; others refresh
                        // lazily when switched to (theme_gen marks them stale).
                        args.theme = THEME_LIST[theme_picker_index].to_string();
                        theme_gen = theme_gen.wrapping_add(1);
                        rebuild_tab(
                            &mut tabs[active_tab],
                            &args,
                            terminal.size()?.width,
                            theme_gen,
                            graphics,
                        );
                    }
                    Action::ScrollUp(_) => {
                        theme_picker_index = if theme_picker_index == 0 {
                            THEME_LIST.len() - 1
                        } else {
                            theme_picker_index - 1
                        };
                        args.theme = THEME_LIST[theme_picker_index].to_string();
                        theme_gen = theme_gen.wrapping_add(1);
                        rebuild_tab(
                            &mut tabs[active_tab],
                            &args,
                            terminal.size()?.width,
                            theme_gen,
                            graphics,
                        );
                    }
                    Action::SearchConfirm => {
                        // Confirm theme selection and persist it to config.
                        theme_picker_open = false;
                        let _ = crate::config::set_theme(THEME_LIST[theme_picker_index]);
                    }
                    _ => {}
                }
                continue;
            }

            // Visual mode is modal: motions move the cursor end of the
            // selection, `y` copies it, Esc/q leaves without copying.
            if visual_mode {
                let plain = &tabs[active_tab].plain;
                let Some(mut cursel) = sel else {
                    visual_mode = false;
                    continue;
                };
                let cur = cursel.cursor;
                let line_text = plain.get(cur.line).map(|t| t.as_str()).unwrap_or("");
                let vh = viewport_height as usize;
                let mut moved = true;
                match action {
                    Action::Yank => {
                        let text = cursel.extract(plain);
                        let what = format!("{} chars", text.chars().count());
                        flash = Some((copy_text(&text, args.clipboard, &what), Instant::now()));
                        visual_mode = false;
                        sel = None;
                        continue;
                    }
                    Action::SelCancel | Action::ExitApp => {
                        visual_mode = false;
                        sel = None;
                        continue;
                    }
                    Action::SelectMode => cursel.mode = SelMode::Char,
                    Action::SelectLineMode => {
                        cursel.mode = if cursel.mode == SelMode::Line {
                            SelMode::Char
                        } else {
                            SelMode::Line
                        }
                    }
                    Action::SelDown(n) => cursel.cursor.line = cur.line.saturating_add(n as usize),
                    Action::SelUp(n) => cursel.cursor.line = cur.line.saturating_sub(n as usize),
                    Action::SelLeft(n) => cursel.cursor.col = cur.col.saturating_sub(n as usize),
                    Action::SelRight(n) => cursel.cursor.col = cur.col.saturating_add(n as usize),
                    Action::SelWordNext => {
                        cursel.cursor.col = selection::next_word_col(line_text, cur.col)
                    }
                    Action::SelWordPrev => {
                        cursel.cursor.col = selection::prev_word_col(line_text, cur.col)
                    }
                    Action::SelLineStart => cursel.cursor.col = 0,
                    Action::SelLineEnd => {
                        cursel.cursor.col = selection::line_width(line_text).saturating_sub(1)
                    }
                    Action::SelDocStart => cursel.cursor = Pos::new(0, 0),
                    Action::SelDocEnd => {
                        cursel.cursor = Pos::new(plain.len().saturating_sub(1), 0);
                    }
                    Action::SelPageDown => {
                        cursel.cursor.line = cur.line.saturating_add(vh.max(1) - 1)
                    }
                    Action::SelPageUp => {
                        cursel.cursor.line = cur.line.saturating_sub(vh.max(1) - 1)
                    }
                    Action::Resize(_, _) => {
                        resize_pending = Some(std::time::Instant::now());
                        moved = false;
                    }
                    _ => moved = false,
                }
                if moved {
                    cursel.cursor = selection::clamp(cursel.cursor, plain);
                    // Keep the moving end of the selection on screen.
                    let scroll = &mut tabs[active_tab].scroll_offset;
                    if cursel.cursor.line < *scroll {
                        *scroll = cursel.cursor.line;
                    } else if cursel.cursor.line >= *scroll + vh {
                        *scroll = cursel.cursor.line + 1 - vh;
                    }
                    *scroll = (*scroll).min(max_scroll);
                }
                sel = Some(cursel);
                continue;
            }

            // The focused sidebar takes its own keys; anything else (mouse,
            // resize, wheel) falls through to the ordinary handling below.
            if let Action::Toc(key) = action {
                let tab = &mut tabs[active_tab];
                if let Some(i) = tab.toc.handle(key, toc_rows) {
                    jump_to_heading(tab, &mut nav, i, viewport_height as usize);
                }
                continue;
            }

            // A confirmed search shows highlighted matches; the first Esc/q
            // dismisses them (vim/less convention) rather than quitting.
            if action == Action::ExitApp && !search.matches.is_empty() {
                search.deactivate();
                let current_offset = tabs[active_tab].scroll_offset;
                tabs[active_tab].toc.update_selection(current_offset);
                continue;
            }

            match action {
                Action::ExitApp => return Ok(AppExit::Quit),
                Action::OpenBrowser => return Ok(AppExit::BackToBrowser),
                Action::CloseDoc => return Ok(AppExit::BackToBrowser),

                // Search
                Action::Search => {
                    search.activate();
                }
                Action::CloseSearch => {
                    search.deactivate();
                }
                Action::SearchConfirm => {
                    // Keep matches visible but exit search input mode
                    search.active = false;
                }
                Action::SearchInput(c) => {
                    search.push_char(c);
                    search.update_matches(&tabs[active_tab].lowered);
                    // Auto-jump to first match
                    if let Some(line) = search.current_line() {
                        tabs[active_tab].scroll_offset = line.min(max_scroll);
                    }
                }
                Action::SearchBackspace => {
                    search.pop_char();
                    search.update_matches(&tabs[active_tab].lowered);
                    if let Some(line) = search.current_line() {
                        tabs[active_tab].scroll_offset = line.min(max_scroll);
                    }
                }
                Action::SearchNext => {
                    search.next_match();
                    if let Some(line) = search.current_line() {
                        tabs[active_tab].scroll_offset = line.min(max_scroll);
                    }
                }
                Action::SearchPrev => {
                    search.prev_match();
                    if let Some(line) = search.current_line() {
                        tabs[active_tab].scroll_offset = line.min(max_scroll);
                    }
                }

                // Scrolling
                Action::ScrollDown(n) => {
                    tabs[active_tab].scroll_offset = tabs[active_tab]
                        .scroll_offset
                        .saturating_add(n as usize)
                        .min(max_scroll);
                }
                Action::ScrollUp(n) => {
                    // Clamp before subtracting: a stale offset past the end
                    // (post-resize) must recover on the first upward scroll.
                    tabs[active_tab].scroll_offset = tabs[active_tab]
                        .scroll_offset
                        .min(max_scroll)
                        .saturating_sub(n as usize);
                }
                Action::PageDown => {
                    let jump = (viewport_height as usize).saturating_sub(2); // keep 2 lines overlap
                    tabs[active_tab].scroll_offset = tabs[active_tab]
                        .scroll_offset
                        .saturating_add(jump)
                        .min(max_scroll);
                }
                Action::PageUp => {
                    let jump = (viewport_height as usize).saturating_sub(2);
                    tabs[active_tab].scroll_offset = tabs[active_tab]
                        .scroll_offset
                        .min(max_scroll)
                        .saturating_sub(jump);
                }
                Action::Home => tabs[active_tab].scroll_offset = 0,
                Action::End => tabs[active_tab].scroll_offset = max_scroll,

                // Slides
                Action::SlideNext => {
                    if active_tab + 1 < tabs.len() {
                        active_tab += 1;
                        ensure_tab_current(
                            &mut tabs[active_tab],
                            &args,
                            terminal.size()?.width,
                            theme_gen,
                            graphics,
                        );
                        search.update_matches(&tabs[active_tab].lowered);
                    }
                }
                Action::SlidePrev => {
                    if active_tab > 0 {
                        active_tab -= 1;
                        ensure_tab_current(
                            &mut tabs[active_tab],
                            &args,
                            terminal.size()?.width,
                            theme_gen,
                            graphics,
                        );
                        search.update_matches(&tabs[active_tab].lowered);
                    }
                }

                // TOC
                Action::TocFocus => {
                    let width = terminal.size()?.width;
                    let tab = &mut tabs[active_tab];
                    if tab.toc.headings.is_empty() {
                        flash = Some(("no headings in this document".into(), Instant::now()));
                    } else if width <= 40 {
                        flash = Some((
                            "window too narrow for the table of contents".into(),
                            Instant::now(),
                        ));
                    } else {
                        if !tab.toc.visible {
                            tab.toc.toggle();
                            rebuild_tab(tab, &args, width, theme_gen, graphics);
                        }
                        tab.toc.update_selection(tab.scroll_offset);
                        tab.toc.focus();
                    }
                }
                Action::ToggleToc => {
                    tabs[active_tab].toc.toggle();
                    rebuild_tab(
                        &mut tabs[active_tab],
                        &args,
                        terminal.size()?.width,
                        theme_gen,
                        graphics,
                    );
                }

                // Help overlay
                Action::Help => {
                    help_open = true;
                }

                // Theme picker
                Action::ThemePicker => {
                    theme_picker_open = true;
                }

                // Link-hint mode: label every link in the viewport.
                Action::LinkMode => {
                    let len = tabs[active_tab].styled_lines.len();
                    let offset = tabs[active_tab].scroll_offset.min(len);
                    let end = (offset + viewport_height as usize).min(len);
                    link_hints = collect_link_hints(&tabs[active_tab].styled_lines[offset..end]);
                }

                // After a confirmed search, n/N cycle matches (less/vim
                // convention); otherwise they jump between headings.
                Action::NextHeading => {
                    if !search.matches.is_empty() {
                        search.next_match();
                        if let Some(line) = search.current_line() {
                            tabs[active_tab].scroll_offset = line.min(max_scroll);
                        }
                    } else {
                        let current = tabs[active_tab].scroll_offset;
                        if let Some(next) = tabs[active_tab]
                            .toc
                            .headings
                            .iter()
                            .find(|h| h.line_index > current + 1)
                        {
                            tabs[active_tab].scroll_offset =
                                next.line_index.saturating_sub(1).min(max_scroll);
                        }
                    }
                }
                Action::PrevHeading => {
                    if !search.matches.is_empty() {
                        search.prev_match();
                        if let Some(line) = search.current_line() {
                            tabs[active_tab].scroll_offset = line.min(max_scroll);
                        }
                    } else {
                        let current = tabs[active_tab].scroll_offset;
                        if let Some(prev) = tabs[active_tab]
                            .toc
                            .headings
                            .iter()
                            .rev()
                            .find(|h| h.line_index + 1 < current)
                        {
                            tabs[active_tab].scroll_offset =
                                prev.line_index.saturating_sub(1).min(max_scroll);
                        }
                    }
                }

                // Links are handled via OSC 8 hyperlinks (Cmd+Click / Ctrl+Click)

                // Tabs — refresh the newly-active tab if it was laid out for a
                // stale width/theme (lazy rebuild).
                Action::NextTab if tabs.len() > 1 => {
                    active_tab = (active_tab + 1) % tabs.len();
                    ensure_tab_current(
                        &mut tabs[active_tab],
                        &args,
                        terminal.size()?.width,
                        theme_gen,
                        graphics,
                    );
                    search.update_matches(&tabs[active_tab].lowered);
                }
                Action::PrevTab if tabs.len() > 1 => {
                    active_tab = if active_tab == 0 {
                        tabs.len() - 1
                    } else {
                        active_tab - 1
                    };
                    ensure_tab_current(
                        &mut tabs[active_tab],
                        &args,
                        terminal.size()?.width,
                        theme_gen,
                        graphics,
                    );
                    search.update_matches(&tabs[active_tab].lowered);
                }

                // Follow the first link on screen that can be followed (web,
                // mail, a .md file or a #heading anchor), through the same
                // path as link-hint mode.
                Action::FollowLink => {
                    let tab = &tabs[active_tab];
                    let found_link = first_followable_link(
                        &tab.styled_lines,
                        tab.scroll_offset,
                        viewport_height,
                    )
                    .map(str::to_string);
                    match found_link {
                        Some(link) => {
                            if let Some(msg) = open_link(
                                &link, &mut tabs, active_tab, &mut nav, &args, terminal, theme_gen,
                                graphics,
                            ) {
                                flash = Some((msg, Instant::now()));
                            }
                        }
                        None => {
                            flash = Some(("no links on screen".into(), Instant::now()));
                        }
                    }
                }

                // Navigation history
                Action::NavBack => {
                    if let Some(entry) = nav.go_back(NavEntry::of(&tabs[active_tab])) {
                        restore_nav_entry(
                            &mut tabs[active_tab],
                            entry,
                            &args,
                            effective_width(terminal.size()?.width, args.toc),
                            theme_gen,
                            graphics,
                        );
                    }
                }
                Action::NavForward => {
                    if let Some(entry) = nav.go_forward(NavEntry::of(&tabs[active_tab])) {
                        restore_nav_entry(
                            &mut tabs[active_tab],
                            entry,
                            &args,
                            effective_width(terminal.size()?.width, args.toc),
                            theme_gen,
                            graphics,
                        );
                    }
                }

                // Selection & clipboard
                Action::SelectMode | Action::SelectLineMode => {
                    let mode = if action == Action::SelectLineMode {
                        SelMode::Line
                    } else {
                        SelMode::Char
                    };
                    // Normal mode has no caret to inherit, so the selection
                    // starts on the first line of the viewport that has text —
                    // anchoring on the blank spacing above a heading looks
                    // broken.
                    let tab = &tabs[active_tab];
                    let start = tab.scroll_offset.min(tab.plain.len().saturating_sub(1));
                    let end = (start + viewport_height as usize).min(tab.plain.len());
                    let line = (start..end)
                        .find(|i| !tab.plain[*i].trim().is_empty())
                        .unwrap_or(start);
                    let col = tab
                        .plain
                        .get(line)
                        .map_or(0, |t| selection::content_start_col(t));
                    sel = Some(Selection::new(Pos::new(line, col), mode));
                    visual_mode = true;
                }

                // Code-block hints: label every block touching the viewport.
                Action::CopyCode => {
                    let tab = &tabs[active_tab];
                    let top = tab.scroll_offset;
                    let bottom = top + viewport_height as usize;
                    code_hints = tab
                        .code_blocks
                        .iter()
                        .filter(|b| b.line_index < bottom && b.line_index + b.rows > top)
                        .zip('a'..='z')
                        .map(|(b, label)| {
                            // A block scrolled off the top keeps its label on
                            // the first visible row.
                            let row = b.line_index.max(top) - top;
                            let width = tab
                                .plain
                                .get(b.line_index)
                                .map_or(0, |t| selection::line_width(t));
                            CodeHint {
                                label,
                                row: row as u16,
                                col: width.saturating_sub(4) as u16,
                                lang: b.lang.clone(),
                                source: b.source.clone(),
                            }
                        })
                        .collect();
                    if code_hints.is_empty() {
                        flash = Some(("no code blocks on screen".to_string(), Instant::now()));
                    }
                }

                // Open the file in $VISUAL/$EDITOR at the line on screen,
                // then reload it.
                Action::Edit => {
                    let width = terminal.size()?.width;
                    let msg = edit_in_editor(
                        terminal,
                        &mut tabs[active_tab],
                        &args,
                        viewport_height as usize,
                        width,
                        theme_gen,
                        graphics,
                    );
                    search.update_matches(&tabs[active_tab].lowered);
                    flash = msg.map(|m| (m, Instant::now()));
                }

                // Copy the section the viewport starts in, as markdown source.
                Action::CopySection => {
                    let tab = &tabs[active_tab];
                    let (text, what) = section_source(tab, viewport_height as usize);
                    flash = Some((copy_text(&text, args.clipboard, &what), Instant::now()));
                }

                // Mouse selection. Only reachable when ink holds the mouse;
                // with `mouse_capture = false` these events never arrive and
                // the terminal's own selection keeps working.
                // A click on a sidebar row jumps to that heading.
                Action::MouseDown(col, row) if toc_rect.contains(Position::new(col, row)) => {
                    let tab = &mut tabs[active_tab];
                    tab.toc.leave();
                    if let Some(i) = tab.toc.heading_at((row - toc_rect.y) as usize) {
                        jump_to_heading(tab, &mut nav, i, viewport_height as usize);
                    }
                }
                Action::MouseDown(col, row) => {
                    tabs[active_tab].toc.leave();
                    if let Some(pos) = screen_to_doc(doc_rect, &tabs[active_tab], col, row) {
                        let clicks = match last_click {
                            Some((at, c, r, n))
                                if c == col
                                    && r == row
                                    && at.elapsed() < Duration::from_millis(400) =>
                            {
                                n % 3 + 1
                            }
                            _ => 1,
                        };
                        last_click = Some((Instant::now(), col, row, clicks));
                        let plain = &tabs[active_tab].plain;
                        let line_text = plain.get(pos.line).map(|t| t.as_str()).unwrap_or("");
                        sel = Some(match clicks {
                            2 => {
                                let (from, to) = selection::word_bounds(line_text, pos.col);
                                Selection {
                                    anchor: Pos::new(pos.line, from),
                                    cursor: Pos::new(pos.line, to),
                                    mode: SelMode::Char,
                                }
                            }
                            3 => Selection::new(pos, SelMode::Line),
                            _ => Selection::new(pos, SelMode::Char),
                        });
                        // A double/triple click has already selected something;
                        // release copies it without any drag.
                        drag_moved = clicks > 1;
                        dragging = true;
                        visual_mode = false;
                    }
                }
                Action::MouseDrag(col, row) => {
                    if dragging {
                        if let Some(pos) = screen_to_doc(doc_rect, &tabs[active_tab], col, row) {
                            if let Some(ref mut cursel) = sel {
                                if pos != cursel.cursor {
                                    drag_moved = true;
                                }
                                cursel.cursor = pos;
                            }
                        }
                    }
                }
                Action::MouseUp(col, row) => {
                    // Pressed and released on one cell with no drag, and not
                    // the second press of a double click.
                    let clicked = dragging
                        && !drag_moved
                        && matches!(last_click, Some((_, c, r, 1)) if c == col && r == row);
                    dragging = false;
                    match sel {
                        // A plain click with no drag is a dismiss, not a copy.
                        Some(_) if !drag_moved => sel = None,
                        Some(cursel) => {
                            let text = cursel.extract(&tabs[active_tab].plain);
                            let what = format!("{} chars", text.chars().count());
                            flash = Some((copy_text(&text, args.clipboard, &what), Instant::now()));
                        }
                        None => {}
                    }
                    drag_moved = false;
                    // A click on a link follows it, the way `f` would.
                    let link = screen_to_doc(doc_rect, &tabs[active_tab], col, row)
                        .filter(|_| clicked)
                        .and_then(|pos| link_at(&tabs[active_tab], pos))
                        .map(str::to_string);
                    if let Some(url) = link {
                        // The page under the pointer changes: the next press
                        // is a fresh click, not the second of a double.
                        last_click = None;
                        if let Some(msg) = open_link(
                            &url, &mut tabs, active_tab, &mut nav, &args, terminal, theme_gen,
                            graphics,
                        ) {
                            flash = Some((msg, Instant::now()));
                        }
                    }
                }

                Action::Resize(_, _) => {
                    // Debounced: dragging a terminal edge fires dozens of
                    // resize events, and every rebuild re-encodes all images.
                    // The actual rebuild happens above once events go quiet.
                    resize_pending = Some(std::time::Instant::now());
                }
                _ => {}
            }

            let current_offset = tabs[active_tab].scroll_offset;
            tabs[active_tab].toc.update_selection(current_offset);
        }
    }
}

/// Put `text` on the clipboard and return the message for the status bar.
///
/// `what` names the thing copied ("84 chars", "code block (bash)") so every
/// copy path reports itself the same way.
fn copy_text(text: &str, mode: ClipboardMode, what: &str) -> String {
    match clipboard::copy(text, mode) {
        CopyOutcome::Copied => format!("copied {what}"),
        CopyOutcome::Disabled => "clipboard disabled in config".to_string(),
        CopyOutcome::Empty => "nothing to copy".to_string(),
        CopyOutcome::TooLarge => "selection too large for clipboard".to_string(),
        CopyOutcome::Failed => "clipboard unavailable".to_string(),
    }
}

/// Translate a screen cell to a document position, or `None` when the click
/// landed outside the document area (top bar, TOC sidebar, status bar).
fn screen_to_doc(area: Rect, tab: &Tab, col: u16, row: u16) -> Option<Pos> {
    if area.width == 0
        || col < area.x
        || row < area.y
        || col >= area.x + area.width
        || row >= area.y + area.height
    {
        return None;
    }
    let line = tab.scroll_offset + (row - area.y) as usize;
    if line >= tab.plain.len() {
        return None;
    }
    // Snap to the start of the grapheme under the pointer, so clicking the
    // right half of a wide character selects that character.
    let col = selection::snap_col(&tab.plain[line], (col - area.x) as usize);
    Some(Pos::new(line, col))
}

/// Why a document cannot be opened in an editor, or `None` when it can.
fn not_editable(tab: &Tab) -> Option<String> {
    if tab.filename == "stdin" || tab.filename == "-" {
        Some("nothing to edit: document came from stdin".into())
    } else if !is_local_file(&tab.filename) {
        Some("nothing to edit: document came from a URL".into())
    } else if tab.mtime.is_none() {
        Some(format!("nothing to edit: {} is not a file", tab.filename))
    } else {
        None
    }
}

/// The heading the viewport is in: the last one at or above its top (one
/// on the second row counts, as for the TOC), with its index.
fn viewport_heading(tab: &Tab) -> Option<(usize, &crate::toc::TocEntry)> {
    tab.toc
        .headings
        .iter()
        .enumerate()
        .rev()
        .find(|(_, h)| h.line_index <= tab.scroll_offset + 1)
}

/// The 1-based line of the file to open the editor at, for the top of the
/// viewport.
///
/// Layout records source positions for headings only, so this is the line
/// of the heading the viewport is in (line 1 at the very top of the
/// document), not the exact line on screen. The line is only trusted when
/// the file on disk (`file`) still has, at that line, what the document was
/// loaded from; otherwise there is no line.
fn editor_line(tab: &Tab, file: &str) -> Option<usize> {
    let content_line = match viewport_heading(tab) {
        Some((_, h)) => h.source_line,
        None if tab.scroll_offset == 0 => 1,
        None => return None,
    };
    let line = content_line.checked_add(tab.line_offset?)?;
    let loaded = tab.source.lines().nth(line.checked_sub(1)?)?;
    (file.lines().nth(line - 1) == Some(loaded)).then_some(line)
}

/// Suspend the TUI, run the editor on the current file, and restore the TUI
/// and reload the file. Returns the message for the status bar.
#[allow(clippy::too_many_arguments)]
fn edit_in_editor(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    tab: &mut Tab,
    args: &Args,
    viewport: usize,
    term_width: u16,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) -> Option<String> {
    if let Some(why) = not_editable(tab) {
        return Some(why);
    }
    let on_disk = std::fs::read_to_string(&tab.filename).unwrap_or_default();
    let line = editor_line(tab, &on_disk);
    let command = crate::editor::editor_command(
        std::env::var("VISUAL").ok().as_deref(),
        std::env::var("EDITOR").ok().as_deref(),
    );
    let Some(argv) = crate::editor::editor_argv(&command, &tab.filename, line) else {
        return Some("no editor: set $VISUAL or $EDITOR".into());
    };
    let outcome = run_suspended(terminal, &argv, args.mouse_capture);
    let reload = reload_after_edit(tab, args, viewport, term_width, gen, graphics);
    match (outcome, reload) {
        (Err(e), _) | (_, Err(e)) => Some(e),
        (Ok(status), Ok(())) if !status.success() => Some(editor_failure(&argv[0], status)),
        (Ok(_), Ok(())) => Some(format!("reloaded {}", short_name(&tab.filename))),
    }
}

fn short_name(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}

fn editor_failure(program: &str, status: std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return format!("{} was killed by signal {sig}", short_name(program));
        }
    }
    match status.code() {
        Some(code) => format!("{} exited with status {code}", short_name(program)),
        None => format!("{} failed", short_name(program)),
    }
}

/// Hand the terminal to `argv` and take it back: the same restore and setup
/// as exit and startup, so every path (the editor failing to start, exiting
/// non-zero, killed by a signal) ends with the TUI restored. Ctrl-C is the
/// editor's while it runs.
fn run_suspended(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    argv: &[String],
    mouse_capture: bool,
) -> Result<std::process::ExitStatus, String> {
    let _ = restore_terminal();
    #[cfg(unix)]
    term_signal::suspend();
    let status = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .status();
    #[cfg(unix)]
    term_signal::resume();
    let back = enter_tui(mouse_capture).and_then(|()| {
        // Everything on screen is gone: a fresh terminal has no previous
        // frame to diff against, so the next draw repaints every cell.
        // (`Terminal::clear` would ask the terminal for its cursor position
        // and stall on one that does not answer.)
        execute!(
            io::stdout(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )?;
        *terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok(())
    });
    back.map_err(|e| format!("could not restore the terminal: {e}"))?;
    status.map_err(|e| format!("could not run {}: {e}", argv[0]))
}

/// Re-read an edited file and keep the reader where it was: on the same
/// heading (matched by text and occurrence, so lines added above it do not
/// matter) at the same distance below it, or at the same offset when there
/// is no such heading.
fn reload_after_edit(
    tab: &mut Tab,
    args: &Args,
    viewport: usize,
    term_width: u16,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) -> Result<(), String> {
    let src = std::fs::read_to_string(&tab.filename)
        .map_err(|e| format!("cannot reload {}: {e}", short_name(&tab.filename)))?;
    let anchor = viewport_heading(tab).map(|(i, h)| {
        let nth = tab.toc.headings[..i]
            .iter()
            .filter(|o| o.text == h.text)
            .count();
        (
            h.text.clone(),
            nth,
            tab.scroll_offset as isize - h.line_index as isize,
        )
    });
    let scroll = tab.scroll_offset;
    let toc_visible = tab.toc.visible;
    let toc_nav = std::mem::take(&mut tab.toc.nav);
    let filename = tab.filename.clone();
    *tab = build_tab(
        src,
        &filename,
        args,
        effective_width(term_width, toc_visible),
        gen,
        graphics,
    );
    tab.toc.visible = toc_visible;
    tab.toc.inherit(toc_nav);
    let max_scroll = tab.ratatui_lines.len().saturating_sub(viewport);
    let same_heading = anchor.and_then(|(text, nth, delta)| {
        let h = tab
            .toc
            .headings
            .iter()
            .filter(|h| h.text == text)
            .nth(nth)?;
        Some((h.line_index as isize + delta).max(0) as usize)
    });
    tab.scroll_offset = same_heading.unwrap_or(scroll).min(max_scroll);
    tab.toc.update_selection(tab.scroll_offset);
    Ok(())
}

/// The link under a document position, if any: the `link_url` of the span
/// covering that display column (wide characters count as two columns,
/// and the left margin is part of the line).
fn link_at(tab: &Tab, pos: Pos) -> Option<&str> {
    use unicode_width::UnicodeWidthStr;
    let mut start = 0;
    for span in &tab.styled_lines.get(pos.line)?.spans {
        let end = start + span.text.width();
        if pos.col < end {
            return span.style.link_url.as_deref();
        }
        start = end;
    }
    None
}

/// Scroll to heading `i` of the TOC, framed the way `n`/`N` frame a heading,
/// and record where the reader was so `[` comes back.
fn jump_to_heading(tab: &mut Tab, nav: &mut NavHistory, i: usize, viewport: usize) {
    let Some(h) = tab.toc.headings.get(i) else {
        return;
    };
    let max_scroll = tab.ratatui_lines.len().saturating_sub(viewport);
    let line = h.line_index.saturating_sub(1).min(max_scroll);
    if line != tab.scroll_offset {
        nav.record(NavEntry::of(tab));
        tab.scroll_offset = line;
    }
    tab.toc.update_selection(line);
}

/// The markdown source of the section the viewport currently starts in, plus a
/// label for the status bar.
///
/// A section runs from its heading to the next heading of the same or higher
/// level — a `##` copies its `###` subsections along with it. A document with
/// no headings copies whole.
fn section_source(tab: &Tab, viewport_height: usize) -> (String, String) {
    let headings = &tab.toc.headings;
    let lines: Vec<&str> = tab.content.lines().collect();
    let Some(current) = headings.get(tab.toc.selected) else {
        return (tab.content.clone(), "document".to_string());
    };
    // `toc.selected` is the last heading at or above the viewport top, and
    // falls back to the first heading when the reader is still above it. In
    // that case the section only counts if its heading is actually on screen;
    // otherwise there is no current section and the whole document is the
    // honest answer.
    if tab.toc.selected == 0
        && current.line_index > tab.scroll_offset
        && current.line_index >= tab.scroll_offset + viewport_height
    {
        return (tab.content.clone(), "document".to_string());
    }
    // sourcepos lines are 1-based.
    let start = current.source_line.saturating_sub(1);
    let end = headings
        .iter()
        .skip(tab.toc.selected + 1)
        .find(|h| h.level <= current.level)
        .map(|h| h.source_line.saturating_sub(1))
        .unwrap_or(lines.len());
    if start >= lines.len() {
        return (String::new(), "nothing".to_string());
    }
    let body = lines[start..end.min(lines.len())]
        .join("\n")
        .trim_end()
        .to_string();
    (body, format!("section \"{}\"", current.text))
}

fn is_local_file(input: &str) -> bool {
    !input.starts_with("http://") && !input.starts_with("https://") && input != "stdin"
}

/// Collect one hint per distinct link URL in the given (visible) lines,
/// labeled a, b, c… Adjacent spans sharing a URL collapse to one hint.
///
/// Each hint carries the link's caption: the text of its consecutive spans,
/// continued across a line wrap when the next line opens with the same link.
fn collect_link_hints(lines: &[crate::layout::StyledLine]) -> Vec<LinkHint> {
    let mut hints: Vec<LinkHint> = Vec::new();
    let mut labels = b'a';
    // The hint whose link ran to the end of the previous line, if any.
    let mut open_at_eol: Option<usize> = None;
    'lines: for line in lines {
        let carried = open_at_eol.take();
        // The hint the current run of link spans feeds, and its URL.
        let mut current: Option<(usize, &str)> = None;
        let mut at_line_start = true;
        // The hint fed by the last link on this line, while nothing but
        // blank padding has followed it.
        let mut tail: Option<usize> = None;
        for span in &line.spans {
            let blank = span.text.trim().is_empty();
            let Some(url) = span.style.link_url.as_deref() else {
                // Any unlinked span ends the run, so `[a](u) [b](u)` stays
                // two links rather than one caption reading "ab".
                current = None;
                if !blank {
                    at_line_start = false;
                    tail = None;
                }
                continue;
            };
            if let Some((i, _)) = current.filter(|&(_, u)| u == url) {
                hints[i].caption.push_str(&span.text);
                continue;
            }
            if let Some(i) = carried.filter(|&i| at_line_start && hints[i].url == url) {
                hints[i].caption.push(' ');
                hints[i].caption.push_str(&span.text);
                current = Some((i, url));
            } else if hints.iter().any(|h| h.url == url) {
                // Already labeled; this repeat contributes no caption.
                current = None;
            } else if labels > b'z' {
                break 'lines;
            } else {
                hints.push(LinkHint {
                    label: labels as char,
                    url: url.to_string(),
                    caption: span.text.clone(),
                });
                labels += 1;
                current = Some((hints.len() - 1, url));
            }
            tail = current.map(|(i, _)| i);
            at_line_start = false;
        }
        open_at_eol = tail;
    }
    for hint in &mut hints {
        hint.caption = link_caption(&hint.caption, &hint.url);
    }
    hints
}

/// Collapse whitespace in a raw link caption, and drop it when it only
/// repeats the URL (an autolink, a bare URL, or a `mailto:` address).
fn link_caption(raw: &str, url: &str) -> String {
    let caption = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let squashed: String = caption.split(' ').collect();
    let bare_url = url.strip_prefix("mailto:").unwrap_or(url);
    if squashed == url || squashed == bare_url {
        String::new()
    } else {
        caption
    }
}

/// Act on a chosen link — the one path for link-hint mode and `Enter`.
///
/// Web/mail URLs open in the default handler. A local `.md` link replaces the
/// tab in place (any readable path, same trust model as images); a `#frag`
/// suffix then scrolls to that heading, and a bare `#frag` scrolls the
/// current document. Every in-ink move is recorded in `nav` first, scroll
/// position included, so `[` comes back to the exact spot.
///
/// Returns a status message to flash when the link leads nowhere.
#[allow(clippy::too_many_arguments)]
fn open_link(
    url: &str,
    tabs: &mut [Tab],
    active_tab: usize,
    nav: &mut NavHistory,
    args: &Args,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) -> Option<String> {
    // Web / mail: hand off to the OS.
    if let Some(safe) = web_target(url) {
        let _ = open::that_detached(&safe);
        return None;
    }
    if !is_followable_local(url) {
        return Some(format!("cannot follow {url}"));
    }
    let size = terminal.size().ok();
    let viewport = size.map(|s| s.height.saturating_sub(3)).unwrap_or(24) as usize;

    if let Some(fragment) = url.strip_prefix('#') {
        let tab = &mut tabs[active_tab];
        let Some(line) = heading_scroll(tab, fragment, viewport) else {
            return Some(format!("no heading #{fragment}"));
        };
        nav.record(NavEntry::of(tab));
        tab.scroll_offset = line;
        return None;
    }

    let base = std::path::Path::new(&tabs[active_tab].filename)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    let Some((src, target, path, fragment)) = read_link_target(&base, url) else {
        let shown = link_targets(url).last().map_or(url, |(path, _)| *path);
        return Some(format!("cannot open {shown}"));
    };
    nav.record(NavEntry::of(&tabs[active_tab]));
    let width = effective_width(size.map(|s| s.width).unwrap_or(80), args.toc);
    let tab = &mut tabs[active_tab];
    *tab = build_tab(
        src,
        target.to_str().unwrap_or(path),
        args,
        width,
        gen,
        graphics,
    );
    let fragment = fragment.filter(|f| !f.is_empty())?;
    match heading_scroll(tab, fragment, viewport) {
        Some(line) => {
            tab.scroll_offset = line;
            None
        }
        None => Some(format!("no heading #{fragment}")),
    }
}

/// A web or mail link, as handed to the OS. (URLs are already
/// scheme-validated by sanitize_url during layout, but re-check
/// defensively.)
fn web_target(url: &str) -> Option<String> {
    let safe = crate::sanitize::sanitize_url(url)?;
    let lower = safe.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:"))
        .then_some(safe)
}

/// The first link `Enter` can follow among the `height` lines from
/// `offset`: a web or mail link, or one ink follows itself.
fn first_followable_link(
    lines: &[crate::layout::StyledLine],
    offset: usize,
    height: u16,
) -> Option<&str> {
    lines
        .iter()
        .skip(offset)
        .take(height as usize)
        .flat_map(|line| &line.spans)
        .filter_map(|span| span.style.link_url.as_deref())
        .find(|url| web_target(url).is_some() || is_followable_local(url))
}

/// Links ink follows itself: a local markdown file, optionally with a
/// `#heading` suffix, or a bare `#heading` in the current document.
fn is_followable_local(url: &str) -> bool {
    match url.strip_prefix('#') {
        Some(fragment) => !fragment.is_empty(),
        None => !link_targets(url).is_empty(),
    }
}

/// Read the file a local link points at, relative to `base`: the first of
/// [`link_targets`] whose file exists wins, so a `#` that is part of the
/// name (`c#-notes.md`) is not taken for a fragment. Returns the source, the
/// resolved path, the link's file part and its fragment.
fn read_link_target<'u>(
    base: &std::path::Path,
    url: &'u str,
) -> Option<(String, std::path::PathBuf, &'u str, Option<&'u str>)> {
    link_targets(url).into_iter().find_map(|(path, fragment)| {
        let target = crate::sanitize::resolve_local_exact(base, path)?;
        let src = std::fs::read_to_string(&target).ok()?;
        Some((src, target, path, fragment))
    })
}

/// The ways to read a local link as (markdown file, fragment), most literal
/// first: the whole link as a file name (`#` is legal in names, and `%23`
/// is decoded when resolving), then split at the last `#`, then at the
/// first. Readings whose file part is not markdown are dropped.
fn link_targets(url: &str) -> Vec<(&str, Option<&str>)> {
    let mut out = vec![(url, None)];
    for i in [url.rfind('#'), url.find('#')].into_iter().flatten() {
        let reading = (&url[..i], Some(&url[i + 1..]));
        if !out.contains(&reading) {
            out.push(reading);
        }
    }
    out.retain(|(path, _)| {
        let lower = path.to_ascii_lowercase();
        !lower.contains("://") && (lower.ends_with(".md") || lower.ends_with(".markdown"))
    });
    out
}

/// GitHub-style anchor for a heading: lowercased, whitespace to `-`, every
/// other character that is not alphanumeric, `-` or `_` dropped.
fn heading_slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

/// Anchors for a document's headings in order. Repeats get `-1`, `-2`, …
/// suffixes the way GitHub numbers them, skipping any already taken.
fn heading_slugs<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut taken = std::collections::HashSet::new();
    texts
        .into_iter()
        .map(|text| {
            let base = heading_slug(text);
            let mut slug = base.clone();
            let mut n = 0;
            while !taken.insert(slug.clone()) {
                n += 1;
                slug = format!("{base}-{n}");
            }
            slug
        })
        .collect()
}

/// Index of the heading a `#fragment` names. The fragment is percent-decoded
/// and normalized like a heading, so `#My%20Notes` finds "My Notes".
fn find_heading<'a>(texts: impl IntoIterator<Item = &'a str>, fragment: &str) -> Option<usize> {
    let decoded = crate::sanitize::percent_decode(fragment).unwrap_or_else(|| fragment.to_string());
    let wanted = heading_slug(&decoded);
    if wanted.is_empty() {
        return None;
    }
    heading_slugs(texts).iter().position(|slug| *slug == wanted)
}

/// Scroll offset that puts the heading named by `fragment` at the top of the
/// viewport, framed the way `n`/`N` heading jumps frame it.
fn heading_scroll(tab: &Tab, fragment: &str, viewport: usize) -> Option<usize> {
    let anchors = &tab.anchors;
    let i = find_heading(anchors.iter().map(|(text, _)| text.as_str()), fragment)?;
    let max_scroll = tab.ratatui_lines.len().saturating_sub(viewport);
    Some(anchors[i].1.saturating_sub(1).min(max_scroll))
}

/// Show a history entry: the same document just scrolls back; anything else
/// is rebuilt from the source the entry carries — or, for a local file that
/// has changed on disk since, from the file (as v0.8.0 always did).
fn restore_nav_entry(
    tab: &mut Tab,
    mut entry: NavEntry,
    args: &Args,
    width: u16,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) {
    // A slide's source is one slide, not the file: never swap in the file.
    if !args.slides {
        let now = std::fs::metadata(&entry.filename)
            .and_then(|m| m.modified())
            .ok();
        if file_changed(entry.mtime, now) {
            if let Ok(fresh) = std::fs::read_to_string(&entry.filename) {
                entry.source = fresh.into();
                entry.mtime = now;
            }
        }
    }
    let same_source = Arc::ptr_eq(&tab.source, &entry.source) || tab.source == entry.source;
    if tab.filename != entry.filename || !same_source {
        *tab = build_tab(
            Arc::clone(&entry.source),
            &entry.filename,
            args,
            width,
            gen,
            graphics,
        );
        tab.mtime = entry.mtime;
    }
    tab.scroll_offset = entry
        .scroll_offset
        .min(tab.ratatui_lines.len().saturating_sub(1));
}

/// Rebuild one tab for the current width/theme, preserving scroll and TOC
/// visibility. Only the tab the user is looking at is rebuilt eagerly; the
/// rest are refreshed lazily the next time they're switched to.
/// The width the document is actually laid out in: the TOC pane (when open
/// on a wide-enough terminal, mirroring the draw-time gate) takes its columns
/// out of the budget. Toggling the TOC previously kept the full-width layout
/// and truncated every line at draw time.
fn effective_width(full: u16, toc_visible: bool) -> u16 {
    const TOC_PANE: u16 = 30;
    if toc_visible && full > 40 {
        full.saturating_sub(TOC_PANE)
    } else {
        full
    }
}

fn rebuild_tab(
    tab: &mut Tab,
    args: &Args,
    term_width: u16,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) {
    let source = Arc::clone(&tab.source);
    let filename = tab.filename.clone();
    let scroll = tab.scroll_offset;
    let toc_visible = tab.toc.visible;
    let toc_nav = std::mem::take(&mut tab.toc.nav);
    let mtime = tab.mtime;
    let width = effective_width(term_width, toc_visible);
    *tab = build_tab(source, &filename, args, width, gen, graphics);
    tab.toc.inherit(toc_nav);
    // Same source as before, so the same snapshot time.
    tab.mtime = mtime;
    // Clamp: the new layout may have far fewer lines (e.g. after widening) —
    // an unclamped stale offset blanked the viewport and panicked link-mode.
    tab.scroll_offset = scroll.min(tab.ratatui_lines.len().saturating_sub(1));
    tab.toc.visible = toc_visible;
    tab.toc.update_selection(tab.scroll_offset);
}

/// Refresh a tab only if it was laid out for a different width or theme.
fn ensure_tab_current(
    tab: &mut Tab,
    args: &Args,
    term_width: u16,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) {
    if tab.built_width != effective_width(term_width, tab.toc.visible) || tab.built_gen != gen {
        rebuild_tab(tab, args, term_width, gen, graphics);
    }
}

/// Has a file changed since a snapshot of it was taken? Only a local file
/// has a snapshot time (`Some`); a file that is gone (`now == None`) keeps
/// its snapshot.
fn file_changed(snapshot: Option<SystemTime>, now: Option<SystemTime>) -> bool {
    matches!((snapshot, now), (Some(then), Some(now)) if then != now)
}

fn build_tab(
    source: impl Into<Arc<str>>,
    filename: &str,
    args: &Args,
    term_width: u16,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) -> Tab {
    let source: Arc<str> = source.into();
    let mtime = if is_local_file(filename) {
        std::fs::metadata(filename).and_then(|m| m.modified()).ok()
    } else {
        None
    };
    // Frontmatter is stripped, or (with --frontmatter) swapped for a fenced
    // block the layout draws as a metadata box.
    let stripped = frontmatter::prepare(&source, args.frontmatter);

    // Pre-process wikilinks before parsing
    let content = crate::wikilink::process_wikilinks(&stripped);
    // Source line = content line + offset. Only the head of the document
    // changes: stripping removes whole lines, and the metadata box usually
    // keeps the line count. If the parsed text has MORE lines than the source
    // (JSON frontmatter gains fence lines), or wikilinks ever change the line
    // count, the mapping is unknown and no line is passed to the editor.
    let line_offset = source
        .matches('\n')
        .count()
        .checked_sub(stripped.matches('\n').count())
        .filter(|_| content.matches('\n').count() == stripped.matches('\n').count());

    let arena = Arena::new();
    let options = crate::parser::options_for(&content);
    let root = parse_document(&arena, &content, &options);

    // Content must fit within the terminal even after the left margin, so cap
    // it at term_width - 4 (a requested --width wider than the terminal would
    // otherwise overflow and be clipped at the right edge).
    let hard_cap = term_width.saturating_sub(4).clamp(8, 120);
    let max_content_width = args.width.unwrap_or(hard_cap).clamp(8, hard_cap);
    let center_margin = if term_width > max_content_width + 4 {
        ((term_width - max_content_width) / 2) as usize
    } else {
        2
    };

    // Resolve base directory for relative image paths
    let base_dir = std::path::Path::new(filename).parent();

    let layout::LayoutResult {
        lines: styled_lines,
        headings,
        images: image_specs,
        code_blocks,
    } = layout::layout_document_with_source(
        root,
        Some(&content),
        &theme::resolve_theme(&args.theme),
        max_content_width,
        args.spacing,
        center_margin,
        base_dir,
        args.images,
        graphics.font_size(),
    );
    // Turn reserved image specs into renderable placements (graphics mode only).
    let images: Vec<ImagePlacement> = image_specs
        .into_iter()
        .map(|spec| {
            let proto = graphics.build((*spec.image).clone(), spec.cols, spec.rows);
            ImagePlacement {
                line_index: spec.line_index,
                col_offset: spec.col_offset,
                rows: spec.rows,
                protocol: proto,
            }
        })
        .collect();
    let ratatui_lines = render::styled_lines_to_ratatui(&styled_lines, &args.theme);
    // Per-span, joined with a separator no typed query can contain, and
    // lowercased char-by-char — the same algorithm the highlighter uses. A
    // query that straddled a span boundary used to count as a match that
    // nothing highlighted; now count and highlight agree by construction.
    let lowered: Vec<String> = styled_lines
        .iter()
        .map(|line| {
            let mut joined = String::new();
            for (i, span) in line.spans.iter().enumerate() {
                if i > 0 {
                    joined.push('\u{1}');
                }
                joined.extend(span.text.chars().flat_map(char::to_lowercase));
            }
            joined
        })
        .collect();

    let (word_count, reading_time) = stats::document_stats(&content);

    let anchors: Vec<(String, usize)> = headings
        .iter()
        .map(|h| (h.anchor.clone(), h.line_index))
        .collect();

    // Headings carry their exact display-line index straight from layout —
    // no substring reverse-scan, and duplicate/split headings map correctly.
    let toc_entries: Vec<crate::toc::TocEntry> = headings
        .into_iter()
        .filter(|h| !h.text.is_empty())
        .map(|h| crate::toc::TocEntry {
            level: h.level,
            text: h.text,
            line_index: h.line_index,
            source_line: h.source_line,
        })
        .collect();

    let mut toc = TocState::empty();
    toc.headings = toc_entries;
    toc.visible = args.toc;

    let plain = crate::selection::plain_lines(&styled_lines);

    Tab {
        filename: filename.to_string(),
        source,
        mtime,
        content,
        line_offset,
        styled_lines,
        ratatui_lines,
        plain,
        code_blocks,
        lowered,
        scroll_offset: 0,
        toc,
        anchors,
        word_count,
        reading_time,
        built_width: term_width,
        built_gen: gen,
        images,
    }
}

#[cfg(test)]
mod link_hint_tests {
    use super::*;
    use crate::layout::{SpanStyle, StyledLine, StyledSpan};

    fn text(t: &str) -> StyledSpan {
        StyledSpan {
            text: t.to_string(),
            style: SpanStyle::default(),
        }
    }

    fn link(t: &str, url: &str, bold: bool) -> StyledSpan {
        StyledSpan {
            text: t.to_string(),
            style: SpanStyle {
                underline: true,
                bold,
                link_url: Some(url.to_string()),
                ..Default::default()
            },
        }
    }

    fn line(spans: Vec<StyledSpan>) -> StyledLine {
        StyledLine { spans }
    }

    fn rows(hints: &[LinkHint]) -> Vec<(char, &str, &str)> {
        hints
            .iter()
            .map(|h| (h.label, h.caption.as_str(), h.url.as_str()))
            .collect()
    }

    #[test]
    fn caption_is_the_link_text() {
        let lines = vec![line(vec![
            text("See the "),
            link("ink docs", "https://example.com/docs", false),
            text(" for more."),
        ])];
        let hints = collect_link_hints(&lines);
        assert_eq!(
            rows(&hints),
            vec![('a', "ink docs", "https://example.com/docs")]
        );
    }

    #[test]
    fn multi_span_caption_is_joined() {
        let lines = vec![line(vec![
            link("the ", "https://e.com", false),
            link("bold", "https://e.com", true),
            link(" part", "https://e.com", false),
        ])];
        let hints = collect_link_hints(&lines);
        assert_eq!(rows(&hints), vec![('a', "the bold part", "https://e.com")]);
    }

    #[test]
    fn wrapped_caption_joins_with_one_space() {
        let lines = vec![
            line(vec![
                text("See the "),
                link("ink  ", "https://e.com/docs", false),
            ]),
            line(vec![
                text("  "),
                link("docs", "https://e.com/docs", false),
                text(" and more"),
            ]),
        ];
        let hints = collect_link_hints(&lines);
        assert_eq!(rows(&hints), vec![('a', "ink docs", "https://e.com/docs")]);
    }

    #[test]
    fn a_later_line_starting_with_other_text_does_not_continue() {
        let lines = vec![
            line(vec![link("first", "https://e.com", false)]),
            line(vec![text("x "), link("again", "https://e.com", false)]),
        ];
        let hints = collect_link_hints(&lines);
        assert_eq!(rows(&hints), vec![('a', "first", "https://e.com")]);
    }

    #[test]
    fn adjacent_links_to_one_url_do_not_merge() {
        let lines = vec![line(vec![
            link("a", "https://e.com", false),
            text(" "),
            link("b", "https://e.com", false),
        ])];
        let hints = collect_link_hints(&lines);
        assert_eq!(rows(&hints), vec![('a', "a", "https://e.com")]);
    }

    #[test]
    fn autolink_caption_is_suppressed() {
        let lines = vec![line(vec![
            link("https://x.y", "https://x.y", false),
            text(" and "),
            link("me@x.y", "mailto:me@x.y", false),
        ])];
        let hints = collect_link_hints(&lines);
        assert_eq!(
            rows(&hints),
            vec![('a', "", "https://x.y"), ('b', "", "mailto:me@x.y")]
        );
    }

    #[test]
    fn hints_dedupe_by_url_keeping_the_first_caption() {
        let lines = vec![
            line(vec![
                link("one", "https://a", false),
                text(" "),
                link("two", "https://b", false),
            ]),
            line(vec![link("uno", "https://a", false)]),
        ];
        let hints = collect_link_hints(&lines);
        assert_eq!(
            rows(&hints),
            vec![('a', "one", "https://a"), ('b', "two", "https://b")]
        );
    }

    #[test]
    fn labels_stop_at_z() {
        let lines: Vec<StyledLine> = (0..30)
            .map(|i| line(vec![link("x", &format!("https://e.com/{i}"), false)]))
            .collect();
        let hints = collect_link_hints(&lines);
        assert_eq!(hints.len(), 26);
        assert_eq!(hints.last().map(|h| h.label), Some('z'));
    }

    #[test]
    fn laid_out_markdown_yields_wrapped_captions() {
        use comrak::{parse_document, Arena};
        let arena = Arena::new();
        let src = "See the [ink project documentation](https://example.com/docs) and <https://x.y>";
        let root = parse_document(&arena, src, &crate::parser::options());
        let theme = crate::theme::resolve_theme("dark");
        let lines = crate::layout::layout_document(
            root,
            &theme,
            24,
            crate::Spacing::Normal,
            0,
            None,
            crate::image::ImageMode::Off,
            None,
        )
        .lines;
        let hints = collect_link_hints(&lines);
        assert_eq!(
            rows(&hints),
            vec![
                ('a', "ink project documentation", "https://example.com/docs"),
                ('b', "", "https://x.y"),
            ]
        );
    }
}

#[cfg(test)]
mod section_tests {
    use super::*;

    /// A tab carrying just the fields the section/mouse helpers read.
    fn tab(content: &str, headings: &[(u8, &str, usize, usize)], scroll: usize) -> Tab {
        let plain: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        let mut toc = TocState::empty();
        toc.headings = headings
            .iter()
            .map(
                |(level, text, line_index, source_line)| crate::toc::TocEntry {
                    level: *level,
                    text: (*text).to_string(),
                    line_index: *line_index,
                    source_line: *source_line,
                },
            )
            .collect();
        toc.update_selection(scroll);
        let anchors = headings
            .iter()
            .map(|(_, text, line_index, _)| ((*text).to_string(), *line_index))
            .collect();
        Tab {
            filename: "t.md".into(),
            source: content.into(),
            mtime: None,
            content: content.into(),
            line_offset: Some(0),
            styled_lines: Vec::new(),
            ratatui_lines: Vec::new(),
            plain,
            code_blocks: Vec::new(),
            lowered: Vec::new(),
            scroll_offset: scroll,
            toc,
            anchors,
            word_count: 0,
            reading_time: 0,
            built_width: 80,
            built_gen: 0,
            images: Vec::new(),
        }
    }

    const DOC: &str =
        "# Top\n\nintro\n\n## Alpha\n\na body\n\n### Alpha sub\n\nnested\n\n## Beta\n\nb body\n";
    // (level, text, display line, source line)
    const HEADINGS: &[(u8, &str, usize, usize)] = &[
        (1, "Top", 2, 1),
        (2, "Alpha", 8, 5),
        (3, "Alpha sub", 14, 9),
        (2, "Beta", 20, 13),
    ];

    #[test]
    fn a_section_runs_to_the_next_heading_of_the_same_level() {
        // Sitting inside Alpha: its subsection comes along, Beta does not.
        let t = tab(DOC, HEADINGS, 8);
        let (body, label) = section_source(&t, 20);
        assert_eq!(label, "section \"Alpha\"");
        assert!(body.starts_with("## Alpha"));
        assert!(
            body.contains("### Alpha sub"),
            "subsection must be included"
        );
        assert!(
            !body.contains("## Beta"),
            "must stop at the next same-level heading"
        );
    }

    #[test]
    fn a_subsection_stops_at_its_own_level() {
        let t = tab(DOC, HEADINGS, 14);
        let (body, label) = section_source(&t, 20);
        assert_eq!(label, "section \"Alpha sub\"");
        assert_eq!(body, "### Alpha sub\n\nnested");
    }

    #[test]
    fn the_last_section_runs_to_the_end_of_the_document() {
        let t = tab(DOC, HEADINGS, 20);
        let (body, _) = section_source(&t, 20);
        assert_eq!(body, "## Beta\n\nb body");
    }

    #[test]
    fn a_document_without_headings_copies_whole() {
        let t = tab("just prose\n\nmore prose\n", &[], 0);
        let (body, label) = section_source(&t, 20);
        assert_eq!(label, "document");
        assert_eq!(body, "just prose\n\nmore prose\n");
    }

    #[test]
    fn scrolled_past_every_heading_still_resolves_the_last_one() {
        let t = tab(DOC, HEADINGS, 99);
        let (_, label) = section_source(&t, 20);
        assert_eq!(label, "section \"Beta\"");
    }

    #[test]
    fn a_viewport_that_has_not_reached_the_first_heading_copies_the_document() {
        // Heading at display line 40, viewport is 20 rows from the top: the
        // reader is not in any section yet.
        let t = tab(DOC, &[(1, "Top", 40, 1)], 0);
        let (_, label) = section_source(&t, 20);
        assert_eq!(label, "document");
    }

    #[test]
    fn screen_to_doc_maps_only_inside_the_document_area() {
        // Ten lines, scrolled to line 5 — the doc must outlast the scroll
        // offset or every lookup is out of range for the wrong reason.
        let doc: String = (0..10).map(|i| format!("line {i}\n")).collect();
        let t = tab(&doc, &[], 5);
        let area = Rect::new(4, 2, 40, 10);
        // Top-left of the area is the first line of the current scroll window.
        assert_eq!(screen_to_doc(area, &t, 4, 2), Some(Pos::new(5, 0)));
        // Outside on every side.
        assert_eq!(screen_to_doc(area, &t, 3, 2), None);
        assert_eq!(screen_to_doc(area, &t, 4, 1), None);
        assert_eq!(screen_to_doc(area, &t, 44, 2), None);
        assert_eq!(screen_to_doc(area, &t, 4, 12), None);
        // Past the end of the document (scroll 5 + row 5 = line 10 of 10).
        assert_eq!(screen_to_doc(area, &t, 4, 7), None);
        // A never-drawn area cannot resolve anything.
        assert_eq!(screen_to_doc(Rect::new(0, 0, 0, 0), &t, 0, 0), None);
    }

    #[test]
    fn slugs_follow_github_rules() {
        assert_eq!(heading_slug("Getting Started"), "getting-started");
        assert_eq!(heading_slug("What's new in v0.8?"), "whats-new-in-v08");
        assert_eq!(heading_slug("C++ & Rust"), "c--rust");
        assert_eq!(heading_slug("snake_case and-kebab"), "snake_case-and-kebab");
        assert_eq!(heading_slug("  Trimmed  "), "trimmed");
        assert_eq!(heading_slug("Café Ünïcode"), "café-ünïcode");
        assert_eq!(heading_slug("`code` in **bold**"), "code-in-bold");
    }

    #[test]
    fn duplicate_headings_get_numbered_suffixes() {
        let slugs = heading_slugs(["Usage", "Notes", "Usage", "Usage", "Usage-1"]);
        assert_eq!(slugs, ["usage", "notes", "usage-1", "usage-2", "usage-1-1"]);
    }

    #[test]
    fn fragments_are_percent_decoded_and_matched_to_headings() {
        let texts = ["Intro", "My Notes", "Intro", "Ünïcode"];
        assert_eq!(find_heading(texts, "intro"), Some(0));
        assert_eq!(find_heading(texts, "intro-1"), Some(2));
        assert_eq!(find_heading(texts, "my-notes"), Some(1));
        assert_eq!(find_heading(texts, "My%20Notes"), Some(1));
        assert_eq!(find_heading(texts, "%C3%BCn%C3%AFcode"), Some(3));
        assert_eq!(find_heading(texts, "missing"), None);
        assert_eq!(find_heading(texts, ""), None);
    }

    #[test]
    fn fragment_splitting_and_followable_links() {
        assert_eq!(link_targets("doc.md#a"), [("doc.md", Some("a"))]);
        assert_eq!(link_targets("doc.md"), [("doc.md", None)]);
        // Literal name first, then the last `#`, then the first.
        assert_eq!(
            link_targets("c#-notes.md#intro"),
            [("c#-notes.md", Some("intro"))]
        );
        assert_eq!(link_targets("issue#12.md"), [("issue#12.md", None)]);
        assert_eq!(
            link_targets("a#b.md#c.md"),
            [("a#b.md#c.md", None), ("a#b.md", Some("c.md"))]
        );
        assert!(link_targets("image.png#frag").is_empty());
        assert!(is_followable_local("#section"));
        assert!(is_followable_local("c#-notes.md"));
        assert!(is_followable_local("issue#12.md"));
        assert!(is_followable_local("docs/x.md#section"));
        assert!(is_followable_local("x.MARKDOWN"));
        assert!(!is_followable_local("#"));
        assert!(!is_followable_local("https://e.com/x.md"));
        assert!(!is_followable_local("image.png#frag"));
    }

    /// Arguments for laying a document out the way the reader does.
    pub(super) fn test_args() -> Args {
        Args {
            inputs: Vec::new(),
            theme: "dark".into(),
            width: None,
            slides: false,
            plain: false,
            watch: false,
            toc: false,
            images: crate::image::ImageMode::Off,
            image_protocol: crate::graphics::ProtocolChoice::HalfBlocks,
            frontmatter: false,
            spacing: crate::Spacing::Normal,
            mouse_capture: false,
            clipboard: crate::clipboard::ClipboardMode::Auto,
        }
    }

    /// A tab built through the real pipeline: wikilinks, parse, layout.
    pub(super) fn built(src: &str) -> Tab {
        let graphics = crate::graphics::Graphics::halfblocks();
        build_tab(src, "t.md", &test_args(), 80, 0, &graphics)
    }

    /// The display line `#fragment` scrolls to, or `None`.
    fn anchor_line(t: &Tab, fragment: &str) -> Option<usize> {
        let i = find_heading(t.anchors.iter().map(|(a, _)| a.as_str()), fragment)?;
        Some(t.anchors[i].1)
    }

    #[test]
    fn anchors_see_inline_code_dashes_shortcodes_and_links() {
        let src = "# Install `ink` CLI\n\n### `--width`\n\n## a -- b\n\n\
                   ## :rocket: Launch\n\n## See [the docs](https://e.com) now\n\n\
                   ## Dash — literal\n\n## Usage\n\n## Usage\n\n## Usage\n";
        let t = built(src);
        let lines: Vec<usize> = t.anchors.iter().map(|(_, l)| *l).collect();
        assert_eq!(lines.len(), 9, "{:?}", t.anchors);
        for (fragment, i) in [
            ("install-ink-cli", 0),
            ("--width", 1),
            ("a----b", 2),
            ("rocket-launch", 3),
            ("see-the-docs-now", 4),
            ("dash--literal", 5),
            ("usage", 6),
            ("usage-1", 7),
            ("usage-2", 8),
        ] {
            assert_eq!(anchor_line(&t, fragment), Some(lines[i]), "#{fragment}");
        }
        // The old, display-text-derived slugs no longer match.
        assert_eq!(anchor_line(&t, "install--cli"), None);
        // The TOC still shows what it showed: no code, no empty entries.
        let toc: Vec<&str> = t.toc.headings.iter().map(|h| h.text.as_str()).collect();
        assert_eq!(toc[0], "Install  CLI");
        assert!(!toc.contains(&""));
        assert!(toc.contains(&"a – b"), "{toc:?}");
    }

    #[test]
    fn anchor_text_undoes_smart_dashes_without_the_source() {
        use comrak::{parse_document, Arena};
        let arena = Arena::new();
        let root = parse_document(&arena, "## a -- b --- c\n", &crate::parser::options());
        let theme = crate::theme::resolve_theme("dark");
        let headings = crate::layout::layout_document(
            root,
            &theme,
            80,
            crate::Spacing::Normal,
            0,
            None,
            crate::image::ImageMode::Off,
            None,
        )
        .headings;
        assert_eq!(headings[0].anchor, "a -- b --- c");
    }

    #[test]
    fn links_to_files_with_hash_in_the_name_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::write(base.join("c#-notes.md"), "# C sharp\n").unwrap();
        std::fs::write(base.join("issue#12.md"), "# Twelve\n").unwrap();
        std::fs::write(base.join("other.md"), "# Other\n\n## Intro\n").unwrap();
        let read = |url| {
            read_link_target(base, url)
                .map(|(src, _, path, frag)| (src.lines().next().unwrap().to_string(), path, frag))
        };
        assert_eq!(
            read("c#-notes.md"),
            Some(("# C sharp".into(), "c#-notes.md", None))
        );
        assert_eq!(
            read("issue#12.md"),
            Some(("# Twelve".into(), "issue#12.md", None))
        );
        // `%23` is a `#` in the name.
        assert_eq!(
            read("issue%2312.md"),
            Some(("# Twelve".into(), "issue%2312.md", None))
        );
        // A `#` after the file name is still a fragment.
        assert_eq!(
            read("other.md#intro"),
            Some(("# Other".into(), "other.md", Some("intro")))
        );
        assert_eq!(
            read("c#-notes.md#top"),
            Some(("# C sharp".into(), "c#-notes.md", Some("top")))
        );
        assert_eq!(read("missing.md#x"), None);
    }

    #[test]
    fn wikilink_sections_jump_to_the_heading_text() {
        // `[[page#Section Name]]` → `page.md` + the heading text, matched by
        // slugging it like a heading; `[[#Section]]` stays in the document.
        let t = built("# Top\n\nSee [[#My Section]].\n\n## My Section\n\nbody\n");
        let urls: Vec<String> = t
            .styled_lines
            .iter()
            .flat_map(|l| &l.spans)
            .filter_map(|s| s.style.link_url.clone())
            .collect();
        assert_eq!(urls, ["#My Section"]);
        assert!(is_followable_local(&urls[0]));
        let i = find_heading(
            t.anchors.iter().map(|(a, _)| a.as_str()),
            urls[0].trim_start_matches('#'),
        );
        assert_eq!(i, Some(1));
        assert_eq!(
            crate::wikilink::resolve_target("notes#Install `ink` CLI"),
            "notes.md#Install `ink` CLI"
        );
    }

    #[test]
    fn anchor_scroll_lands_like_heading_jumps() {
        let mut t = tab(DOC, HEADINGS, 0);
        t.ratatui_lines = vec![Line::default(); 40];
        assert_eq!(heading_scroll(&t, "beta", 10), Some(19));
        assert_eq!(heading_scroll(&t, "alpha-sub", 10), Some(13));
        // Clamped so the last screen stays full.
        assert_eq!(heading_scroll(&t, "beta", 30), Some(10));
        assert_eq!(heading_scroll(&t, "gamma", 10), None);
    }

    #[test]
    fn enter_follows_the_first_followable_link_anywhere_on_screen() {
        let mut doc: String = (0..12).map(|i| format!("para {i}\n\n")).collect();
        doc.push_str("[bad](javascript:alert) [site](https://example.com) [doc](other.md)\n");
        let t = built(&doc);
        let line = t.plain.iter().position(|l| l.contains("site")).unwrap();
        // Far below the old six-line window, but on screen.
        assert!(line > 6);
        assert_eq!(
            first_followable_link(&t.styled_lines, 0, line as u16 + 1),
            Some("https://example.com")
        );
        // Not on screen: nothing.
        assert_eq!(first_followable_link(&t.styled_lines, 0, line as u16), None);
        assert_eq!(web_target("mailto:a@b.c").as_deref(), Some("mailto:a@b.c"));
        assert_eq!(web_target("javascript:alert(1)"), None);
        assert_eq!(web_target("other.md"), None);
    }

    #[test]
    fn link_at_counts_display_columns_past_wide_characters() {
        use unicode_width::UnicodeWidthStr;
        let t = built("漢字 [go](#x) after\n");
        let line = t.plain.iter().position(|l| l.contains("go")).unwrap();
        let text = &t.plain[line];
        let col = text[..text.find("go").unwrap()].width();
        assert_eq!(link_at(&t, Pos::new(line, col)), Some("#x"));
        assert_eq!(link_at(&t, Pos::new(line, col + 1)), Some("#x"));
        assert_eq!(link_at(&t, Pos::new(line, col + 2)), None);
        assert_eq!(link_at(&t, Pos::new(line, col - 1)), None);
        // The left margin is not a link either.
        assert_eq!(link_at(&t, Pos::new(line, 0)), None);
        assert_eq!(link_at(&t, Pos::new(999, 0)), None);
    }

    const FM_DOC: &str = "---\ntitle: x\n---\n# A\n\ntext\n\n## B\n\nmore\n";

    fn scrolled_to(mut t: Tab, heading: &str) -> Tab {
        let h = t.toc.headings.iter().find(|h| h.text == heading).unwrap();
        t.scroll_offset = h.line_index.saturating_sub(1);
        t
    }

    #[test]
    fn the_editor_line_is_the_viewport_heading_in_file_coordinates() {
        let t = built(FM_DOC);
        // Three frontmatter lines sit above the parsed content.
        assert_eq!(t.line_offset, Some(3));
        let t = scrolled_to(t, "B");
        assert_eq!(editor_line(&t, FM_DOC), Some(8));
        assert_eq!(FM_DOC.lines().nth(7), Some("## B"));
        // Scrolled a little past the heading: still that heading's line.
        let mut t = t;
        t.scroll_offset += 2;
        assert_eq!(editor_line(&t, FM_DOC), Some(8));
        // At the very top: the first heading.
        let t = built(FM_DOC);
        assert_eq!(editor_line(&t, FM_DOC), Some(4));
    }

    #[test]
    fn the_editor_line_is_dropped_when_it_cannot_be_trusted() {
        let t = scrolled_to(built(FM_DOC), "B");
        // The file changed on disk at that line since it was loaded.
        let edited = FM_DOC.replace("## B", "## Renamed");
        assert_eq!(editor_line(&t, &edited), None);
        // An unknown source-to-file mapping.
        let mut t = t;
        t.line_offset = None;
        assert_eq!(editor_line(&t, FM_DOC), None);
        // Scrolled into text above any heading.
        let mut t = built("intro\n\n\n\n\n\n\n\n# Late\n");
        t.scroll_offset = 2;
        assert_eq!(editor_line(&t, "intro\n\n\n\n\n\n\n\n# Late\n"), None);
    }

    #[test]
    fn only_local_files_are_editable() {
        let mut t = built("# x\n");
        t.filename = "stdin".into();
        assert!(not_editable(&t).unwrap().contains("stdin"));
        t.filename = "https://example.com/a.md".into();
        assert!(not_editable(&t).unwrap().contains("URL"));
        t.filename = "t.md".into();
        t.mtime = None;
        assert!(not_editable(&t).is_some());
        t.mtime = Some(SystemTime::now());
        assert_eq!(not_editable(&t), None);
    }

    #[test]
    fn reloading_after_an_edit_stays_on_the_same_heading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.md");
        let doc: String = (0..30)
            .map(|i| format!("## H{i}\n\nbody {i}\n\n"))
            .collect();
        std::fs::write(&path, &doc).unwrap();
        let graphics = crate::graphics::Graphics::halfblocks();
        let args = test_args();
        let name = path.to_str().unwrap();
        let t = build_tab(doc.as_str(), name, &args, 80, 0, &graphics);
        let mut t = scrolled_to(t, "H20");
        t.scroll_offset += 1;
        // Lines added above the heading the reader is in.
        std::fs::write(&path, format!("# New\n\nadded\n\nlines\n\n{doc}")).unwrap();
        reload_after_edit(&mut t, &args, 10, 80, 0, &graphics).unwrap();
        let h20 = t.toc.headings.iter().find(|h| h.text == "H20").unwrap();
        assert_eq!(t.scroll_offset, h20.line_index);
        assert!(t.content.starts_with("# New"));
        // A file that is gone reports it.
        std::fs::remove_file(&path).unwrap();
        let err = reload_after_edit(&mut t, &args, 10, 80, 0, &graphics).unwrap_err();
        assert!(err.contains("cannot reload"), "{err}");
    }

    #[test]
    fn a_toc_jump_is_recorded_in_history_and_marks_the_heading() {
        let mut t = tab(DOC, HEADINGS, 0);
        t.ratatui_lines = vec![Line::default(); 40];
        let mut nav = NavHistory::default();
        jump_to_heading(&mut t, &mut nav, 3, 10);
        assert_eq!(t.scroll_offset, 19);
        assert_eq!(t.toc.selected, 3);
        assert_eq!(nav.back.last().map(|e| e.scroll_offset), Some(0));
        // Jumping to where the reader already is records nothing.
        jump_to_heading(&mut t, &mut nav, 3, 10);
        assert_eq!(nav.back.len(), 1);
    }

    fn entry(name: &str, scroll: usize) -> NavEntry {
        NavEntry {
            filename: name.into(),
            source: format!("# {name}").into(),
            mtime: None,
            scroll_offset: scroll,
        }
    }

    #[test]
    fn an_in_page_jump_records_a_pointer_not_a_copy() {
        let t = built(DOC);
        let mut nav = NavHistory::default();
        nav.record(NavEntry::of(&t));
        nav.record(NavEntry::of(&t));
        assert!(nav.back.iter().all(|e| Arc::ptr_eq(&e.source, &t.source)));
        // Back and forward hand the same allocation around too.
        let back = nav.go_back(NavEntry::of(&t)).unwrap();
        assert!(Arc::ptr_eq(&back.source, &t.source));
        assert!(Arc::ptr_eq(&nav.forward[0].source, &t.source));
    }

    #[test]
    fn history_is_capped_dropping_the_oldest() {
        let mut nav = NavHistory::default();
        for i in 0..NAV_HISTORY_CAP + 44 {
            nav.record(entry("a.md", i));
        }
        assert_eq!(nav.back.len(), NAV_HISTORY_CAP);
        assert_eq!(nav.back[0].scroll_offset, 44);
        // Forward steps that refill the back stack respect the cap too.
        let here = nav.go_back(entry("b.md", 0)).unwrap();
        nav.go_forward(here).unwrap();
        assert_eq!(nav.back.len(), NAV_HISTORY_CAP);
    }

    #[test]
    fn going_back_rereads_a_file_only_when_it_changed() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let t1 = t0 + Duration::from_secs(5);
        assert!(file_changed(Some(t0), Some(t1)), "edited since");
        assert!(!file_changed(Some(t0), Some(t0)), "unchanged");
        assert!(!file_changed(Some(t0), None), "deleted: keep the snapshot");
        assert!(!file_changed(None, Some(t1)), "stdin / URL: no file");
        assert!(!file_changed(None, None));
    }

    #[test]
    fn local_files_carry_their_mtime_and_stdin_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.md");
        std::fs::write(&path, "# Doc\n").unwrap();
        let graphics = crate::graphics::Graphics::halfblocks();
        let file = build_tab(
            "# Doc\n".to_string(),
            path.to_str().unwrap(),
            &test_args(),
            80,
            0,
            &graphics,
        );
        assert!(file.mtime.is_some());
        let stdin = build_tab(
            "# Doc\n".to_string(),
            "stdin",
            &test_args(),
            80,
            0,
            &graphics,
        );
        assert_eq!(stdin.mtime, None);
    }

    #[test]
    fn history_returns_to_the_recorded_position() {
        let mut nav = NavHistory::default();
        // a.md at line 12 -> #anchor jump to line 40 -> b.md
        nav.record(entry("a.md", 12));
        nav.record(entry("a.md", 40));
        let back = nav.go_back(entry("b.md", 0)).unwrap();
        assert_eq!(back, entry("a.md", 40));
        let back = nav.go_back(back).unwrap();
        assert_eq!(back, entry("a.md", 12));
        assert_eq!(nav.go_back(back.clone()), None);
        // Forward retraces the path.
        let fwd = nav.go_forward(back).unwrap();
        assert_eq!(fwd, entry("a.md", 40));
        assert_eq!(nav.go_forward(fwd).unwrap(), entry("b.md", 0));
    }

    #[test]
    fn following_a_new_link_drops_the_forward_stack() {
        let mut nav = NavHistory::default();
        nav.record(entry("a.md", 0));
        let back = nav.go_back(entry("b.md", 5)).unwrap();
        nav.record(back);
        assert_eq!(nav.go_forward(entry("c.md", 0)), None);
    }

    #[test]
    fn history_entries_carry_the_loaded_source() {
        let mut t = tab(DOC, HEADINGS, 7);
        t.filename = "stdin".into();
        let e = NavEntry::of(&t);
        assert_eq!(e.filename, "stdin");
        assert_eq!(&*e.source, DOC);
        assert_eq!(e.scroll_offset, 7);
    }
}
