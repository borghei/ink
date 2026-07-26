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

### Homebrew (macOS / Linux)

```bash
brew tap borghei/tap
brew trust borghei/tap   # recent Homebrew requires trusting third-party taps
brew install ink
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

Grab the latest binary for your platform from the [releases page](https://github.com/borghei/ink/releases). Available for Linux (amd64, arm64), macOS (amd64, arm64), and Windows (amd64); `SHA256SUMS` is published alongside.

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

# Pipe from stdin
cat notes.md | ink

# Plain output (no TUI, pipe-friendly)
ink --plain README.md
```

## Features

### Renders markdown the way it should look

Headings, bold, italic, strikethrough, links, blockquotes, lists, task lists, tables, footnotes, horizontal rules — all rendered with proper styling and colors.

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

`[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, and `[!CAUTION]` blocks render with distinct colors and icons.

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

### Open links from the keyboard

Press `f` to label every link on screen with a letter. Press that letter to open web and mail links in your browser, or to follow a relative `.md` link right inside ink.

### Help overlay

Press `?` any time for a popup listing every active keybinding — including your own overrides.

### Table of contents

Press `t` to toggle a sidebar showing every heading in the document. Tracks your position as you scroll.

### 8 built-in themes

Dark, Light, Dracula, Catppuccin, Nord, Tokyo Night, Gruvbox, and Solarized. Press `T` to open the theme picker and preview each one live.

Auto-detects your terminal background and picks dark or light mode by default.

### Math and emoji

Inline `$E=mc^2$` and block `$$...$$` math render in code style, and `:emoji:` shortcodes resolve to their glyph (`:rocket:` → 🚀).

### Presentation mode

Split any markdown file into slides on `---` separators and navigate with ←/→/Space:

```bash
ink --slides deck.md
```

### Works as a pager

Point `ink --plain` at a long document on an interactive terminal and it pages the output through `$PAGER` (default `less -R`) — a drop-in markdown replacement for `cat`/`less`. Piped or redirected output prints straight through, so it stays friendly for scripts, fzf previews, and git. Use `--no-pager` to always print directly.

Honors `NO_COLOR`, and automatically downgrades 24-bit colors to the 256-color palette on terminals that don't advertise truecolor.

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
| `f` | Link-hint mode (open/follow links by letter) |
| `t` | Toggle table of contents |
| `T` | Theme picker (choice is saved to config) |
| `?` | Help overlay |
| `Enter` | Follow first visible link |
| `[` / `]` | Navigation back / forward |
| `Tab` / `Shift+Tab` | Next / previous tab |
| `Shift+B` | Back to file browser (when launched via browser) |
| `q` / `Esc` / `Ctrl+C` | Quit ink (first press clears an active search) |

### Customizing keybindings

Pick a preset or override individual bindings in `~/.config/ink/config.toml`:

```toml
[keybindings]
preset = "emacs"   # default | vim | emacs

[keybindings.bindings]
# Per-action overrides, applied on top of the preset.
toggle_toc = ["ctrl-t"]
```

The **emacs** preset binds `Ctrl+N`/`Ctrl+P` (line nav), `Ctrl+V`/`Alt+V` (page nav), `Ctrl+A`/`Ctrl+E` (home/end), `Ctrl+S` (search), `Ctrl+F`/`Ctrl+B` (next/prev heading), and `Ctrl+X Ctrl+C` (chord exit).

Two-key chord bindings work — write them with a space: `"ctrl-x ctrl-c"`.

Run `ink keybindings` to print the active map. Run `ink config init` to drop a commented starter config in the right place for your platform.

## Configuration

Create `~/.config/ink/config.toml`:

```toml
# Default theme (dark, light, dracula, catppuccin, nord, tokyo-night, gruvbox, solarized)
theme = "catppuccin"

# Max rendering width in columns
width = 90

# Line spacing: compact, normal, relaxed
spacing = "normal"

# Show table of contents on startup
toc = false

# Show YAML/TOML frontmatter
frontmatter = false

# Behavior
[behavior]
# Set true to restore the old "return to file browser after closing a doc" behavior
browser_loop = false

# Set false to let your terminal handle the mouse (click-to-open links, text
# selection) instead of ink capturing it for wheel-scroll. Default: true
mouse_capture = true
```

### Custom themes

Drop a `.toml` file in `~/.config/ink/themes/` and use it by name:

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
  -p, --plain            Plain output (no TUI)
      --watch            Watch file for changes
      --toc              Show table of contents on startup
      --no-images        Disable image rendering
      --remote-images    Allow fetching remote (http/https) images
      --image-protocol <P>  auto | kitty | iterm2 | sixel | halfblocks
      --list-themes      List available themes and exit
      --no-pager         Never page --plain output, even on a TTY
      --frontmatter      Show YAML/TOML frontmatter
      --spacing <MODE>   Line spacing: compact, normal, relaxed

Subcommands:
  outline      Show document heading structure
  stats        Show document statistics
  diff         Diff two markdown files
  shell-setup  Print shell integration snippets
  completions  Generate shell completions
  man          Generate a man page
  config       Config helpers (init, path)
  keybindings  Print the active keybinding map
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
3. **Collect a diagnostic report:** run `ink doctor` in the affected terminal — it prints your terminal identity, what the graphics negotiation chose, and decoder self-tests. `ink doctor --save ink-doctor.txt` writes it to a file.
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
