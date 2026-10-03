# Command line

- [Usage](#usage)
- [Options](#options)
- [Subcommands](#subcommands)
- [Using ink as a pager](#using-ink-as-a-pager)
- [Color in plain output](#color-in-plain-output)
- [Rendering part of a file](#rendering-part-of-a-file)
- [Recipes](#recipes)
- [Shell integration](#shell-integration)

## Usage

```
ink [OPTIONS] [FILE|URL]... [COMMAND]
```

With no file, ink reads stdin when something is piped in and opens the file browser otherwise. `-` means stdin explicitly.

## Options

| Option | What it does |
|---|---|
| `-t`, `--theme <THEME>` | Color theme. Default `auto`, which matches the terminal background. See [Themes](themes.md). |
| `-w`, `--width <WIDTH>` | Max text width: 20 to 1000 columns, or `narrow`, `wide`, `full` |
| `-s`, `--slides` | Presentation mode, split on `---` |
| `-p`, `--plain` | Print rendered output instead of opening the reader. Automatic when stdout isn't a terminal. |
| `--color <WHEN>` | `auto`, `always` or `never`. Default `auto`. See [Color in plain output](#color-in-plain-output). |
| `--watch` | Re-render when the file changes |
| `--toc` | Open with the table of contents showing |
| `--no-images` | Don't render images |
| `--remote-images` | Allow `http` and `https` images. Off by default. |
| `--image-protocol <P>` | `auto`, `kitty`, `iterm2`, `sixel` or `halfblocks` |
| `--list-themes` | List built-in and custom themes, then exit |
| `--no-pager` | Never page `--plain` output, even on a terminal |
| `--frontmatter` | Show frontmatter as a box at the top |
| `--line-range <START:END>` | Render only these source lines. Repeatable. Implies `--plain`. |
| `--spacing <MODE>` | `compact`, `normal` or `relaxed` |
| `--no-mouse` | Leave the mouse to your terminal for this session |
| `--ascii` | Plain ASCII borders, bullets and markers |
| `-h`, `--help` | Help. `--help` is longer than `-h`. |
| `-V`, `--version` | Version |

Most options have a config equivalent. See [Configuration](configuration.md).

## Subcommands

| Command | What it does |
|---|---|
| `ink outline <FILE>` | Print the heading structure |
| `ink stats <FILE>` | Word count, reading time and element counts |
| `ink diff <OLD> <NEW>` | Diff two markdown files |
| `ink keybindings` | Print the active key map, preset plus your overrides |
| `ink config init` | Write a commented starter config |
| `ink config path` | Print where the config lives |
| `ink doctor [--save FILE]` | Diagnostic report for bug reports. See [Troubleshooting](troubleshooting.md). |
| `ink shell-setup <SHELL>` | Print shell snippets: aliases, fzf preview, git setup |
| `ink completions <SHELL>` | Shell completions: `bash`, `zsh`, `fish`, `powershell`, `elvish` |
| `ink man` | Man page (troff) on stdout |

## Using ink as a pager

`ink --plain` renders the document and prints it, styled, without the interactive reader.

- On a terminal, long output goes through `$PAGER` (default `less -R`), so ink works as a markdown `cat` or `less`.
- Piped or redirected, it prints straight through. Scripts, fzf previews and git get clean output.
- `--no-pager` always prints directly.

You rarely need the flag: when stdout isn't a terminal, `ink file.md` already behaves like `ink --plain file.md`, as `bat` and `glow` do.

## Color in plain output

`--color` decides whether `--plain` and `ink diff` emit color and OSC 8 hyperlinks. In order:

1. `--color=always` or `--color=never` wins.
2. `NO_COLOR` turns color off.
3. `CLICOLOR_FORCE=1` or `FORCE_COLOR` turns it on.
4. `TERM=dumb` turns it off.
5. Otherwise, color only when stdout is a terminal.

So redirects, pipes and `git` textconv get clean text by default. A tool that shows ANSI from a pipe, like fzf or `less -R`, needs `--color=always`.

24-bit colors drop to the 256-color palette on terminals that don't advertise truecolor.

## Rendering part of a file

`--line-range` works like `bat`'s. It picks lines of the markdown source (1-based, inclusive) and always prints plain output.

```bash
ink --line-range 40:80 README.md    # lines 40 to 80
ink --line-range 40: README.md      # line 40 to the end
ink --line-range :80 README.md      # start to line 80
ink --line-range 12 README.md       # one line
ink --line-range 1:5 --line-range 40:50 README.md
```

A range that cuts through a fenced code block keeps the block intact and highlighted, inside a blockquote too.

## Recipes

```bash
# fzf with a rendered preview
fzf --preview 'ink --plain --color=always {}'

# page a file yourself
ink --plain --color=always notes.md | less -R

# readable markdown in git diffs (clean text, no escapes)
git config --global diff.markdown.textconv "ink --plain"
echo '*.md diff=markdown' >> ~/.gitattributes

# read a README from the web
ink https://raw.githubusercontent.com/borghei/ink/main/README.md
```

## Shell integration

```bash
ink shell-setup zsh    # or bash, fish
```

This prints snippets for your shell profile: aliases, an fzf preview and git setup. Read them, then paste the parts you want.

Completions and the man page:

```bash
ink completions zsh > ~/.zfunc/_ink
ink completions fish > ~/.config/fish/completions/ink.fish
ink man > /usr/local/share/man/man1/ink.1
```

The release archives and the `.deb` and `.rpm` packages already include both.
