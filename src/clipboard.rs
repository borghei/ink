//! Putting text on the system clipboard from inside a full-screen TUI.
//!
//! Two independent, best-effort deliveries, because neither one covers every
//! terminal ink runs in:
//!
//! * **OSC 52** — an escape sequence the terminal itself acts on, so it crosses
//!   an SSH boundary. Some terminals ship it disabled (Terminal.app) and none
//!   of them report back, so it can silently do nothing.
//! * **A native helper** (`pbcopy`, `wl-copy`, `xclip`, `xsel`, `clip.exe`) —
//!   reliable locally, useless over SSH (it would write the *server's*
//!   clipboard).
//!
//! Running both can write the same bytes twice. That is harmless, and cheaper
//! than trying to detect which one worked.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use std::io::Write;
use std::process::{Command, Stdio};

/// How `copy` is allowed to reach the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipboardMode {
    /// Escape sequence *and* native helper (default).
    #[default]
    Auto,
    /// Escape sequence only.
    Osc52,
    /// Native helper only.
    Native,
    /// Copy is a no-op.
    Off,
}

impl ClipboardMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "both" => Some(Self::Auto),
            "osc52" | "osc" => Some(Self::Osc52),
            "native" | "helper" => Some(Self::Native),
            "off" | "none" | "false" => Some(Self::Off),
            _ => None,
        }
    }
}

/// What happened, for the status-bar flash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOutcome {
    /// Reached at least one delivery path.
    Copied,
    /// Clipboard is turned off in config.
    Disabled,
    /// Nothing to copy (empty selection).
    Empty,
    /// Too big for OSC 52 and no helper on PATH.
    TooLarge,
    /// Every available path failed.
    Failed,
}

/// xterm's hard cap on an OSC 52 payload, in base64 bytes. Past it terminals
/// drop the whole sequence, so a 200 KB selection would silently copy nothing.
const OSC52_MAX_B64: usize = 74_994;

/// Build the OSC 52 sequence for `text`, or `None` if it exceeds the cap.
///
/// Inside tmux the sequence has to be wrapped in a passthrough envelope, with
/// every inner ESC doubled, or tmux swallows it instead of forwarding it to the
/// outer terminal.
pub fn osc52_payload(text: &str) -> Option<String> {
    osc52_payload_in(text, std::env::var_os("TMUX").is_some())
}

/// Testable core of [`osc52_payload`]: `tmux` selects the passthrough envelope
/// instead of reading the environment.
pub fn osc52_payload_in(text: &str, tmux: bool) -> Option<String> {
    let encoded = STANDARD.encode(text.as_bytes());
    if encoded.len() > OSC52_MAX_B64 {
        return None;
    }
    let inner = format!("\x1b]52;c;{encoded}\x07");
    if !tmux {
        return Some(inner);
    }
    Some(format!(
        "\x1bPtmux;{}\x1b\\",
        inner.replace('\x1b', "\x1b\x1b")
    ))
}

/// The clipboard helper to use on this machine, as (program, args).
///
/// Ordered by how specific the signal is: a Wayland session that also exports
/// `DISPLAY` (XWayland) should still get `wl-copy`.
fn native_helper() -> Option<(&'static str, &'static [&'static str])> {
    let candidates: &[(&str, &[&str], bool)] = &[
        ("pbcopy", &[], cfg!(target_os = "macos")),
        (
            "wl-copy",
            &[],
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
        ),
        (
            "xclip",
            &["-selection", "clipboard"],
            std::env::var_os("DISPLAY").is_some(),
        ),
        (
            "xsel",
            &["--clipboard", "--input"],
            std::env::var_os("DISPLAY").is_some(),
        ),
        ("clip.exe", &[], true),
    ];
    candidates
        .iter()
        .find(|(prog, _, applicable)| *applicable && on_path(prog))
        .map(|(prog, args, _)| (*prog, *args))
}

fn on_path(program: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
}

/// Hand `text` to the native helper, if there is one.
///
/// Deliberately does not wait for the child: `wl-copy` and `xclip` stay
/// resident to *own* the X/Wayland selection, so waiting would hang ink until
/// the user copied something else.
fn copy_native(text: &str) -> bool {
    let Some((prog, args)) = native_helper() else {
        return false;
    };
    let Ok(mut child) = Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let wrote = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(text.as_bytes()).is_ok(),
        None => false,
    };
    // Reap it if it already exited (pbcopy, clip.exe); leave the persistent
    // ones alone.
    let _ = child.try_wait();
    wrote
}

/// Write the OSC 52 sequence straight to the terminal.
///
/// Safe to do mid-frame: it paints nothing, and ratatui redraws over it on the
/// next tick regardless.
fn copy_osc52(text: &str) -> Option<bool> {
    let payload = osc52_payload(text)?;
    let mut out = std::io::stdout();
    let ok = out.write_all(payload.as_bytes()).is_ok() && out.flush().is_ok();
    Some(ok)
}

/// Put `text` on the clipboard by every route `mode` allows.
pub fn copy(text: &str, mode: ClipboardMode) -> CopyOutcome {
    if mode == ClipboardMode::Off {
        return CopyOutcome::Disabled;
    }
    if text.is_empty() {
        return CopyOutcome::Empty;
    }

    let want_osc = matches!(mode, ClipboardMode::Auto | ClipboardMode::Osc52);
    let want_native = matches!(mode, ClipboardMode::Auto | ClipboardMode::Native);

    let mut any = false;
    // `Some(false)` = tried and failed, `None` = over the size cap.
    let mut oversized = false;
    if want_osc {
        match copy_osc52(text) {
            Some(true) => any = true,
            Some(false) => {}
            None => oversized = true,
        }
    }
    if want_native && copy_native(text) {
        any = true;
    }

    if any {
        CopyOutcome::Copied
    } else if oversized {
        CopyOutcome::TooLarge
    } else {
        CopyOutcome::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_payload_is_base64_in_an_osc_52_sequence() {
        assert_eq!(osc52_payload_in("hi", false).unwrap(), "\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn osc52_payload_wraps_for_tmux_and_doubles_escapes() {
        let wrapped = osc52_payload_in("hi", true).unwrap();
        assert_eq!(wrapped, "\x1bPtmux;\x1b\x1b]52;c;aGk=\x07\x1b\\");
        // No lone ESC survives inside the envelope: every one is doubled, so
        // tmux forwards the sequence instead of eating it. Removing the pairs
        // must leave a body with no ESC left in it.
        let body = &wrapped["\x1bPtmux;".len()..wrapped.len() - 2];
        assert!(!body.replace("\x1b\x1b", "").contains('\x1b'));
    }

    #[test]
    fn osc52_payload_refuses_an_oversized_selection() {
        // 200 KB of text encodes well past the 74_994-byte cap.
        let big = "x".repeat(200_000);
        assert!(osc52_payload_in(&big, false).is_none());
        // Just under the cap still encodes.
        let ok = "x".repeat(OSC52_MAX_B64 / 4 * 3);
        assert!(osc52_payload_in(&ok, false).is_some());
    }

    #[test]
    fn osc52_payload_survives_non_ascii() {
        let payload = osc52_payload_in("héllo → 世界", false).unwrap();
        let b64 = payload
            .trim_start_matches("\x1b]52;c;")
            .trim_end_matches('\x07');
        let decoded = STANDARD.decode(b64).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), "héllo → 世界");
    }

    #[test]
    fn off_writes_nothing() {
        assert_eq!(copy("anything", ClipboardMode::Off), CopyOutcome::Disabled);
    }

    #[test]
    fn empty_selection_is_not_a_copy() {
        assert_eq!(copy("", ClipboardMode::Auto), CopyOutcome::Empty);
    }

    #[test]
    fn mode_parses_its_config_spellings() {
        assert_eq!(ClipboardMode::parse("auto"), Some(ClipboardMode::Auto));
        assert_eq!(ClipboardMode::parse("OSC52"), Some(ClipboardMode::Osc52));
        assert_eq!(
            ClipboardMode::parse(" native "),
            Some(ClipboardMode::Native)
        );
        assert_eq!(ClipboardMode::parse("off"), Some(ClipboardMode::Off));
        assert_eq!(ClipboardMode::parse("sometimes"), None);
    }
}
