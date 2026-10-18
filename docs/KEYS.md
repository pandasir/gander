# Keybindings

Identical to htop on purpose: a user trying gander should not have to relearn
anything. Column/key semantics are taken from htop's `htop.1` and `Action.c`.

## Navigation

| Key | Action |
|---|---|
| `Up` / `Down` | Move the selection |
| `PgUp` / `PgDn` | Page scroll |
| `Home` / `End`, `g` / `G` | Jump to first / last row |
| `Left` / `Right` | Scroll the table horizontally |
| `Tab` / `Shift-Tab` | Switch screen tab |
| `Space` | Tag / untag the selected process |
| `c` | Tag the selected process and its children |
| `U` | Untag everything |

## Finding processes

| Key | Action |
|---|---|
| `F3` / `/` | Incremental search; wraps around |
| `F4` / `\` | Incremental filter, applied while typing |
| `Esc` | Leave search/filter; while on the list, clears the filter |
| `F6` / `<` `>` / `.` | Cycle the sort column |
| `F5` / `t` | Toggle tree view |

Filter terms are **fixed strings** (no regex), case-insensitive, and any term
matching is enough. Separate terms with `|`.

## Acting on processes

| Key | Action |
|---|---|
| `F9` / `k` | Open the signal picker |
| `F7` / `F8` | Lower / raise nice (renice) |
| `a` | CPU affinity panel (real mask on Linux) |
| `i` | I/O priority panel (Linux only) |
| `e` | Environment of the selected process |
| `w` | Full command line, wrapped |
| `l` | Open file descriptors |
| `x` | File locks |
| `s` | Attach strace *(M2)* |
| `b` | Backtrace *(M3)* |

## Display

| Key | Action |
|---|---|
| `L` | Switch layout: `panels` ⇄ `htop` (see [LAYOUT.md](LAYOUT.md)) |
| `Shift+Left` / `Shift+Right` | Move the header meter cursor (htop layout) |
| `I` | Cycle the selected meter's mode: text → bar → graph (persisted per meter; htop layout) |
| `K` | Toggle kernel threads |
| `H` | Toggle one row per thread (Linux) |
| `H` | Hide kernel threads |
| `+` / `-` | Faster / slower sampling (100 ms … 10 s) |
| `F2` | Setup screen: Header, Columns, Tabs, Display, Colors |
| `F1` / `h` / `?` | Help |
| `F10` / `q` | Quit; configuration is saved on clean exit only |
| `Ctrl-C` | Quit immediately |

## Batch mode (no TUI)

```sh
gander --json                     # one sample, JSON
gander -n 5 -d 1000 --filter rust # five text samples, one per second
gander -n 1 --sort PERCENT_MEM --limit 10
```

`--json` implies `-n 1`. Field names accepted by `--sort` are the same ones used
in the config file (`PERCENT_CPU`, `PERCENT_MEM`, `RES`, `TIME`, ...).
