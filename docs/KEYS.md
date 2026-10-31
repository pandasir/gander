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
| `Left` / `Right` | Scroll the table horizontally, for a command line wider than the screen |
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
| `F6` / `<` `>` / `.` | Cycle the sort column (the list opens sorted by PID, ascending) |
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

`F9`, `F7` and `F8` all act on **every tagged process** when anything is tagged, and
on the selection otherwise. A status message on the function bar reports the result.
Selection follows the same row across samples, and expired process tags are removed.
The signal picker freezes its targets when opened; later samples cannot retarget it.
Signals, renice and panel changes verify the process start time before acting.
Linux uses pidfds for signals when supported; other operations retain a small
identity-check/system-call race. On macOS and FreeBSD, a thread row's actions
apply to its owning process.

### Info screens

These each open a scrollable panel; `Esc` closes it.

| Key | Action |
|---|---|
| `a` | CPU affinity panel (real mask on Linux and FreeBSD via `cpuset`; macOS explains that it has only a scheduler hint) |
| `i` | I/O priority panel (`ioprio_set` is a Linux syscall; the panel says so elsewhere) |
| `e` | Environment of the selected process |
| `w` | Full command line, wrapped |
| `l` | Open file descriptors |
| `x` | File locks |
| `Z` | SELinux security context (Linux; explains itself elsewhere) |
| `s` | Trace — **not implemented**; the panel gives the command and the actual `ptrace_scope` |
| `b` | Backtrace — **not implemented**, the panel names the obstacle |

`s` and `b` have no in-app implementation. They are bound on purpose: a key that
silently does nothing is indistinguishable from a broken program, so both open a
screen that names the obstacle and the workaround instead. `s` in particular used
to be documented here and in `DESIGN.md` with **no handler at all**, so pressing it
produced nothing whatsoever.

`s` does more than explain. Attaching a tracer is a terminal handover — `strace`
writes a live stream the alternate screen would swallow — so the panel prints the
command to run (`strace -p <pid>`, `sudo strace -f -p <pid>`) **and the reason the
attach would fail**: on Linux it reads `/sys/kernel/yama/ptrace_scope` and prints
the value, because the common `1` forbids tracing anything that is not a child, so
the attach fails for every pid on the machine. A generic "ptrace permission may be
needed" is what makes people conclude the tool is broken. macOS names SIP, FreeBSD
names `truss`.

## Display

| Key | Action |
|---|---|
| `L` | Switch layout: `panels` ⇄ `htop` (see [LAYOUT.md](LAYOUT.md)) |
| `Shift+Left` / `Shift+Right` | Move the header meter cursor (htop layout) |
| `F2` → Header | Change the CPU grid's cores-per-line (`LeftCPUs`, `LeftCPUs2`, `LeftCPUs4`, `LeftCPUs8`, and the right/all variants) |
| `I` | Cycle the selected meter's mode: text → bar → graph (persisted per meter). **htop's `I` inverts the sort order** — see the table above |
| `K` | Toggle kernel threads |
| `H` | Toggle one row per thread |
| `O` | Hide containerised processes (Linux; says so elsewhere) |
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
