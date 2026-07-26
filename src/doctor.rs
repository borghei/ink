//! `ink doctor` — a self-contained diagnostic report for image-rendering
//! issues. When a user says "images don't show", this collects everything
//! needed to debug it remotely: version, platform, terminal identity, the
//! graphics-protocol negotiation result, and decoder self-tests.
//!
//! The protocol query talks to the terminal over stdio, so it only runs when
//! stdout is a real TTY; `--save` additionally writes the report to a file
//! the user can attach to a GitHub issue.

use std::fmt::Write as _;
use std::io::IsTerminal;

/// Build the report and print it; optionally save to `path`.
pub fn run(save: Option<&std::path::Path>) -> anyhow::Result<()> {
    let report = build_report();
    print!("{report}");
    if let Some(path) = save {
        std::fs::write(path, &report)?;
        println!("\nReport saved to {}.", path.display());
        println!("Attach it to an issue: https://github.com/borghei/ink/issues/new?template=image-rendering.yml");
    }
    Ok(())
}

fn build_report() -> String {
    let mut r = String::new();
    let _ = writeln!(r, "ink doctor — image rendering diagnostics");
    let _ = writeln!(r, "=========================================");
    let _ = writeln!(r, "ink version : {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        r,
        "platform    : {} / {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    let _ = writeln!(r, "\n[environment]");
    // Values that identify the terminal; session IDs are reported only as
    // set/unset (they can encode window titles or paths).
    for var in [
        "TERM",
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "LC_TERMINAL",
        "COLORTERM",
        "KONSOLE_VERSION",
        "NO_COLOR",
    ] {
        let _ = writeln!(r, "{var:<21}= {}", fmt_env(var));
    }
    for var in [
        "TMUX",
        "KITTY_WINDOW_ID",
        "WEZTERM_EXECUTABLE",
        "ITERM_SESSION_ID",
    ] {
        let set = std::env::var_os(var).is_some_and(|v| !v.is_empty());
        let _ = writeln!(r, "{var:<21}= {}", if set { "(set)" } else { "(unset)" });
    }

    let _ = writeln!(r, "\n[terminal]");
    match crossterm::terminal::size() {
        Ok((w, h)) => {
            let _ = writeln!(r, "size                 = {w}x{h} cells");
        }
        Err(e) => {
            let _ = writeln!(r, "size                 = unavailable ({e})");
        }
    }
    let _ = writeln!(
        r,
        "stdout is a TTY      = {}",
        std::io::stdout().is_terminal()
    );

    let _ = writeln!(r, "\n[graphics protocol]");
    if std::io::stdout().is_terminal() {
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
    } else {
        let _ = writeln!(
            r,
            "query result         = skipped (stdout is not a TTY; run `ink doctor` directly in your terminal)"
        );
    }

    let _ = writeln!(r, "\n[decoder self-test]");
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="lime"/></svg>"##;
    let _ = writeln!(r, "svg rasterize        = {}", self_test_svg(svg));
    let _ = writeln!(r, "png decode           = {}", self_test_png());

    let _ = writeln!(
        r,
        "\nIf images are blank or wrong: try `ink --image-protocol halfblocks <file>`.\n\
         If half-blocks work but pixels don't, your terminal's graphics protocol\n\
         is the problem — please open an issue with this report attached:\n\
         https://github.com/borghei/ink/issues/new?template=image-rendering.yml"
    );
    r
}

fn fmt_env(var: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| "(unset)".into())
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
