# Troubleshooting

- [Start with ink doctor](#start-with-ink-doctor)
- [Images](#images)
- [Colors look wrong](#colors-look-wrong)
- [Boxes and symbols show as junk](#boxes-and-symbols-show-as-junk)
- [Copying doesn't reach the clipboard](#copying-doesnt-reach-the-clipboard)
- [Image paths on Windows](#image-paths-on-windows)
- [Security model](#security-model)
- [Reporting a bug](#reporting-a-bug)

## Start with ink doctor

```bash
ink doctor
ink doctor --save ink-doctor.txt
```

Run it in the terminal that misbehaves. It reports:

- your platform and terminal
- the color depth and theme ink picked, and the signal that decided each
- the clipboard helper and OSC 52 wrapping it would use
- mouse capture
- config problems
- what the graphics negotiation chose
- decoder self-tests

It leaves out anything that identifies you: no hostnames, user names, IPs or session IDs, and your home folder shows as `~`. It's safe to paste into an issue.

## Images

Terminal graphics support varies a lot. If an image is blank, garbled or missing:

1. **Try the renderer that works everywhere:** `ink --image-protocol halfblocks file.md`. If the image shows up as coarse colored blocks, decoding is fine and the problem is your terminal's pixel protocol.
2. **Try a specific protocol:** `--image-protocol iterm2` (iTerm2, WezTerm, VS Code, Warp) or `--image-protocol sixel`. Some terminals advertise protocols they only half support. Auto-detection already works around the known cases, like iTerm2 claiming Kitty support, but new terminal versions bring new quirks.
3. **Check remote images:** `http` and `https` images only load with `--remote-images`.
4. **Report it:** open an [image rendering issue](https://github.com/borghei/ink/issues/new?template=image-rendering.yml) with the `ink doctor` output attached. It usually pins the problem down right away.

## Colors look wrong

- **Wrong light or dark theme:** your terminal may not answer the background query. Set the theme yourself with `--theme` or `theme =` in the config. `ink doctor` shows what was detected.
- **Washed-out or odd colors:** your terminal may not advertise truecolor, so ink uses the 256- or 16-color palette. Set `COLORTERM=truecolor` if your terminal supports it, or use the `terminal` theme, which only uses your palette.
- **No color at all:** check for `NO_COLOR` in your environment, or `--color=never`.
- **Escape codes in a pipe:** that's `--color=always`. Leave it at `auto` for files and scripts.

More in [Themes and color](themes.md).

## Boxes and symbols show as junk

The Linux console, non-UTF-8 locales and legacy fonts can't draw box characters. Use `--ascii`, or set `ascii = true` under `[behavior]`. It switches on by itself for `TERM=linux` and non-UTF-8 locales.

## Copying doesn't reach the clipboard

ink sends copies through OSC 52 and a native helper at once.

- **Over SSH or in tmux:** OSC 52 has to be allowed. In tmux, add `set -s set-clipboard on` to `.tmux.conf`. Some terminals ask before letting a program write the clipboard.
- **Locally on Linux:** install `wl-copy` (Wayland) or `xclip` or `xsel` (X11).
- **Nothing at all:** check that `clipboard` in your config isn't `off`.

`ink doctor` shows which route ink would use.

## Image paths on Windows

Markdown treats `\` as an escape character, so `C:\pics\.cache\x.png` loses its separators in any CommonMark renderer. Write image paths with forward slashes, `C:/pics/.cache/x.png`. They work everywhere, Windows included.

## Security model

ink treats every document as untrusted.

- Control bytes and escape sequences are stripped before anything reaches the terminal.
- Links only open with `http`, `https` or `mailto`.
- Remote images are off unless you pass `--remote-images`. Private, loopback and cloud-metadata addresses stay blocked even then.
- Local images, absolute paths included, are only ever shown as pixels. The bytes must decode as an image and are never printed as text, so a hostile document can't use an image tag to put a file's contents on screen.

Found a security problem? Please don't open a public issue. Reach [@borghei](https://github.com/borghei) privately first.

## Reporting a bug

Open an [issue](https://github.com/borghei/ink/issues/new/choose) with:

- the `ink doctor` output
- the smallest markdown that shows the problem
- what you expected and what you got (a screenshot helps)
