//! `ink doctor` — a self-contained diagnostic report. When a user says
//! "images don't show", "the colours are wrong" or "copy does nothing", this
//! collects everything needed to debug it remotely: version, platform,
//! terminal identity, colour and theme decisions, clipboard route, config
//! problems, the graphics-protocol negotiation, and decoder self-tests.
//!
//! The report is meant to be pasted into a public issue, so it never prints
//! anything that identifies a person or a session: no hostnames, usernames,
//! IP addresses, session IDs or socket paths, and the home directory is
//! shown as `~`.
//!
//! Terminal queries only run when stdout is a real TTY; piped output
//! (`ink doctor | pbcopy`) says "not a terminal" for those fields. `--save`
//! additionally writes the report to a file the user can attach to an issue.

use crate::cli::ThemeOrigin;
use crate::clipboard::ClipboardMode;
use crate::theme::caps::ColorChoice;
use std::fmt::Write as _;
use std::io::IsTerminal;

/// What the CLI resolved before handing over to the doctor.
pub struct Context<'a> {
    pub theme: &'a str,
    pub theme_origin: ThemeOrigin,
    pub color_choice: ColorChoice,
    pub clipboard: ClipboardMode,
    /// Where the clipboard mode came from ("config" / "default").
    pub clipboard_source: &'static str,
    /// Mouse capture on/off and what decided it.
    pub mouse: (bool, &'static str),
    pub config_warnings: &'a [String],
}

/// Build the report and print it; optionally save to `path`.
pub fn run(save: Option<&std::path::Path>, ctx: &Context) -> anyhow::Result<()> {
    let report = build_report(ctx);
    print!("{report}");
    if let Some(path) = save {
        std::fs::write(path, &report)?;
        println!("\nReport saved to {}.", path.display());
        println!("Attach it to an issue: https://github.com/borghei/ink/issues/new/choose");
    }
    Ok(())
}

fn build_report(ctx: &Context) -> String {
    let tty = std::io::stdout().is_terminal();
    let home = dirs::home_dir().map(|h| h.display().to_string());
    let mut r = String::new();
    let _ = writeln!(r, "ink doctor — environment diagnostics");
    let _ = writeln!(r, "====================================");
    let _ = writeln!(r, "ink version : {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        r,
        "platform    : {} / {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH,
        libc_flavour()
    );

    environment_section(&mut r);
    terminal_section(&mut r, tty);
    colour_section(&mut r, ctx, tty);
    theme_section(&mut r, ctx);
    clipboard_section(&mut r, ctx);
    input_section(&mut r, ctx);
    config_section(&mut r, ctx, home.as_deref());
    graphics_section(&mut r, tty);
    decoder_section(&mut r);

    let _ = writeln!(
        r,
        "\nIf images are blank or wrong: try `ink --image-protocol halfblocks <file>`.\n\
         If half-blocks work but pixels don't, your terminal's graphics protocol\n\
         is the problem — please open an issue with this report attached:\n\
         https://github.com/borghei/ink/issues/new?template=image-rendering.yml"
    );
    match home {
        Some(h) => redact_home(&r, &h),
        None => r,
    }
}

/// Which C library the binary was built against.
fn libc_flavour() -> &'static str {
    if cfg!(target_env = "musl") {
        "musl"
    } else if cfg!(target_env = "gnu") {
        "glibc"
    } else if cfg!(target_env = "msvc") {
        "msvc"
    } else {
        "system libc"
    }
}

/// `true`/`false` as `yes`/`no`.
fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn set_or_unset(var: &str) -> &'static str {
    if std::env::var_os(var).is_some_and(|v| !v.is_empty()) {
        "(set)"
    } else {
        "(unset)"
    }
}

fn environment_section(r: &mut String) {
    let _ = writeln!(r, "\n[environment]");
    let _ = writeln!(r, "{:<21}= {}", "WSL", yn(crate::platform::is_wsl()));
    // Only whether this is SSH: the connection string holds both IPs.
    let _ = writeln!(
        r,
        "{:<21}= {}",
        "SSH session",
        yn(crate::platform::is_ssh())
    );
    let mux: Vec<&str> = [("TMUX", "tmux"), ("STY", "screen"), ("ZELLIJ", "zellij")]
        .into_iter()
        .filter(|(var, _)| std::env::var_os(var).is_some())
        .map(|(_, name)| name)
        .collect();
    let mux = if mux.is_empty() {
        "none".to_string()
    } else {
        mux.join(", ")
    };
    let _ = writeln!(r, "{:<21}= {mux}", "multiplexer");
    // Values that identify the terminal program, not the person.
    for var in [
        "TERM",
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "LC_TERMINAL",
        "COLORTERM",
        "KONSOLE_VERSION",
        "NO_COLOR",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
    ] {
        let _ = writeln!(r, "{var:<21}= {}", fmt_env(var));
    }
    let utf8 = match crate::platform::locale_is_utf8() {
        Some(true) => "yes".to_string(),
        Some(false) => "no".to_string(),
        None => "unknown (no LC_ALL / LC_CTYPE / LANG)".to_string(),
    };
    let _ = writeln!(r, "{:<21}= {utf8}", "locale is UTF-8");
    // Presence only: display names and session IDs can encode hostnames,
    // window titles or paths.
    for var in [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "TMUX",
        "STY",
        "WT_SESSION",
        "KITTY_WINDOW_ID",
        "WEZTERM_EXECUTABLE",
        "ITERM_SESSION_ID",
        "TERMUX_VERSION",
    ] {
        let _ = writeln!(r, "{var:<21}= {}", set_or_unset(var));
    }
}

fn terminal_section(r: &mut String, tty: bool) {
    let _ = writeln!(r, "\n[terminal]");
    let _ = writeln!(r, "stdout is a TTY      = {tty}");
    let _ = writeln!(
        r,
        "stdin is a TTY       = {}",
        std::io::stdin().is_terminal()
    );
    if !tty {
        let _ = writeln!(r, "size                 = not a terminal");
        return;
    }
    match crossterm::terminal::size() {
        Ok((w, h)) => {
            let _ = writeln!(r, "size                 = {w}x{h} cells");
        }
        Err(e) => {
            let _ = writeln!(r, "size                 = unavailable ({e})");
        }
    }
}

fn colour_section(r: &mut String, ctx: &Context, tty: bool) {
    use crate::theme::caps;
    let _ = writeln!(r, "\n[colour]");
    let (level, signal) = caps::depth_report();
    let _ = writeln!(r, "depth                = {level} (decided by {signal})");
    // The reader always runs on a terminal; --plain follows stdout.
    let (tui, tui_why) = caps::color_enabled_why(ctx.color_choice, true);
    let _ = writeln!(
        r,
        "reader colours       = {} ({tui_why})",
        if tui { "on" } else { "off" }
    );
    let (plain, plain_why) = caps::color_enabled_why(ctx.color_choice, tty);
    let _ = writeln!(
        r,
        "--plain colours here = {} ({plain_why})",
        if plain { "on" } else { "off" }
    );
}

fn theme_section(r: &mut String, ctx: &Context) {
    use crate::theme::detect::BackgroundSource;
    let _ = writeln!(r, "\n[theme]");
    let theme = ctx.theme;
    match ctx.theme_origin {
        ThemeOrigin::Flag => {
            let _ = writeln!(r, "theme                = {theme} (from --theme)");
        }
        ThemeOrigin::Config => {
            let _ = writeln!(r, "theme                = {theme} (from config)");
        }
        ThemeOrigin::Auto => {
            let bg = crate::theme::detect::background();
            let resolved = if bg.dark { "dark" } else { "light" };
            let why = match &bg.source {
                BackgroundSource::Osc11 {
                    rgb: (r8, g8, b8),
                    raw,
                } => format!("terminal background {raw} = #{r8:02x}{g8:02x}{b8:02x}, via OSC 11"),
                BackgroundSource::Colorfgbg(v) => format!("COLORFGBG={v}"),
                BackgroundSource::Default => "default; nothing reported a background".into(),
            };
            let _ = writeln!(r, "theme                = auto -> {resolved} ({why})");
            let _ = writeln!(r, "background query     = {}", bg.query);
        }
    }
}

fn clipboard_section(r: &mut String, ctx: &Context) {
    use crate::clipboard::{self, Wrap};
    let _ = writeln!(r, "\n[clipboard]");
    let _ = writeln!(
        r,
        "mode                 = {:?} ({})",
        ctx.clipboard, ctx.clipboard_source
    );
    let helper = clipboard::native_helper()
        .map(|h| h.program.to_string())
        .unwrap_or_else(|| "none found".into());
    let _ = writeln!(r, "native helper        = {helper}");
    let wrap = match Wrap::detect() {
        Wrap::None => "none",
        Wrap::Tmux => "tmux passthrough",
        Wrap::Screen => "GNU screen DCS chunks",
    };
    let _ = writeln!(r, "OSC 52 wrapping      = {wrap}");
}

fn input_section(r: &mut String, ctx: &Context) {
    let _ = writeln!(r, "\n[input]");
    let (on, why) = ctx.mouse;
    let _ = writeln!(
        r,
        "mouse capture        = {} ({why})",
        if on { "on" } else { "off" }
    );
}

fn config_section(r: &mut String, ctx: &Context, home: Option<&str>) {
    let _ = writeln!(r, "\n[config]");
    match crate::config::config_path() {
        Some(p) => {
            let shown = p.display().to_string();
            let shown = home.map_or(shown.clone(), |h| redact_home(&shown, h));
            let _ = writeln!(r, "path                 = {shown}");
            let _ = writeln!(r, "exists               = {}", yn(p.is_file()));
        }
        None => {
            let _ = writeln!(
                r,
                "path                 = (no config directory on this platform)"
            );
        }
    }
    if ctx.config_warnings.is_empty() {
        let _ = writeln!(r, "warnings             = none");
    } else {
        for w in ctx.config_warnings {
            let _ = writeln!(r, "warning              = {w}");
        }
    }
}

fn graphics_section(r: &mut String, tty: bool) {
    let _ = writeln!(r, "\n[graphics protocol]");
    if std::env::var_os("TMUX").is_some() {
        let _ = writeln!(
            r,
            "tmux allow-passthrough = {}",
            tmux_allow_passthrough().unwrap_or_else(|| "unknown".into())
        );
    }
    if !tty {
        let _ = writeln!(
            r,
            "query result         = skipped (not a terminal; run `ink doctor` directly in your terminal)"
        );
        return;
    }
    if let Some(why) = crate::graphics::query_skip_reason() {
        let _ = writeln!(
            r,
            "query result         = skipped ({why}) — ink uses half-blocks unless --image-protocol forces one"
        );
        return;
    }
    match ratatui_image::picker::Picker::from_query_stdio() {
        Ok(p) => {
            let detected = p.protocol_type();
            let _ = writeln!(r, "query result         = {detected:?}");
            let _ = writeln!(
                r,
                "font size            = {}x{} px",
                p.font_size().width,
                p.font_size().height
            );
            let chosen = crate::graphics::auto_protocol_for_report(detected);
            let _ = writeln!(r, "ink will use         = {chosen:?}");
            if chosen != detected {
                let _ = writeln!(
                    r,
                    "                       (overridden: this terminal's {detected:?} support is known-incomplete)"
                );
            }
        }
        Err(e) => {
            let _ = writeln!(
                r,
                "query result         = failed ({e}) — ink falls back to half-blocks"
            );
        }
    }
}

/// tmux's `allow-passthrough` for this pane. The image probe switches it on
/// under tmux (images need it); report what it is rather than change it.
fn tmux_allow_passthrough() -> Option<String> {
    let out = std::process::Command::new("tmux")
        .args(["show", "-p", "-v", "allow-passthrough"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Some(if v.is_empty() {
            "off (unset)".into()
        } else {
            v
        })
    } else {
        None
    }
}

fn decoder_section(r: &mut String) {
    let _ = writeln!(r, "\n[decoder self-test]");
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="lime"/></svg>"##;
    let _ = writeln!(r, "svg rasterize        = {}", self_test_svg(svg));
    let _ = writeln!(r, "png decode           = {}", self_test_png());
}

fn fmt_env(var: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| "(unset)".into())
}

/// Replace every occurrence of the home directory with `~`, so paths in the
/// report do not carry the user name.
fn redact_home(text: &str, home: &str) -> String {
    let home = home.trim_end_matches(['/', '\\']);
    // A bare `/` (or empty) home would rewrite every path; leave it alone.
    if home.len() < 2 {
        return text.to_string();
    }
    text.replace(home, "~")
}

fn self_test_svg(svg: &[u8]) -> &'static str {
    match crate::image::self_test_rasterize(svg) {
        true => "OK",
        false => "FAILED",
    }
}

fn self_test_png() -> &'static str {
    // 1x1 red PNG, embedded.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    match crate::image::self_test_decode(PNG) {
        true => "OK",
        false => "FAILED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(warnings: &[String]) -> Context<'_> {
        Context {
            theme: "dracula",
            theme_origin: ThemeOrigin::Flag,
            color_choice: ColorChoice::Auto,
            clipboard: ClipboardMode::Auto,
            clipboard_source: "default",
            mouse: (false, "--no-mouse"),
            config_warnings: warnings,
        }
    }

    #[test]
    fn report_has_every_section() {
        let report = build_report(&ctx(&[]));
        for header in [
            "[environment]",
            "[terminal]",
            "[colour]",
            "[theme]",
            "[clipboard]",
            "[input]",
            "[config]",
            "[graphics protocol]",
            "[decoder self-test]",
        ] {
            assert!(report.contains(header), "missing {header}:\n{report}");
        }
        assert!(report.contains("mouse capture        = off (--no-mouse)"));
        assert!(report.contains("dracula (from --theme)"));
    }

    #[test]
    fn home_directory_is_redacted() {
        assert_eq!(
            redact_home("path = /home/alice/.config/ink/config.toml", "/home/alice"),
            "path = ~/.config/ink/config.toml"
        );
        assert_eq!(
            redact_home(r"C:\Users\alice\AppData\Roaming\ink", r"C:\Users\alice\"),
            r"~\AppData\Roaming\ink"
        );
        // A degenerate home must not mangle every path.
        assert_eq!(redact_home("/etc/x", "/"), "/etc/x");
    }

    #[test]
    fn config_warnings_are_reported_with_home_redacted() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let warning = format!(
            "ink: config error in {}/.config/ink/config.toml: bad",
            home.display()
        );
        let report = build_report(&ctx(std::slice::from_ref(&warning)));
        assert!(report.contains("config error in ~/.config/ink/config.toml: bad"));
        assert!(!report.contains(&home.display().to_string()));
    }

    #[test]
    fn piped_report_does_not_query_the_terminal() {
        // `cargo test` captures stdout, so this is the non-TTY path.
        if std::io::stdout().is_terminal() {
            return;
        }
        let report = build_report(&ctx(&[]));
        assert!(report.contains("size                 = not a terminal"));
        assert!(report.contains("skipped (not a terminal"));
    }
}
