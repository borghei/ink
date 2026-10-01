//! End-to-end checks that need a real terminal: ink runs on the slave side
//! of a pseudo-terminal while the test plays the terminal on the master side
//! (answering queries, typing keys, and recording every byte ink writes).
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A scripted terminal: `respond` sees everything ink has written so far and
/// returns bytes to "type" back (query answers or keys), or nothing.
struct Session {
    output: Vec<u8>,
}

fn run_in_pty(
    args: &[&str],
    envs: &[(&str, &str)],
    mut respond: impl FnMut(&[u8], Duration) -> Option<Vec<u8>>,
) -> Session {
    let (mut master, slave) = open_pty();
    let config_home = tempfile::tempdir().unwrap();
    // An `ink/` directory makes ink use this (empty) config instead of the
    // developer's real one.
    std::fs::create_dir(config_home.path().join("ink")).unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ink"));
    cmd.args(args)
        .env("TERM", "xterm-256color")
        .env("XDG_CONFIG_HOME", config_home.path())
        .env_remove("COLORFGBG")
        .env_remove("TMUX")
        .env_remove("STY")
        .env_remove("NO_COLOR")
        .env_remove("COLORTERM")
        .env_remove("TERM_PROGRAM")
        .env_remove("LC_TERMINAL")
        .env_remove("ITERM_SESSION_ID")
        .env_remove("WT_SESSION")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()));
    for (k, v) in envs {
        cmd.env(k, v);
    }
    // SAFETY: only async-signal-safe libc calls between fork and exec. Gives
    // ink a session whose controlling terminal is the pty, like a shell would.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }
    let mut child = cmd.spawn().expect("spawn ink");
    drop(slave);
    drop(cmd);

    let fd = master.as_raw_fd();
    let start = Instant::now();
    let mut output = Vec::new();
    let mut buf = [0u8; 8192];
    while start.elapsed() < Duration::from_secs(15) {
        if let Some(reply) = respond(&output, start.elapsed()) {
            master.write_all(&reply).unwrap();
        }
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let ready = unsafe { libc::poll(&mut pfd, 1, 50) };
        if ready > 0 {
            match master.read(&mut buf) {
                Ok(0) | Err(_) => break, // EOF / EIO: the slave side closed
                Ok(n) => output.extend_from_slice(&buf[..n]),
            }
        } else if let Ok(Some(_)) = child.try_wait() {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Session { output }
}

fn open_pty() -> (std::fs::File, OwnedFd) {
    let (mut m, mut s) = (0, 0);
    let mut size = libc::winsize {
        ws_row: 30,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: out-pointers to two ints and a winsize; null name/termios.
    let rc = unsafe {
        libc::openpty(
            &mut m,
            &mut s,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    assert_eq!(rc, 0, "openpty failed");
    // SAFETY: openpty returned two fresh descriptors we now own.
    unsafe { (std::fs::File::from_raw_fd(m), OwnedFd::from_raw_fd(s)) }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Answers the OSC 11 background query (once) with `reply`, as a terminal
/// whose background is that colour would.
fn answer_osc11(reply: &'static [u8]) -> impl FnMut(&[u8], Duration) -> Option<Vec<u8>> {
    let mut answered = false;
    move |out, _| {
        if !answered && contains(out, b"\x1b]11;?") {
            answered = true;
            Some(reply.to_vec())
        } else {
            None
        }
    }
}

#[test]
fn doctor_reports_a_light_background_from_osc_11() {
    let s = run_in_pty(
        &["doctor"],
        &[],
        answer_osc11(b"\x1b]11;rgb:ffff/ffff/ffff\x07\x1b[?62;22c"),
    );
    let text = String::from_utf8_lossy(&s.output);
    assert!(
        text.contains("auto -> light") && text.contains("via OSC 11"),
        "doctor did not report the light background:\n{text}"
    );
}

#[test]
fn doctor_reports_a_dark_background_from_osc_11() {
    let s = run_in_pty(
        &["doctor"],
        &[],
        answer_osc11(b"\x1b]11;#1e1e2e\x1b\\\x1b[?62;22c"),
    );
    let text = String::from_utf8_lossy(&s.output);
    assert!(text.contains("auto -> dark"), "{text}");
}

#[test]
fn an_explicit_theme_never_queries_the_terminal() {
    let s = run_in_pty(&["--theme", "dracula", "doctor"], &[], |_, _| None);
    assert!(!contains(&s.output, b"\x1b]11;?"));
    assert!(String::from_utf8_lossy(&s.output).contains("dracula (from --theme)"));
}

#[test]
fn the_reader_uses_the_light_theme_and_leaves_no_reply_on_screen() {
    let mut osc = answer_osc11(b"\x1b]11;rgb:ffff/ffff/ffff\x07\x1b[?62;22c");
    let mut last_q = Duration::ZERO;
    let s = run_in_pty(
        &[
            "--image-protocol",
            "halfblocks",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test.md"),
        ],
        &[("COLORTERM", "truecolor")],
        |out, t| {
            if let Some(r) = osc(out, t) {
                return Some(r);
            }
            // Quit once the reader is up; repeat in case a slow start lost it.
            if contains(out, b"\x1b[?1049h") && t > last_q + Duration::from_millis(500) {
                last_q = t;
                return Some(b"q".to_vec());
            }
            None
        },
    );
    // The light theme's white background is painted…
    assert!(contains(&s.output, b"48;2;255;255;255"));
    // …and the reply was consumed, never echoed or drawn.
    assert!(!contains(&s.output, b"rgb:ffff"));
    // The terminal is handed back: alternate screen left.
    let tail =
        String::from_utf8_lossy(&s.output[s.output.len().saturating_sub(400)..]).into_owned();
    assert!(contains(&s.output, b"\x1b[?1049l"), "{tail:?}");
}

#[test]
fn a_reply_that_arrives_after_300_ms_is_still_consumed() {
    // A slow link nobody flags as SSH (docker exec, mosh, serial): the
    // answer comes 300 ms after the query. The query must still read it —
    // the light theme proves it was parsed — so it is never drawn or typed.
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test.md");
    let mut asked_at: Option<Duration> = None;
    let mut answered = false;
    let mut quit = quit_when_up();
    let s = run_in_pty(
        &["--image-protocol", "halfblocks", fixture],
        &[("COLORTERM", "truecolor")],
        |out, t| {
            if asked_at.is_none() && contains(out, b"\x1b]11;?") {
                asked_at = Some(t);
            }
            match asked_at {
                Some(at) if !answered && t >= at + Duration::from_millis(300) => {
                    answered = true;
                    Some(b"\x1b]11;rgb:ffff/ffff/ffff\x07\x1b[?62;22c".to_vec())
                }
                _ => quit(out, t),
            }
        },
    );
    assert!(answered, "ink never asked");
    assert!(contains(&s.output, b"48;2;255;255;255"), "reply not used");
    assert!(!contains(&s.output, b"rgb:ffff"), "reply echoed");
    assert!(contains(&s.output, b"\x1b[?1049l"), "reader did not exit");
}

#[test]
fn list_themes_never_queries_the_terminal() {
    let s = run_in_pty(&["--list-themes"], &[], |_, _| None);
    assert!(String::from_utf8_lossy(&s.output).contains("dracula"));
    assert!(!contains(&s.output, b"\x1b]11;?"), "OSC 11 query written");
    assert!(!contains(&s.output, b"\x1b[c"), "DA1 query written");
}

/// Quits the reader (`q`) once it has entered the alternate screen, retrying
/// in case a slow start swallowed the key.
fn quit_when_up() -> impl FnMut(&[u8], Duration) -> Option<Vec<u8>> {
    let mut last = Duration::ZERO;
    move |out, t| {
        if contains(out, b"\x1b[?1049h") && t > last + Duration::from_millis(500) {
            last = t;
            Some(b"q".to_vec())
        } else {
            None
        }
    }
}

const MOUSE_ON: [&[u8]; 3] = [b"\x1b[?1000h", b"\x1b[?1002h", b"\x1b[?1006h"];

#[test]
fn no_mouse_never_enables_mouse_reporting() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test.md");
    // Control: by default the reader does capture the mouse.
    let s = run_in_pty(
        &["--theme", "dark", "--image-protocol", "halfblocks", fixture],
        &[],
        quit_when_up(),
    );
    assert!(MOUSE_ON.iter().all(|seq| contains(&s.output, seq)));

    let s = run_in_pty(
        &[
            "--no-mouse",
            "--theme",
            "dark",
            "--image-protocol",
            "halfblocks",
            fixture,
        ],
        &[],
        quit_when_up(),
    );
    assert!(contains(&s.output, b"\x1b[?1049l"), "reader did not exit");
    for seq in MOUSE_ON {
        assert!(
            !contains(&s.output, seq),
            "{:?} was written",
            String::from_utf8_lossy(seq)
        );
    }
}

#[test]
fn no_mouse_also_applies_to_the_file_browser() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "# a\n").unwrap();
    let path = dir.path().to_str().unwrap();
    let s = run_in_pty(
        &["--no-mouse", "--theme", "dark", path],
        &[],
        quit_when_up(),
    );
    assert!(contains(&s.output, b"\x1b[?1049l"), "browser did not exit");
    for seq in MOUSE_ON {
        assert!(!contains(&s.output, seq));
    }
}

/// Any extended (256 / 24-bit) colour selection in the output?
/// The parameter lists of every SGR (`CSI … m`) sequence in the output.
fn sgr_params(out: &[u8]) -> Vec<Vec<u16>> {
    let text = String::from_utf8_lossy(out);
    text.split("\x1b[")
        .skip(1)
        .filter_map(|seq| {
            let end = seq.find(|c: char| !(c.is_ascii_digit() || c == ';'))?;
            if !seq[end..].starts_with('m') {
                return None;
            }
            Some(
                seq[..end]
                    .split(';')
                    .filter_map(|p| p.parse().ok())
                    .collect(),
            )
        })
        .collect()
}

/// Any 24-bit colour, or a 256-palette colour beyond the 16 ANSI ones?
/// (crossterm writes even the named colours as `38;5;0`–`38;5;15`, which
/// select the terminal's own ANSI palette entries.)
fn has_extended_colour(out: &[u8]) -> bool {
    sgr_params(out).iter().any(|p| {
        p.windows(3)
            .any(|w| matches!(w[0], 38 | 48) && (w[1] == 2 || (w[1] == 5 && w[2] >= 16)))
    })
}

/// Any SGR foreground/background colour at all (other than the 39/49
/// "default colour" resets)?
fn has_any_colour(out: &[u8]) -> bool {
    sgr_params(out)
        .iter()
        .flatten()
        .any(|p| matches!(p, 30..=38 | 40..=48 | 90..=97 | 100..=107))
}

#[test]
fn colour_never_draws_the_reader_with_attributes_only() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test.md");
    let mut step = 0;
    let s = run_in_pty(
        &[
            "--color=never",
            "--theme",
            "dark",
            "--image-protocol",
            "halfblocks",
            fixture,
        ],
        &[("COLORTERM", "truecolor")],
        |out, t| {
            // Once up: select the current line (V), then leave (Esc, q).
            let up = contains(out, b"\x1b[?1049h");
            let keys: &[u8] = match step {
                0 if up => b"V",
                1 if t > Duration::from_millis(1200) => b"j",
                2 if t > Duration::from_millis(1600) => b"\x1b",
                3 if t > Duration::from_millis(2000) => b"q",
                _ => return None,
            };
            step += 1;
            Some(keys.to_vec())
        },
    );
    assert!(contains(&s.output, b"\x1b[?1049l"), "reader did not exit");
    assert!(!has_any_colour(&s.output), "colour escapes were written");
    // The selection is still visible: reverse video.
    assert!(
        contains(&s.output, b"\x1b[7m"),
        "no reverse-video selection"
    );
}

#[test]
fn a_16_colour_terminal_gets_only_ansi_colours() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test.md");
    let s = run_in_pty(
        &["--theme", "dark", "--image-protocol", "halfblocks", fixture],
        &[("TERM", "xterm")],
        quit_when_up(),
    );
    assert!(
        contains(&s.output, b"\x1b[?1049l"),
        "reader did not exit: {:?}",
        String::from_utf8_lossy(&s.output[s.output.len().saturating_sub(600)..])
    );
    assert!(has_any_colour(&s.output), "expected ANSI colours");
    assert!(
        !has_extended_colour(&s.output),
        "256/24-bit colour on TERM=xterm"
    );
}

/// Time from launch until the reader enters the alternate screen, on a
/// terminal that answers no queries at all.
fn startup_time(envs: &[(&str, &str)]) -> Duration {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test.md");
    let mut up_at = None;
    let mut quit = quit_when_up();
    run_in_pty(&["--theme", "dark", fixture], envs, |out, t| {
        if up_at.is_none() && contains(out, b"\x1b[?1049h") {
            up_at = Some(t);
        }
        quit(out, t)
    });
    up_at.expect("reader never started")
}

#[test]
fn terminals_without_graphics_skip_the_image_query() {
    // Terminal.app never answers the graphics probe; asking would stall
    // startup for the probe's full ~2 s timeout.
    let t = startup_time(&[("TERM_PROGRAM", "Apple_Terminal")]);
    assert!(t < Duration::from_millis(1500), "startup took {t:?}");
    let t = startup_time(&[("TERM", "linux")]);
    assert!(t < Duration::from_millis(1500), "startup took {t:?}");
}

#[test]
fn ascii_mode_reader_and_overlays_write_only_7_bit_bytes() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ascii.md");
    let mut step = 0;
    let s = run_in_pty(
        &[
            "--ascii",
            "--color=never",
            "--theme",
            "dark",
            "--image-protocol",
            "halfblocks",
            fixture,
        ],
        &[],
        |out, t| {
            // Once up: table of contents, help overlay, close help, quit.
            let up = contains(out, b"\x1b[?1049h");
            let keys: &[u8] = match step {
                0 if up => b"t",
                1 if t > Duration::from_millis(1200) => b"?",
                2 if t > Duration::from_millis(1600) => b"x",
                // Close the TOC, then quit (repeated until it takes).
                n if n >= 3 && t > Duration::from_millis(2000 + 400 * (n as u64 - 3)) => {
                    if n == 3 {
                        b"t"
                    } else {
                        b"q"
                    }
                }
                _ => return None,
            };
            step += 1;
            Some(keys.to_vec())
        },
    );
    let text = String::from_utf8_lossy(&s.output);
    assert!(text.contains("Contents") && text.contains("Keys"), "{text}");
    assert!(contains(&s.output, b"\x1b[?1049l"), "reader did not exit");
    assert!(s.output.is_ascii(), "non-ASCII output: {text}");
}

type Marks = std::rc::Rc<std::cell::RefCell<Vec<usize>>>;

/// Once the reader is up, types each step 400 ms apart, then quits (`q`,
/// repeated until it takes). `marks` records how much output there was when
/// each step went out, so a test can tell what was drawn before and after.
fn type_steps(steps: Vec<Vec<u8>>, marks: Marks) -> impl FnMut(&[u8], Duration) -> Option<Vec<u8>> {
    let mut up_at = None;
    let mut sent = 0;
    let mut last_q = Duration::ZERO;
    move |out, t| {
        if up_at.is_none() && contains(out, b"\x1b[?1049h") {
            up_at = Some(t);
        }
        let due = up_at? + Duration::from_millis(700 + 400 * sent as u64);
        if t < due {
            return None;
        }
        if sent < steps.len() {
            sent += 1;
            marks.borrow_mut().push(out.len());
            return Some(steps[sent - 1].clone());
        }
        if t > last_q + Duration::from_millis(500) {
            last_q = t;
            return Some(b"q".to_vec());
        }
        None
    }
}

/// A document whose first line links to a heading far below the first
/// screen. On the 100x30 test terminal the link text "go to target" is drawn
/// on row 2, columns 15-26 (1-based).
fn long_doc_with_anchor_link() -> tempfile::NamedTempFile {
    let mut doc = String::from("Start here: [go to target](#target-heading) then more.\n\n");
    for i in 0..60 {
        doc.push_str(&format!("filler {i}\n\n"));
    }
    doc.push_str("## Target heading\n\nXYZZY ARRIVED\n");
    let file = tempfile::Builder::new().suffix(".md").tempfile().unwrap();
    std::fs::write(file.path(), doc).unwrap();
    file
}

fn reader_args(path: &std::path::Path) -> Vec<String> {
    [
        "--color=never",
        "--theme",
        "dark",
        "--image-protocol",
        "halfblocks",
        "--no-images",
        path.to_str().unwrap(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn the_toc_takes_focus_filters_and_jumps() {
    let doc = long_doc_with_anchor_link();
    let marks = Marks::default();
    let steps = vec![b"o".to_vec(), b"/targ".to_vec(), b"\r".to_vec()];
    let args = reader_args(doc.path());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let s = run_in_pty(&args, &[], type_steps(steps, Marks::clone(&marks)));
    let marks = marks.borrow();
    assert_eq!(marks.len(), 3, "steps not all sent");
    let before = &s.output[..marks[2]];
    let after = &s.output[marks[2]..];
    // Focused: the sidebar opened and its key reminder (the filtering one,
    // after `/`) replaced the status bar.
    let text = String::from_utf8_lossy(before);
    assert!(text.contains("Contents"), "sidebar did not open");
    assert!(text.contains("to filter"), "no TOC key reminder");
    // The cursor is reverse video, which reads without colour.
    assert!(contains(before, b"7m"), "no reverse-video cursor");
    assert!(!contains(before, b"ARRIVED"));
    // Enter jumped to the heading.
    assert!(contains(after, b"ARRIVED"), "did not jump");
    assert!(contains(&s.output, b"\x1b[?1049l"), "reader did not exit");
}

#[test]
fn the_focused_toc_writes_only_7_bit_bytes_in_ascii_mode() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ascii.md");
    let mut args = reader_args(std::path::Path::new(fixture));
    args.insert(0, "--ascii".into());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // Focus, fold the first heading, filter, clear the filter.
    let steps = vec![
        b"o".to_vec(),
        b"h".to_vec(),
        b"/a".to_vec(),
        b"\x1b".to_vec(),
    ];
    let s = run_in_pty(&args, &[], type_steps(steps, Marks::default()));
    let text = String::from_utf8_lossy(&s.output);
    assert!(text.contains("Contents") && text.contains("fold"), "{text}");
    assert!(contains(&s.output, b"\x1b[?1049l"), "reader did not exit");
    assert!(s.output.is_ascii(), "non-ASCII output: {text}");
}

/// An SGR mouse report: `btn` 0 = left press, 32 = left drag; `press`
/// false = release. Columns and rows are 1-based.
fn mouse(btn: u8, col: u16, row: u16, press: bool) -> Vec<u8> {
    let end = if press { 'M' } else { 'm' };
    format!("\x1b[<{btn};{col};{row}{end}").into_bytes()
}

/// A config directory whose clipboard route is OSC 52 only, so a test that
/// copies never touches the machine's real clipboard.
fn osc52_only_config() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("ink")).unwrap();
    std::fs::write(
        dir.path().join("ink/config.toml"),
        "[behavior]\nclipboard = \"osc52\"\n",
    )
    .unwrap();
    dir
}

/// Runs the reader on the long anchor-link document with mouse capture on,
/// sends `gesture` once it is up, and returns the output from before and
/// after it.
fn after_gesture(gesture: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
    let doc = long_doc_with_anchor_link();
    let config = osc52_only_config();
    let marks = Marks::default();
    let args = reader_args(doc.path());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let s = run_in_pty(
        &args,
        &[("XDG_CONFIG_HOME", config.path().to_str().unwrap())],
        type_steps(vec![gesture], Marks::clone(&marks)),
    );
    assert!(contains(&s.output, b"\x1b[?1049l"), "reader did not exit");
    let at = *marks.borrow().first().expect("gesture not sent");
    (s.output[..at].to_vec(), s.output[at..].to_vec())
}

#[test]
fn a_click_on_an_anchor_link_follows_it() {
    // "go to target" is on row 2, columns 15-26.
    let mut click = mouse(0, 18, 2, true);
    click.extend(mouse(0, 18, 2, false));
    let (before, after) = after_gesture(click);
    assert!(!contains(&before, b"ARRIVED"));
    assert!(
        contains(&after, b"ARRIVED"),
        "the click did not follow the link"
    );
    assert!(!contains(&after, b"\x1b]52;"), "a click copied something");
}

#[test]
fn a_drag_across_a_link_selects_and_copies_instead_of_following() {
    let mut drag = mouse(0, 15, 2, true);
    drag.extend(mouse(32, 20, 2, true));
    drag.extend(mouse(32, 26, 2, true));
    drag.extend(mouse(0, 26, 2, false));
    let (_, after) = after_gesture(drag);
    // "go to target" as OSC 52 base64.
    assert!(
        contains(&after, b"\x1b]52;c;Z28gdG8gdGFyZ2V0\x07"),
        "no OSC 52 copy of the selection: {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(!contains(&after, b"ARRIVED"), "the drag followed the link");
}

#[test]
fn a_click_off_any_link_does_nothing() {
    let mut click = mouse(0, 6, 2, true);
    click.extend(mouse(0, 6, 2, false));
    let (_, after) = after_gesture(click);
    assert!(!contains(&after, b"ARRIVED"));
    assert!(!contains(&after, b"\x1b]52;"));
}

/// Index of the first `needle` at or after `from`.
fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

/// Runs the reader on a short document with `editor` as `$EDITOR`, presses
/// `e`, and returns the output from the key press on, plus the document.
fn after_edit(editor: &str) -> (Vec<u8>, String) {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("doc.md");
    std::fs::write(&doc, "# Notes\n\nfirst line\n").unwrap();
    let marks = Marks::default();
    let args = reader_args(&doc);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let s = run_in_pty(
        &args,
        &[("EDITOR", editor), ("VISUAL", "")],
        type_steps(vec![b"e".to_vec()], Marks::clone(&marks)),
    );
    assert!(contains(&s.output, b"\x1b[?1049l"), "reader did not exit");
    let at = *marks.borrow().first().expect("e not sent");
    (
        s.output[at..].to_vec(),
        std::fs::read_to_string(&doc).unwrap(),
    )
}

#[test]
fn e_suspends_for_the_editor_and_reloads_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("fake-editor.sh");
    // Appends a line to its last argument (the file) and records its argv.
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" > '{}'\nfor last; do :; done\nprintf 'EDITEDBYSCRIPT\\n' >> \"$last\"\n",
            dir.path().join("argv").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let (after, doc) = after_edit(script.to_str().unwrap());
    assert!(doc.ends_with("first line\nEDITEDBYSCRIPT\n"), "{doc:?}");
    // An unknown editor gets just the file, no line argument.
    let argv = std::fs::read_to_string(dir.path().join("argv")).unwrap();
    assert!(argv.trim_end().ends_with("doc.md"), "{argv:?}");
    assert!(!argv.contains('+'), "{argv:?}");
    // The alternate screen was left for the editor and entered again…
    let left = find_from(&after, b"\x1b[?1049l", 0).expect("never left the alternate screen");
    let back = find_from(&after, b"\x1b[?1049h", left).expect("never came back");
    // …with the mouse captured again, and the new content drawn.
    assert!(
        find_from(&after, b"\x1b[?1006h", left).is_some(),
        "mouse not re-enabled"
    );
    assert!(
        find_from(&after, b"EDITEDBYSCRIPT", back).is_some(),
        "not reloaded"
    );
    assert!(
        find_from(&after, b"reloaded", back).is_some(),
        "no reload message"
    );
}

#[test]
fn an_editor_that_cannot_start_leaves_the_reader_usable() {
    let (after, doc) = after_edit("/nonexistent/editor --flag");
    assert_eq!(doc, "# Notes\n\nfirst line\n");
    let left = find_from(&after, b"\x1b[?1049l", 0).expect("never left the alternate screen");
    let back = find_from(&after, b"\x1b[?1049h", left).expect("never came back");
    assert!(
        find_from(&after, b"/nonexistent/editor:", back).is_some(),
        "{:?}",
        String::from_utf8_lossy(&after)
    );
}
