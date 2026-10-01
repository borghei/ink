<p align="center">
  <img src="https://raw.githubusercontent.com/borghei/ink/main/docs/assets/demo.gif" alt="ink demo" width="720">
</p>

<h1 align="center">ink</h1>

<p align="center">
  A terminal markdown reader that actually looks good.
</p>

<p align="center">
  <a href="https://github.com/borghei/ink/releases"><img src="https://img.shields.io/github/v/release/borghei/ink?style=flat-square" alt="Release"></a>
  <a href="https://github.com/borghei/ink/blob/main/LICENSE"><img src="https://img.shields.io/github/license/borghei/ink?style=flat-square" alt="License"></a>
  <a href="https://crates.io/crates/ink-md"><img src="https://img.shields.io/crates/v/ink-md?style=flat-square" alt="Crates.io"></a>
</p>

---

ink renders markdown in your terminal with syntax highlighting, inline images, mermaid diagrams, themes, tabs, search, and a table of contents. It's fast on large documents, safe with untrusted files, and works as both an interactive reader and a `bat`-style pager. One binary. No dependencies. Built in Rust.

## Install

### Quick install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/borghei/ink/main/install.sh | sh
```

The installer picks the right binary for your OS, CPU and libc (static musl build on Alpine, Raspberry Pi and older-glibc systems), checks it against `SHA256SUMS` and installs to `/usr/local/bin` (using `sudo` only if it has to). Pin a version with `INK_VERSION=v0.8.0` or change the target with `INK_INSTALL_DIR="$HOME/.local/bin"`:

```bash
curl -fsSL https://raw.githubusercontent.com/borghei/ink/main/install.sh | INK_VERSION=v0.9.0 INK_INSTALL_DIR="$HOME/.local/bin" sh
```

### Quick install (Windows PowerShell)

```powershell
irm https://raw.githubusercontent.com/borghei/ink/main/install.ps1 | iex
```

Installs `ink.exe` (x64 or ARM64) to `%LOCALAPPDATA%\Programs\ink` and adds it to your user `PATH`. Set `$env:INK_VERSION = "v0.9.0"` first to pin a version.

### Homebrew (macOS / Linux)

```bash
brew tap borghei/tap
brew trust borghei/tap   # recent Homebrew requires trusting third-party taps
brew install ink
```

### Conda / Pixi

The community-maintained [conda-forge package](https://anaconda.org/conda-forge/ink-md) can be installed with:

```bash
conda install -c conda-forge ink-md
# or
pixi global install ink-md
```

### Cargo

```bash
cargo install ink-md        # build from source
cargo binstall ink-md       # or grab the prebuilt binary (needs cargo-binstall)
```

### mise

```bash
mise use -g cargo:ink-md    # via crates.io
mise use -g ubi:borghei/ink # or straight from GitHub releases
```

### Debian / Ubuntu (.deb) and Fedora / openSUSE (.rpm)

Download from the [releases page](https://github.com/borghei/ink/releases), then:

```bash
sudo apt install ./ink-md_*_amd64.deb    # Debian/Ubuntu
sudo dnf install ./ink-md-*.x86_64.rpm   # Fedora/openSUSE
```

### Scoop (Windows)

```powershell
scoop bucket add borghei https://github.com/borghei/scoop-bucket
scoop install ink
```

### Pre-built binaries

Grab the latest binary for your platform from the [releases page](https://github.com/borghei/ink/releases):

| Platform | Asset |
|---|---|
| Linux x86_64 / arm64 (glibc 2.35+: Ubuntu 22.04, Debian 12 and newer) | `ink-linux-amd64`, `ink-linux-arm64` |
| Linux x86_64 / arm64 / armv7, static (any distro: Alpine, Debian 11, Ubuntu 20.04, RHEL 8, NixOS, containers, Raspberry Pi) | `ink-linux-amd64-musl`, `ink-linux-arm64-musl`, `ink-linux-armv7-musl` |
| macOS Intel / Apple Silicon | `ink-macos-amd64`, `ink-macos-arm64` |
| Windows x64 / ARM64 | `ink-windows-amd64.exe`, `ink-windows-arm64.exe` |

The glibc builds need glibc 2.35 or newer; older and musl-based systems use the static builds, and the install script picks for you. The Windows builds link the C runtime statically, so no Visual C++ redistributable is needed. Each binary also comes as an archive (`ink-<version>-<os>-<arch>.tar.gz`, `.zip` on Windows) bundling the man page and bash/zsh/fish completions, and the `.deb`/`.rpm` packages install those too.

`SHA256SUMS` is published alongside, and every asset carries a signed build provenance attestation you can check with the GitHub CLI:

```bash
gh attestation verify ink-linux-amd64 --repo borghei/ink
```

The static musl builds and Windows ARM64 binaries are new in v0.9.0; earlier releases only have the glibc, macOS and Windows x64 binaries.

### From source

```bash
git clone https://github.com/borghei/ink.git
cd ink
cargo build --release
# binary is at ./target/release/ink
```

## Quick start

```bash
# Read a file
ink README.md

# Browse all markdown files in a directory
ink .

# Read from a URL
ink https://raw.githubusercontent.com/borghei/ink/main/README.md

# Pipe from stdin (or name it explicitly with -)
cat notes.md | ink
cat notes.md | ink -

# Plain output (no TUI, pipe-friendly)
ink --plain README.md
```

## Features

### Renders markdown the way it should look

Headings, bold, italic, strikethrough, links, blockquotes, lists, task lists, tables, footnotes, horizontal rules — all rendered with proper styling and colors.

Tables follow their column alignment (`:--`, `:-:`, `--:`), wide CJK and emoji cells included. Superscript `x^2^` (or `x<sup>2</sup>`) and subscript `H<sub>2</sub>O` come out as `x²` and `H₂O` (as `^(…)`/`_(…)` when a character has no Unicode form). Single and double tildes, `~x~` and `~~x~~`, strike through as on GitHub. `||spoilers||` stay hidden until you select or copy them, and definition lists (`Term` then `: definition`) get a bold term with its definitions indented underneath.

### Syntax-highlighted code blocks

Language-aware highlighting for every major language. Code blocks get clean borders with the language label shown at the top.

### Inline images

Images in your markdown render directly in the terminal. On terminals with a graphics protocol (Kitty, iTerm2, WezTerm, Ghostty, or Sixel) ink draws real pixels — auto-detected at startup; everywhere else it falls back to Unicode half-blocks, which work in any true-color terminal. Images scroll with the document and clip cleanly at the edges. If an image can't load, ink shows a placeholder instead of crashing.

Pick a renderer explicitly with `--image-protocol <auto|kitty|iterm2|sixel|halfblocks>` (default `auto`).

Supported formats: PNG, JPEG, GIF, WebP, BMP, TIFF, ICO, TGA, QOI, PNM, HDR, OpenEXR, farbfeld — and SVG (including gzipped `.svgz`), rasterized at load time so vector diagrams render like any other image.

Remote (`http`/`https`) images are **not** fetched by default — a document you didn't write shouldn't be able to phone home or probe your network. Pass `--remote-images` to enable them (private, loopback, and cloud-metadata addresses stay blocked even then).

### Mermaid diagrams

Flowcharts, sequence diagrams, pie charts, and Gantt charts rendered as ASCII art. No external tools needed.

### GitHub-style admonitions

`[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, and `[!CAUTION]` blocks render with distinct colors and icons. A custom title (`> [!NOTE] Read this first`) replaces the type name, and Obsidian callouts (`> [!info]- Title`, `[!bug]`, `[!example]`, …) map onto the same five colours, fold markers ignored.

### Frontmatter

YAML (`---`), TOML (`+++`) and JSON (`{`) frontmatter is hidden by default. With `--frontmatter` (or `frontmatter = true`) it shows as a small key/value box at the top of the document — lists comma-joined, nested values as written — instead of being read as markdown.

### Wikilinks

`[[page]]` and `[[page|display text]]` syntax works out of the box. Great for browsing Obsidian vaults and personal wikis.

### File browser

Run `ink` with no arguments or point it at a directory. You'll get an interactive file picker that lists every `.md` file, with filtering and keyboard navigation.

### Multi-tab support

Open multiple files at once:

```bash
ink README.md CHANGELOG.md docs/guide.md
```

Switch between them with `Tab` and `Shift+Tab`.

### Search

Press `/` to search within a document. Matches highlight inline; press Enter to lock in the search, then `n`/`N` to cycle forward and backward through results. `Esc` clears the highlights.

### Select text and copy it

ink captures the mouse for wheel-scroll, which used to mean giving up your terminal's own text
selection. Now it does the job itself, and does it better — it knows where a code block starts and
where ink's own decoration ends. If you'd rather keep your terminal's own selection and link
clicking, run `ink --no-mouse` (one session) or set `mouse_capture = false` (always); both apply to
the reader and the file browser.

- **`v`** starts a character-wise selection, **`V`** a line-wise one. Move with `h j k l`, arrows,
  `w`/`b` (word), `0`/`$` (line ends), `g`/`G` (document ends), `Ctrl+d`/`Ctrl+u` (half page).
  **`y`** copies and exits; `Esc` cancels.
- **Drag with the mouse** to select, release to copy. Double-click takes the word, triple-click the
  line. A single click on a link follows it instead (see below).
- **`c`** labels every code block on screen — press its letter to copy the block's *raw* source: no
  borders, no line numbers, no syntax-highlighting escapes.
- **`Y`** copies the markdown source of the section you are reading (heading included, down to the
  next heading of the same level). Inside link-hint mode (`f`), `Y` switches the labels from
  "open this link" to "copy this URL".

Copied text reaches the clipboard two ways at once: an OSC 52 escape sequence, which works over
SSH and inside tmux, and a native helper (`pbcopy`, `wl-copy`, `xclip`, `xsel`, `clip.exe`) when one
is installed locally. No extra dependencies, nothing to configure. Set `clipboard` in your config to
narrow it down or turn it off.

`v` and `V` copy lines the way they are drawn, ink's heading bars and code-box borders included —
they select what you can see. Reach for `c` and `Y` when you want the source instead.

### Open links from the keyboard

Press `f` to label every link on screen with a letter; the popup lists each link's text next to its URL, so you can tell which is which. Press that letter to open web and mail links in your browser, or to follow a relative `.md` link or `#heading` anchor right inside ink (`[` goes back).

Clicking works too: a plain click on a link (press and release without moving) does exactly what its letter would. Dragging across a link still selects it. With `--no-mouse` ink leaves clicks to your terminal, which opens links its own way (usually Cmd- or Ctrl-click).

### Help overlay

Press `?` any time for a popup listing every active keybinding — including your own overrides.

### Table of contents

Press `t` to toggle a sidebar showing every heading in the document. Tracks your position as you scroll.

Press `o` to move into it (it opens if it was closed). `j`/`k` or the arrows move, `g`/`G` go to the first and last heading, `Ctrl+d`/`Ctrl+u` page, and `Enter` jumps there — `[` brings you back. `h`/`l` fold and unfold a section's subheadings in the sidebar, and `/` filters the list as you type (every word you type must appear, case doesn't matter). `Esc` clears the filter; `Esc` again, or `o`, goes back to the document without moving. With the mouse, click a heading to jump to it; the wheel over the sidebar scrolls the sidebar, not the page.

### 8 built-in themes

Dark, Light, Dracula, Catppuccin, Nord, Tokyo Night, Gruvbox, and Solarized. Press `T` to open the theme picker and preview each one live.

With no theme set, ink asks the terminal for its background colour (an OSC 11 query, answered by nearly every modern terminal and passed through by tmux) and picks Light or Dark to match, falling back to `COLORFGBG` and then Dark. An explicit `--theme` or `theme =` in the config always wins, and never triggers the query.

Themes adapt to what the terminal can show: 24-bit colour where it is advertised, the 256-colour palette on `*-256color` terminals, and the 16 ANSI colours on the Linux console and other 16-colour terminals (`TERM=linux`, `xterm`, `vt100`, `screen`, …) — there ink uses your terminal's own palette, so your colour scheme applies. With `--color=never` or `NO_COLOR` the reader draws without colour at all, using bold, underline and reverse video for headings, links, search hits and selections.

On the Linux console, a non-UTF-8 locale, a legacy console font or a screen reader, box-drawing and symbol characters come out as junk. `ink --ascii` (or `ascii = true` under `[behavior]`) draws every border, bullet, task box, quote bar, rule and status-bar marker in plain 7-bit ASCII instead — `+--+`, `|`, `*`, `[x]`, `#` — and turns off smart quotes and emoji shortcodes, so an ASCII document prints as pure ASCII. It switches on by itself for `TERM=linux` and non-UTF-8 locales; `ascii = false` in the config keeps Unicode regardless.

### Math and emoji

Inline `$E=mc^2$`, block `$$...$$` and ```` ```math ```` render as Unicode text: `$x = \frac{-b \pm \sqrt{b^2-4ac}}{2a}$` reads `x = (-b ± √(b² - 4ac))/2a`. Greek letters, operators, `\mathbb{R}` → ℝ, super- and subscripts, fractions, roots, and matrix, `cases` and `aligned` environments (drawn on several lines with brackets) are covered; anything else is shown as written. A lone `$` in prose (`costs $5 and $10`) stays a dollar sign. With `--ascii` the LaTeX source is shown instead. `:emoji:` shortcodes resolve to their glyph (`:rocket:` → 🚀).

### Presentation mode

Split any markdown file into slides on `---` separators and navigate with ←/→/Space:

```bash
ink --slides deck.md
```

### Works as a pager

Point `ink --plain` at a long document on an interactive terminal and it pages the output through `$PAGER` (default `less -R`) — a drop-in markdown replacement for `cat`/`less`. Piped or redirected output prints straight through, so it stays friendly for scripts, fzf previews, and git. Use `--no-pager` to always print directly. When stdout is not a terminal, `ink file.md` behaves like `ink --plain file.md`, as `bat` and `glow` do.

`--line-range` works like `bat`'s: `ink --line-range 40:80 README.md` renders only those lines of the markdown *source* (1-based, inclusive; also `40:`, `:80`, a single line, or several `--line-range`s). A range that cuts through a fenced code block keeps the block intact and highlighted. It always prints plain output.

Color follows `--color <auto|always|never>` (default `auto`). In `auto` mode, `--plain` and `ink diff` emit color and OSC 8 hyperlinks only when stdout is a terminal, so redirects, pipes, and `git` textconv get clean text. Precedence: an explicit `--color` wins; then `NO_COLOR` turns color off; then `CLICOLOR_FORCE=1` or `FORCE_COLOR` turns it on; then `TERM=dumb` turns it off; then the terminal check. Tools that display ANSI from a pipe need `--color=always`:

```bash
fzf --preview 'ink --plain --color=always {}'
ink --plain --color=always notes.md | less -R

# Render markdown in git diffs (clean text, no escapes)
git config --global diff.markdown.textconv "ink --plain"
echo '*.md diff=markdown' >> ~/.gitattributes
```

24-bit colors are downgraded to the 256-color palette on terminals that don't advertise truecolor (`COLORTERM`, Windows Terminal, VS Code, Ghostty, WezTerm and iTerm2 are detected).

### Watch mode

Auto-reload when the file changes on disk. Edit in another terminal, see the rendered view update within ~100ms. Scroll position is preserved.

```bash
ink --watch draft.md
```

Works with editors that save by replacing the inode (vim, IntelliJ) — ink watches the parent directory, not the file handle.

### Document stats and outline

```bash
# Heading structure
ink outline README.md

# Word count, reading time, element counts
ink stats README.md

# Diff two markdown files
ink diff old.md new.md
```

## Keybindings

| Key | Action |
|---|---|
| `j` / `k` / `↑` / `↓` | Scroll up/down |
| `Space` / `Page Down` | Page down |
| `Page Up` | Page up |
| `G` / `End` | Jump to end |
| `Home` | Jump to start |
| `Ctrl+f` / `Ctrl+b` | Page down / up |
| `Ctrl+d` / `Ctrl+u` | Half-page down / up |
| `n` / `N` | Next / previous heading (cycles search matches after a search) |
| `/` | Search |
| `f` | Link-hint mode (open links by letter, `Y` to copy one instead) |
| `v` / `V` | Select text — character-wise / line-wise (`y` copies, `Esc` cancels) |
| `c` | Copy a code block by letter |
| `Y` | Copy the current section as markdown |
| `e` | Edit the file in `$VISUAL` / `$EDITOR` (opens at the heading you are reading, reloads when you quit the editor) |
| `t` | Toggle table of contents |
| `o` | Focus the table of contents (`j`/`k` move, `Enter` jumps, `h`/`l` fold, `/` filters, `Esc` returns) |
| `T` | Theme picker (choice is saved to config) |
| `?` | Help overlay |
| `Enter` | Follow first visible link |
| `[` / `]` | Navigation back / forward |
| `Tab` / `Shift+Tab` | Next / previous tab |
| `Shift+B` | Back to file browser (when launched via browser) |
| `q` / `Esc` / `Ctrl+C` | Quit ink (first press clears an active search) |

### Customizing keybindings

Pick a preset or override individual bindings in your config file (see [Configuration](#configuration)):

```toml
[keybindings]
preset = "emacs"   # default | vim | emacs

[keybindings.bindings]
# Per-action overrides, applied on top of the preset.
toggle_toc = ["ctrl-t"]
toc_focus = ["ctrl-o"]
edit = ["alt-e"]
```

The **emacs** preset binds `Ctrl+N`/`Ctrl+P` (line nav), `Ctrl+V`/`Alt+V` (page nav), `Ctrl+A`/`Ctrl+E` (home/end), `Ctrl+S` (search), `Ctrl+F`/`Ctrl+B` (next/prev heading), and `Ctrl+X Ctrl+C` (chord exit).

Two-key chord bindings work — write them with a space: `"ctrl-x ctrl-c"`.

Run `ink keybindings` to print the active map. Run `ink config init` to drop a commented starter config in the right place for your platform.

## Configuration

ink reads `$XDG_CONFIG_HOME/ink/config.toml` when `XDG_CONFIG_HOME` is set, otherwise `~/.config/ink/config.toml` on Linux, `~/Library/Application Support/ink/config.toml` on macOS, and `%APPDATA%\ink\config.toml` on Windows. If you already have a config in the platform folder and nothing under `$XDG_CONFIG_HOME/ink` yet, ink keeps using the existing one until you move it. Run `ink config path` to print the resolved location.

Create the file:

```toml
# Default theme (dark, light, dracula, catppuccin, nord, tokyo-night, gruvbox, solarized)
theme = "catppuccin"

# Max rendering width in columns
width = 90

# Line spacing: compact, normal, relaxed
spacing = "normal"

# Show table of contents on startup
toc = false

# Show YAML/TOML/JSON frontmatter as a metadata box at the top
frontmatter = false

# Behavior
[behavior]
# Set true to restore the old "return to file browser after closing a doc" behavior
browser_loop = false

# Set false to let your terminal handle the mouse (click-to-open links, text
# selection) instead of ink capturing it for wheel-scroll. Default: true
mouse_capture = true

# How copied text reaches the clipboard:
#   auto   - OSC 52 escape (crosses SSH and tmux) plus a native helper, if present
#   osc52  - escape sequence only
#   native - pbcopy / wl-copy / xclip / xsel / clip.exe / termux-clipboard-set only
#   off    - copying is disabled
clipboard = "auto"

# Plain-ASCII borders, bullets and markers (Linux console, legacy fonts,
# screen readers). Unset: automatic on TERM=linux and non-UTF-8 locales.
# ascii = false
```

### Custom themes

Drop a `.toml` file in the `themes/` folder next to your config file (e.g. `$XDG_CONFIG_HOME/ink/themes/` or `~/.config/ink/themes/`) and use it by name:

```bash
ink --theme mytheme README.md
```

Every color is customizable — headings, code, links, blockquotes, admonitions, status bar, and more. Check any built-in theme in `src/theme/builtin.rs` for the full list of color keys.

## Shell integration

```bash
ink shell-setup bash   # or zsh, fish
```

Prints config snippets you can add to your shell profile — aliases, fzf preview, git pager setup.

### Shell completions and man page

```bash
ink completions zsh > ~/.zfunc/_ink        # bash | zsh | fish | powershell | elvish
ink man > /usr/local/share/man/man1/ink.1  # man page (troff)
```

## CLI reference

```
ink [OPTIONS] [FILE|URL]...

Options:
  -t, --theme <THEME>    Color theme [default: auto]
  -w, --width <WIDTH>    Max width (columns, or: narrow, wide, full)
  -s, --slides           Presentation mode
  -p, --plain            Plain output (no TUI); automatic when stdout is not a terminal
      --color <WHEN>     auto | always | never [default: auto]
      --watch            Watch file for changes
      --toc              Show table of contents on startup
      --no-images        Disable image rendering
      --remote-images    Allow fetching remote (http/https) images
      --image-protocol <P>  auto | kitty | iterm2 | sixel | halfblocks
      --list-themes      List available themes and exit
      --no-pager         Never page --plain output, even on a TTY
      --frontmatter      Show YAML/TOML/JSON frontmatter as a metadata box
      --line-range <START:END>  Render only these source lines (N, START:, :END;
                         repeatable; implies --plain)
      --spacing <MODE>   Line spacing: compact, normal, relaxed
      --no-mouse         Don't capture the mouse (overrides mouse_capture in config)
      --ascii            Draw borders, bullets and markers in plain ASCII

Subcommands:
  outline      Show document heading structure
  stats        Show document statistics
  diff         Diff two markdown files
  shell-setup  Print shell integration snippets
  completions  Generate shell completions
  man          Generate a man page
  config       Config helpers (init, path)
  keybindings  Print the active keybinding map
  doctor       Print a diagnostic report to attach to an issue
```

## How it compares

| Feature | ink | glow | mdcat | frogmouth |
|---|---|---|---|---|
| Interactive TUI | Yes | Yes | No | Yes |
| Multi-tab | Yes | No | No | No |
| In-document search | Yes | No | No | No |
| Table of contents | Yes | No | No | Yes |
| Inline images (graphics protocols) | Yes | No | Yes | No |
| Inline images (half-block fallback) | Yes | No | No | No |
| Mermaid diagrams | Yes | No | No | No |
| Admonitions | Yes | No | No | No |
| Wikilinks | Yes | No | No | No |
| File browser | Yes | Yes | No | Yes |
| Watch mode | Yes | No | No | No |
| Presentation mode | Yes | No | No | No |
| Pager mode | Yes | Yes | No | No |
| Shell completions + man page | Yes | Yes | Yes | No |
| Math + emoji | Yes | No | No | No |
| Themes | 8 | 2 | 0 | 0 |
| Single binary | Yes | Yes | Yes | No |

## Troubleshooting images

Terminal graphics support varies wildly. If an image is blank, garbled, or missing:

1. **Try the universal renderer:** `ink --image-protocol halfblocks file.md`. If the image appears as coarse colored blocks, decoding is fine — the problem is your terminal's pixel graphics protocol.
2. **Try a specific protocol:** `--image-protocol iterm2` (iTerm2, WezTerm, VS Code, Warp and friends) or `--image-protocol sixel`. Some terminals advertise protocols they only partially implement — auto-detection already works around the known cases (e.g. iTerm2 claiming kitty support), but new terminal versions ship new quirks.
3. **Collect a diagnostic report:** run `ink doctor` in the affected terminal — it prints your platform and terminal identity, the colour depth and theme ink picked (and which signal decided each), the clipboard helper and OSC 52 wrapping it would use, mouse capture, config warnings, what the graphics negotiation chose, and decoder self-tests. It leaves out anything that identifies you (no hostnames, user names, IPs or session IDs; your home directory shows as `~`), so it is safe to paste into an issue. `ink doctor --save ink-doctor.txt` writes it to a file. The same report helps with colour, theme and clipboard problems.
4. **Send it to us:** open an [image rendering issue](https://github.com/borghei/ink/issues/new?template=image-rendering.yml) with the report attached. That output usually pinpoints the problem immediately.

## Security model

ink treats every document as untrusted input: control bytes and escape
sequences are stripped before anything reaches the terminal, link schemes are
restricted to `http`/`https`/`mailto`, and remote image fetching is **off by
default** (`--remote-images` opts in). Local images referenced by a document
— absolute paths included — are read and rendered, but only ever as pixels:
image bytes must decode as an image and are never shown as text, so a hostile
document cannot use an image tag to put file *contents* on screen.

**Note for Windows authors:** markdown treats `\` as an escape character, so
a path like `C:\pics\.cache\x.png` loses its separators in any CommonMark
renderer. Write image paths with forward slashes (`C:/pics/.cache/x.png`) —
they work everywhere, including on Windows.

## Contributing

Contributions are welcome. Check out [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Free to use, modify, and distribute. Cannot be sold — not the original, not forks, not derivatives. See [LICENSE](LICENSE) for the full text.

## Author

Made by [borghei](https://github.com/borghei) — who got tired of reading raw markdown like a caveman.

## Contributors

Thanks to everyone who has sent a pull request:

- [@mariusrueve](https://github.com/mariusrueve) — Conda and Pixi install instructions ([#9](https://github.com/borghei/ink/pull/9)), and the [conda-forge package](https://github.com/conda-forge/ink-md-feedstock) behind them
