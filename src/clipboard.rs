//! Putting text on the system clipboard from inside a full-screen TUI.
//!
//! Two independent, best-effort deliveries, because neither one covers every
//! terminal ink runs in:
//!
//! * **OSC 52** — an escape sequence the terminal itself acts on, so it crosses
//!   an SSH boundary. Some terminals ship it disabled (Terminal.app) and none
//!   of them report back, so it can silently do nothing.
//! * **A native helper** (`pbcopy`, `termux-clipboard-set`, `wl-copy`,
//!   `xclip`, `xsel`, `clip.exe`, or PowerShell's `Set-Clipboard` on WSL
//!   without `clip.exe`) — reliable locally, useless over SSH (it would write
//!   the *server's* clipboard).
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

/// Which multiplexer envelope an OSC 52 sequence needs to reach the outer
/// terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    /// Write it as is.
    None,
    /// tmux passthrough: `ESC P tmux; … ESC \`, inner ESCs doubled.
    Tmux,
    /// GNU screen: the sequence split into short DCS strings, which screen
    /// strips and forwards verbatim.
    Screen,
}

impl Wrap {
    /// The envelope this session needs.
    pub fn detect() -> Self {
        Self::from_env(
            std::env::var_os("TMUX").is_some(),
            std::env::var_os("STY").is_some(),
            &std::env::var("TERM").unwrap_or_default(),
        )
    }

    /// Pure form of [`Wrap::detect`]. tmux wins when both are present: when
    /// screen runs tmux, tmux is the layer ink talks to.
    pub fn from_env(tmux: bool, sty: bool, term: &str) -> Self {
        if tmux {
            Self::Tmux
        } else if sty && term.starts_with("screen") {
            Self::Screen
        } else {
            Self::None
        }
    }
}

/// GNU screen drops a DCS string past its buffer (768 bytes in most builds,
/// less in some), so the sequence goes out in pieces well under that.
const SCREEN_DCS_CHUNK: usize = 76;

/// Build the OSC 52 sequence for `text`, or `None` if it exceeds the cap.
///
/// Inside tmux the sequence has to be wrapped in a passthrough envelope, with
/// every inner ESC doubled, or tmux swallows it instead of forwarding it to the
/// outer terminal. GNU screen needs it in DCS chunks.
pub fn osc52_payload(text: &str) -> Option<String> {
    osc52_payload_wrapped(text, Wrap::detect())
}

/// Testable core of [`osc52_payload`]: `tmux` selects the passthrough envelope
/// instead of reading the environment.
pub fn osc52_payload_in(text: &str, tmux: bool) -> Option<String> {
    osc52_payload_wrapped(text, if tmux { Wrap::Tmux } else { Wrap::None })
}

/// Build the OSC 52 sequence for `text` in the given envelope.
pub fn osc52_payload_wrapped(text: &str, wrap: Wrap) -> Option<String> {
    let encoded = STANDARD.encode(text.as_bytes());
    if encoded.len() > OSC52_MAX_B64 {
        return None;
    }
    let inner = format!("\x1b]52;c;{encoded}\x07");
    Some(match wrap {
        Wrap::None => inner,
        Wrap::Tmux => format!("\x1bPtmux;{}\x1b\\", inner.replace('\x1b', "\x1b\x1b")),
        Wrap::Screen => {
            // The sequence is ASCII (ESC, `]52;c;`, base64, BEL), so byte
            // chunks never split a character.
            let mut out = String::with_capacity(inner.len() + inner.len() / 8 + 8);
            for chunk in inner.as_bytes().chunks(SCREEN_DCS_CHUNK) {
                out.push_str("\x1bP");
                out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
                out.push_str("\x1b\\");
            }
            out
        }
    })
}

/// How a helper wants its input encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    /// `clip.exe` decodes plain bytes with the console code page, which turns
    /// anything outside ASCII into mojibake; given a UTF-16LE byte-order mark
    /// it reads the text as Unicode.
    Utf16LeBom,
}

/// A native clipboard program and how to feed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Helper {
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub encoding: Encoding,
}

impl Helper {
    /// The exact bytes to write to the helper's stdin for `text`.
    pub fn input_bytes(&self, text: &str) -> Vec<u8> {
        match self.encoding {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Utf16LeBom => utf16le_with_bom(text),
        }
    }
}

/// `text` as UTF-16LE, prefixed with the byte-order mark `FF FE`.
pub fn utf16le_with_bom(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + text.len() * 2);
    out.extend_from_slice(&[0xFF, 0xFE]);
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// What the helper choice depends on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HelperEnv {
    pub macos: bool,
    /// `TERMUX_VERSION` is set (Android, Termux).
    pub termux: bool,
    /// `WAYLAND_DISPLAY` is set.
    pub wayland: bool,
    /// `DISPLAY` is set.
    pub x11: bool,
    /// Windows Subsystem for Linux.
    pub wsl: bool,
}

impl HelperEnv {
    pub fn detect() -> Self {
        Self {
            macos: cfg!(target_os = "macos"),
            termux: std::env::var_os("TERMUX_VERSION").is_some(),
            wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
            x11: std::env::var_os("DISPLAY").is_some(),
            wsl: crate::platform::is_wsl(),
        }
    }
}

const PBCOPY: Helper = Helper {
    program: "pbcopy",
    args: &[],
    encoding: Encoding::Utf8,
};
const TERMUX: Helper = Helper {
    program: "termux-clipboard-set",
    args: &[],
    encoding: Encoding::Utf8,
};
const WL_COPY: Helper = Helper {
    program: "wl-copy",
    args: &[],
    encoding: Encoding::Utf8,
};
const XCLIP: Helper = Helper {
    program: "xclip",
    args: &["-selection", "clipboard"],
    encoding: Encoding::Utf8,
};
const XSEL: Helper = Helper {
    program: "xsel",
    args: &["--clipboard", "--input"],
    encoding: Encoding::Utf8,
};
const CLIP_EXE: Helper = Helper {
    program: "clip.exe",
    args: &[],
    encoding: Encoding::Utf16LeBom,
};
/// WSL with `clip.exe` missing from PATH (Windows PATH appending turned
/// off): PowerShell, told to read stdin as UTF-8, sets the clipboard.
const POWERSHELL: Helper = Helper {
    program: "powershell.exe",
    args: &[
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "[Console]::InputEncoding = [Text.Encoding]::UTF8; Set-Clipboard -Value ([Console]::In.ReadToEnd())",
    ],
    encoding: Encoding::Utf8,
};

/// Pick the clipboard helper, given the session and which programs exist.
///
/// Ordered by how specific the signal is: a Wayland session that also exports
/// `DISPLAY` (XWayland) should still get `wl-copy`, and Termux (which may run
/// an X server) gets its own bridge to the Android clipboard.
pub fn choose_helper(env: HelperEnv, available: &dyn Fn(&str) -> bool) -> Option<Helper> {
    let candidates = [
        (PBCOPY, env.macos),
        (TERMUX, env.termux),
        (WL_COPY, env.wayland),
        (XCLIP, env.x11),
        (XSEL, env.x11),
        (CLIP_EXE, true),
        (POWERSHELL, env.wsl),
    ];
    candidates
        .into_iter()
        .find(|(h, applicable)| *applicable && available(h.program))
        .map(|(h, _)| h)
}

/// The clipboard helper to use on this machine.
pub fn native_helper() -> Option<Helper> {
    choose_helper(HelperEnv::detect(), &on_path)
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
    let Some(helper) = native_helper() else {
        return false;
    };
    let Ok(mut child) = Command::new(helper.program)
        .args(helper.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let wrote = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(&helper.input_bytes(text)).is_ok(),
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
    fn osc52_under_screen_is_chunked_into_dcs_strings() {
        let text = "x".repeat(300);
        let wrapped = osc52_payload_wrapped(&text, Wrap::Screen).unwrap();
        let inner = osc52_payload_wrapped(&text, Wrap::None).unwrap();
        // Every piece is `ESC P … ESC \` and short enough for screen.
        let pieces: Vec<&str> = wrapped.split("\x1b\\").filter(|p| !p.is_empty()).collect();
        assert!(pieces.len() > 1);
        let mut rebuilt = String::new();
        for p in &pieces {
            let body = p.strip_prefix("\x1bP").expect("DCS introducer");
            assert!(body.len() <= SCREEN_DCS_CHUNK);
            rebuilt.push_str(body);
        }
        // Screen strips the envelopes; what reaches the terminal is the
        // original sequence.
        assert_eq!(rebuilt, inner);
    }

    #[test]
    fn wrap_follows_the_multiplexer() {
        assert_eq!(Wrap::from_env(true, false, "tmux-256color"), Wrap::Tmux);
        assert_eq!(
            Wrap::from_env(false, true, "screen.xterm-256color"),
            Wrap::Screen
        );
        assert_eq!(Wrap::from_env(false, true, "screen"), Wrap::Screen);
        // tmux inside screen: tmux is the layer ink talks to.
        assert_eq!(Wrap::from_env(true, true, "screen"), Wrap::Tmux);
        // STY leaked into a shell that is not under screen any more.
        assert_eq!(Wrap::from_env(false, true, "xterm-256color"), Wrap::None);
        assert_eq!(Wrap::from_env(false, false, "screen"), Wrap::None);
    }

    #[test]
    fn utf16le_has_a_bom_and_surrogate_pairs() {
        // é (U+00E9), 世 (U+4E16), 😀 (U+1F600 → D83D DE00).
        assert_eq!(
            utf16le_with_bom("é世😀"),
            vec![0xFF, 0xFE, 0xE9, 0x00, 0x16, 0x4E, 0x3D, 0xD8, 0x00, 0xDE]
        );
        assert_eq!(utf16le_with_bom(""), vec![0xFF, 0xFE]);
    }

    fn only(programs: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |p| programs.contains(&p)
    }

    fn pick(env: HelperEnv, programs: &'static [&'static str]) -> Option<&'static str> {
        choose_helper(env, &only(programs)).map(|h| h.program)
    }

    #[test]
    fn helper_choice_per_platform() {
        let mac = HelperEnv {
            macos: true,
            ..Default::default()
        };
        assert_eq!(pick(mac, &["pbcopy"]), Some("pbcopy"));
        // XWayland exports DISPLAY too; wl-copy still wins.
        let wayland = HelperEnv {
            wayland: true,
            x11: true,
            ..Default::default()
        };
        assert_eq!(pick(wayland, &["wl-copy", "xclip"]), Some("wl-copy"));
        assert_eq!(pick(wayland, &["xsel"]), Some("xsel"));
        // A helper that is installed but whose session is absent is skipped.
        assert_eq!(pick(HelperEnv::default(), &["xclip", "wl-copy"]), None);
        let termux = HelperEnv {
            termux: true,
            ..Default::default()
        };
        assert_eq!(
            pick(termux, &["termux-clipboard-set", "xclip"]),
            Some("termux-clipboard-set")
        );
        // Windows and WSL: clip.exe, with PowerShell only as WSL's fallback.
        let wsl = HelperEnv {
            wsl: true,
            ..Default::default()
        };
        assert_eq!(pick(wsl, &["clip.exe", "powershell.exe"]), Some("clip.exe"));
        assert_eq!(pick(wsl, &["powershell.exe"]), Some("powershell.exe"));
        assert_eq!(pick(HelperEnv::default(), &["powershell.exe"]), None);
        assert_eq!(pick(HelperEnv::default(), &["clip.exe"]), Some("clip.exe"));
    }

    #[test]
    fn helper_input_bytes_match_the_helper() {
        let wsl = HelperEnv {
            wsl: true,
            ..Default::default()
        };
        let clip = choose_helper(wsl, &only(&["clip.exe"])).unwrap();
        assert_eq!(
            clip.input_bytes("é世😀"),
            vec![0xFF, 0xFE, 0xE9, 0x00, 0x16, 0x4E, 0x3D, 0xD8, 0x00, 0xDE]
        );
        let ps = choose_helper(wsl, &only(&["powershell.exe"])).unwrap();
        assert_eq!(ps.input_bytes("é"), "é".as_bytes());
        let termux = HelperEnv {
            termux: true,
            ..Default::default()
        };
        let t = choose_helper(termux, &only(&["termux-clipboard-set"])).unwrap();
        assert_eq!(t.input_bytes("世"), "世".as_bytes());
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
