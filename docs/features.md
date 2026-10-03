# Features

Everything ink does, grouped by what you're trying to do. For flags and config keys, see the [CLI reference](cli.md) and [Configuration](configuration.md).

- [Opening documents](#opening-documents)
- [Markdown rendering](#markdown-rendering)
- [Code blocks](#code-blocks)
- [Images](#images)
- [Mermaid diagrams](#mermaid-diagrams)
- [Math and emoji](#math-and-emoji)
- [Admonitions and callouts](#admonitions-and-callouts)
- [Frontmatter](#frontmatter)
- [Wikilinks](#wikilinks)
- [Moving around](#moving-around)
- [Search](#search)
- [Table of contents](#table-of-contents)
- [Links](#links)
- [Selecting and copying](#selecting-and-copying)
- [Editing](#editing)
- [Tabs and the file browser](#tabs-and-the-file-browser)
- [Watch mode](#watch-mode)
- [Presentation mode](#presentation-mode)
- [Outline, stats and diff](#outline-stats-and-diff)
- [How ink compares](#how-ink-compares)

## Opening documents

```bash
ink README.md                 # one file
ink README.md CHANGELOG.md    # several files, one tab each
ink .                         # browse every markdown file under a folder
ink                           # same as ink .
ink https://raw.githubusercontent.com/borghei/ink/main/README.md
cat notes.md | ink            # stdin (or name it with -)
```

When stdout isn't a terminal, `ink file.md` prints plain output instead of opening the reader, the way `bat` and `glow` do. More on that in [using ink as a pager](cli.md#using-ink-as-a-pager).

## Markdown rendering

Headings, emphasis, strikethrough, links, blockquotes, nested lists, task lists, tables, footnotes and horizontal rules all get proper styling.

A few details worth knowing:

- **Tables** follow their column alignment (`:--`, `:-:`, `--:`), with wide CJK and emoji cells measured correctly.
- **`<sup>` and `<sub>`** become real superscript and subscript when every character has a Unicode form: `x<sup>2</sup>` is `x²` and `H<sub>2</sub>O` is `H₂O`. With `--ascii` they read `x^(2)` and `H_(2)O`. Anything else is shown as written.
- **Strikethrough** works with one or two tildes, as on GitHub. Carets and double bars in prose (`2^10`, `a||b`) stay as typed, since `^x^` and `||x||` aren't GitHub markdown.
- **Definition lists** (`Term`, then `: definition`) get a bold term with the definitions indented underneath.

## Code blocks

Fenced code is syntax highlighted for every major language. Each block gets a border with the language label on top. Long lines wrap inside the box instead of breaking it.

Press `c` to copy a block's raw source. See [Selecting and copying](#selecting-and-copying).

## Images

Images render inside the document and scroll with it.

| Terminal | What you get |
|---|---|
| Kitty, iTerm2, WezTerm, Ghostty, anything with Sixel | Real pixels, picked automatically at startup |
| Any other true-color terminal | Unicode half-blocks |

Force a renderer with `--image-protocol auto|kitty|iterm2|sixel|halfblocks`, or turn images off with `--no-images`.

Supported formats: PNG, JPEG, GIF, WebP, BMP, TIFF, ICO, TGA, QOI, PNM, HDR, OpenEXR and farbfeld. SVG and gzipped `.svgz` are rasterized at load time, so vector diagrams render like any other image. An image that can't load shows a placeholder.

**Remote images are off by default.** A document you didn't write shouldn't be able to phone home or probe your network. Pass `--remote-images` to allow `http` and `https` images. Private, loopback and cloud-metadata addresses stay blocked even then.

Blank or garbled images? See [Troubleshooting](troubleshooting.md#images).

## Mermaid diagrams

` ```mermaid ` blocks are drawn as text that fits your terminal width. No external tools needed.

| Diagram | How it's drawn |
|---|---|
| Flowchart | Boxes and routed arrows in every direction (TD, BT, LR, RL), all node shapes and link styles, edge labels, subgraphs as frames |
| State, class, ER | The same box-and-arrow layout |
| Gantt | Bars on a scaled time axis |
| Mindmap | A tree |
| Sequence, pie | A compact text form |
| Anything else | The source, in a titled box |

Box-drawing characters by default, plain ASCII with `--ascii`.

## Math and emoji

Inline `$...$`, block `$$...$$` and ` ```math ` blocks render as Unicode text. `$x = \frac{-b \pm \sqrt{b^2-4ac}}{2a}$` reads:

```
x = (-b ± √(b² - 4ac))/2a
```

Greek letters, operators, `\mathbb{R}` (ℝ), superscripts, subscripts, fractions and roots are covered. Matrices and the `cases` and `aligned` environments draw across several lines with brackets. Anything ink doesn't know is shown as written.

A lone `$` in prose stays a dollar sign, so `costs $5 and $10` is safe. With `--ascii` you get the LaTeX source.

`:emoji:` shortcodes become the glyph: `:rocket:` is 🚀.

## Admonitions and callouts

GitHub's `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` and `[!CAUTION]` blocks get their own color and icon.

```markdown
> [!NOTE] Read this first
> A custom title replaces the type name.
```

Obsidian callouts (`> [!info]- Title`, `[!bug]`, `[!example]` and friends) map onto the same five colors. Fold markers are ignored.

## Frontmatter

YAML (`---`) and TOML (`+++`) frontmatter is hidden by default. With `--frontmatter`, or `frontmatter = true` in the config, it shows as a small key/value box at the top. Lists are comma-joined and nested values are shown as written.

A leading JSON object is never hidden. With `--frontmatter` it gets the same box when markdown follows it. Otherwise it's document text.

## Wikilinks

`[[page]]` and `[[page|display text]]` work out of the box, which makes ink a good way to read Obsidian vaults and personal wikis.

## Moving around

Vim-style keys, arrows and the usual paging keys all work. `n` and `N` jump between headings. Press `?` at any time for a popup of every active binding, your own overrides included.

The full list is in [Configuration: keybindings](configuration.md#keybindings).

## Search

Press `/` and type. Matches highlight as you go. `Enter` locks the search in, `n` and `N` cycle through results, and `Esc` clears them.

## Table of contents

`t` toggles a sidebar with every heading. It follows your position as you scroll.

`o` moves focus into the sidebar, opening it if needed:

| Key | Action |
|---|---|
| `j` `k`, arrows | Move |
| `g` `G` | First, last heading |
| `Ctrl+d` `Ctrl+u` | Page |
| `Enter` | Jump to the heading (`[` brings you back) |
| `h` `l` | Fold, unfold a section |
| `/` | Filter headings. Every word you type must appear, case ignored. |
| `Esc` | Clear the filter, then return to the document |

With the mouse, click a heading to jump to it. The wheel over the sidebar scrolls the sidebar, not the page.

## Links

Press `f` and every link on screen gets a letter. The popup lists each link's text next to its URL so you can tell them apart. Press the letter to:

- open a web or mail link in your browser
- follow a relative `.md` link or a `#heading` anchor inside ink (`[` goes back, `]` forward)

Press `Y` while the labels are up to copy a link's URL instead.

Clicking works too. A plain click on a link does what its letter would, and dragging across a link still selects text. With `--no-mouse`, clicks go to your terminal, which opens links its own way (usually Cmd-click or Ctrl-click).

Only `http`, `https` and `mailto` links are ever opened.

## Selecting and copying

ink captures the mouse so the wheel scrolls the document. It makes up for taking over your terminal's selection by doing a better job of it: it knows where a code block starts and where its own decoration ends.

| Key | What it copies |
|---|---|
| `v`, `V` | Start a character or line selection. Move with `h j k l`, arrows, `w` `b`, `0` `$`, `g` `G`, `Ctrl+d` `Ctrl+u`. `y` copies, `Esc` cancels. |
| Mouse drag | Selects. Release to copy. Double-click takes a word, triple-click a line. |
| `c` | Labels every code block on screen. Press a letter to copy that block's raw source, with no borders or highlighting. |
| `Y` | The markdown source of the section you're reading, from its heading down to the next heading of the same level |

`v` and `V` copy what you see, borders and heading bars included. Use `c` and `Y` when you want the source.

Copied text goes out two ways at once: an OSC 52 escape sequence, which works over SSH and inside tmux, and a native helper (`pbcopy`, `wl-copy`, `xclip`, `xsel`, `clip.exe`, `termux-clipboard-set`) when one is installed. Nothing to set up. The `clipboard` setting narrows this down or turns it off.

Prefer your terminal's own selection and link clicking? Run `ink --no-mouse` for one session, or set `mouse_capture = false` for good.

## Editing

Press `e` to open the file in `$VISUAL` or `$EDITOR`, at the section you're reading. ink reloads it when the editor exits. On Windows, editors installed as `.cmd` shims (VS Code, Cursor) are found.

## Tabs and the file browser

Open several files and each gets a tab. `Tab` and `Shift+Tab` switch between them.

Run `ink` with no arguments, or point it at a folder, and you get a file picker listing every `.md` file, with filtering and keyboard navigation. Opening a file from the browser exits back to the shell when you quit. `Shift+B` returns to the browser, or set `browser_loop = true` to come back every time.

## Watch mode

```bash
ink --watch draft.md
```

The view re-renders within about 100 ms of a save and keeps your scroll position. It works with editors that save by replacing the file (vim, IntelliJ), because ink watches the folder rather than the file handle.

## Presentation mode

```bash
ink --slides deck.md
```

Slides split on `---`. Move with the arrow keys or `Space`.

## Outline, stats and diff

```bash
ink outline README.md     # heading structure
ink stats README.md       # word count, reading time, element counts
ink diff old.md new.md    # diff two markdown files
```

## How ink compares

| | ink | glow | mdcat | frogmouth |
|---|---|---|---|---|
| Interactive reader | Yes | Yes | No | Yes |
| Tabs | Yes | No | No | No |
| Search in a document | Yes | No | No | No |
| Table of contents | Yes | No | No | Yes |
| Images, graphics protocols | Yes | No | Yes | No |
| Images, half-block fallback | Yes | No | No | No |
| Mermaid diagrams | Yes | No | No | No |
| Math and emoji | Yes | No | No | No |
| Admonitions | Yes | No | No | No |
| Wikilinks | Yes | No | No | No |
| File browser | Yes | Yes | No | Yes |
| Watch mode | Yes | No | No | No |
| Presentation mode | Yes | No | No | No |
| Pager mode | Yes | Yes | No | No |
| Completions and man page | Yes | Yes | Yes | No |
| Built-in themes | 9 | 2 | 0 | 0 |
| Single binary | Yes | Yes | Yes | No |
