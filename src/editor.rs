//! Opening the current document in the user's editor (`e`).
//!
//! Only the pure parts live here — which editor, how to split its command
//! line, how to ask it for a line — so they can be tested without spawning
//! anything. Suspending and restoring the TUI around the editor is in `app`.

/// The editor command line to run: `$VISUAL`, then `$EDITOR`, then the
/// platform's fallback. Blank values count as unset.
pub fn editor_command(visual: Option<&str>, editor: Option<&str>) -> String {
    [visual, editor]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|v| !v.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "notepad".into()
            } else {
                "vi".into()
            }
        })
}

/// Split a command line the way a POSIX shell splits words, without
/// expanding anything: whitespace separates, `'…'` is literal, `"…"` keeps
/// spaces and honours `\"`, `\\`, `\$` and `` \` ``, and a backslash outside
/// quotes escapes the next character. With `backslash_escapes` off (Windows,
/// where `\` is the path separator) backslashes are always literal.
pub fn split_command(s: &str, backslash_escapes: bool) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    // A word has started, even if it is empty so far (`''`).
    let mut in_word = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    word.push(c);
                }
            }
            '"' => {
                in_word = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' if backslash_escapes
                            && matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) =>
                        {
                            word.extend(chars.next());
                        }
                        c => word.push(c),
                    }
                }
            }
            '\\' if backslash_escapes => {
                in_word = true;
                word.extend(chars.next());
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// How an editor is told which line to open at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineArg {
    /// `+N file` (vi, vim, nvim, nano, emacs, emacsclient, kak, micro).
    Plus,
    /// `--goto file:N` (VS Code and its forks).
    Goto,
    /// `file:N` (Sublime Text, Zed, Helix).
    Suffix,
    /// Unknown editor: just the file.
    None,
}

fn line_arg_for(program: &str) -> LineArg {
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    match name {
        "vi" | "vim" | "nvim" | "gvim" | "view" | "nano" | "emacs" | "emacsclient" | "kak"
        | "micro" => LineArg::Plus,
        "code" | "code-insiders" | "codium" | "vscodium" | "cursor" => LineArg::Goto,
        "subl" | "zed" | "hx" | "helix" => LineArg::Suffix,
        _ => LineArg::None,
    }
}

/// A path no editor can mistake for an option or a command: a leading `-`
/// (an option almost everywhere) or `+` (an Ex command in vi, `+N` in many
/// others) gets `./` in front. A file reached by following a link in an
/// untrusted document can be called anything.
fn safe_path(path: &str) -> String {
    if path.starts_with('-') || path.starts_with('+') {
        format!("./{path}")
    } else {
        path.to_string()
    }
}

/// The argv to run: the editor command (split shell-style, never handed to
/// a shell), any line argument the editor understands, and the file as its
/// own element. `None` when the command is empty.
pub fn editor_argv(command: &str, path: &str, line: Option<usize>) -> Option<Vec<String>> {
    let mut argv = split_command(command, !cfg!(windows));
    let program = argv.first()?.clone();
    let path = safe_path(path);
    match (line.filter(|&n| n > 0), line_arg_for(&program)) {
        (Some(n), LineArg::Plus) => {
            argv.push(format!("+{n}"));
            argv.push(path);
        }
        (Some(n), LineArg::Goto) => {
            argv.push("--goto".into());
            argv.push(format!("{path}:{n}"));
        }
        (Some(n), LineArg::Suffix) => argv.push(format!("{path}:{n}")),
        _ => argv.push(path),
    }
    Some(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn visual_wins_then_editor_then_the_fallback() {
        assert_eq!(editor_command(Some("nvim"), Some("nano")), "nvim");
        assert_eq!(editor_command(Some("  "), Some("nano")), "nano");
        assert_eq!(editor_command(None, Some(" hx ")), "hx");
        let fallback = if cfg!(windows) { "notepad" } else { "vi" };
        assert_eq!(editor_command(None, None), fallback);
        assert_eq!(editor_command(Some(""), Some("")), fallback);
    }

    #[test]
    fn commands_split_like_a_shell_without_expansion() {
        assert_eq!(split_command("code --wait", true), v(&["code", "--wait"]));
        assert_eq!(
            split_command("  emacsclient   -t  ", true),
            v(&["emacsclient", "-t"])
        );
        assert_eq!(
            split_command("'/Applications/My Editor/bin/ed' -w", true),
            v(&["/Applications/My Editor/bin/ed", "-w"])
        );
        assert_eq!(
            split_command(r#""C:\Program Files\ed.exe" --x"#, false),
            v(&[r"C:\Program Files\ed.exe", "--x"])
        );
        assert_eq!(
            split_command(r#"ed "a \"q\" b" c\ d"#, true),
            v(&["ed", r#"a "q" b"#, "c d"])
        );
        // No expansion: `$HOME` and `;` are just characters.
        assert_eq!(split_command("vim $HOME;rm", true), v(&["vim", "$HOME;rm"]));
        assert_eq!(split_command("ed ''", true), v(&["ed", ""]));
        assert!(split_command("   ", true).is_empty());
    }

    #[test]
    fn each_editor_family_gets_its_line_syntax() {
        let argv = |cmd: &str, line| editor_argv(cmd, "doc.md", line).unwrap();
        for cmd in ["vi", "vim", "nvim", "nano", "emacs", "kak", "micro"] {
            assert_eq!(argv(cmd, Some(12)), v(&[cmd, "+12", "doc.md"]), "{cmd}");
        }
        assert_eq!(
            argv("emacsclient -t", Some(3)),
            v(&["emacsclient", "-t", "+3", "doc.md"])
        );
        assert_eq!(
            argv("/usr/local/bin/nvim", Some(3)),
            v(&["/usr/local/bin/nvim", "+3", "doc.md"])
        );
        for cmd in ["code", "codium", "cursor"] {
            assert_eq!(
                argv(&format!("{cmd} --wait"), Some(7)),
                v(&[cmd, "--wait", "--goto", "doc.md:7"])
            );
        }
        for cmd in ["subl", "zed", "hx"] {
            assert_eq!(argv(cmd, Some(7)), v(&[cmd, "doc.md:7"]));
        }
        // Unknown editors and unknown lines: just the file.
        assert_eq!(argv("ed", Some(7)), v(&["ed", "doc.md"]));
        assert_eq!(argv("vim", None), v(&["vim", "doc.md"]));
        assert_eq!(argv("vim", Some(0)), v(&["vim", "doc.md"]));
        assert_eq!(editor_argv("  ", "doc.md", Some(1)), None);
    }

    #[test]
    fn a_path_that_looks_like_an_option_or_command_is_defused() {
        assert_eq!(
            editor_argv("vim", "-c:!cmd.md", Some(2)).unwrap(),
            v(&["vim", "+2", "./-c:!cmd.md"])
        );
        assert_eq!(
            editor_argv("vi", "+!rm -rf ~.md", None).unwrap(),
            v(&["vi", "./+!rm -rf ~.md"])
        );
        assert_eq!(
            editor_argv("code", "--help.md", Some(1)).unwrap(),
            v(&["code", "--goto", "./--help.md:1"])
        );
        // The path stays one argv element, spaces and all.
        assert_eq!(
            editor_argv("nano", "my notes; rm x.md", None).unwrap(),
            v(&["nano", "my notes; rm x.md"])
        );
        assert_eq!(
            editor_argv("vim", "/abs/-x.md", None).unwrap(),
            v(&["vim", "/abs/-x.md"])
        );
    }
}
