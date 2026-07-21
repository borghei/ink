# Changelog

## 0.5.0 — 2026-07-21

### Added
- **Presentation mode (`--slides`) is now implemented.** Splits the document on top-level `---` rules (ignoring `---` inside code fences) into slides; navigate with ←/→/Space (or Tab), with a slide counter in the status bar. Previously the flag was accepted but did nothing.
- **Math and emoji.** `$inline$` and `$$block$$` math render in code style (kept literal — terminals can't typeset LaTeX); `:emoji:` shortcodes resolve to their glyph (`:rocket:` → 🚀).
- **Saner inline HTML.** `<br>` becomes a break and other inline tags (`<sub>`, `<sup>`, `<kbd>`, …) are dropped instead of silently swallowing their surrounding text.
- **Pager mode.** `ink --plain` on an interactive terminal now pages long output through `$PAGER` (default `less -R`), like `bat`. Piped/redirected output and `--no-pager` print directly.

### Changed
- **`ink diff` now uses a real Myers line diff** (via `similar`) instead of a positional line-by-line compare, so a single inserted or deleted line no longer marks everything after it as changed.

## 0.4.0 — 2026-07-21

### Added
- **Help overlay.** Press `?` in the viewer for a popup of the active keybindings.
- **Open links from the keyboard.** Press `f` to label every link on screen; press its letter to open web/mail links in your browser or follow a relative `.md` link in place. (Previously `Enter` only guessed the first visible link.)
- **Search result cycling.** After running a search and pressing Enter, `n`/`N` now cycle forward/backward through matches; the first `Esc`/`q` clears the highlights, the next exits.
- **`NO_COLOR` support** and automatic color downgrade: honors the `NO_COLOR` convention in `--plain`, and quantizes 24-bit colors to the 256-color palette on terminals that don't advertise truecolor.
- **Theme persistence.** Picking a theme in the theme picker (`T`, then Enter) now saves it to your config, preserving existing comments.
- **`ink completions <shell>`** generates bash/zsh/fish/PowerShell/elvish completions; **`ink man`** generates a man page; **`ink --list-themes`** lists available themes.
- **`mouse_capture` config option** (`[behavior]`) — set to `false` to let your terminal's own click-to-open links and text selection work instead of ink capturing the mouse.
- Friendly error messages (`ink: cannot read 'x.md'`) instead of raw OS errors; non-UTF-8 files render with replacement characters and a warning instead of failing.
- A "terminal too small" message instead of a broken layout on very small terminals.

## 0.3.0 — 2026-07-21

### Security
- **Terminal escape-sequence injection fixed.** Untrusted markdown could embed raw ANSI/OSC escape bytes (in text or code blocks) that reached the terminal verbatim — able to rewrite the window title, move the cursor, or spoof hyperlinks. This mattered most in `--plain`, which the docs promote as an fzf preview and git diff pager for arbitrary files. All rendered text is now stripped of control bytes (ESC, C0, C1, DEL) as a final layout pass covering both the TUI and `--plain` outputs.
- **OSC 8 hyperlink injection fixed.** Link destinations are validated: only `http`, `https`, `mailto`, and relative paths are emitted; `javascript:`, `file:`, `data:`, and any URL containing control bytes are dropped.
- **Remote images are now opt-in.** By default a remote image shows a placeholder; pass `--remote-images` to load them. Auto-fetching images from untrusted documents was an SSRF and tracking-pixel vector. When enabled, requests to loopback / private / link-local / cloud-metadata addresses are refused (re-checked on every redirect hop) and capped at 20 MB.
- **URL document fetches** now enforce a 10s timeout, connection timeout, redirect limit, and a 10 MB size cap (previously unbounded — a slow or huge URL could hang or OOM ink).
- **Path traversal fixed.** Relative image and link targets are resolved with symlink canonicalization and contained to the document's directory tree; absolute paths are rejected. Local image reads are size-capped.
- **Supply chain:** `install.sh` now verifies the downloaded binary against a published `SHA256SUMS` manifest before installing; the release workflow generates and uploads it. CI runs `cargo audit` on every push.

### Performance
- Syntax-highlighting assets (syntect syntax + theme sets) load once for the process instead of on every render rebuild — cuts a large constant off startup, window resize, theme switching, and `--watch` reloads.
- The reader is now event-driven: it redraws only when something changes. An idle document consumes effectively no CPU (previously it redrew ~20×/second).
- Large-document scrolling is now virtualized — only the on-screen lines are processed per frame, instead of cloning the entire document every frame. A 5,000-line document scrolls without lag.
- The theme is resolved once per frame rather than several times; the table of contents is built from exact heading positions recorded during layout (also fixing duplicate headings jumping to the wrong place); search caches lowercased line text; decoded images are cached; and only the visible tab rebuilds on resize/theme change.

### Internal
- Split into a library target (`ink_md`) plus a thin binary, enabling integration tests. Added CLI tests, `--plain` snapshot tests, layout/heading tests, adversarial security tests, and a virtualized-render test.

## 0.2.2 — 2026-06-22

### Fixed
- Inline text no longer loses the space before a styled span. `word **bold**` (and the same with code/links/italics) rendered as `wordbold` because the word-wrapper dropped a span's trailing space at the boundary. Spacing is now preserved across span boundaries.

## 0.2.1 — 2026-04-19

### Added
- Two-key chord bindings (e.g. `ctrl-x ctrl-c`). The Emacs preset now binds `Ctrl+X Ctrl+C` to `exit_app` alongside the existing single-key shortcuts.
- `ink config init` writes a commented starter config to `~/.config/ink/config.toml`. `--force` to overwrite an existing file.
- `ink config path` prints the resolved config file location.
- `[file missing]` indicator in the status bar when `--watch` detects the watched file has been deleted or renamed away. The doc keeps showing the last good content.

### Internal
- `build_keymap` now returns a `ResolvedKeymap { singles, chord_prefixes }` to support two-key chord dispatch.
- Input dispatcher tracks pending chord prefix; on miss it dispatches the new key normally so users aren't trapped after a stray prefix press.

## 0.2.0 — 2026-04-19

### Fixed
- `--watch` flag now actually watches the file and re-renders on change. Scroll position is preserved across rebuilds. Watches the parent directory so editors that save by replacing the inode (vim, IntelliJ) keep working. ([#2])

### Added
- Customizable keybindings via `~/.config/ink/config.toml` under `[keybindings]`. Pick a preset (`default`, `vim`, `emacs`) and override individual actions under `[keybindings.bindings]`. ([#1])
- Emacs preset: `Ctrl+N`/`Ctrl+P` for line nav, `Ctrl+V`/`Alt+V` for page nav, `Ctrl+A`/`Ctrl+E` for home/end, `Ctrl+S` for search.
- New `ink keybindings` subcommand prints the resolved key map.
- New `Shift+B` binding in the doc viewer reopens the file browser (only meaningful when ink was launched via the browser).

### Changed
- **BREAKING UX:** `q` / `Esc` now exits ink entirely, even when a file was opened via the file browser. Previously, closing a file looped back to the browser.
  - To restore the old behavior, set `browser_loop = true` under `[behavior]` in your config.
  - To return to the browser from inside a doc on demand, press `Shift+B`.

### Internal
- Added `notify = "8"` for cross-platform file watching (FSEvents on macOS, inotify on Linux, ReadDirectoryChangesW on Windows).
- `Action` enum split: `Quit` is now `ExitApp`, with new `CloseDoc` and `OpenBrowser` variants.
- `app::run` now returns an `AppExit` enum (`Quit` | `BackToBrowser`) so the browser loop can distinguish.

[#1]: https://github.com/borghei/ink/issues/1
[#2]: https://github.com/borghei/ink/issues/2

## 0.1.0 — 2026-04-17

Initial release.
