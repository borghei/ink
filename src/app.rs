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
    scroll_offset: u16,
    toc: TocState,
    word_count: usize,
    reading_time: usize,
}

struct NavEntry {
    filename: String,
    scroll_offset: u16,
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

pub fn run(source: String, args: Args) -> Result<AppExit> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_inner(&mut terminal, source, args);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_inner(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    source: String,
    mut args: Args,
) -> Result<AppExit> {
    let size = terminal.size()?;

    let mut tabs: Vec<Tab> = Vec::new();
    let mut active_tab: usize = 0;

    let first_tab = build_tab(
        source,
        args.inputs.first().map(|s| s.as_str()).unwrap_or("stdin"),
        &args,
        size.width,
    );
    tabs.push(first_tab);

    for input in args.inputs.iter().skip(1) {
        if let Ok(src) = std::fs::read_to_string(input) {
            tabs.push(build_tab(src, input, &args, size.width));
        }
    }

    let mut search = SearchState::new();
    let mut nav_history: Vec<NavEntry> = Vec::new();
    let mut nav_forward: Vec<NavEntry> = Vec::new();
    let mut theme_picker_open = false;
    let mut theme_picker_index: usize = 0;

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

    loop {
        let viewport_height = terminal.size()?.height.saturating_sub(3); // top + separator + bottom

        // --watch: if the current doc is the watched file and it changed, rebuild it.
        if let (Some(w), Some(input)) = (watcher.as_ref(), args.inputs.first()) {
            let path = std::path::Path::new(input);
            if tabs[active_tab].filename == *input && w.check(path) {
                if let Ok(new_source) = std::fs::read_to_string(path) {
                    let scroll = tabs[active_tab].scroll_offset;
                    let term_w = terminal.size()?.width;
                    let mut new_tab = build_tab(new_source, input, &args, term_w);
                    let new_max = (new_tab.ratatui_lines.len() as u16).saturating_sub(viewport_height);
                    new_tab.scroll_offset = scroll.min(new_max);
                    new_tab.toc.visible = tabs[active_tab].toc.visible;
                    tabs[active_tab] = new_tab;
                }
            }
        }

        let tab = &tabs[active_tab];
        let total_lines = tab.ratatui_lines.len();
        let max_scroll = (total_lines as u16).saturating_sub(viewport_height);

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
                &args.theme,
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
                render::render_toc(
                    frame,
                    toc_area,
                    &tab.toc.headings,
                    tab.toc.selected,
                    &args.theme,
                );
            }

            // Render document (with search highlights)
            render::render_document_with_search(
                frame,
                doc_area,
                &tab.ratatui_lines,
                tab.scroll_offset,
                total_lines,
                &search,
                &args.theme,
            );

            // Theme picker overlay
            if theme_picker_open {
                render::render_theme_picker(
                    frame,
                    main_area,
                    THEME_LIST,
                    theme_picker_index,
                    &args.theme,
                );
            }

            // Bottom bar: search input OR keybindings + stats
            if search.active {
                render::render_search_bar(frame, bottom_bar_area, &search, &args.theme);
            } else {
                render::render_bottom_bar(
                    frame,
                    bottom_bar_area,
                    &args.theme,
                    &tab.filename,
                    tab.word_count,
                    tab.reading_time,
                    tabs.len() > 1,
                    tab_info,
                );
            }
        })?;

        // Handle input
        if let Some(action) = input::poll_action(Duration::from_millis(50), search.active) {
            // Theme picker mode
            if theme_picker_open {
                match action {
                    Action::ExitApp | Action::CloseSearch => {
                        theme_picker_open = false;
                    }
                    Action::ScrollDown(_) => {
                        theme_picker_index = (theme_picker_index + 1) % THEME_LIST.len();
                        // Live preview: rebuild all tabs with new theme
                        args.theme = THEME_LIST[theme_picker_index].to_string();
                        rebuild_all_tabs(&mut tabs, &args, terminal.size()?.width);
                    }
                    Action::ScrollUp(_) => {
                        theme_picker_index = if theme_picker_index == 0 {
                            THEME_LIST.len() - 1
                        } else {
                            theme_picker_index - 1
                        };
                        args.theme = THEME_LIST[theme_picker_index].to_string();
                        rebuild_all_tabs(&mut tabs, &args, terminal.size()?.width);
                    }
                    Action::SearchConfirm => {
                        // Confirm theme selection
                        theme_picker_open = false;
                    }
                    _ => {}
                }
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
                    search.update_matches(&tabs[active_tab].styled_lines);
                    // Auto-jump to first match
                    if let Some(line) = search.current_line() {
                        tabs[active_tab].scroll_offset = (line as u16).min(max_scroll);
                    }
                }
                Action::SearchBackspace => {
                    search.pop_char();
                    search.update_matches(&tabs[active_tab].styled_lines);
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

                // TOC
                Action::ToggleToc => tabs[active_tab].toc.toggle(),

                // Theme picker
                Action::ThemePicker => {
                    theme_picker_open = true;
                }

                // Heading jump: n = next heading, N = previous heading
                Action::NextHeading => {
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
                Action::PrevHeading => {
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

                // Links are handled via OSC 8 hyperlinks (Cmd+Click / Ctrl+Click)

                // Tabs
                Action::NextTab if tabs.len() > 1 => {
                    active_tab = (active_tab + 1) % tabs.len();
                }
                Action::PrevTab if tabs.len() > 1 => {
                    active_tab = if active_tab == 0 {
                        tabs.len() - 1
                    } else {
                        active_tab - 1
                    };
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
                        let target = base.join(&link_path);
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
                            let mut new_tab =
                                build_tab(src, &entry.filename, &args, terminal.size()?.width);
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
                            let mut new_tab =
                                build_tab(src, &entry.filename, &args, terminal.size()?.width);
                            new_tab.scroll_offset = entry.scroll_offset;
                            tabs[active_tab] = new_tab;
                        }
                    }
                }

                Action::Resize(_, _) => {
                    // Rebuild on resize
                    rebuild_all_tabs(&mut tabs, &args, terminal.size()?.width);
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

fn rebuild_all_tabs(tabs: &mut [Tab], args: &Args, term_width: u16) {
    for tab in tabs.iter_mut() {
        let source = tab.source.clone();
        let filename = tab.filename.clone();
        let scroll = tab.scroll_offset;
        let toc_visible = tab.toc.visible;
        *tab = build_tab(source, &filename, args, term_width);
        tab.scroll_offset = scroll;
        tab.toc.visible = toc_visible;
    }
}

fn build_tab(source: String, filename: &str, args: &Args, term_width: u16) -> Tab {
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

    let max_content_width = args.width.unwrap_or(term_width.saturating_sub(4)).min(120);
    let center_margin = if term_width > max_content_width + 4 {
        ((term_width - max_content_width) / 2) as usize
    } else {
        2
    };

    // Resolve base directory for relative image paths
    let base_dir = std::path::Path::new(filename).parent();

    let styled_lines = layout::layout_document(
        root,
        &theme::resolve_theme(&args.theme),
        max_content_width,
        args.spacing,
        center_margin,
        base_dir,
        args.no_images,
    );
    let ratatui_lines = render::styled_lines_to_ratatui(&styled_lines, &args.theme);

    let (word_count, reading_time) = stats::document_stats(&content);

    // Extract headings from the already-parsed AST (avoid parsing twice)
    let headings = crate::parser::extract_headings_from_ast(root);
    let toc_entries: Vec<crate::toc::TocEntry> = headings
        .iter()
        .filter_map(|h| {
            for (i, line) in styled_lines.iter().enumerate() {
                for span in &line.spans {
                    if span.style.bold && span.text.contains(&h.text) && !h.text.is_empty() {
                        return Some(crate::toc::TocEntry {
                            level: h.level,
                            text: h.text.clone(),
                            line_index: i,
                        });
                    }
                }
            }
            None
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
        scroll_offset: 0,
        toc,
        word_count,
        reading_time,
    }
}
