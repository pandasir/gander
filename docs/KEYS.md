# Keybindings

Mostly identical to htop on purpose: a user trying gander should not have to
relearn anything. Column/key semantics are taken from htop's `htop.1` and
`Action.c`.

## Compatibility and Defaults

`I` reverses sort order and tree `+/-` expand/collapse, matching htop's familiar
operations. `m` cycles the selected header meter. `[/]` change sampling speed in
either list mode; outside tree mode, `+/-` remain speed aliases.

Fresh configurations open the CPU tab (CPU descending), with Memory and All
(PID ascending) also available. Existing and imported tab collections are kept.
Selection follows the same live process across sorting, tabs, and sampling.

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
| `F6` | Open sort picker; Enter commits, Esc cancels |
| `<` / `>` / `.` | Previous / next sort field |
| `I` | Reverse the current sort direction |
| Header click | Choose the field, or reverse it when already active |
| `u` | Searchable user picker, including All users |
| `F5` / `t` | Toggle tree view |
| Tree `+` / `-` | Expand / collapse the selected subtree |
| Tree `*` | Collapse all expanded nodes, otherwise expand all |

Filter terms are **fixed strings** (no regex), case-insensitive, and any term
matching is enough. Separate terms with `|`.

Search and filter history is kept apart and persists across runs
(`$XDG_STATE_HOME/gander/`). A filter is applied as you type, so `Up` recalling one
takes effect immediately; `Esc` clears the filter and closes the prompt.
Applied text and owner filters remain visible after confirmation, with matching
and eligible counts. Eligible rows respect thread/container visibility, but counts
are independent of tree folding. A no-match result has an explicit empty state.

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
| `m` | Cycle the selected meter's mode: text, bar, graph (persisted per meter) |
| `B` / `V` | Toggle system sidebar / inspector in panels layout |
| `K` | Toggle kernel threads |
| `H` | Toggle one row per thread |
| `O` | Hide containerised processes (Linux; says so elsewhere) |
| `[` / `]` | Slower / faster sampling (100 ms to 10 s); list `-` / `+` are aliases |
| `F2` | Setup screen: Header, Columns, Tabs, Display, Colors |
| `F1` / `h` / `?` | Help |
| `F10` / `q` | Quit; configuration is saved on clean exit only |
| `Ctrl-C` | Quit from any screen without saving configuration (exit status 130) |

## Batch mode (no TUI)

```sh
gander --json                     # one sample, JSON
gander -n 5 -d 1000 --filter rust # five text samples, one per second
gander -n 1 --sort PERCENT_MEM --limit 10
```

`--json` implies `-n 1`. Field names accepted by `--sort` are the same ones used
in the config file (`PERCENT_CPU`, `PERCENT_MEM`, `M_RESIDENT`, `TIME`, ...).
The first emitted snapshot follows one unreported warm-up interval; `-n` counts
outputs only. Missing values are `-` in text and nullable in JSON, with reasons in
`availability`. See [METRICS.md](METRICS.md).
