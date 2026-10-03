# Themes and color

- [Built-in themes](#built-in-themes)
- [The terminal theme](#the-terminal-theme)
- [Automatic light or dark](#automatic-light-or-dark)
- [Color depth and NO_COLOR](#color-depth-and-no_color)
- [ASCII mode](#ascii-mode)
- [Custom themes](#custom-themes)

## Built-in themes

| Theme | Notes |
|---|---|
| `dark` | Default on dark terminals |
| `light` | Default on light terminals |
| `dracula` | |
| `catppuccin` | |
| `nord` | |
| `tokyo-night` | |
| `gruvbox` | |
| `solarized` | |
| `terminal` | Uses your terminal's own colors. See below. |

Press `T` in the reader to open the theme picker. Themes preview live as you move, and the one you pick is saved to your config.

```bash
ink --theme nord README.md   # one session
ink --list-themes            # built-ins plus your custom themes
```

## The terminal theme

`terminal` doesn't bring colors of its own. It draws with the 16 colors of your terminal's palette and its default foreground, sets no background, and highlights code with the same palette, like `bat --theme=ansi`.

So ink matches whatever scheme your terminal runs and looks right on light and dark backgrounds without switching themes. If you change your terminal scheme, ink follows.

```toml
theme = "terminal"
```

One catch: muted text (borders, link URLs, comments) uses "bright black". A few schemes, Solarized Dark among them, set bright black to the background color, which hides that text. Pick another theme there, or copy `terminal` into a [custom theme](#custom-themes) and change those keys.

## Automatic light or dark

With no theme set, ink asks the terminal for its background color through an OSC 11 query. Nearly every modern terminal answers, and tmux passes it through. ink picks `light` or `dark` to match. When the terminal doesn't answer it tries `COLORFGBG`, then falls back to `dark`.

An explicit `--theme` or `theme =` in the config always wins and skips the query.

`ink doctor` shows which theme was picked and why.

## Color depth and NO_COLOR

ink matches what your terminal can show:

| Terminal | Colors |
|---|---|
| Advertises truecolor (`COLORTERM`, Windows Terminal, VS Code, Ghostty, WezTerm, iTerm2) | 24-bit |
| `*-256color` | The 256-color palette |
| Linux console and other 16-color terminals (`TERM=linux`, `xterm`, `vt100`, `screen`) | The 16 ANSI colors, from your own palette |

With `--color=never` or `NO_COLOR` set, the reader draws without any color. Headings, links, search hits and selections use bold, underline and reverse video instead.

Plain output has its own rules. See [Color in plain output](cli.md#color-in-plain-output).

## ASCII mode

The Linux console, non-UTF-8 locales, legacy console fonts and screen readers turn box-drawing characters into junk. `--ascii`, or `ascii = true` under `[behavior]`, draws every border, bullet, task box, quote bar, rule and status-bar marker in 7-bit ASCII (`+--+`, `|`, `*`, `[x]`, `#`). It also turns off smart quotes and emoji shortcodes, so an ASCII document prints as pure ASCII.

It switches on by itself for `TERM=linux` and non-UTF-8 locales. `ascii = false` keeps Unicode regardless.

## Custom themes

Put a `.toml` file in the `themes/` folder next to your config file and use it by name. `ink config path` shows where the config lives. The theme folder is beside it, e.g. `~/.config/ink/themes/mytheme.toml`.

```bash
ink --theme mytheme README.md
```

### Color values

Every color is one of:

| Value | Meaning |
|---|---|
| `"#7aa2f7"` | A fixed 24-bit color, downgraded on terminals with fewer colors |
| `"ansi:blue"`, `"ansi:bright-black"` | One of the terminal's 16 palette colors. Names: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, each with a `bright-` form. |
| `"ansi:0"` to `"ansi:15"` | The same palette colors by number |
| `"default"` | The terminal's own foreground or background |

`code_theme` picks the syntax-highlighting theme for code blocks. Use `"ansi"` for the terminal palette, or any [syntect default theme](https://docs.rs/syntect/latest/syntect/highlighting/struct.ThemeSet.html#method.load_defaults) such as `"base16-ocean.dark"`, `"InspiredGitHub"` or `"Solarized (light)"`.

### Template

Every key below is required, except `bg`, `selection_bg` and `selection_fg`. Leave `bg` out to keep the terminal background.

```toml
name = "mytheme"
code_theme = "base16-ocean.dark"

[colors]
bg = "#1a1b26"
fg = "#c0caf5"
heading1 = "#7aa2f7"
heading2 = "#7dcfff"
heading3 = "#bb9af7"
heading4 = "#9ece6a"
heading5 = "#e0af68"
heading6 = "#f7768e"
bold = "#e6e8f0"
italic = "#c0caf5"
strikethrough = "#565f89"
code_fg = "#a9b1d6"
code_bg = "#24283b"
code_block_bg = "#24283b"
link = "#7aa2f7"
link_url = "#565f89"
blockquote_bar = "#565f89"
blockquote_text = "#a9b1d6"
list_bullet = "#7aa2f7"
list_number = "#7aa2f7"
table_border = "#3b4261"
table_header = "#7dcfff"
hr = "#3b4261"
task_done = "#9ece6a"
task_pending = "#565f89"
search_match = "#e0af68"
search_current = "#ff9e64"
selection_bg = "#33467c"
selection_fg = "#c0caf5"
status_bar_bg = "#16161e"
status_bar_fg = "#a9b1d6"
toc_active = "#7aa2f7"
toc_inactive = "#565f89"
admonition_note = "#7aa2f7"
admonition_warning = "#e0af68"
admonition_tip = "#9ece6a"
admonition_important = "#bb9af7"
admonition_caution = "#f7768e"
```

`search_match` and `search_current` color the text of matches, so they need good contrast against your background. Without `selection_bg`, selections fall back to `search_match`.

The built-in themes in [`src/theme/builtin.rs`](../src/theme/builtin.rs) are good starting points. `terminal()` there shows a theme made only of palette colors.

A theme file that fails to load prints a warning and falls back to `dark`.
