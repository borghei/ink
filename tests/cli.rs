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
