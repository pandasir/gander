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
Set TypingSpeed 5ms
Set Framerate 10
Set Width 1600
Set Height 760
Set Padding 12
Set Theme "Catppuccin Mocha"

Hide
Type "demo_dir=$(mktemp -d) && cp assets/demo-config.toml $demo_dir/config.toml && GANDER_CONFIG=$demo_dir/config.toml XDG_STATE_HOME=$demo_dir/state ./target/release/gander"
Enter
Sleep 2s
Type '\'
Type "iTerm.app/Contents/MacOS/iTerm2|com.docker.backend|Virtualization.framework|Google Chrome.app/Contents/MacOS/Google Chrome|./target/release/gander"
Enter
Sleep 6s
Show
Sleep 1200ms
Down
Sleep 900ms
Down
Sleep 1600ms
Screenshot "screenshot-panels.png"
Sleep 300ms
Type "t"
Sleep 2200ms
Screenshot "screenshot-tree.png"
Sleep 300ms
Type "t"
Sleep 900ms
Type "L"
Sleep 2200ms
Screenshot "screenshot-htop.png"
Sleep 300ms
Type "L"
Sleep 1200ms
Type "q"
Sleep 400ms
Hide
Type "rm -rf $demo_dir"
Enter
