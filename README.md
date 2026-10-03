<p align="center">
  <img src="https://raw.githubusercontent.com/borghei/ink/main/docs/assets/demo.gif" alt="ink rendering a markdown file in the terminal: headings, a code block, a mermaid flowchart, the table of contents and the theme picker" width="720">
</p>

<h1 align="center">ink</h1>

<p align="center">
  A terminal markdown reader that actually looks good.
</p>

<p align="center">
  <a href="https://github.com/borghei/ink/releases"><img src="https://img.shields.io/github/v/release/borghei/ink?style=flat-square" alt="Release"></a>
  <a href="https://crates.io/crates/ink-md"><img src="https://img.shields.io/crates/v/ink-md?style=flat-square" alt="Crates.io"></a>
  <a href="https://github.com/borghei/ink/blob/main/LICENSE"><img src="https://img.shields.io/github/license/borghei/ink?style=flat-square" alt="License"></a>
</p>

<p align="center">
  <a href="https://borghei.github.io/ink/">Website</a> ·
  <a href="https://github.com/borghei/ink/blob/main/docs/install.md">Install</a> ·
  <a href="https://github.com/borghei/ink/blob/main/docs/README.md">Docs</a> ·
  <a href="https://github.com/borghei/ink/blob/main/CHANGELOG.md">Changelog</a>
</p>

---

ink renders markdown in your terminal: code, tables, images, diagrams and math, in a reader with search, tabs and a table of contents. It also works as a `bat`-style pager for scripts and git. One binary, no dependencies, written in Rust.

## Features

- **Proper rendering.** Headings, tables with alignment, task lists, footnotes, admonitions, frontmatter and wikilinks.
- **Highlighted code.** Every major language, in bordered blocks. Press `c` to copy a block's raw source.
- **Inline images.** Real pixels on Kitty, iTerm2, WezTerm, Ghostty and Sixel terminals, half-blocks everywhere else. SVG too.
- **Mermaid diagrams.** Flowcharts, state, class and ER diagrams drawn as boxes and arrows. Gantt charts, mindmaps, sequence diagrams and pie charts too.
- **Math and emoji.** LaTeX renders as Unicode (`√(b² - 4ac)`) and `:rocket:` becomes 🚀.
- **Made for reading.** Search, a table of contents you can filter, tabs, a file browser, link hints and watch mode.
- **Select and copy.** Keyboard or mouse selection, copy a section as markdown, and it works over SSH and tmux.
- **9 themes.** Including `terminal`, which follows your terminal's own color scheme. Or write your own.
- **Pager mode.** `ink --plain` pages long output, prints clean text into pipes and handles `--line-range`, fzf previews and git diffs.
- **Safe with untrusted files.** Escape sequences are stripped, and remote images stay off unless you ask for them.

## Install

```bash
# macOS, Linux
curl -fsSL https://raw.githubusercontent.com/borghei/ink/main/install.sh | sh

# Homebrew
brew tap borghei/tap
brew trust borghei/tap
brew install ink

# Cargo
cargo install ink-md
```

```powershell
# Windows
irm https://raw.githubusercontent.com/borghei/ink/main/install.ps1 | iex
```

Scoop, conda, mise, `.deb`, `.rpm`, prebuilt binaries and source builds are covered in the [install guide](https://github.com/borghei/ink/blob/main/docs/install.md).

## Quick start

```bash
ink README.md                     # read a file
ink .                             # browse markdown files in a folder
ink a.md b.md                     # one tab per file
cat notes.md | ink                # read stdin
ink --plain README.md | head      # plain output for pipes
ink --theme terminal README.md    # use your terminal's colors
```

Press `?` in the reader for every key. The basics: `j` `k` scroll, `/` search, `t` table of contents, `f` open a link, `T` themes, `q` quit.

## Documentation

| | |
|---|---|
| [Install](https://github.com/borghei/ink/blob/main/docs/install.md) | Every install route, binaries, verifying downloads |
| [Features](https://github.com/borghei/ink/blob/main/docs/features.md) | Everything ink renders and does, plus how it compares to glow, mdcat and frogmouth |
| [Themes and color](https://github.com/borghei/ink/blob/main/docs/themes.md) | Built-in themes, the terminal theme, custom themes |
| [Configuration](https://github.com/borghei/ink/blob/main/docs/configuration.md) | Config file, settings, keybindings and presets |
| [Command line](https://github.com/borghei/ink/blob/main/docs/cli.md) | Flags, subcommands, pager use, recipes, shell setup |
| [Troubleshooting](https://github.com/borghei/ink/blob/main/docs/troubleshooting.md) | `ink doctor`, images, colors, clipboard, security model |

## Contributing

Bug reports and pull requests are welcome. Read [CONTRIBUTING.md](https://github.com/borghei/ink/blob/main/CONTRIBUTING.md) first, and attach `ink doctor` output to bug reports.

Thanks to everyone who has sent a pull request:

- [@mariusrueve](https://github.com/mariusrueve) for the Conda and Pixi instructions ([#9](https://github.com/borghei/ink/pull/9)) and the [conda-forge package](https://github.com/conda-forge/ink-md-feedstock) behind them

## License

Free to use, modify and share. It can't be sold: not the original, not forks, not derivatives. See [LICENSE](https://github.com/borghei/ink/blob/main/LICENSE).

Made by [borghei](https://github.com/borghei), who got tired of reading raw markdown like a caveman.
