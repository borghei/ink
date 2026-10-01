use assert_cmd::Command;
use predicates::prelude::*;

fn ink() -> Command {
    Command::cargo_bin("ink").unwrap()
}

#[test]
fn version_prints() {
    ink()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("ink"));
}

#[test]
fn plain_renders_fixture() {
    ink()
        .args(["--plain", "--theme", "dark", "--width", "80"])
        .arg("tests/fixtures/test.md")
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn plain_renders_demo_fixture() {
    ink()
        .args(["--plain", "--theme", "dark", "--width", "80"])
        .arg("tests/fixtures/demo.md")
        .assert()
        .success();
}

#[test]
fn outline_prints_headings() {
    ink()
        .args(["outline", "tests/fixtures/test.md"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn stats_prints() {
    ink()
        .args(["stats", "tests/fixtures/test.md"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Words").or(predicate::str::contains("words")));
}

#[test]
fn missing_file_fails() {
    ink()
        .args(["--plain", "no-such-file-anywhere.md"])
        .assert()
        .failure();
}

#[test]
fn stdin_is_rendered() {
    ink()
        .args(["--plain", "--theme", "dark", "--width", "80"])
        .write_stdin("# Hello from stdin\n\nBody text.\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello from stdin"));
}

#[test]
fn keybindings_lists_actions() {
    ink()
        .arg("keybindings")
        .assert()
        .success()
        .stdout(predicate::str::contains("toggle_toc"));
}

#[test]
fn list_themes_includes_builtins() {
    ink()
        .arg("--list-themes")
        .assert()
        .success()
        .stdout(predicate::str::contains("dracula").and(predicate::str::contains("nord")));
}

#[test]
fn completions_bash_generates_script() {
    ink()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_ink"));
}

#[test]
fn completions_zsh_and_fish() {
    for shell in ["zsh", "fish"] {
        ink()
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::is_empty().not());
    }
}

#[test]
fn man_page_renders_troff() {
    ink()
        .arg("man")
        .assert()
        .success()
        .stdout(predicate::str::contains(".TH").and(predicate::str::contains("ink")));
}

#[test]
fn no_color_strips_ansi_color() {
    // NO_COLOR must drop SGR color codes from --plain output.
    ink()
        .env("NO_COLOR", "1")
        .args(["--plain", "--theme", "dark", "--width", "80"])
        .arg("tests/fixtures/test.md")
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b[38;2;").not());
}

#[test]
fn unknown_theme_warns_on_stderr_and_still_renders() {
    // A mistyped or broken theme falls back to dark, but must say so on
    // stderr — silently ignoring the request left users debugging nothing.
    ink()
        .args([
            "--plain",
            "--theme",
            "definitely-not-a-real-theme",
            "--width",
            "80",
        ])
        .arg("tests/fixtures/test.md")
        .assert()
        .success()
        .stderr(
            predicate::str::contains("definitely-not-a-real-theme")
                .and(predicate::str::contains("falling back to 'dark'")),
        )
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn known_theme_is_quiet() {
    // The fallback warning must not fire for a valid builtin.
    ink()
        .args(["--plain", "--theme", "nord", "--width", "80"])
        .arg("tests/fixtures/test.md")
        .assert()
        .success()
        .stderr(predicate::str::contains("falling back").not());
}

#[test]
fn doctor_reports_without_a_tty() {
    // In a test harness stdout is a pipe: the protocol query must be skipped
    // gracefully while the env report and decoder self-tests still run.
    let output = assert_cmd::Command::cargo_bin("ink")
        .unwrap()
        .arg("doctor")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("[environment]"), "env section: {text}");
    assert!(text.contains("skipped"), "query must be skipped off-tty");
    assert!(text.contains("svg rasterize        = OK"), "svg self-test");
    assert!(text.contains("png decode           = OK"), "png self-test");
}

#[test]
fn doctor_save_writes_report_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report.txt");
    assert_cmd::Command::cargo_bin("ink")
        .unwrap()
        .args(["doctor", "--save"])
        .arg(&path)
        .assert()
        .success();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("ink doctor — image rendering diagnostics"));
}

#[test]
fn keybindings_lists_copy_actions() {
    ink().arg("keybindings").assert().success().stdout(
        predicate::str::contains("select_mode")
            .and(predicate::str::contains("select_line_mode"))
            .and(predicate::str::contains("copy_code"))
            .and(predicate::str::contains("copy_section")),
    );
}

#[test]
fn config_path_honors_xdg_config_home() {
    let dir = tempfile::tempdir().unwrap();
    // The directory must exist: an existing platform config dir is kept in
    // use until `$XDG_CONFIG_HOME/ink` is created.
    std::fs::create_dir_all(dir.path().join("ink")).unwrap();
    let expected = dir.path().join("ink").join("config.toml");
    let out = ink()
        .args(["config", "path"])
        .env("XDG_CONFIG_HOME", dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.trim_end().ends_with(&expected.display().to_string()),
        "expected {} in: {text}",
        expected.display()
    );
}

#[test]
fn config_path_without_xdg_config_home_uses_platform_default() {
    let dir = tempfile::tempdir().unwrap();
    let out = ink()
        .args(["config", "path"])
        .env_remove("XDG_CONFIG_HOME")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(
        !text.contains(&dir.path().display().to_string()),
        "unexpected temp path in: {text}"
    );
    assert!(text.trim_end().ends_with("config.toml"), "got: {text}");
}

#[test]
fn list_themes_finds_user_themes_under_xdg_config_home() {
    let dir = tempfile::tempdir().unwrap();
    let themes = dir.path().join("ink").join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(themes.join("xdg-probe.toml"), "").unwrap();
    ink()
        .arg("--list-themes")
        .env("XDG_CONFIG_HOME", dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("xdg-probe"));
}

/// Remove SGR and OSC 8 sequences so tests can measure visible text.
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC: runs to ST (ESC \) or BEL.
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// A temp `$XDG_CONFIG_HOME` holding `ink/config.toml` with `body`.
fn xdg_with_config(body: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("ink")).unwrap();
    std::fs::write(dir.path().join("ink").join("config.toml"), body).unwrap();
    dir
}

const LONG_PARAGRAPH: &str = "word word word word word word word word word word word word \
word word word word word word word word word word word word word word word word\n";

#[test]
fn bad_flag_values_are_rejected_with_the_flag_named() {
    for (flag, value) in [
        ("--width", "abc"),
        ("--width", "-5"),
        ("--width", "99999999"),
        ("--width", "0"),
        ("--spacing", "bogus"),
        ("--image-protocol", "bogus"),
    ] {
        ink()
            .arg(format!("{flag}={value}"))
            .args(["--plain", "tests/fixtures/test.md"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains(flag).and(predicate::str::contains(value)));
    }
}

#[test]
fn documented_flag_values_still_work() {
    for args in [
        ["--width", "narrow"],
        ["--width", "wide"],
        ["--width", "full"],
        ["--width", "80"],
        ["--spacing", "compact"],
        ["--spacing", "relaxed"],
        ["--image-protocol", "halfblocks"],
        ["--image-protocol", "half-blocks"],
        ["--image-protocol", "Kitty"],
    ] {
        ink()
            .args(args)
            .args(["--plain", "tests/fixtures/test.md"])
            .assert()
            .success();
    }
}

#[test]
fn config_syntax_error_warns_and_ink_still_runs() {
    let dir = xdg_with_config("theme = \"nord\"\nwidth = \n");
    ink()
        .env("XDG_CONFIG_HOME", dir.path())
        .arg("--plain")
        .write_stdin("# Still renders\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Still renders"))
        .stderr(
            predicate::str::contains("ink: config error in")
                .and(predicate::str::contains("config.toml: line 2"))
                .and(predicate::str::contains("using defaults")),
        );
}

#[test]
fn config_unknown_key_warns_and_valid_keys_still_apply() {
    let dir = xdg_with_config("widht = 90\nwidth = 30\n[behavior]\nmouse_capure = false\n");
    let out = ink()
        .env("XDG_CONFIG_HOME", dir.path())
        .arg("--plain")
        .write_stdin(LONG_PARAGRAPH)
        .assert()
        .success()
        .stderr(
            predicate::str::contains("unknown config key `widht`")
                .and(predicate::str::contains("`behavior.mouse_capure`")),
        )
        .get_output()
        .stdout
        .clone();
    let text = strip_ansi(&String::from_utf8(out).unwrap());
    let widest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    assert!(
        (10..=30).contains(&widest),
        "config width = 30 must apply, widest line {widest}: {text}"
    );
}

#[test]
fn config_bad_value_warns_and_falls_back() {
    let dir = xdg_with_config("width = \"wide\"\nspacing = \"bogus\"\n");
    ink()
        .env("XDG_CONFIG_HOME", dir.path())
        .arg("--plain")
        .write_stdin("# ok\n")
        .assert()
        .success()
        .stderr(predicate::str::contains("`width`").and(predicate::str::contains("`spacing`")));
}

/// `ink` with the color-related environment cleared, so the host shell's
/// NO_COLOR / FORCE_COLOR cannot change what these tests see.
fn ink_clean_env() -> Command {
    let mut cmd = ink();
    for var in ["NO_COLOR", "CLICOLOR_FORCE", "FORCE_COLOR", "TERM"] {
        cmd.env_remove(var);
    }
    cmd
}

fn stdout_of(cmd: &mut Command) -> String {
    let out = cmd.assert().success().get_output().stdout.clone();
    String::from_utf8(out).unwrap()
}

// Regression: piped --plain output carried truecolor SGR and OSC 8 links,
// so `git` textconv and redirects were full of escapes.
#[test]
fn piped_plain_output_has_no_escapes_by_default() {
    let out = stdout_of(ink_clean_env().args(["--plain", "tests/fixtures/test.md"]));
    assert!(!out.is_empty());
    assert!(!out.contains('\x1b'), "escape in piped output");
}

#[test]
fn color_always_forces_escapes_into_a_pipe() {
    let out =
        stdout_of(ink_clean_env().args(["--plain", "--color=always", "tests/fixtures/test.md"]));
    assert!(out.contains("\x1b["), "expected SGR codes");
}

#[test]
fn color_flag_beats_no_color() {
    let out = stdout_of(ink_clean_env().env("NO_COLOR", "1").args([
        "--plain",
        "--color=always",
        "tests/fixtures/test.md",
    ]));
    assert!(
        out.contains("\x1b[38;"),
        "explicit --color=always must color"
    );
}

#[test]
fn color_never_and_term_dumb_emit_nothing() {
    let never = stdout_of(ink_clean_env().env("FORCE_COLOR", "1").args([
        "--plain",
        "--color=never",
        "tests/fixtures/test.md",
    ]));
    assert!(!never.contains('\x1b'));
    let dumb = stdout_of(
        ink_clean_env()
            .env("TERM", "dumb")
            .args(["--plain", "tests/fixtures/test.md"]),
    );
    assert!(!dumb.contains('\x1b'));
    // NO_COLOR now drops attributes (bold/underline) too, not just color.
    let no_color = stdout_of(
        ink_clean_env()
            .env("NO_COLOR", "1")
            .args(["--plain", "tests/fixtures/test.md"]),
    );
    assert!(!no_color.contains('\x1b'));
}

#[test]
fn force_color_env_colors_a_pipe() {
    let out = stdout_of(
        ink_clean_env()
            .env("CLICOLOR_FORCE", "1")
            .args(["--plain", "tests/fixtures/test.md"]),
    );
    assert!(out.contains("\x1b["));
}

#[test]
fn bad_color_value_is_rejected() {
    ink()
        .args(["--color=sometimes", "--plain", "tests/fixtures/test.md"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--color").and(predicate::str::contains("always")));
}

// Regression: without --plain, a piped stdout made the interactive reader
// fail with "Device not configured (os error 6)".
#[test]
fn piped_stdout_without_plain_renders_like_plain() {
    let out = stdout_of(ink_clean_env().arg("tests/fixtures/test.md"));
    let plain = stdout_of(ink_clean_env().args(["--plain", "tests/fixtures/test.md"]));
    assert!(!out.is_empty());
    assert_eq!(out, plain);
}

#[test]
fn dash_reads_stdin() {
    let out = stdout_of(
        ink_clean_env()
            .args(["--plain", "-"])
            .write_stdin("# From dash stdin\n"),
    );
    assert!(out.contains("From dash stdin"), "{out}");
    // Also without --plain (piped stdout auto-selects plain).
    let out = stdout_of(ink_clean_env().arg("-").write_stdin("# Auto plain dash\n"));
    assert!(out.contains("Auto plain dash"), "{out}");
}

#[test]
fn diff_is_escape_free_in_a_pipe_and_colored_on_request() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.md"), dir.path().join("b.md"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    let plain = stdout_of(ink_clean_env().arg("diff").arg(&a).arg(&b));
    assert!(
        plain.contains("- one") && plain.contains("+ two"),
        "{plain}"
    );
    assert!(!plain.contains('\x1b'));
    let colored = stdout_of(
        ink_clean_env()
            .args(["diff", "--color=always"])
            .arg(&a)
            .arg(&b),
    );
    assert!(colored.contains("\x1b[31m- one"), "{colored}");
}

#[test]
fn valid_config_is_quiet() {
    let dir = xdg_with_config("width = 60\n[behavior]\nmouse_capture = false\n");
    ink()
        .env("XDG_CONFIG_HOME", dir.path())
        .arg("--plain")
        .write_stdin("# ok\n")
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}
