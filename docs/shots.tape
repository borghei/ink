# Screenshots for the site that aren't in demo.tape. Run from the repo root
# after `cargo build --release`:  vhs docs/shots.tape
# Same settings as demo.tape so every shot on the page matches.
Output target/shots.gif

Set Shell "bash"
Set FontFamily "Geist Mono"
Set FontSize 15
Set Width 1200
Set Height 720
Set Padding 20
Set Framerate 20
Set Theme "Catppuccin Mocha"

Hide
Type "export PATH=$PWD/target/release:$PATH && cd docs/assets && clear"
Enter
Sleep 500ms
Type "ink --theme catppuccin diagrams.md"
Enter
Sleep 2s
Show

Sleep 500ms
Screenshot docs/assets/shot-diagrams.png
Sleep 200ms
Hide
Type "q"
