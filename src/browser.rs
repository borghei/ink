use crate::theme;
use anyhow::Result;
use crossterm::event::{self, EnableMouseCapture, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

struct FileEntry {
    relative_path: String,
    full_path: PathBuf,
    size: u64,
}

/// Launch an interactive file browser for the given directory.
///
/// Returns `Some(path)` if the user selected a file, or `None` if they quit.
/// Gracefully handles empty directories, permission errors, etc.
///
/// `mouse_capture` follows the same setting as the reader (`--no-mouse`,
/// `[behavior] mouse_capture`); the browser has no mouse actions of its own,
/// but switching capture on here and off in the reader would flip the
/// terminal's selection behaviour between screens.
pub fn browse(dir: &Path, theme_name: &str, mouse_capture: bool) -> Result<Option<PathBuf>> {
    let files = find_markdown_files(dir);
    if files.is_empty() {
        eprintln!("ink: no markdown files found in {}", dir.display());
        return Ok(None);
    }

    crate::app::install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    if mouse_capture {
        execute!(stdout, EnableMouseCapture)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = browse_inner(&mut terminal, dir, &files, theme_name);

    crate::app::restore_terminal()?;
    crate::app::exit_if_signalled();

    result
}

fn browse_inner(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    dir: &Path,
    files: &[FileEntry],
    theme_name: &str,
) -> Result<Option<PathBuf>> {
    let mut selected: usize = 0;
    let mut scroll_offset: usize = 0;
    let mut filter = String::new();
    let mut filter_active = false;

    loop {
        if crate::app::termination_requested() {
            return Ok(None);
        }
        // Build filtered index list
        let filtered: Vec<usize> = if filter.is_empty() {
            (0..files.len()).collect()
        } else {
            let q = filter.to_lowercase();
            (0..files.len())
                .filter(|&i| files[i].relative_path.to_lowercase().contains(&q))
                .collect()
        };

        if selected >= filtered.len() && !filtered.is_empty() {
            selected = filtered.len() - 1;
        }

        terminal.draw(|frame| {
            let size = frame.area();
            let t = theme::resolve_theme(theme_name);
            let g = crate::glyphs::current();
            let dot = format!(" {} ", g.dot);

            // Fill background
            if let Some(ref bg_hex) = t.colors.bg {
                let bg_color = theme::hex_to_color(bg_hex);
                let bg_block =
                    ratatui::widgets::Block::default().style(Style::default().bg(bg_color));
                frame.render_widget(bg_block, size);
            }

            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // header
                    Constraint::Min(1),    // file list
                    Constraint::Length(1), // bottom bar
                ])
                .split(size);

            let header_area = vertical[0];
            let list_area = vertical[1];
            let bottom_area = vertical[2];

            let bg = t.colors.bg.as_ref().map(|b| theme::hex_to_color(b));
            let fg = theme::hex_to_color(&t.colors.fg);
            let accent = theme::hex_to_color(&t.colors.heading1);
            let dim = theme::hex_to_color(&t.colors.link_url);

            // ── Header ──
            let dir_display = dir.display().to_string();
            let mut header_style = Style::default();
            if let Some(bg) = bg {
                header_style = header_style.bg(bg);
            }
            let header_lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  ink ", Style::default().fg(accent).bold()),
                    Span::styled(
                        format!("{} {dir_display}", g.dash),
                        Style::default().fg(dim),
                    ),
                ]),
            ];
            frame.render_widget(
                Paragraph::new(header_lines).style(header_style),
                header_area,
            );

            // ── File list ──
            let viewport = list_area.height as usize;
            // Keep selected visible
            if selected >= scroll_offset + viewport {
                scroll_offset = selected + 1 - viewport;
            }
            if selected < scroll_offset {
                scroll_offset = selected;
            }

            let file_lines: Vec<Line<'static>> = filtered
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .take(viewport)
                .map(|(i, &file_idx)| {
                    let entry = &files[file_idx];
                    let is_sel = i == selected;
                    let marker = if is_sel {
                        format!("  {} ", g.pointer)
                    } else {
                        "    ".to_string()
                    };
                    let size_str = format_size(entry.size);

                    let mut name_style = Style::default().fg(if is_sel { accent } else { fg });
                    if let Some(bg) = bg {
                        name_style = name_style.bg(bg);
                    }
                    if is_sel {
                        name_style = name_style.bold();
                    }

                    let mut dim_style = Style::default().fg(dim);
                    if let Some(bg) = bg {
                        dim_style = dim_style.bg(bg);
                    }

                    let mut marker_style = Style::default().fg(if is_sel { accent } else { fg });
                    if let Some(bg) = bg {
                        marker_style = marker_style.bg(bg);
                    }

                    Line::from(vec![
                        Span::styled(marker, marker_style),
                        Span::styled(entry.relative_path.clone(), name_style),
                        Span::styled(format!("  {size_str}"), dim_style),
                    ])
                })
                .collect();

            let mut list_style = Style::default();
            if let Some(bg) = bg {
                list_style = list_style.bg(bg);
            }
            frame.render_widget(Paragraph::new(file_lines).style(list_style), list_area);

            // ── Bottom bar ──
            let bar_bg = theme::hex_to_color(&t.colors.status_bar_bg);
            let bar_fg = theme::hex_to_color(&t.colors.status_bar_fg);
            let bar_dim = theme::hex_to_color(&t.colors.link_url);

            let bottom_line = if filter_active {
                Line::from(vec![
                    Span::styled("  / ", Style::default().fg(accent).bg(bar_bg).bold()),
                    Span::styled(filter.clone(), Style::default().fg(bar_fg).bg(bar_bg)),
                    Span::styled(g.block, Style::default().fg(accent).bg(bar_bg)),
                    Span::styled(
                        format!("  {} matches", filtered.len()),
                        Style::default().fg(bar_dim).bg(bar_bg),
                    ),
                    Span::styled(" ".repeat(size.width as usize), Style::default().bg(bar_bg)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", g.scroll_keys),
                        Style::default().fg(bar_fg).bg(bar_bg).bold(),
                    ),
                    Span::styled("navigate", Style::default().fg(bar_dim).bg(bar_bg)),
                    Span::styled(dot.clone(), Style::default().fg(bar_dim).bg(bar_bg)),
                    Span::styled(" Enter ", Style::default().fg(bar_fg).bg(bar_bg).bold()),
                    Span::styled("open", Style::default().fg(bar_dim).bg(bar_bg)),
                    Span::styled(dot.clone(), Style::default().fg(bar_dim).bg(bar_bg)),
                    Span::styled(" / ", Style::default().fg(bar_fg).bg(bar_bg).bold()),
                    Span::styled("filter", Style::default().fg(bar_dim).bg(bar_bg)),
                    Span::styled(dot.clone(), Style::default().fg(bar_dim).bg(bar_bg)),
                    Span::styled(" q ", Style::default().fg(bar_fg).bg(bar_bg).bold()),
                    Span::styled("quit", Style::default().fg(bar_dim).bg(bar_bg)),
                    Span::styled(" ".repeat(size.width as usize), Style::default().bg(bar_bg)),
                ])
            };

            frame.render_widget(Paragraph::new(vec![bottom_line]), bottom_area);
        })?;

        // ── Input handling ──
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // Windows reports key releases too; acting on them would
                // move twice and type every filter character twice.
                if !crate::input::is_actionable_key(&key) {
                    continue;
                }
                if filter_active {
                    match key.code {
                        KeyCode::Esc => {
                            filter_active = false;
                            filter.clear();
                        }
                        KeyCode::Enter => {
                            filter_active = false;
                        }
                        KeyCode::Backspace => {
                            filter.pop();
                        }
                        KeyCode::Char(c) => {
                            if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                                filter_active = false;
                                filter.clear();
                            } else {
                                filter.push(c);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(None);
                    }
                    KeyCode::Down | KeyCode::Char('j')
                        if !filtered.is_empty() && selected < filtered.len() - 1 =>
                    {
                        selected += 1;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = selected.saturating_sub(1);
                    }
                    KeyCode::Char('G') | KeyCode::End if !filtered.is_empty() => {
                        selected = filtered.len() - 1;
                    }
                    KeyCode::Home | KeyCode::Char('g') => {
                        selected = 0;
                    }
                    KeyCode::Enter => {
                        if let Some(&file_idx) = filtered.get(selected) {
                            return Ok(Some(files[file_idx].full_path.clone()));
                        }
                    }
                    KeyCode::Char('/') => {
                        filter_active = true;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Recursively find all markdown files in a directory.
fn find_markdown_files(dir: &Path) -> Vec<FileEntry> {
    let mut files = Vec::new();
    let mut visited = HashSet::new();
    walk_dir(dir, dir, 0, &mut visited, &mut files);
    // Sort: README files first, then alphabetical by path
    files.sort_by(|a, b| {
        let a_readme = a.relative_path.to_lowercase().contains("readme");
        let b_readme = b.relative_path.to_lowercase().contains("readme");
        match (a_readme, b_readme) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .relative_path
                .to_lowercase()
                .cmp(&b.relative_path.to_lowercase()),
        }
    });
    files
}

/// Deepest directory nesting the browser descends into.
const MAX_DEPTH: usize = 32;
/// Stop collecting after this many files, so a huge tree cannot stall startup.
const MAX_FILES: usize = 50_000;

/// Collect markdown files under `dir`.
///
/// Directory symlinks are followed, but each directory is entered at most
/// once, keyed by its canonical path: a symlink loop (`self -> .`) or two
/// links to the same tree would otherwise recurse forever or list every file
/// many times over.
fn walk_dir(
    base: &Path,
    dir: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    files: &mut Vec<FileEntry>,
) {
    if depth > MAX_DEPTH || files.len() >= MAX_FILES {
        return;
    }
    let Ok(canonical) = std::fs::canonicalize(dir) else {
        return;
    };
    if !visited.insert(canonical) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        if files.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Skip hidden files and directories
        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            // Skip common non-doc directories to keep the list clean and fast
            if matches!(
                name.as_str(),
                "node_modules"
                    | "target"
                    | "vendor"
                    | "__pycache__"
                    | "dist"
                    | "build"
                    | "out"
                    | "venv"
                    | ".venv"
            ) {
                continue;
            }
            walk_dir(base, &path, depth + 1, visited, files);
        } else if path
            .extension()
            .map(|e| e == "md" || e == "markdown")
            .unwrap_or(false)
        {
            let relative = path.strip_prefix(base).unwrap_or(&path);
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            files.push(FileEntry {
                relative_path: relative.to_string_lossy().to_string(),
                full_path: path,
                size,
            });
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn paths(files: &[FileEntry]) -> Vec<String> {
        let mut v: Vec<String> = files
            .iter()
            .map(|f| f.relative_path.replace('\\', "/"))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn finds_nested_markdown_and_skips_hidden_and_build_dirs() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("docs/deep")).unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("a.md"), "# a").unwrap();
        fs::write(dir.path().join("docs/deep/b.markdown"), "# b").unwrap();
        fs::write(dir.path().join(".git/c.md"), "# c").unwrap();
        fs::write(dir.path().join("target/d.md"), "# d").unwrap();
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        let files = find_markdown_files(dir.path());
        assert_eq!(paths(&files), ["a.md", "docs/deep/b.markdown"]);
    }

    #[test]
    fn depth_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let mut deep = dir.path().to_path_buf();
        for _ in 0..(MAX_DEPTH + 3) {
            deep.push("d");
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("too-deep.md"), "x").unwrap();
        fs::write(dir.path().join("top.md"), "x").unwrap();
        let files = find_markdown_files(dir.path());
        assert_eq!(paths(&files), ["top.md"]);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loops_terminate_without_duplicates() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("a.md"), "# a").unwrap();
        fs::write(root.join("sub/b.md"), "# b").unwrap();
        // self -> . (loop to the root), sub/up -> .. (loop through a parent),
        // and two links into one directory.
        symlink(".", root.join("self")).unwrap();
        symlink("..", root.join("sub/up")).unwrap();
        symlink("sub", root.join("alias")).unwrap();

        let start = std::time::Instant::now();
        let files = find_markdown_files(root);
        assert!(start.elapsed() < std::time::Duration::from_secs(5));

        let listed = paths(&files);
        assert_eq!(listed.len(), 2, "duplicates listed: {listed:?}");
        assert!(listed.contains(&"a.md".to_string()));
        assert!(listed.iter().any(|p| p.ends_with("b.md")));
    }

    #[cfg(unix)]
    #[test]
    fn two_mutually_linked_directories_terminate() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("x")).unwrap();
        fs::create_dir_all(root.join("y")).unwrap();
        fs::write(root.join("x/one.md"), "1").unwrap();
        fs::write(root.join("y/two.md"), "2").unwrap();
        symlink("../y", root.join("x/to-y")).unwrap();
        symlink("../x", root.join("y/to-x")).unwrap();
        let files = find_markdown_files(root);
        assert_eq!(files.len(), 2, "{:?}", paths(&files));
    }
}
