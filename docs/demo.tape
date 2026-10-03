# Hero recording for the README and the site. Run from the repo root with a
# v0.11+ `ink` on PATH:  vhs docs/demo.tape
# Screenshots (docs/assets/shot-*.png) use the same settings plus `Screenshot`.
Output docs/assets/demo.gif

Set Shell "bash"
Set FontFamily "Geist Mono"
Set FontSize 15
Set Width 1200
Set Height 720
Set Padding 20
Set Framerate 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms

Hide
Type "cd docs/assets && clear"
Enter
Sleep 500ms
Show

Sleep 400ms
Type "ink --theme catppuccin showcase.md"
Sleep 300ms
Enter
Sleep 2s

# Scroll
Down 6
Sleep 300ms
Down 6
Sleep 1s

# Table of contents
Type "t"
Sleep 1.8s
Type "t"
Sleep 600ms

# Search
Type "/"
Sleep 300ms
Type "rollout"
Sleep 400ms
Enter
Sleep 1.8s

# Theme picker, ending on `terminal`
Type "T"
Sleep 900ms
Down 5
Sleep 2s
Escape
Sleep 1s

Type "q"
Sleep 500ms
