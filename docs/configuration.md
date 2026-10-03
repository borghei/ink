# Configuration

- [Where the config lives](#where-the-config-lives)
- [Settings](#settings)
- [Keybindings](#keybindings)
- [Custom keybindings](#custom-keybindings)

## Where the config lives

| Platform | Path |
|---|---|
| `XDG_CONFIG_HOME` set (any platform) | `$XDG_CONFIG_HOME/ink/config.toml` |
| Linux | `~/.config/ink/config.toml` |
| macOS | `~/Library/Application Support/ink/config.toml` |
| Windows | `%APPDATA%\ink\config.toml` |

If you already have a config in the platform folder and nothing under `$XDG_CONFIG_HOME/ink` yet, ink keeps using the existing one until you move it.

```bash
ink config path   # print the location in effect
ink config init   # write a commented starter config there
```

Command-line flags override the config for one run.

## Settings

Every setting is optional. These are the defaults:

```toml
# dark, light, dracula, catppuccin, nord, tokyo-night, gruvbox, solarized,
# terminal, or the name of a custom theme. Unset: match the terminal background.
# theme = "dark"

# Max text width in columns, 20 to 1000. Unset: the terminal width, up to 120.
# Plain output uses 80.
# width = 90

# Line spacing: compact, normal, relaxed
spacing = "normal"

# Open with the table of contents showing
toc = false

# Show YAML/TOML/JSON frontmatter as a box at the top
frontmatter = false

[behavior]
# true: quitting a document returns to the file browser instead of exiting
browser_loop = false

# false: leave the mouse to your terminal, so its own selection and link
# clicking keep working. You lose wheel-scroll inside ink.
mouse_capture = true

# How copied text reaches the clipboard:
#   auto   - OSC 52 escape (works over SSH and tmux) plus a native helper
#   osc52  - escape sequence only
#   native - pbcopy, wl-copy, xclip, xsel, clip.exe or termux-clipboard-set only
#   off    - copying is disabled
clipboard = "auto"

# Plain ASCII borders, bullets and markers. Unset: automatic on TERM=linux
# and non-UTF-8 locales.
# ascii = false

[keybindings]
preset = "default"
```

Themes have their own page: [Themes and color](themes.md).

`ink doctor` reports config problems, such as an unknown key or a bad value, without touching the file.

## Keybindings

The default preset is vim-flavored. Press `?` in the reader for this list, including your own changes, or run `ink keybindings` to print it.

| Key | Action |
|---|---|
| `j` `k`, `↓` `↑` | Scroll |
| `Alt+↓` `Alt+↑` | Scroll faster |
| `Space`, `Page Down`, `Ctrl+f`, `Ctrl+d` | Page down |
| `Page Up`, `Ctrl+b`, `Ctrl+u` | Page up |
| `Home` | Top |
| `G`, `End` | Bottom |
| `n` `N` | Next, previous heading. After a search, next and previous match. |
| `/` | Search |
| `t` | Toggle the table of contents |
| `o` | Move focus into the table of contents |
| `f` | Label links. Press a letter to open one, or `Y` then a letter to copy its URL. |
| `Enter` | Follow the first visible link |
| `[` `]`, `Alt+←` `Alt+→` | Back, forward |
| `v` `V` | Select by character, by line. `y` copies. |
| `c` | Copy a code block by letter |
| `Y` | Copy the current section as markdown |
| `e` | Open the file in `$VISUAL` or `$EDITOR` |
| `T` | Theme picker |
| `?` | Help |
| `Tab` `Shift+Tab` | Next, previous tab |
| `B` | Back to the file browser |
| `q`, `Esc`, `Ctrl+c` | Quit. The first press clears an active search. |

## Custom keybindings

Pick a preset, then override single actions on top of it:

```toml
[keybindings]
preset = "emacs"   # default, vim or emacs

[keybindings.bindings]
toggle_toc = ["ctrl-t"]
toc_focus = ["ctrl-o"]
edit = ["alt-e"]
exit_app = ["ctrl-x ctrl-c"]   # two-key chord: separate the keys with a space
```

The **emacs** preset adds `Ctrl+n` `Ctrl+p` to move by line, `Ctrl+v` `Alt+v` to page, `Ctrl+a` `Ctrl+e` for top and bottom, `Ctrl+s` to search, `Ctrl+f` for the next heading and the chord `Ctrl+x Ctrl+c` to quit. The arrow keys, `j` `k`, `/` and most single letters keep working.

Action names:

| Action | Default keys |
|---|---|
| `scroll_down`, `scroll_up` | `j` `down`, `k` `up` |
| `scroll_down_fast`, `scroll_up_fast` | `alt-down`, `alt-up` |
| `page_down` | `space` `pagedown` `ctrl-f` `ctrl-d` |
| `page_up` | `pageup` `ctrl-b` `ctrl-u` |
| `home`, `end` | `home`, `G` `end` |
| `next_heading`, `prev_heading` | `n`, `N` |
| `search` | `/` |
| `toggle_toc`, `toc_focus` | `t`, `o` |
| `link_mode`, `follow_link` | `f`, `enter` |
| `nav_back`, `nav_forward` | `[` `alt-left`, `]` `alt-right` |
| `select_mode`, `select_line_mode` | `v`, `V` |
| `copy_code`, `copy_section` | `c`, `Y` |
| `edit` | `e` |
| `theme_picker` | `T` |
| `help` | `?` |
| `next_tab`, `prev_tab` | `tab`, `backtab` |
| `open_browser` | `B` |
| `exit_app` | `q` `esc` `ctrl-c` |

Key names are lowercase, joined with `-`: `ctrl-t`, `alt-e`, `shift-v`, `shift-tab`, `pagedown`. A capital letter (`V`) is the same as `shift-v`.
