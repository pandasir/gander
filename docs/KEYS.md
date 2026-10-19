# Keybindings

Mostly identical to htop on purpose: a user trying gander should not have to
relearn anything. Column/key semantics are taken from htop's `htop.1` and
`Action.c`.

## Where gander differs from htop

Two of htop's existing bindings were repurposed, so an htop user pressing them gets
a different action than expected:

| Key | htop | gander |
|---|---|---|
| `I` | invert the sort order | cycle the selected header meter's display mode |
| `+` / `-` | expand / collapse a subtree (tree view only) | faster / slower sampling |

There is no invert-sort key any more; use `F6` / `<` `>` / `.` to reach the order
you want. `+`/`-` only clash inside tree view, where htop binds them and gander
does not expand or collapse at all.

Keys that are genuinely new (not in htop): `L`, and the header meter cursor moved
by `Shift+Left`/`Shift+Right`.

`H` is **not** new — it is htop's "hide user threads" — and it works on both
platforms.

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
| `Up` / `Down` | In the prompt: recall an earlier search or filter |
| `Esc` | Leave search/filter; while on the list, clears the filter |
| `F6` / `<` `>` / `.` | Cycle the sort column |
| `F5` / `t` | Toggle tree view |

Filter terms are **fixed strings** (no regex), case-insensitive, and any term
matching is enough. Separate terms with `|`.

Search and filter history is kept apart and persists across runs
(`$XDG_STATE_HOME/gander/`). A filter is applied as you type, so `Up` recalling one
takes effect immediately; `Esc` clears the filter and closes the prompt.

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
| `I` | Cycle the selected meter's mode: text → bar → graph (persisted per meter). **htop's `I` inverts the sort order** — see the table above |
| `K` | Toggle kernel threads |
| `H` | Toggle one row per thread |
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
