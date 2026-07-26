use crate::input::{self, Action};
use crate::layout;
use crate::parser::frontmatter;
use crate::render;
use crate::search::SearchState;
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
use std::time::Duration;

struct Tab {
    filename: String,
    #[allow(dead_code)]
    source: String,
    styled_lines: Vec<crate::layout::StyledLine>,
    ratatui_lines: Vec<Line<'static>>,
    /// Per-line text, lowercased once, for allocation-free search scans.
    lowered: Vec<String>,
    scroll_offset: u16,
    toc: TocState,
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
    protocol: ratatui_image::sliced::SlicedProtocol,
}

struct NavEntry {
    filename: String,
    scroll_offset: u16,
}

/// A labeled link in the current viewport for hint-mode selection.
struct LinkHint {
    label: char,
    url: String,
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
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
            default_hook(info);
        }));
    });
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
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    if mouse_capture {
        execute!(stdout, EnableMouseCapture)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_inner(&mut terminal, source, args, &graphics);

    disable_raw_mode()?;
    if mouse_capture {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

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
    let mut nav_history: Vec<NavEntry> = Vec::new();
    let mut nav_forward: Vec<NavEntry> = Vec::new();
    let mut theme_picker_open = false;
    let mut theme_picker_index: usize = 0;
    let mut help_open = false;
    // Active link-hint overlay: labeled links currently on screen.
    let mut link_hints: Vec<LinkHint> = Vec::new();
    // Bumped on every theme change so tabs know their cached layout is stale.
    let mut theme_gen: u32 = 0;

    let filename = args.inputs.first().map(|s| s.as_str()).unwrap_or("stdin");
    if args.slides {
        // Presentation mode: one tab per slide, navigated with ←/→/Space.
        for slide in crate::slides::split_slides(&source) {
            tabs.push(build_tab(
                slide, filename, &args, size.width, theme_gen, graphics,
            ));
        }
    } else {
        tabs.push(build_tab(
            source, filename, &args, size.width, theme_gen, graphics,
        ));
        for input in args.inputs.iter().skip(1) {
            if let Ok(src) = std::fs::read_to_string(input) {
                tabs.push(build_tab(
                    src, input, &args, size.width, theme_gen, graphics,
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

    loop {
        let viewport_height = terminal.size()?.height.saturating_sub(3); // top + separator + bottom

        // --watch: if the current doc is the watched file and it changed, rebuild it.
        if let (Some(w), Some(input)) = (watcher.as_ref(), args.inputs.first()) {
            let path = std::path::Path::new(input);
            if tabs[active_tab].filename == *input && w.check(path) {
                match std::fs::read_to_string(path) {
                    Ok(new_source) => {
                        let scroll = tabs[active_tab].scroll_offset;
                        let term_w = terminal.size()?.width;
                        let mut new_tab =
                            build_tab(new_source, input, &args, term_w, theme_gen, graphics);
                        let new_max =
                            (new_tab.ratatui_lines.len() as u16).saturating_sub(viewport_height);
                        new_tab.scroll_offset = scroll.min(new_max);
                        new_tab.toc.visible = tabs[active_tab].toc.visible;
                        tabs[active_tab] = new_tab;
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

        let tab = &tabs[active_tab];
        let total_lines = tab.ratatui_lines.len();
        let max_scroll = (total_lines as u16).saturating_sub(viewport_height);

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
                    "▁".repeat(separator_area.width as usize),
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
                    tab.scroll_offset as usize,
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
                    render::render_toc(frame, toc_area, &tab.toc.headings, tab.toc.selected, &t);
                }

                // Render document (with search highlights)
                render::render_document_with_search(
                    frame,
                    doc_area,
                    &tab.ratatui_lines,
                    tab.scroll_offset,
                    total_lines,
                    &search,
                    &t,
                );

                // Paint graphics-protocol images over their reserved blank rows.
                // SlicedImage self-clips to doc_area, so partially-scrolled images
                // (position.y negative or past the bottom) render correctly.
                for img in &tab.images {
                    let y = img.line_index as i32 - tab.scroll_offset as i32;
                    // Skip images fully above or below the viewport.
                    if y + img.rows as i32 <= 0 || y >= doc_area.height as i32 {
                        continue;
                    }
                    let pos = ratatui_image::sliced::SignedPosition::from((
                        img.col_offset as i16,
                        y as i16,
                    ));
                    frame.render_widget(
                        ratatui_image::sliced::SlicedImage::new(&img.protocol, pos),
                        doc_area,
                    );
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
                    let hints: Vec<(char, String)> = link_hints
                        .iter()
                        .map(|h| (h.label, h.url.clone()))
                        .collect();
                    render::render_link_hints(frame, main_area, &hints, &t);
                }

                // Help overlay
                if help_open {
                    render::render_help(frame, main_area, &t);
                }

                // Bottom bar: search input OR keybindings + stats
                if search.active {
                    render::render_search_bar(frame, bottom_bar_area, &search, &t);
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
        } else if !link_hints.is_empty() {
            input::InputMode::LinkHint
        } else if args.slides && !theme_picker_open && !help_open {
            input::InputMode::Slides
        } else {
            input::InputMode::Normal
        };
        if let Some(action) = input::poll_action(Duration::from_millis(50), input_mode) {
            // Any recognized action changes state → redraw on the next iteration.
            dirty = true;

            // Help overlay is modal: any key closes it.
            if help_open {
                if action != Action::None {
                    help_open = false;
                }
                continue;
            }

            // Link-hint overlay is modal: a label opens that link, Esc cancels.
            if !link_hints.is_empty() {
                match action {
                    Action::LinkHint(c) => {
                        if let Some(hint) = link_hints.iter().find(|h| h.label == c) {
                            open_link(
                                &hint.url, &mut tabs, active_tab, &args, terminal, theme_gen,
                                graphics,
                            );
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

            // A confirmed search shows highlighted matches; the first Esc/q
            // dismisses them (vim/less convention) rather than quitting.
            if action == Action::ExitApp && !search.matches.is_empty() {
                search.deactivate();
                let current_offset = tabs[active_tab].scroll_offset as usize;
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
                        tabs[active_tab].scroll_offset = (line as u16).min(max_scroll);
                    }
                }
                Action::SearchBackspace => {
                    search.pop_char();
                    search.update_matches(&tabs[active_tab].lowered);
                    if let Some(line) = search.current_line() {
                        tabs[active_tab].scroll_offset = (line as u16).min(max_scroll);
                    }
                }
                Action::SearchNext => {
                    search.next_match();
                    if let Some(line) = search.current_line() {
                        tabs[active_tab].scroll_offset = (line as u16).min(max_scroll);
                    }
                }
                Action::SearchPrev => {
                    search.prev_match();
                    if let Some(line) = search.current_line() {
                        tabs[active_tab].scroll_offset = (line as u16).min(max_scroll);
                    }
                }

                // Scrolling
                Action::ScrollDown(n) => {
                    tabs[active_tab].scroll_offset = tabs[active_tab]
                        .scroll_offset
                        .saturating_add(n)
                        .min(max_scroll);
                }
                Action::ScrollUp(n) => {
                    tabs[active_tab].scroll_offset =
                        tabs[active_tab].scroll_offset.saturating_sub(n);
                }
                Action::PageDown => {
                    let jump = viewport_height.saturating_sub(2); // keep 2 lines overlap
                    tabs[active_tab].scroll_offset = tabs[active_tab]
                        .scroll_offset
                        .saturating_add(jump)
                        .min(max_scroll);
                }
                Action::PageUp => {
                    let jump = viewport_height.saturating_sub(2);
                    tabs[active_tab].scroll_offset =
                        tabs[active_tab].scroll_offset.saturating_sub(jump);
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
                    }
                }

                // TOC
                Action::ToggleToc => tabs[active_tab].toc.toggle(),

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
                    let offset = tabs[active_tab].scroll_offset as usize;
                    let end = (offset + viewport_height as usize)
                        .min(tabs[active_tab].styled_lines.len());
                    link_hints = collect_link_hints(&tabs[active_tab].styled_lines[offset..end]);
                }

                // After a confirmed search, n/N cycle matches (less/vim
                // convention); otherwise they jump between headings.
                Action::NextHeading => {
                    if !search.matches.is_empty() {
                        search.next_match();
                        if let Some(line) = search.current_line() {
                            tabs[active_tab].scroll_offset = (line as u16).min(max_scroll);
                        }
                    } else {
                        let current = tabs[active_tab].scroll_offset as usize;
                        if let Some(next) = tabs[active_tab]
                            .toc
                            .headings
                            .iter()
                            .find(|h| h.line_index > current + 1)
                        {
                            tabs[active_tab].scroll_offset =
                                (next.line_index.saturating_sub(1) as u16).min(max_scroll);
                        }
                    }
                }
                Action::PrevHeading => {
                    if !search.matches.is_empty() {
                        search.prev_match();
                        if let Some(line) = search.current_line() {
                            tabs[active_tab].scroll_offset = (line as u16).min(max_scroll);
                        }
                    } else {
                        let current = tabs[active_tab].scroll_offset as usize;
                        if let Some(prev) = tabs[active_tab]
                            .toc
                            .headings
                            .iter()
                            .rev()
                            .find(|h| h.line_index + 1 < current)
                        {
                            tabs[active_tab].scroll_offset =
                                (prev.line_index.saturating_sub(1) as u16).min(max_scroll);
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
                }

                // Follow relative link
                Action::FollowLink => {
                    let offset = tabs[active_tab].scroll_offset as usize;
                    let mut found_link = None;
                    for i in offset..=(offset + 5).min(total_lines.saturating_sub(1)) {
                        if let Some(line) = tabs[active_tab].styled_lines.get(i) {
                            for span in &line.spans {
                                if let Some(ref url) = span.style.link_url {
                                    if url.ends_with(".md") || url.ends_with(".markdown") {
                                        found_link = Some(url.clone());
                                        break;
                                    }
                                }
                            }
                            if found_link.is_some() {
                                break;
                            }
                        }
                    }
                    if let Some(link_path) = found_link {
                        let base = std::path::Path::new(&tabs[active_tab].filename)
                            .parent()
                            .unwrap_or(std::path::Path::new("."));
                        // Same trust model as images: following a link renders
                        // a local .md file to the local user, so any readable
                        // path is fair game (absolute included).
                        let Some(target) = crate::sanitize::resolve_local(base, &link_path) else {
                            continue;
                        };
                        if let Ok(src) = std::fs::read_to_string(&target) {
                            nav_history.push(NavEntry {
                                filename: tabs[active_tab].filename.clone(),
                                scroll_offset: tabs[active_tab].scroll_offset,
                            });
                            nav_forward.clear();
                            tabs[active_tab] = build_tab(
                                src,
                                target.to_str().unwrap_or(&link_path),
                                &args,
                                terminal.size()?.width,
                                theme_gen,
                                graphics,
                            );
                        }
                    }
                }

                // Navigation history
                Action::NavBack => {
                    if let Some(entry) = nav_history.pop() {
                        nav_forward.push(NavEntry {
                            filename: tabs[active_tab].filename.clone(),
                            scroll_offset: tabs[active_tab].scroll_offset,
                        });
                        if let Ok(src) = std::fs::read_to_string(&entry.filename) {
                            let mut new_tab = build_tab(
                                src,
                                &entry.filename,
                                &args,
                                terminal.size()?.width,
                                theme_gen,
                                graphics,
                            );
                            new_tab.scroll_offset = entry.scroll_offset;
                            tabs[active_tab] = new_tab;
                        }
                    }
                }
                Action::NavForward => {
                    if let Some(entry) = nav_forward.pop() {
                        nav_history.push(NavEntry {
                            filename: tabs[active_tab].filename.clone(),
                            scroll_offset: tabs[active_tab].scroll_offset,
                        });
                        if let Ok(src) = std::fs::read_to_string(&entry.filename) {
                            let mut new_tab = build_tab(
                                src,
                                &entry.filename,
                                &args,
                                terminal.size()?.width,
                                theme_gen,
                                graphics,
                            );
                            new_tab.scroll_offset = entry.scroll_offset;
                            tabs[active_tab] = new_tab;
                        }
                    }
                }

                Action::Resize(_, _) => {
                    // Rebuild only the visible tab; background tabs rebuild
                    // lazily when switched to (their built_width won't match).
                    rebuild_tab(
                        &mut tabs[active_tab],
                        &args,
                        terminal.size()?.width,
                        theme_gen,
                        graphics,
                    );
                }
                _ => {}
            }

            let current_offset = tabs[active_tab].scroll_offset as usize;
            tabs[active_tab].toc.update_selection(current_offset);
        }
    }
}

fn is_local_file(input: &str) -> bool {
    !input.starts_with("http://") && !input.starts_with("https://") && input != "stdin"
}

/// Collect one hint per distinct link URL in the given (visible) lines,
/// labeled a, b, c… Adjacent spans sharing a URL collapse to one hint.
fn collect_link_hints(lines: &[crate::layout::StyledLine]) -> Vec<LinkHint> {
    let mut hints: Vec<LinkHint> = Vec::new();
    let mut labels = b'a';
    for line in lines {
        for span in &line.spans {
            if let Some(ref url) = span.style.link_url {
                if hints.iter().any(|h| &h.url == url) {
                    continue;
                }
                if labels > b'z' {
                    return hints;
                }
                hints.push(LinkHint {
                    label: labels as char,
                    url: url.clone(),
                });
                labels += 1;
            }
        }
    }
    hints
}

/// Act on a chosen link: open web/mail URLs in the default handler; follow a
/// local `.md` link in-place (any readable path, same trust model as images).
#[allow(clippy::too_many_arguments)]
fn open_link(
    url: &str,
    tabs: &mut [Tab],
    active_tab: usize,
    args: &Args,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) {
    // Web / mail: hand off to the OS. (URLs are already scheme-validated by
    // sanitize_url during layout, but re-check defensively.)
    if let Some(safe) = crate::sanitize::sanitize_url(url) {
        let lower = safe.to_ascii_lowercase();
        if lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("mailto:")
        {
            let _ = open::that_detached(&safe);
            return;
        }
    }
    // Local markdown link: follow in-place. Same trust model as images —
    // rendering a local .md file to the local user, so any readable path is
    // fair game (absolute included).
    if !(url.ends_with(".md") || url.ends_with(".markdown")) {
        return;
    }
    let base = std::path::Path::new(&tabs[active_tab].filename)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    let Some(target) = crate::sanitize::resolve_local(&base, url) else {
        return;
    };
    if let Ok(src) = std::fs::read_to_string(&target) {
        let width = terminal.size().map(|s| s.width).unwrap_or(80);
        tabs[active_tab] = build_tab(
            src,
            target.to_str().unwrap_or(url),
            args,
            width,
            gen,
            graphics,
        );
    }
}

/// Rebuild one tab for the current width/theme, preserving scroll and TOC
/// visibility. Only the tab the user is looking at is rebuilt eagerly; the
/// rest are refreshed lazily the next time they're switched to.
fn rebuild_tab(
    tab: &mut Tab,
    args: &Args,
    term_width: u16,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) {
    let source = tab.source.clone();
    let filename = tab.filename.clone();
    let scroll = tab.scroll_offset;
    let toc_visible = tab.toc.visible;
    *tab = build_tab(source, &filename, args, term_width, gen, graphics);
    tab.scroll_offset = scroll;
    tab.toc.visible = toc_visible;
}

/// Refresh a tab only if it was laid out for a different width or theme.
fn ensure_tab_current(
    tab: &mut Tab,
    args: &Args,
    term_width: u16,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) {
    if tab.built_width != term_width || tab.built_gen != gen {
        rebuild_tab(tab, args, term_width, gen, graphics);
    }
}

fn build_tab(
    source: String,
    filename: &str,
    args: &Args,
    term_width: u16,
    gen: u32,
    graphics: &crate::graphics::Graphics,
) -> Tab {
    let (_, content) = if args.frontmatter {
        (None, source.clone())
    } else {
        frontmatter::strip_frontmatter(&source)
    };

    // Pre-process wikilinks before parsing
    let content = crate::wikilink::process_wikilinks(&content);

    let arena = Arena::new();
    let options = crate::parser::options();
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
    } = layout::layout_document(
        root,
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
        .filter_map(|spec| {
            let proto = graphics.build((*spec.image).clone(), spec.cols, spec.rows)?;
            Some(ImagePlacement {
                line_index: spec.line_index,
                col_offset: spec.col_offset,
                rows: spec.rows,
                protocol: proto,
            })
        })
        .collect();
    let ratatui_lines = render::styled_lines_to_ratatui(&styled_lines, &args.theme);
    let lowered: Vec<String> = styled_lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
                .to_lowercase()
        })
        .collect();

    let (word_count, reading_time) = stats::document_stats(&content);

    // Headings carry their exact display-line index straight from layout —
    // no substring reverse-scan, and duplicate/split headings map correctly.
    let toc_entries: Vec<crate::toc::TocEntry> = headings
        .into_iter()
        .filter(|h| !h.text.is_empty())
        .map(|h| crate::toc::TocEntry {
            level: h.level,
            text: h.text,
            line_index: h.line_index,
        })
        .collect();

    let mut toc = TocState::empty();
    toc.headings = toc_entries;
    toc.visible = args.toc;

    Tab {
        filename: filename.to_string(),
        source,
        styled_lines,
        ratatui_lines,
        lowered,
        scroll_offset: 0,
        toc,
        word_count,
        reading_time,
        built_width: term_width,
        built_gen: gen,
        images,
    }
}
