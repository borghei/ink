# Changelog

## Unreleased

### Fixed
- **`.svgz` gets `currentColor` theming too** — the gzip layer is now decompressed (with a decompression-bomb cap) before the theme color is injected, so gzipped monochrome icons follow the theme like plain `.svg`.
- **A broken or unknown `--theme` warns on stderr** ("falling back to 'dark'") instead of silently ignoring the request.
- **Percent-encoded binary `data:` URIs decode** — the payload decoder is byte-level now, so a percent-encoded PNG works, not just UTF-8 SVG text.

### Docs
- README documents the security model (untrusted-input handling, why local image paths are safe to read, remote images opt-in) and warns Windows authors that markdown eats `\.` in paths like `C:\pics\.cache\x.png` — use forward slashes.

## 0.6.5 — 2026-07-26

A systematic bug hunt across the whole codebase: five parallel audits (Unicode/width, hand-rolled scanners, image pipeline, app state, CLI/plain) followed by fixes for everything found. 30+ fixes; every one carries a regression test.

### Fixed — crashes and data loss
- **`ink --plain … | head` no longer panics on the closed pipe.** It exits 0 quietly — fzf previews and `git` textconv (the workflows the README recommends) got a broken-pipe panic and exit 101 before.
- **A document starting with a `---` thematic break no longer loses its first section.** The frontmatter stripper matched any later `---` (even mid-`-----`); it now requires a real `---` opener, a `---`/`...` closer on its own line, and at least one YAML-ish `key:` line in between.
- **Stale scroll offsets can no longer blank the viewport or crash link-mode.** After any re-layout (resize, nav-back into a shrunk file) the restored offset is clamped; previously widening the window after `G` left an out-of-range offset that blanked the screen, made upward scrolling appear dead, and made `f` panic on an inverted slice.
- **Documents beyond 65,535 rendered lines are fully reachable.** Scroll state was `u16` and wrapped: `G` landed ~6% in, the rest was unreachable, and a watch reload yanked the reader to the wrong position. All line arithmetic is `usize` now.

### Fixed — rendering
- **Emoji and ZWJ clusters no longer blow the line width.** Width accounting was per-`char` while rendering is per-cluster: a row of `⚠️` rendered 76 columns into a 40-column budget, and code-block borders jutted past the box. Wrapping, code blocks, and table labels now measure grapheme clusters (`unicode-segmentation`), and clusters are never split.
- **Tab characters in code blocks.** Tabs expand at 4-column stops; previously `--plain` counted them 1 wide (blown borders) and the TUI dropped them entirely (tab-indented Go/Makefile code lost all indentation).
- **Transposed-table labels** with emoji fit their column exactly instead of overflowing by up to 14 columns; table cell hard-breaking had the same per-char bug.
- **Mermaid box borders** are sized by display width, not byte length (CJK titles skewed the top border).
- **Bare-URL trailing punctuation** stays in the text ("see https://x.test/foo." kept its period as prose instead of deleting it).
- **Transparent image regions no longer render as black boxes.** Half-block cells are opaque, so transparency is composited over the theme background; on light themes every transparent SVG/logo sat in a black rectangle.
- **SVG `currentColor` follows the theme.** Monochrome icons resolved to SVG's black default — invisible on dark terminals. An SVG that sets its own `color` keeps it; theme switches re-rasterize.
- **Graphics-mode images are no longer squeezed** by subtracting the centering margin twice — `--width narrow` on a wide terminal rendered screenshots as a 1×1-cell dot.
- **A failed graphics encode shows a notice** in the reserved rows instead of a silent 30-row blank hole.

### Fixed — scanners
- **Wikilink fence tracking no longer inverts.** A ``` shown inside a `~~~` block flipped the scanner's state: it rewrote wikilinks *inside* code samples and skipped real ones after. Fences now track their char and length per CommonMark. Double-backtick code spans (` ``…`` `) are also respected, and output no longer gains a trailing newline.
- **HTML attribute scanning is quote-aware.** `<img alt="pass src=here …" src="diagram.png">` loaded `here`; a `>` inside a quoted value truncated the tag (dropping the image entirely) and could corrupt SVG `currentColor` injection. A real per-character attribute scanner replaces the substring matching.
- **`--slides` no longer shows YAML frontmatter as slide 1**, and a setext `---` heading underline no longer splits the deck mid-heading.
- **`--slides --watch`: saving the file re-splits the deck.** Previously the whole document replaced the current slide and the deck never regained its shape.

### Fixed — behavior
- **Opening the TOC re-lays out the document** for the narrower area instead of truncating ~30 columns off every line until the next resize.
- **Search state stays consistent.** Matches recompute on tab switches and re-layouts (highlights previously pointed at another tab's line numbers; the first `q` got swallowed by a phantom search). Match counting and highlighting now use the same per-span scan and the same lowercasing, so `[1/1]` always corresponds to something visibly highlighted.
- **Resize is debounced (80 ms).** Dragging the terminal edge fired a full re-layout — including a deep copy and re-encode of every graphics image — per event.
- **The image cache checks the cache before doing I/O.** Every re-layout re-read every image file from disk, and with `--remote-images` re-downloaded every remote image synchronously per resize tick. Failures are cached too (no more repeating 10s network stalls), and the cache is bounded (64 entries) instead of growing without limit across theme switches and watch reloads.
- **`ink --plain a.md b.md` renders every file**, not silently just the first.
- **`ink --plain docs/` fails with a clear error** instead of launching the interactive browser into the pipe.
- **Config keys `spacing`, `toc`, and `frontmatter` now apply.** They were parsed — and written by `ink config init` — but never read.
- **CJK-aware word counts.** A Chinese document reported "Words: 1, ~1 min"; Han and kana characters now count individually.
- `sanitize_url` no longer lets a leading-space `" javascript:…"` URL through as scheme-less.

## 0.6.4 — 2026-07-26

Packaging only — the renderer is byte-for-byte 0.6.3.

### Packaging ([#4](https://github.com/borghei/ink/issues/4))
- **`.deb` and `.rpm` packages** (amd64 + arm64) are now built and attached to every release, covered by `SHA256SUMS` — `sudo apt install ./ink-md_*.deb` / `sudo dnf install ./ink-md-*.rpm`. They install `ink` to `/usr/bin` and uninstall cleanly.
- **crates.io is published automatically on release** (it had been stale at 0.2.1 while binaries reached 0.6.x), so `cargo install ink-md` and `mise use cargo:ink-md` track releases again.
- **`cargo binstall ink-md`** now fetches the prebuilt binary instead of compiling.
- **`install.sh` accepts `INK_INSTALL_DIR`** — set `INK_INSTALL_DIR="$HOME/.local/bin"` to install without `sudo`.
- README documents all install paths, including `mise` (`cargo:` and `ubi:` backends).

## 0.6.3 — 2026-07-26

Also carries the 0.6.2 robustness fixes below: 0.6.2 was tagged in the source
tree but never published, so this is the first build to ship them.

### Fixed
- **Images referenced by absolute path now render** ([#3](https://github.com/borghei/ink/issues/3) follow-up). The 0.6.1 SVG fix never got a chance to run for the reported document: `![](/tmp/sample.svg)` is an absolute path, and the image loader rejected every absolute path (and every relative path escaping the document's directory) before reading a single byte — which is also why a PNG referenced the same way didn't show. Local images now load from any readable path, like kitty's `icat`. This is display-only and safe: image bytes must decode and only ever reach the screen as pixels, never as text, and remote fetching stays opt-in via `--remote-images`.
- **The same root cause, fixed everywhere it appeared:**
  - Percent-encoded destinations resolve: `![](my%20pic.png)` finds `my pic.png` (the encoding Obsidian/Notion exports and standard markdown produce). Applies to images and followed links; a file literally named with `%` still wins.
  - `file://` image URLs load as local paths instead of silently failing.
  - Following a `.md` link works for absolute and out-of-tree paths, matching the image policy.
- **Failed images now say why.** A missing file shows `🖼 … (image not found)`, an undecodable one `(cannot decode image)`, instead of a bare placeholder indistinguishable from a rendering bug — the reason #3 took two rounds to diagnose.

### Added
- **Linked images render.** `[![alt](shot.png)](https://…)` — the badge/linked-screenshot pattern — now renders the image as a block; the caption carries the link so hint-mode and open still reach it.
- **Image galleries render.** A paragraph holding several images in a row renders each one, instead of degrading all of them to inline placeholders.
- **Raw HTML `<img>` renders.** `<p align="center"><img src="logo.png" …></p>` — the README way to size/center a logo — now shows the image (alt text as caption) instead of dim raw markup. A mid-sentence inline `<img>` shows the standard `🖼` placeholder instead of vanishing.
- **`data:` URI images render.** Base64 or percent-encoded payloads, as produced by notebook and HTML exports.
- **`~/pics/x.png` resolves** to the home directory, for images and followed links.
- **`pic.png?raw=true` / `pic.png#gh-light-mode-only` resolve.** GitHub-habit query/fragment suffixes are stripped as a fallback; a file literally named with the suffix still wins.
- **EXIF orientation is honored.** Phone photos no longer render sideways.
- **SVGs with embedded raster images render** (resvg's `raster-images` feature was off, leaving `<image href="…">` elements blank; relative hrefs now resolve against the SVG's own directory).

## 0.6.2 — 2026-07-24

### Fixed (robustness)
- **Searching no longer crashes on text whose case mapping changes length.** Match highlighting located matches in a lowercased copy of each line but applied those byte offsets to the original text. Unicode lowercasing isn't length-preserving — `İ` (U+0130) is two bytes and lowercases to three — so the offsets drifted and could land mid-character. Searching `i` in a document containing `İstanbul` aborted the process. Because release builds use `panic = "abort"`, the terminal-restore path was skipped too, leaving the shell in raw mode inside the alternate screen. Offsets are now mapped back to the source string explicitly.
- **A panic can no longer leave the terminal unusable.** Raw mode and the alternate screen are now torn down from a panic hook before the message is printed, so an unexpected failure produces a readable error instead of a wrecked terminal. Applies to both the document viewer and the file browser.
- **Malformed theme colors fall back instead of panicking.** The hex-color guard measured byte length, so a six-byte string of multi-byte characters (`€€`) passed the check and then split a character while slicing. Non-ASCII color values now fall back to the default gray.

## 0.6.1 — 2026-07-24

### Fixed
- **SVG images now render** ([#3](https://github.com/borghei/ink/issues/3)). SVGs are vector documents the raster decoder can't read, so they silently fell back to a placeholder. They're now rasterized with resvg (SVG text and gzipped `.svgz` included) and flow through the same pipeline as every other image — graphics protocols and half-blocks alike. SVGs are detected by extension or content sniffing, so an SVG served without an extension works too.

### Added
- **More raster formats.** Inline images now also decode TIFF, ICO, TGA, QOI, PNM (PBM/PGM/PPM/PAM), Radiance HDR, OpenEXR, and farbfeld, alongside the existing PNG, JPEG, GIF, WebP, and BMP.

## 0.6.0 — 2026-07-21

### Added
- **True inline images via terminal graphics protocols.** On terminals that support them, images now render as real pixels using the Kitty graphics protocol, iTerm2 inline images, or Sixel — auto-detected at startup. Terminals without a graphics protocol keep the universal Unicode half-block rendering, so nothing changes there.
  - Images scroll with the document and clip cleanly at the viewport edges.
  - New `--image-protocol <auto|kitty|iterm2|sixel|halfblocks>` flag (default `auto`). Force `halfblocks` for the previous behavior everywhere, or pin a specific protocol.
  - Detection is skipped for `--plain`, `--no-images`, and when `halfblocks` is forced; it never runs off a TTY and falls back safely on any non-graphics terminal.

## 0.5.2 — 2026-07-21

### CI
- The release workflow's Homebrew tap bump is now non-blocking: if the tap token is missing or expired, the bump fails on its own but no longer marks the whole release run as failed (the binaries and checksums have already published). No user-facing changes to `ink` itself.

## 0.5.1 — 2026-07-21

### Fixed (rendering)
- **Nothing overflows the render width anymore.** Long code lines, long headings, and long unbreakable tokens (URLs, long identifiers) now wrap or hard-break to fit instead of spilling past the edge (and, in the TUI, being silently clipped). Every block type is verified to stay within the width from 40 columns up.
- **Code blocks are now a proper closed box.** Long lines wrap inside the frame and a right border is drawn on every row (previously the box had top and bottom borders but no right side, and long lines shot past it). Code blocks also match the paragraph width instead of being capped at 80.
- **The left margin is now reserved from the width budget.** `ink --plain --width N` produces exactly N columns (was N+2), and an explicit `--width` wider than the terminal no longer overflows.
- **Inline code spacing.** `` `code` `` no longer renders with a doubled leading space and a stray space before following punctuation.
- **Footnote definitions** render the label and text on one line (`[^1]: text`) instead of splitting them across two lines.
- Removed stray blank/indented lines after nested list items and the trailing empty bar line after blockquotes and admonitions; consecutive admonitions and a following block now have proper spacing.
- Raw HTML blocks (e.g. centered `<img>` headers) wrap to the width instead of overflowing.

### Added
- **Responsive tables.** A table too wide to fit the terminal — even after shrinking columns — now falls back to a stacked key/value layout (like `psql -x`): each row becomes a record of `Header  value` lines with wrapped values, separated by a thin rule. This always fits any width and stays readable, instead of a grid that overflows and gets clipped. Tables that fit still render as the usual bordered grid.

### Internal
- `--plain` output coalesces adjacent same-styled spans, so multi-word runs stay contiguous (greppable) and emit far fewer escape sequences.
- Added a width-compliance regression test asserting no laid-out line exceeds the target width across widths 40–100.

## 0.5.0 — 2026-07-21

A large release focused on security hardening, a performance overhaul, and closing the feature gap with other terminal markdown tools.

### Security
- **Terminal escape-sequence injection fixed.** Untrusted markdown could embed raw ANSI/OSC escape bytes (in text or code blocks) that reached the terminal verbatim — able to rewrite the window title, move the cursor, or spoof hyperlinks. This mattered most in `--plain`, which the docs promote as an fzf preview and git diff pager for arbitrary files. All rendered text is now stripped of control bytes (ESC, C0, C1, DEL) as a final layout pass covering both the TUI and `--plain` outputs.
- **OSC 8 hyperlink injection fixed.** Link destinations are validated: only `http`, `https`, `mailto`, and relative paths are emitted; `javascript:`, `file:`, `data:`, and any URL containing control bytes are dropped.
- **Remote images are now opt-in.** By default a remote image shows a placeholder; pass `--remote-images` to load them. Auto-fetching images from untrusted documents was an SSRF and tracking-pixel vector. When enabled, requests to loopback / private / link-local / cloud-metadata addresses are refused (re-checked on every redirect hop) and capped at 20 MB.
- **URL document fetches** now enforce a 10s timeout, connection timeout, redirect limit, and a 10 MB size cap (previously unbounded — a slow or huge URL could hang or OOM ink).
- **Path traversal fixed.** Relative image and link targets are resolved with symlink canonicalization and contained to the document's directory tree; absolute paths are rejected. Local image reads are size-capped.
- **Supply chain:** `install.sh` now verifies the downloaded binary against a published `SHA256SUMS` manifest before installing; the release workflow generates and uploads it. CI runs `cargo audit` on every push, and all advisory-affected transitive dependencies were updated to patched versions.

### Performance
- Syntax-highlighting assets (syntect syntax + theme sets) load once for the process instead of on every render rebuild — cuts a large constant off startup, window resize, theme switching, and `--watch` reloads.
- The reader is now event-driven: it redraws only when something changes. An idle document consumes effectively no CPU (previously it redrew ~20×/second).
- Large-document scrolling is now virtualized — only the on-screen lines are processed per frame, instead of cloning the entire document every frame. A 5,000-line document scrolls without lag.
- The theme is resolved once per frame rather than several times; the table of contents is built from exact heading positions recorded during layout (also fixing duplicate headings jumping to the wrong place); search caches lowercased line text; decoded images are cached; and only the visible tab rebuilds on resize/theme change.

### Added
- **Presentation mode (`--slides`) is now implemented.** Splits the document on top-level `---` rules (ignoring `---` inside code fences) into slides; navigate with ←/→/Space, with a slide counter in the status bar. Previously the flag was accepted but did nothing.
- **Help overlay.** Press `?` in the viewer for a popup of the active keybindings.
- **Open links from the keyboard.** Press `f` to label every link on screen; press its letter to open web/mail links in your browser or follow a relative `.md` link in place. (Previously `Enter` only guessed the first visible link.)
- **Search result cycling.** After running a search and pressing Enter, `n`/`N` cycle forward/backward through matches; the first `Esc`/`q` clears the highlights, the next exits.
- **Math and emoji.** `$inline$` and `$$block$$` math render in code style (kept literal — terminals can't typeset LaTeX); `:emoji:` shortcodes resolve to their glyph (`:rocket:` → 🚀). `<br>` and other inline HTML tags are handled instead of swallowing surrounding text.
- **Pager mode.** `ink --plain` on an interactive terminal pages long output through `$PAGER` (default `less -R`), like `bat`. Piped/redirected output and `--no-pager` print directly.
- **`NO_COLOR` support** and automatic color downgrade: honors the `NO_COLOR` convention in `--plain`, and quantizes 24-bit colors to the 256-color palette on terminals that don't advertise truecolor.
- **Theme persistence.** Picking a theme in the theme picker (`T`) now saves it to your config, preserving existing comments.
- **New subcommands and flags:** `ink completions <shell>` (bash/zsh/fish/PowerShell/elvish), `ink man` (man page), `ink --list-themes`, and `--no-pager`.
- **`mouse_capture` config option** (`[behavior]`) — set to `false` to let your terminal's own click-to-open links and text selection work instead of ink capturing the mouse.
- Friendly error messages (`ink: cannot read 'x.md'`) instead of raw OS errors; non-UTF-8 files render with replacement characters and a warning instead of failing; a "terminal too small" message instead of a broken layout on tiny terminals.

### Changed
- **`ink diff` now uses a real Myers line diff** (via `similar`) instead of a positional line-by-line compare, so a single inserted or deleted line no longer marks everything after it as changed.

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
