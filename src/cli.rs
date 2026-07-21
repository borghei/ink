use crate::{app, browser, config, input, render, stats};
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
    pub no_images: bool,
    pub frontmatter: bool,
    pub spacing: Spacing,
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
                let source = std::fs::read_to_string(file)?;
                stats::print_outline(&source);
                Ok(())
            }
            Commands::Stats { file } => {
                let source = std::fs::read_to_string(file)?;
                stats::print_stats(&source, file);
                Ok(())
            }
            Commands::Diff { file_a, file_b } => {
                let source_a = std::fs::read_to_string(file_a)?;
                let source_b = std::fs::read_to_string(file_b)?;
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
            Commands::Config { action } => match action {
                ConfigAction::Init { force } => config_init(*force),
                ConfigAction::Path => {
                    println!("{}", config::config_path_display());
                    Ok(())
                }
            },
        };
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
        no_images: cli.no_images,
        frontmatter: cli.frontmatter,
        spacing,
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
        print!("{rendered}");
        return Ok(());
    }

    app::run(source, args)?;

    Ok(())
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
        let resp = reqwest::blocking::get(input)?;
        Ok(resp.text()?)
    } else {
        let path = PathBuf::from(input);
        Ok(std::fs::read_to_string(&path)?)
    }
}
