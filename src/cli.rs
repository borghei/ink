use crate::{app, browser, config, input, render, stats, theme};
use anyhow::Result;
use clap::{Parser as ClapParser, Subcommand, ValueEnum};
use std::io::IsTerminal;
use std::path::PathBuf;

#[derive(ClapParser, Debug)]
#[command(
    name = "ink",
    about = "The most advanced terminal markdown reader",
    version,
    author
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Markdown file path(s) or URL (reads stdin if omitted or `-`)
    #[arg(value_name = "FILE|URL")]
    pub input: Vec<String>,

    /// Color theme
    #[arg(short, long, default_value = "auto")]
    pub theme: String,

    /// Max rendering width in columns, 20-1000 (or: narrow, wide, full)
    #[arg(short, long, value_parser = parse_width)]
    pub width: Option<WidthArg>,

    /// Presentation mode (split on ---)
    #[arg(short, long)]
    pub slides: bool,

    /// Plain output mode (no TUI, pipe-friendly); automatic when stdout is
    /// not a terminal
    #[arg(short, long)]
    pub plain: bool,

    /// When to emit color and hyperlinks in --plain and diff output.
    /// `auto`: only when stdout is a terminal, honoring NO_COLOR,
    /// CLICOLOR_FORCE / FORCE_COLOR and TERM=dumb. `never` (or NO_COLOR) also
    /// draws the reader without colour
    #[arg(long, value_enum, value_name = "WHEN", default_value_t, global = true)]
    pub color: theme::caps::ColorChoice,

    /// Watch file for changes and re-render
    #[arg(long)]
    pub watch: bool,

    /// Show table of contents on startup
    #[arg(long)]
    pub toc: bool,

    /// Disable image rendering
    #[arg(long)]
    pub no_images: bool,

    /// Allow fetching remote (http/https) images referenced in documents
    #[arg(long)]
    pub remote_images: bool,

    /// Image rendering protocol: auto, kitty, iterm2, sixel, halfblocks
    #[arg(long, default_value = "auto", value_parser = parse_protocol)]
    pub image_protocol: crate::graphics::ProtocolChoice,

    /// List available themes and exit
    #[arg(long)]
    pub list_themes: bool,

    /// Do not page --plain output through $PAGER even on a TTY
    #[arg(long)]
    pub no_pager: bool,

    /// Show YAML/TOML frontmatter
    #[arg(long)]
    pub frontmatter: bool,

    /// Line spacing [default: normal]
    #[arg(long, value_enum)]
    pub spacing: Option<Spacing>,

    /// Do not capture the mouse, in the reader or the file browser. Your
    /// terminal keeps its own text selection and link clicking; the wheel no
    /// longer scrolls inside ink. Overrides `[behavior] mouse_capture`
    #[arg(long, global = true)]
    pub no_mouse: bool,

    /// Draw borders, bullets and markers with plain ASCII instead of
    /// box-drawing and symbol characters (for the Linux console, legacy
    /// console fonts, screen readers). Automatic on TERM=linux and non-UTF-8
    /// locales; `[behavior] ascii` sets it either way
    #[arg(long, global = true)]
    pub ascii: bool,
}

/// A parsed `--width` value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WidthArg {
    /// A column count (`narrow` = 60, `wide` = 100).
    Columns(u16),
    /// `full`: no width cap.
    Full,
}

/// `--width` parser: a column count in [`config::WIDTH_RANGE`] or a keyword.
fn parse_width(s: &str) -> Result<WidthArg, String> {
    let (lo, hi) = (config::WIDTH_RANGE.start(), config::WIDTH_RANGE.end());
    match s {
        "narrow" => Ok(WidthArg::Columns(60)),
        "wide" => Ok(WidthArg::Columns(100)),
        "full" => Ok(WidthArg::Full),
        _ => s
            .parse::<u16>()
            .ok()
            .filter(|n| config::WIDTH_RANGE.contains(n))
            .map(WidthArg::Columns)
            .ok_or_else(|| {
                format!("expected a column count from {lo} to {hi}, or one of: narrow, wide, full")
            }),
    }
}

/// `--image-protocol` parser (keeps the aliases `ProtocolChoice::parse` accepts).
fn parse_protocol(s: &str) -> Result<crate::graphics::ProtocolChoice, String> {
    crate::graphics::ProtocolChoice::parse(s)
        .ok_or_else(|| "valid values: auto, kitty, iterm2, sixel, halfblocks".to_string())
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Show document outline (heading structure)
    Outline {
        /// File to analyze
        file: String,
    },
    /// Show document statistics
    Stats {
        /// File to analyze
        file: String,
    },
    /// Show diff between two markdown files
    Diff {
        /// First file
        file_a: String,
        /// Second file
        file_b: String,
    },
    /// Print shell integration snippets (bash, zsh, fish)
    ShellSetup {
        /// Shell name: bash, zsh, or fish
        shell: String,
    },
    /// Show the active keybinding map (preset + user overrides)
    Keybindings,
    /// Generate shell completions (bash, zsh, fish, powershell, elvish)
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
    /// Generate a man page (troff, to stdout)
    Man,
    /// Print a diagnostic report to attach to an issue: platform, terminal,
    /// colour and theme detection, clipboard route, mouse, config problems,
    /// graphics-protocol negotiation, decoder self-tests (no personal data)
    Doctor {
        /// Also write the report to this file (attach it to a GitHub issue)
        #[arg(long, value_name = "PATH")]
        save: Option<std::path::PathBuf>,
    },
    /// Configuration helpers
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Write a starter config to the path shown by `ink config path`
    Init {
        /// Overwrite an existing config file
        #[arg(long)]
        force: bool,
    },
    /// Print the path to the active config file
    Path,
}

/// Resolved arguments for the app.
#[derive(Clone)]
pub struct Args {
    pub inputs: Vec<String>,
    pub theme: String,
    pub width: Option<u16>,
    pub slides: bool,
    pub plain: bool,
    pub watch: bool,
    pub toc: bool,
    pub images: crate::image::ImageMode,
    pub image_protocol: crate::graphics::ProtocolChoice,
    pub frontmatter: bool,
    pub spacing: Spacing,
    pub mouse_capture: bool,
    pub clipboard: crate::clipboard::ClipboardMode,
}

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
pub enum Spacing {
    Compact,
    Normal,
    Relaxed,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    // Load config + initialize keymap up front so subcommands (e.g. `keybindings`)
    // can read the resolved map.
    // Config problems are reported here, on stderr, before any alternate
    // screen can hide them; the valid keys still apply.
    let config::ConfigLoad {
        config: user_config,
        warnings,
    } = config::load_config_with_warnings();
    for w in &warnings {
        eprintln!("{w}");
    }
    input::init_keymap(user_config.as_ref().and_then(|c| c.keybindings.as_ref()));
    crate::glyphs::select(
        cli.ascii,
        user_config
            .as_ref()
            .and_then(|c| c.behavior.as_ref())
            .and_then(|b| b.ascii),
    );

    // Escape sequences only when wanted: `--color`, then NO_COLOR, the
    // force variables, TERM=dumb, and finally whether stdout is a terminal.
    let stdout_tty = std::io::stdout().is_terminal();
    let color = theme::caps::color_enabled(cli.color, stdout_tty);
    // The reader always draws on a terminal, so it asks the same question as
    // if stdout were one: `--color=never`, NO_COLOR and TERM=dumb turn its
    // colours off (attributes only).
    theme::caps::set_tui_level(theme::caps::color_enabled(cli.color, true));

    let (theme, theme_origin) = theme_choice(&cli.theme, &user_config);
    // `auto` asks the terminal for its background colour (OSC 11) once, now,
    // before anything owns the screen. Only with a terminal on both ends:
    // the reply arrives on stdin, and a pipe would never answer.
    if theme == "auto" && uses_background(&cli, color) {
        theme::detect::init_background(stdout_tty && std::io::stdin().is_terminal());
    }

    // Handle subcommands
    if let Some(cmd) = &cli.command {
        return match cmd {
            Commands::Outline { file } => {
                let source = read_file(file)?;
                stats::print_outline(&source);
                Ok(())
            }
            Commands::Stats { file } => {
                let source = read_file(file)?;
                stats::print_stats(&source, file);
                Ok(())
            }
            Commands::Diff { file_a, file_b } => {
                let source_a = read_file(file_a)?;
                let source_b = read_file(file_b)?;
                stats::print_diff(&source_a, &source_b, file_a, file_b, color);
                Ok(())
            }
            Commands::ShellSetup { shell } => {
                print_shell_setup(shell);
                Ok(())
            }
            Commands::Doctor { save } => {
                let (clipboard, clipboard_source) = clipboard_mode(&user_config);
                return crate::doctor::run(
                    save.as_deref(),
                    &crate::doctor::Context {
                        theme: &theme,
                        theme_origin,
                        color_choice: cli.color,
                        clipboard,
                        clipboard_source,
                        mouse: mouse_capture(cli.no_mouse, &user_config),
                        config_warnings: &warnings,
                    },
                );
            }
            Commands::Keybindings => {
                print_keybindings();
                Ok(())
            }
            Commands::Completions { shell } => {
                use clap::CommandFactory;
                clap_complete::generate(*shell, &mut Cli::command(), "ink", &mut std::io::stdout());
                Ok(())
            }
            Commands::Man => {
                use clap::CommandFactory;
                let man = clap_mangen::Man::new(Cli::command());
                man.render(&mut std::io::stdout())?;
                Ok(())
            }
            Commands::Config { action } => match action {
                ConfigAction::Init { force } => config_init(*force),
                ConfigAction::Path => {
                    println!("{}", config::config_path_display());
                    Ok(())
                }
            },
        };
    }

    if cli.list_themes {
        for name in theme::available_themes() {
            println!("{name}");
        }
        return Ok(());
    }

    let width = resolve_width(&cli.width, &user_config);
    // CLI flag wins; otherwise the config file's key; otherwise the default.
    // (config values were validated on load, so the parse cannot fail).
    let spacing = cli
        .spacing
        .or_else(|| {
            let s = user_config.as_ref()?.spacing.as_deref()?;
            Spacing::from_str(s, false).ok()
        })
        .unwrap_or(Spacing::Normal);
    let cfg_flag = |get: fn(&config::Config) -> Option<bool>| {
        user_config.as_ref().and_then(get).unwrap_or(false)
    };
    let toc = cli.toc || cfg_flag(|c| c.toc);
    let frontmatter = cli.frontmatter || cfg_flag(|c| c.frontmatter);

    // Resolve once now, while stderr still reaches the user: a broken or
    // unknown theme warns here instead of inside the alternate screen.
    let _ = theme::resolve_theme(&theme);

    // `ink -` means stdin: a lone `-` takes the same path as no argument
    // (but never opens the file browser, even when stdin is a terminal).
    let stdin_dash = cli.input == ["-"];
    let inputs = if stdin_dash {
        Vec::new()
    } else {
        cli.input.clone()
    };

    let args = Args {
        inputs,
        theme,
        width,
        slides: cli.slides,
        // The interactive reader needs a terminal: with stdout piped or
        // redirected (`ink file.md | cat`), render plain output instead.
        plain: cli.plain || !stdout_tty,
        watch: cli.watch,
        toc,
        images: if cli.no_images {
            crate::image::ImageMode::Off
        } else if cli.remote_images {
            crate::image::ImageMode::All
        } else {
            crate::image::ImageMode::LocalOnly
        },
        image_protocol: cli.image_protocol,
        frontmatter,
        spacing,
        mouse_capture: mouse_capture(cli.no_mouse, &user_config).0,
        clipboard: clipboard_mode(&user_config).0,
    };

    // Check if input is a directory or no input with a TTY → launch file browser
    let browse_dir = if args.inputs.is_empty() {
        if std::io::stdin().is_terminal() && !stdin_dash {
            Some(std::env::current_dir()?)
        } else {
            None
        }
    } else {
        let path = PathBuf::from(&args.inputs[0]);
        if path.is_dir() {
            Some(path)
        } else {
            None
        }
    };

    if let Some(dir) = browse_dir {
        // The browser is a full-screen TUI: refuse to launch it under --plain
        // or into a pipe (previously `ink --plain docs/ > out.md` pushed raw
        // escape sequences and raw-mode into the redirect).
        if args.plain || !std::io::stdout().is_terminal() {
            anyhow::bail!(
                "'{}' is a directory — pass a markdown file (the interactive browser needs a terminal and is not available with --plain)",
                dir.display()
            );
        }
        // Browser → doc → exit by default. User can press Shift-B inside the
        // doc (or set behavior.browser_loop = true in config) to return here.
        let browser_loop = user_config
            .as_ref()
            .and_then(|c| c.behavior.as_ref())
            .and_then(|b| b.browser_loop)
            .unwrap_or(false);

        loop {
            let Some(selected) = browser::browse(&dir, &args.theme, args.mouse_capture)? else {
                break;
            };
            let source = std::fs::read_to_string(&selected)?;
            let mut file_args = args.clone();
            file_args.inputs = vec![selected.to_string_lossy().to_string()];
            if file_args.plain {
                let rendered = render::plain::render_plain_with_color(&source, &file_args, color)?;
                write_stdout(&rendered);
                break;
            }
            match app::run(source, file_args)? {
                app::AppExit::Quit => {
                    if browser_loop {
                        continue;
                    }
                    break;
                }
                app::AppExit::BackToBrowser => continue,
            }
        }
        return Ok(());
    }

    if args.plain {
        // Render every input (cat-style concatenation), not just the first.
        let sources = read_all_inputs(&args)?;
        let mut rendered = String::new();
        for source in &sources {
            rendered.push_str(&render::plain::render_plain_with_color(
                source, &args, color,
            )?);
        }
        emit_plain(&rendered, cli.no_pager);
        return Ok(());
    }

    let source = read_input(&args)?;

    app::run(source, args)?;

    Ok(())
}

/// Print rendered plain output, paging it through `$PAGER` (or `less -R`) when
/// stdout is an interactive terminal and the content is taller than the
/// screen — making `ink --plain` a drop-in markdown pager like `bat`/`less`.
/// Falls back to a direct print for pipes, redirects, or `--no-pager`.
fn emit_plain(rendered: &str, no_pager: bool) {
    use std::io::Write;

    let stdout_tty = std::io::stdout().is_terminal();
    let term_height = crossterm::terminal::size().map(|(_, h)| h).unwrap_or(24);
    let long_enough = rendered.lines().count() > term_height as usize;

    if no_pager || !stdout_tty || !long_enough {
        write_stdout(rendered);
        return;
    }

    let pager = std::env::var("PAGER").unwrap_or_else(|_| "less -R".to_string());
    let mut parts = pager.split_whitespace();
    let Some(program) = parts.next() else {
        write_stdout(rendered);
        return;
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(parts);
    // Ensure `less` passes through our ANSI colors even if $PAGER is bare `less`.
    if program == "less" {
        cmd.env(
            "LESS",
            std::env::var("LESS").unwrap_or_else(|_| "-R".to_string()),
        );
    }
    match cmd.stdin(std::process::Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(rendered.as_bytes());
            }
            let _ = child.wait();
        }
        Err(_) => write_stdout(rendered),
    }
}

/// Write to stdout without panicking when the reader has gone away. A closed
/// pipe (`ink --plain … | head`, fzf previews, git textconv) is a normal way
/// for output to end: exit 0 quietly instead of `print!`'s broken-pipe panic.
fn write_stdout(s: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let res = out.write_all(s.as_bytes()).and_then(|_| out.flush());
    if let Err(e) = res {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
    }
}

/// Read every input for --plain: all named files/URLs (`-` is stdin), or
/// stdin when none.
fn read_all_inputs(args: &Args) -> Result<Vec<String>> {
    if args.inputs.is_empty() {
        return Ok(vec![read_input(args)?]);
    }
    args.inputs.iter().map(|input| read_source(input)).collect()
}

/// Read one input: `-` is stdin, `http(s)://` is fetched, anything else is
/// a file.
fn read_source(input: &str) -> Result<String> {
    if input == "-" {
        read_stdin()
    } else if input.starts_with("http://") || input.starts_with("https://") {
        crate::net::fetch_text(input, crate::net::DOC_FETCH_CAP)
    } else {
        read_file(input)
    }
}

fn read_stdin() -> Result<String> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

/// Read a file as UTF-8, falling back to a lossy decode (with a stderr note)
/// for non-UTF-8 input instead of hard-failing. Missing files get a friendly
/// message.
fn read_file(path: &str) -> Result<String> {
    use anyhow::Context;
    let bytes = std::fs::read(path).with_context(|| format!("cannot read '{path}'"))?;
    match String::from_utf8(bytes) {
        Ok(s) => Ok(s),
        Err(e) => {
            eprintln!("ink: '{path}' is not valid UTF-8; rendering with replacements");
            Ok(String::from_utf8_lossy(e.as_bytes()).into_owned())
        }
    }
}

/// Where the active theme name came from (for `ink doctor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeOrigin {
    /// `--theme NAME`.
    Flag,
    /// `theme = "NAME"` in the config file.
    Config,
    /// Neither: `auto`, detected from the terminal background.
    Auto,
}

/// Will this run draw with the `auto` theme's colours? Only then is the
/// terminal worth asking for its background: `ink doctor` reports it, the
/// reader and `--plain` draw with it when colour is on. `--list-themes` and
/// the other subcommands never look at it.
fn uses_background(cli: &Cli, plain_color: bool) -> bool {
    match cli.command {
        Some(Commands::Doctor { .. }) => true,
        Some(_) => false,
        None if cli.list_themes => false,
        None if cli.plain => plain_color,
        None => theme::caps::color_enabled(cli.color, true),
    }
}

/// The theme to use: an explicit `--theme` beats the config file, which beats
/// `auto`. (`--theme auto` is the clap default, so it means "not given".)
fn theme_choice(flag: &str, config: &Option<config::Config>) -> (String, ThemeOrigin) {
    if flag != "auto" {
        return (flag.to_string(), ThemeOrigin::Flag);
    }
    match config.as_ref().and_then(|c| c.theme.clone()) {
        Some(t) if t != "auto" => (t, ThemeOrigin::Config),
        _ => ("auto".to_string(), ThemeOrigin::Auto),
    }
}

/// The configured clipboard mode and where it came from. An unknown value
/// warns (on stderr, before any alternate screen) and falls back to `auto`.
fn clipboard_mode(
    config: &Option<config::Config>,
) -> (crate::clipboard::ClipboardMode, &'static str) {
    use crate::clipboard::ClipboardMode;
    let Some(v) = config
        .as_ref()
        .and_then(|c| c.behavior.as_ref())
        .and_then(|b| b.clipboard.as_deref())
    else {
        return (ClipboardMode::default(), "default");
    };
    match ClipboardMode::parse(v) {
        Some(mode) => (mode, "config behavior.clipboard"),
        None => {
            eprintln!("ink: unknown clipboard mode '{v}', using 'auto'");
            (ClipboardMode::Auto, "config value not recognised; default")
        }
    }
}

/// Whether to capture the mouse, and what decided it: `--no-mouse` beats
/// `[behavior] mouse_capture`, which beats the default (on).
pub(crate) fn mouse_capture(
    no_mouse_flag: bool,
    config: &Option<config::Config>,
) -> (bool, &'static str) {
    if no_mouse_flag {
        return (false, "--no-mouse");
    }
    match config
        .as_ref()
        .and_then(|c| c.behavior.as_ref())
        .and_then(|b| b.mouse_capture)
    {
        Some(v) => (v, "config behavior.mouse_capture"),
        None => (true, "default"),
    }
}

fn resolve_width(width: &Option<WidthArg>, config: &Option<config::Config>) -> Option<u16> {
    match width {
        Some(WidthArg::Columns(n)) => Some(*n),
        Some(WidthArg::Full) => None,
        None => config.as_ref().and_then(|c| c.width),
    }
}

fn config_init(force: bool) -> Result<()> {
    let path = config::config_path()
        .ok_or_else(|| anyhow::anyhow!("could not resolve config directory"))?;
    if path.exists() && !force {
        eprintln!(
            "ink: {} already exists (pass --force to overwrite)",
            path.display()
        );
        std::process::exit(1);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, STARTER_CONFIG)?;
    println!("Wrote starter config to {}", path.display());
    Ok(())
}

pub(crate) const STARTER_CONFIG: &str = r#"# ink configuration
# https://github.com/borghei/ink

# Color theme: dark, light, dracula, catppuccin, nord, tokyo-night, gruvbox, solarized
# theme = "catppuccin"

# Max rendering width in columns (or use --width on the CLI)
# width = 90

# Line spacing: compact, normal, relaxed
# spacing = "normal"

# Show table of contents on startup
# toc = false

# Show YAML/TOML frontmatter as a code block at the top of the document
# frontmatter = false

[behavior]
# When true, q/Esc returns to the file browser instead of exiting.
# Default: false (q exits ink entirely; Shift+B reopens the browser on demand).
# browser_loop = false

# When false, ink does not capture the mouse, so your terminal's own
# click-to-open links and text selection keep working (you lose wheel-scroll
# inside ink). Default: true.
# mouse_capture = true

# How copied text reaches the clipboard:
#   auto   - OSC 52 escape (works over SSH) and a native helper, if present
#   osc52  - escape sequence only
#   native - pbcopy / wl-copy / xclip / xsel / clip.exe / termux-clipboard-set only
#   off    - copying is disabled
# clipboard = "auto"

# Draw borders, bullets and markers with plain ASCII (for the Linux console,
# legacy console fonts, screen readers). Unset: automatic on TERM=linux and
# non-UTF-8 locales. Same as --ascii.
# ascii = false

[keybindings]
# Built-in preset: "default" (vim-flavored), "vim", or "emacs"
# preset = "default"

# Per-action overrides on top of the preset.
# Run `ink keybindings` to see the full list of action IDs and their current keys.
# [keybindings.bindings]
# toggle_toc = ["ctrl-t"]
"#;

fn print_keybindings() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use std::collections::BTreeMap;

    let Some(map) = input::current_keymap() else {
        eprintln!("ink: keymap not initialized");
        return;
    };

    // Group keys by action for readable output (sorted by action name).
    let mut by_action: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ((code, mods), action) in map.iter() {
        let id = action_id(action);
        let key_str = format_key(code, mods);
        by_action.entry(id).or_default().push(key_str);
    }
    for (prefix, second, action) in input::current_chords() {
        let id = action_id(&action);
        let key_str = format!(
            "{} {}",
            format_key(&prefix.0, &prefix.1),
            format_key(&second.0, &second.1)
        );
        by_action.entry(id).or_default().push(key_str);
    }

    println!("{:<22} Keys", "Action");
    println!("{:<22} ----", "------");
    for (action, mut keys) in by_action {
        keys.sort();
        println!("{:<22} {}", action, keys.join(", "));
    }

    fn format_key(code: &KeyCode, mods: &KeyModifiers) -> String {
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
            KeyCode::PageUp => "pageup".into(),
            KeyCode::PageDown => "pagedown".into(),
            KeyCode::Home => "home".into(),
            KeyCode::End => "end".into(),
            KeyCode::Up => "up".into(),
            KeyCode::Down => "down".into(),
            KeyCode::Left => "left".into(),
            KeyCode::Right => "right".into(),
            KeyCode::Backspace => "backspace".into(),
            other => format!("{other:?}").to_lowercase(),
        };
        if parts.is_empty() {
            key
        } else {
            format!("{}-{}", parts.join("-"), key)
        }
    }

    fn action_id(a: &input::Action) -> String {
        use input::Action::*;
        match a {
            ExitApp => "exit_app",
            CloseDoc => "close_doc",
            OpenBrowser => "open_browser",
            ScrollUp(1) => "scroll_up",
            ScrollDown(1) => "scroll_down",
            ScrollUp(_) => "scroll_up_fast",
            ScrollDown(_) => "scroll_down_fast",
            PageUp => "page_up",
            PageDown => "page_down",
            Home => "home",
            End => "end",
            NextHeading => "next_heading",
            PrevHeading => "prev_heading",
            NextTab => "next_tab",
            PrevTab => "prev_tab",
            ToggleToc => "toggle_toc",
            Search => "search",
            ThemePicker => "theme_picker",
            FollowLink => "follow_link",
            LinkMode => "link_mode",
            Help => "help",
            NavBack => "nav_back",
            NavForward => "nav_forward",
            SelectMode => "select_mode",
            SelectLineMode => "select_line_mode",
            CopyCode => "copy_code",
            CopySection => "copy_section",
            _ => "?",
        }
        .to_string()
    }
}

fn print_shell_setup(shell: &str) {
    match shell.to_lowercase().as_str() {
        "bash" | "zsh" => {
            println!(
                r#"# ink — terminal markdown reader
# Add these lines to your ~/.{shell}rc:

# Quick alias to view markdown
alias md="ink"

# Browse markdown files in current directory
alias mdb="ink ."

# Use ink for fzf markdown preview (fzf pipes the preview, so ask for color)
export FZF_DEFAULT_OPTS='--preview "ink --plain --color=always {{}} 2>/dev/null"'

# Render markdown in git diffs (piped output is escape-free by default)
# git config --global diff.markdown.textconv "ink --plain"
# echo '*.md diff=markdown' >> ~/.gitattributes"#
            );
        }
        "fish" => {
            println!(
                r#"# ink — terminal markdown reader
# Add these lines to your ~/.config/fish/config.fish:

# Quick alias to view markdown
alias md "ink"

# Browse markdown files in current directory
alias mdb "ink .""#
            );
        }
        _ => {
            eprintln!(
                "ink: unsupported shell '{}'. Supported: bash, zsh, fish",
                shell
            );
        }
    }
}

fn read_input(args: &Args) -> Result<String> {
    match args.inputs.first() {
        None => read_stdin(),
        Some(input) => read_source(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(toml: &str) -> Option<config::Config> {
        config::parse_config(toml, "test").config
    }

    #[test]
    fn no_mouse_flag_parses_before_and_after_a_subcommand() {
        assert!(
            Cli::try_parse_from(["ink", "--no-mouse", "a.md"])
                .unwrap()
                .no_mouse
        );
        assert!(
            Cli::try_parse_from(["ink", "doctor", "--no-mouse"])
                .unwrap()
                .no_mouse
        );
        assert!(!Cli::try_parse_from(["ink", "a.md"]).unwrap().no_mouse);
    }

    #[test]
    fn no_mouse_beats_config_which_beats_the_default() {
        let on = cfg("[behavior]\nmouse_capture = true\n");
        let off = cfg("[behavior]\nmouse_capture = false\n");
        assert_eq!(mouse_capture(false, &None), (true, "default"));
        assert!(!mouse_capture(false, &off).0);
        assert!(mouse_capture(false, &on).0);
        assert_eq!(mouse_capture(true, &on), (false, "--no-mouse"));
        assert_eq!(mouse_capture(true, &None), (false, "--no-mouse"));
    }

    #[test]
    fn theme_flag_beats_config_which_beats_auto() {
        let c = cfg("theme = \"nord\"\n");
        assert_eq!(
            theme_choice("dracula", &c),
            ("dracula".into(), ThemeOrigin::Flag)
        );
        assert_eq!(
            theme_choice("auto", &c),
            ("nord".into(), ThemeOrigin::Config)
        );
        assert_eq!(
            theme_choice("auto", &None),
            ("auto".into(), ThemeOrigin::Auto)
        );
        let auto = cfg("theme = \"auto\"\n");
        assert_eq!(
            theme_choice("auto", &auto),
            ("auto".into(), ThemeOrigin::Auto)
        );
    }
}
