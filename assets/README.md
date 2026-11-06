# assets

Screenshots and the demo recording used by the READMEs and the landing page.

| File | What it shows |
|---|---|
| `demo.gif` | The `panels` layout, selection following into the inspector, tree view, then `L` to the `htop` layout |
| `demo-social.gif` | The ~8 s social cut for launch posts (Hacker News, Reddit, V2EX): inspector following the selection, `L` out and back, ending on `panels` |
| `screenshot-panels.png` | The default `panels` layout, with a process selected |
| `screenshot-htop.png` | The same machine under `L`, the classic full-width layout |
| `screenshot-tree.png` | `t`, process tree with the inspector pinned |
| `demo-config.toml` | The config the recording runs with |
| `demo.tape` | The [vhs](https://github.com/charmbracelet/vhs) script that produces all of the above |
| `demo-social.tape` | The vhs script behind `demo-social.gif`; same config and filter as `demo.tape` |

## Regenerating them

The captures are made with vhs so they are reproducible instead of being one
person's terminal. From the repository root, after `cargo build --release`:

```bash
brew install vhs ffmpeg pngquant
vhs assets/demo.tape
```

The tape copies `assets/demo-config.toml` into a temporary directory and isolates
search/filter history there, so recording cannot save over the example or personal
settings. CPU, Memory, and All tabs mirror the starter collection; Memory is
selected to keep the recording stable. The filter is entered interactively and
uses specific application paths rather than broad `node` or workspace matches,
which avoids capturing development-session commands.

`demo.tape` writes `demo-raw.gif` and the three PNGs into the working directory.
Copy the PNGs to `assets/` after inspecting them. The 300 ms waits after Screenshot
are intentional: VHS captures the next frame, and changing views immediately can
otherwise give the screenshot the following view's contents.

The raw GIF is a 1600×760 capture of a 13 px terminal font and comes out around
3.5 MB. It is resampled to 1200 px wide and a 48-colour palette, which is what
keeps the text legible at a size a README can load:

```bash
ffmpeg -i demo-raw.gif \
  -vf "fps=8,scale=1200:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=48[p];[b][p]paletteuse=dither=none" \
  -loop 0 assets/demo.gif
pngquant --quality=70-95 --speed 1 --force assets/screenshot-*.png
rm demo-raw.gif
```

Do not edit the PNGs or the GIF by hand; the tape and the commands above are the
source. `site/preview.sh` copies these files next to `site/index.html` for a local
look at the landing page — they are gitignored there, so this directory stays the
only copy.
