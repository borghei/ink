//! Light/dark background detection for `theme = "auto"`.
//!
//! Precedence (an explicit `--theme` or config theme never reaches this
//! module):
//!
//! 1. the terminal's own answer to an OSC 11 background-colour query;
//! 2. `COLORFGBG` (set by rxvt, Konsole and a few others);
//! 3. dark.
//!
//! The query runs once, before the TUI owns the terminal, through
//! [`init_background`]; every later [`is_dark_background`] reads the cached
//! answer (the theme is re-resolved on every frame).

use std::sync::OnceLock;
use std::time::Duration;

/// Where the light/dark decision came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundSource {
    /// The terminal answered OSC 11 with this colour.
    Osc11 { rgb: (u8, u8, u8), raw: String },
    /// `COLORFGBG` named this palette index as the background.
    Colorfgbg(String),
    /// Nothing to go on.
    Default,
}

/// The detected background, plus why the OSC 11 query did or did not answer
/// (for `ink doctor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Background {
    pub dark: bool,
    pub source: BackgroundSource,
    /// What happened to the OSC 11 query: "answered", "no reply", or why it
    /// was skipped.
    pub query: String,
}

static BACKGROUND: OnceLock<Background> = OnceLock::new();

/// Detect the background once, querying the terminal only when `allow_query`
/// is true (the caller has checked that stdin and stdout are terminals).
/// Later calls return the first result.
pub fn init_background(allow_query: bool) -> &'static Background {
    BACKGROUND.get_or_init(|| detect(allow_query))
}

/// The cached background, detecting it without a terminal query if nothing
/// has run [`init_background`] yet (tests, `--plain` into a pipe).
pub fn background() -> &'static Background {
    init_background(false)
}

/// Is the terminal background dark?
pub fn is_dark_background() -> bool {
    background().dark
}

fn detect(allow_query: bool) -> Background {
    let query = if allow_query {
        match query_skip_reason() {
            Some(reason) => Err(reason.to_string()),
            None => query_background(),
        }
    } else {
        Err("skipped (not a terminal)".to_string())
    };
    let query_note = match &query {
        Ok(_) => "answered".to_string(),
        Err(why) => why.clone(),
    };
    if let Ok(reply) = &query {
        if let Some(rgb) = parse_osc11_reply(reply) {
            return Background {
                dark: !is_light(rgb),
                source: BackgroundSource::Osc11 {
                    rgb,
                    raw: osc11_payload(reply).unwrap_or_default(),
                },
                query: query_note,
            };
        }
    }
    let query_note = match &query {
        Ok(reply) if osc11_payload(reply).is_none() => {
            "not supported (terminal answered only DA1)".to_string()
        }
        Ok(_) => "unparseable reply".to_string(),
        Err(_) => query_note,
    };
    if let Ok(v) = std::env::var("COLORFGBG") {
        if let Some(dark) = colorfgbg_is_dark(&v) {
            return Background {
                dark,
                source: BackgroundSource::Colorfgbg(v),
                query: query_note,
            };
        }
    }
    Background {
        dark: true,
        source: BackgroundSource::Default,
        query: query_note,
    }
}

/// Environment-based reasons not to ask the terminal at all.
fn query_skip_reason() -> Option<&'static str> {
    let term = std::env::var("TERM").unwrap_or_default();
    skip_reason_for(
        &term,
        std::env::var_os("WT_SESSION").is_some(),
        cfg!(windows),
    )
}

/// Pure form of [`query_skip_reason`].
fn skip_reason_for(term: &str, wt_session: bool, windows: bool) -> Option<&'static str> {
    if term == "dumb" || term == "linux" {
        return Some("skipped (TERM cannot answer)");
    }
    // Legacy conhost ignores OSC 11 and would make us wait out the timeout;
    // Windows Terminal answers it.
    if windows && !wt_session {
        return Some("skipped (not Windows Terminal)");
    }
    None
}

/// How long to wait for a terminal that answers neither query. Responsive
/// terminals end the wait early through the DA1 reply, so this only costs
/// time on the rare terminal that answers nothing — but across SSH the round
/// trip itself can take longer.
fn query_timeout() -> Duration {
    let ssh = std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
    if ssh {
        Duration::from_millis(400)
    } else {
        Duration::from_millis(100)
    }
}

/// The background query, followed by a primary device attributes request
/// (DA1). Nearly every terminal answers DA1, and answers in order, so the
/// DA1 reply marks the point where an OSC 11 answer would already have come.
const QUERY: &str = "\x1b]11;?\x07\x1b[c";

/// Ask the terminal for its background colour; returns everything it sent
/// back (the OSC 11 reply, if any, and the DA1 reply).
#[cfg(unix)]
fn query_background() -> Result<String, String> {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::time::Instant;

    let stdin = std::io::stdin();
    let fd = stdin.as_raw_fd();

    // Raw mode (no echo, no line buffering) for the duration of the query, so
    // the reply is neither printed nor held back until Enter. The guard puts
    // the original mode back on every path out of this function.
    struct Restore {
        fd: i32,
        orig: libc::termios,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: restoring attributes previously read from the same fd.
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSANOW, &self.orig);
            }
        }
    }
    // SAFETY: termios is plain data; tcgetattr fills it or fails.
    let mut orig: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut orig) } != 0 {
        return Err("skipped (cannot read terminal mode)".into());
    }
    let mut raw = orig;
    // SAFETY: cfmakeraw only edits the struct it is given.
    unsafe { libc::cfmakeraw(&mut raw) };
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
        return Err("skipped (cannot set raw mode)".into());
    }
    let _restore = Restore { fd, orig };

    let mut out = std::io::stdout();
    if out
        .write_all(QUERY.as_bytes())
        .and_then(|_| out.flush())
        .is_err()
    {
        return Err("skipped (cannot write to terminal)".into());
    }

    let deadline = Instant::now() + query_timeout();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd, bounded timeout.
        let n = unsafe { libc::poll(&mut pfd, 1, left.as_millis().max(1) as libc::c_int) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if n == 0 {
            break;
        }
        // SAFETY: reading into a stack buffer of the stated length.
        let got = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if got <= 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..got as usize]);
        if contains_da1_reply(&buf) {
            break;
        }
    }
    if buf.is_empty() {
        return Err("no reply".into());
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Windows Terminal: the reply arrives as console input, which crossterm
/// surfaces as key presses. Reassemble them into the byte stream.
#[cfg(windows)]
fn query_background() -> Result<String, String> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use std::io::Write;
    use std::time::Instant;

    if crossterm::terminal::enable_raw_mode().is_err() {
        return Err("skipped (cannot set raw mode)".into());
    }
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    let _restore = Restore;

    let mut out = std::io::stdout();
    if out
        .write_all(QUERY.as_bytes())
        .and_then(|_| out.flush())
        .is_err()
    {
        return Err("skipped (cannot write to terminal)".into());
    }
    let deadline = Instant::now() + query_timeout();
    let mut text = String::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !event::poll(left).unwrap_or(false) {
            break;
        }
        let Ok(Event::Key(k)) = event::read() else {
            continue;
        };
        if k.kind == KeyEventKind::Release {
            continue;
        }
        match k.code {
            KeyCode::Esc => text.push('\x1b'),
            KeyCode::Char('g') if k.modifiers.contains(KeyModifiers::CONTROL) => text.push('\x07'),
            KeyCode::Char(c) => text.push(c),
            _ => {}
        }
        if contains_da1_reply(text.as_bytes()) {
            break;
        }
    }
    if text.is_empty() {
        return Err("no reply".into());
    }
    Ok(text)
}

#[cfg(not(any(unix, windows)))]
fn query_background() -> Result<String, String> {
    Err("skipped (unsupported platform)".into())
}

/// Does `buf` hold a complete DA1 reply (`ESC [ ? … c`)?
fn contains_da1_reply(buf: &[u8]) -> bool {
    let mut i = 0;
    while i + 2 < buf.len() {
        if buf[i] == 0x1b && buf[i + 1] == b'[' && buf[i + 2] == b'?' {
            let rest = &buf[i + 3..];
            if let Some(end) = rest
                .iter()
                .position(|b| !(b.is_ascii_digit() || *b == b';'))
            {
                if rest[end] == b'c' {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// The colour spec inside an OSC 11 reply (`rgb:…` or `#…`), without the
/// introducer or terminator. `None` when there is no complete reply.
fn osc11_payload(reply: &str) -> Option<String> {
    let start = reply.find("\x1b]11;")? + "\x1b]11;".len();
    let rest = &reply[start..];
    // BEL or ST (ESC \) ends it; a reply cut off before either is truncated.
    let end = rest.find(['\x07', '\x1b'])?;
    Some(rest[..end].to_string())
}

/// Parse a terminal's OSC 11 reply into 8-bit RGB. Accepts the X11 forms
/// terminals actually send: `rgb:R/G/B` with 1–4 hex digits per channel (and
/// `rgba:R/G/B/A`), and `#RGB`, `#RRGGBB`, `#RRRGGGBBB`, `#RRRRGGGGBBBB`.
pub fn parse_osc11_reply(reply: &str) -> Option<(u8, u8, u8)> {
    parse_color_spec(&osc11_payload(reply)?)
}

/// Parse an X11 colour spec (see [`parse_osc11_reply`]).
pub fn parse_color_spec(spec: &str) -> Option<(u8, u8, u8)> {
    let spec = spec.trim();
    if let Some(body) = spec
        .strip_prefix("rgb:")
        .or_else(|| spec.strip_prefix("rgba:"))
    {
        let mut parts = body.split('/');
        let r = scale_hex(parts.next()?)?;
        let g = scale_hex(parts.next()?)?;
        let b = scale_hex(parts.next()?)?;
        return Some((r, g, b));
    }
    if let Some(hex) = spec.strip_prefix('#') {
        if hex.is_empty() || hex.len() % 3 != 0 || hex.len() > 12 {
            return None;
        }
        let n = hex.len() / 3;
        return Some((
            scale_hex(&hex[..n])?,
            scale_hex(&hex[n..2 * n])?,
            scale_hex(&hex[2 * n..])?,
        ));
    }
    None
}

/// Scale a 1–4 digit hex channel to 0..=255 (`f` → 255, `ffff` → 255,
/// `80` → 128).
fn scale_hex(digits: &str) -> Option<u8> {
    if digits.is_empty() || digits.len() > 4 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(digits, 16).ok()?;
    let max = (1u32 << (4 * digits.len())) - 1;
    Some(((v * 255 + max / 2) / max) as u8)
}

/// Perceived luminance (Rec. 709 weights on the encoded values), 0.0–1.0.
pub fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    (0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64) / 255.0
}

/// A background is light when its luminance is above one half.
pub fn is_light(rgb: (u8, u8, u8)) -> bool {
    luminance(rgb) > 0.5
}

/// `COLORFGBG` is `fg;bg` (sometimes `fg;default;bg`); the last field is the
/// background palette index. Indices 7 and 9–15 are the light ones.
fn colorfgbg_is_dark(value: &str) -> Option<bool> {
    let bg = value.split(';').next_back()?.trim().parse::<u8>().ok()?;
    Some(!matches!(bg, 7 | 9..=15))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rgb_replies_with_either_terminator() {
        let white = "\x1b]11;rgb:ffff/ffff/ffff\x07";
        assert_eq!(parse_osc11_reply(white), Some((255, 255, 255)));
        let st = "\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\";
        assert_eq!(parse_osc11_reply(st), Some((30, 30, 46)));
        // Followed by the DA1 reply, as the real query produces.
        let both = "\x1b]11;rgb:0000/0000/0000\x07\x1b[?62;22c";
        assert_eq!(parse_osc11_reply(both), Some((0, 0, 0)));
    }

    #[test]
    fn rgb_channels_of_one_to_four_digits_scale_to_8_bit() {
        assert_eq!(parse_color_spec("rgb:f/0/8"), Some((255, 0, 136)));
        assert_eq!(parse_color_spec("rgb:ff/00/80"), Some((255, 0, 128)));
        assert_eq!(parse_color_spec("rgb:fff/000/800"), Some((255, 0, 128)));
        assert_eq!(parse_color_spec("rgb:ffff/0000/8000"), Some((255, 0, 128)));
        assert_eq!(
            parse_color_spec("rgba:ffff/ffff/ffff/ffff"),
            Some((255, 255, 255))
        );
    }

    #[test]
    fn parses_hash_forms() {
        assert_eq!(parse_color_spec("#ffffff"), Some((255, 255, 255)));
        assert_eq!(parse_color_spec("#1E1E2E"), Some((30, 30, 46)));
        assert_eq!(parse_color_spec("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_color_spec("#ffff00000000"), Some((255, 0, 0)));
        assert_eq!(
            parse_osc11_reply("\x1b]11;#fdf6e3\x07"),
            Some((253, 246, 227))
        );
    }

    #[test]
    fn garbage_and_truncated_replies_are_rejected() {
        for bad in [
            "",
            "\x1b[?62;c",                     // DA1 only: terminal ignored OSC 11
            "\x1b]11;rgb:ffff/ffff",          // truncated, no terminator
            "\x1b]11;rgb:ffff/ffff\x07",      // two channels
            "\x1b]11;rgb:fffff/0/0\x07",      // five digits
            "\x1b]11;rgb:zz/00/00\x07",       // not hex
            "\x1b]11;#ffff\x07",              // not a multiple of three
            "\x1b]11;?\x07",                  // our own query echoed back
            "\x1b]11;white\x07",              // named colour
            "\x1b]10;rgb:ffff/ffff/ffff\x07", // foreground, not background
        ] {
            assert_eq!(parse_osc11_reply(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn luminance_threshold() {
        assert!(is_light((255, 255, 255)));
        assert!(is_light((253, 246, 227)), "solarized light");
        assert!(is_light((200, 200, 200)));
        assert!(!is_light((0, 0, 0)));
        assert!(!is_light((30, 30, 46)), "catppuccin mocha");
        assert!(!is_light((0, 43, 54)), "solarized dark");
        // Mid grey sits on the line and stays dark (strictly greater).
        assert!(!is_light((127, 127, 127)));
        assert!(is_light((129, 129, 129)));
        // Pure blue is dark, pure yellow is light.
        assert!(!is_light((0, 0, 255)));
        assert!(is_light((255, 255, 0)));
    }

    #[test]
    fn da1_reply_detection() {
        assert!(contains_da1_reply(b"\x1b[?62;22c"));
        assert!(contains_da1_reply(b"\x1b]11;rgb:0/0/0\x07\x1b[?1;2c"));
        assert!(!contains_da1_reply(b"\x1b[?62;22"));
        assert!(!contains_da1_reply(b"\x1b]11;rgb:0/0/0\x07"));
    }

    #[test]
    fn colorfgbg_light_and_dark_indices() {
        assert_eq!(colorfgbg_is_dark("15;0"), Some(true));
        assert_eq!(colorfgbg_is_dark("0;15"), Some(false));
        assert_eq!(colorfgbg_is_dark("0;7"), Some(false));
        assert_eq!(colorfgbg_is_dark("15;default;0"), Some(true));
        assert_eq!(colorfgbg_is_dark("15;default"), None);
    }

    #[test]
    fn query_skipped_where_it_cannot_be_answered() {
        assert!(skip_reason_for("linux", false, false).is_some());
        assert!(skip_reason_for("dumb", false, false).is_some());
        assert!(skip_reason_for("xterm-256color", false, false).is_none());
        assert!(skip_reason_for("", false, true).is_some(), "conhost");
        assert!(
            skip_reason_for("", true, true).is_none(),
            "Windows Terminal"
        );
    }
}
