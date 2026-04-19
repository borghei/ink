# Changelog

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
