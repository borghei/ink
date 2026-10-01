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
