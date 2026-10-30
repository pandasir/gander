# The social cut of the demo: a ~10 s loop for launch posts (Hacker News,
# Reddit, V2EX), where the reader decides in the first seconds whether to keep
# watching. The full recording in demo.tape tells the whole story and stays the
# README's demo; this one keeps only the two moments that differentiate gander —
# the inspector following the selection, and `L` flipping to the classic htop
# layout — and ends back on `panels`, because that is the layout to remember.
#
# Run from the repository root, after `cargo build --release`:
#
#     vhs assets/demo-social.tape
#
# Then compress, same recipe as demo.gif but 10 fps so the loop stays tight:
#
#     ffmpeg -i demo-social-raw.gif \
#       -vf "fps=10,scale=1200:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=48[p];[b][p]paletteuse=dither=none" \
#       -loop 0 assets/demo-social.gif
#     rm demo-social-raw.gif

Output "demo-social-raw.gif"

Set Shell "bash"
Set FontSize 14
Set Width 1600
Set Height 760
Set Padding 12
Set Theme "Catppuccin Mocha"

Hide
Type "export GANDER_CONFIG=assets/demo-config.toml && ./target/release/gander -F 'pycharm|iTerm2|Docker|node|gander|postgres'"
Enter
Sleep 8s
Show
Sleep 800ms
Down
Sleep 700ms
Down
Sleep 1400ms
Type "t"
Sleep 1800ms
Type "t"
Sleep 700ms
Type "L"
Sleep 1800ms
Type "L"
Sleep 900ms
