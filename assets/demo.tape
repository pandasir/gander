# Record the demo GIF and the README screenshots.
#
# Run from the repository root, after `cargo build --release`:
#
#     vhs assets/demo.tape
#
# Then post-process, because a raw 1600x760 capture is several megabytes. The
# full set of commands is in assets/README.md; the short version:
#
#     ffmpeg -i demo-raw.gif \
#       -vf "fps=8,scale=1200:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=48[p];[b][p]paletteuse=dither=none" \
#       -loop 0 assets/demo.gif
#     pngquant --quality=70-95 --speed 1 --force assets/screenshot-*.png
#
# The config lives in assets/demo-config.toml so the recording never depends on
# the machine's own ~/.config/gander/config.toml.

Output "demo-raw.gif"

Set Shell "bash"
Set FontSize 13
Set Width 1600
Set Height 760
Set Padding 12
Set Theme "Catppuccin Mocha"

Hide
Type "export GANDER_CONFIG=assets/demo-config.toml && ./target/release/gander -F 'pycharm|iTerm2|Docker|node|gander|postgres'"
Enter
Sleep 8s
Show
Sleep 1200ms
Down
Sleep 900ms
Down
Sleep 1600ms
Screenshot "screenshot-panels.png"
Type "t"
Sleep 2200ms
Screenshot "screenshot-tree.png"
Type "t"
Sleep 900ms
Type "L"
Sleep 2200ms
Screenshot "screenshot-htop.png"
Type "L"
Sleep 1200ms
Type "q"
Sleep 400ms
