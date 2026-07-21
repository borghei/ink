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
