use crate::{app, browser, config, input, render, stats, theme};
use anyhow::Result;
use clap::{Parser as ClapParser, Subcommand};
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

    /// Markdown file path(s) or URL (reads stdin if omitted)
    #[arg(value_name = "FILE|URL")]
    pub input: Vec<String>,

    /// Color theme
    #[arg(short, long, default_value = "auto")]
    pub theme: String,

    /// Max rendering width in columns (or: narrow, wide, full)
    #[arg(short, long)]
    pub width: Option<String>,

    /// Presentation mode (split on ---)
    #[arg(short, long)]
    pub slides: bool,

    /// Plain output mode (no TUI, pipe-friendly)
    #[arg(short, long)]
    pub plain: bool,

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
    #[arg(long, default_value = "auto")]
    pub image_protocol: String,

    /// List available themes and exit
    #[arg(long)]
    pub list_themes: bool,

    /// Do not page --plain output through $PAGER even on a TTY
    #[arg(long)]
    pub no_pager: bool,

    /// Show YAML/TOML frontmatter
    #[arg(long)]
    pub frontmatter: bool,

    /// Line spacing: compact, normal, relaxed
    #[arg(long, default_value = "normal")]
    pub spacing: String,
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
    /// Configuration helpers
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Write a starter config to ~/.config/ink/config.toml
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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    Compact,
    Normal,
    Relaxed,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    // Load config + initialize keymap up front so subcommands (e.g. `keybindings`)
    // can read the resolved map.
    let user_config = config::load_config();
    input::init_keymap(user_config.as_ref().and_then(|c| c.keybindings.as_ref()));

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
                stats::print_diff(&source_a, &source_b, file_a, file_b);
                Ok(())
            }
            Commands::ShellSetup { shell } => {
                print_shell_setup(shell);
                Ok(())
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
    let spacing = match cli.spacing.as_str() {
        "compact" => Spacing::Compact,
        "relaxed" => Spacing::Relaxed,
        _ => Spacing::Normal,
    };

    let theme = if cli.theme == "auto" {
        user_config
            .as_ref()
            .and_then(|c| c.theme.clone())
            .unwrap_or_else(|| "auto".to_string())
    } else {
        cli.theme.clone()
    };

    let args = Args {
        inputs: cli.input.clone(),
        theme,
        width,
        slides: cli.slides,
        plain: cli.plain,
        watch: cli.watch,
        toc: cli.toc,
        images: if cli.no_images {
            crate::image::ImageMode::Off
        } else if cli.remote_images {
            crate::image::ImageMode::All
        } else {
            crate::image::ImageMode::LocalOnly
        },
        image_protocol: crate::graphics::ProtocolChoice::parse(&cli.image_protocol)
            .unwrap_or(crate::graphics::ProtocolChoice::Auto),
        frontmatter: cli.frontmatter,
        spacing,
        mouse_capture: user_config
            .as_ref()
            .and_then(|c| c.behavior.as_ref())
            .and_then(|b| b.mouse_capture)
            .unwrap_or(true),
    };

    // Check if input is a directory or no input with a TTY → launch file browser
    let browse_dir = if args.inputs.is_empty() {
        if std::io::stdin().is_terminal() {
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
        // Browser → doc → exit by default. User can press Shift-B inside the
        // doc (or set behavior.browser_loop = true in config) to return here.
        let browser_loop = user_config
            .as_ref()
            .and_then(|c| c.behavior.as_ref())
            .and_then(|b| b.browser_loop)
            .unwrap_or(false);

        loop {
            let Some(selected) = browser::browse(&dir, &args.theme)? else {
                break;
            };
            let source = std::fs::read_to_string(&selected)?;
            let mut file_args = args.clone();
            file_args.inputs = vec![selected.to_string_lossy().to_string()];
            if file_args.plain {
                let rendered = render::plain::render_plain(&source, &file_args)?;
                print!("{rendered}");
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

    let source = read_input(&args)?;

    if args.plain {
        let rendered = render::plain::render_plain(&source, &args)?;
        emit_plain(&rendered, cli.no_pager);
        return Ok(());
    }

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
        print!("{rendered}");
        return;
    }

    let pager = std::env::var("PAGER").unwrap_or_else(|_| "less -R".to_string());
    let mut parts = pager.split_whitespace();
    let Some(program) = parts.next() else {
        print!("{rendered}");
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
        Err(_) => print!("{rendered}"),
    }
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

fn resolve_width(width_str: &Option<String>, config: &Option<config::Config>) -> Option<u16> {
    if let Some(w) = width_str {
        match w.as_str() {
            "narrow" => return Some(60),
            "wide" => return Some(100),
            "full" => return None,
            _ => {
                if let Ok(n) = w.parse::<u16>() {
                    return Some(n);
                }
            }
        }
    }
    config.as_ref().and_then(|c| c.width)
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

const STARTER_CONFIG: &str = r#"# ink configuration
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

# Use ink for fzf markdown preview
export FZF_DEFAULT_OPTS='--preview "ink --plain {{}} 2>/dev/null"'

# Use ink as a git pager for markdown diffs
# git config --global diff.markdown.textconv "ink --plain""#
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
    if args.inputs.is_empty() {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        return Ok(buf);
    }

    let input = &args.inputs[0];
    if input.starts_with("http://") || input.starts_with("https://") {
        crate::net::fetch_text(input, crate::net::DOC_FETCH_CAP)
    } else {
        read_file(input)
    }
}
