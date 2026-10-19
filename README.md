# gander

**English** | [简体中文](README.zh-CN.md)

> A cross-platform interactive process viewer in Rust — `take a gander` at your system.

gander shows you what is running on your machine and what it is costing. It runs
on **Linux and macOS**, keeps almost all of htop's keybindings (the two that differ
are [listed below](#keys-that-differ-from-htop)), and is built on a different
screen layout than htop — one that spends its space on the process list instead
of on horizontal bars.

## Install

**Prebuilt binaries** (macOS and Linux) are on the
[Releases page](https://github.com/pandasir/gander/releases):

```bash
curl -LO https://github.com/pandasir/gander/releases/latest/download/gander-0.5.0-aarch64-apple-darwin.tar.gz
tar -xzf gander-0.5.0-aarch64-apple-darwin.tar.gz
./gander-0.5.0-aarch64-apple-darwin/gander
```

Pick the archive for your platform (`aarch64`/`x86_64` Apple Silicon or Intel
macOS; `x86_64-unknown-linux-musl` is a static binary that runs on any distro).
Each archive has a `.sha256` checksum next to it.

**From source** — requires **Rust 1.89+**. Nothing else: no libcurses, no ncurses
development headers, no C compiler beyond what `rustc` already needs.

The floor is not decorative: `ratatui` 0.30 needs 1.88 and `uuid` needs 1.89, and
building on an older toolchain fails. CI builds against whatever `Cargo.toml`
declares, so this number cannot drift away from reality unnoticed.

```bash
git clone https://github.com/pandasir/gander
cd gander
cargo install --path crates/bin
```

That puts `gander` in `~/.cargo/bin`, so it is on your `PATH` if you installed
Rust with rustup.

Prefer to run it without installing:

```bash
cargo run --release              # the TUI
```

There are no distro packages yet; building from source or grabbing a release
binary are the supported ways to install today.

## Usage

```bash
gander                                     # the interactive TUI
gander --delay 500                         # sample every 500 ms
gander -n 5 -d 1000                        # 5 text samples, one per second
gander --json --limit 10                   # one JSON sample, 10 processes
gander --sort PERCENT_MEM --limit 20       # top 20 by memory
gander --filter "python|node"              # only matching command lines
gander --import-htoprc ~/.config/htop/htoprc --save
```

Run `gander --help` for the full list; every keybinding is either htop's or
listed in [docs/KEYS.md](docs/KEYS.md).

## Keys that differ from htop

Two of htop's existing bindings mean something else here, so an htop user pressing
them gets a different action than expected:

| Key | htop | gander |
|---|---|---|
| `I` | invert the sort order | cycle the selected header meter's display mode |
| `+` / `-` | expand / collapse a subtree in tree view | faster / slower sampling |

`+`/`-` were only ambiguous in tree view; outside it htop does not bind them, so
the common case is unaffected. `I` is a real conflict — there is no invert-sort
key any more; use `F6` / `<` `>` / `.` to reach the order you want.

Genuinely new keys:

| Key | Action |
|---|---|
| `L` | switch layout: `panels` ⇄ `htop` |
| `I` | cycle the selected header meter's mode (text / bar / graph) |
| `Shift+Left` / `Shift+Right` | move the header meter cursor |

`H` is **not** new — it is htop's "hide user threads" — and it works on both
platforms, not just Linux.

## Why gander over htop

**The process list gets the vertical space.** htop draws its meters horizontally,
and its height grows with the core count: one row per core. gander puts the system
state in a vertical side column whose height barely changes, with per-core load in
a grid that fits 16 cores in two rows.

Measured on this 10-core machine in a 120x24 terminal, both freshly started and
captured through a real terminal emulator: htop shows **11** process rows, gander
shows **21** — nearly twice the list. The gap widens on machines with more cores,
because only htop's side of it grows.

**You never leave the list to inspect a process.** In htop, learning a process's
command line, its exact memory, or whether it is sleeping or running means opening
a modal screen (`w`, `e`, `l`) and losing your position, your tags and what you
were comparing against. gander keeps an inspector beside the list that updates as
you move the selection: the wrapped command line, the state spelled out in words,
and the exact figures the narrow columns round. The modal screens still exist for
what genuinely needs them — environment, open files, locks, backtrace.

**The sort order is visible.** The `CPU%` and `MEM%` columns carry a bar painted
behind the number, scaled against the largest value in the table rather than
against 100%. On a quiet machine where the top process sits at 4%, an absolute bar
would be invisible on every row — exactly when "which of these is biggest" matters.

**Nothing blocks the UI.** Sampling runs on a background thread and the renderer
reads a snapshot, so a slow scan never freezes the interface. This matters most
where htop struggles: reading `smaps_rollup` across thousands of PIDs.

**Scriptable output.** `--json`, `-n`, `--sort`, `--filter` and `--limit` make it
usable in monitoring and shell pipelines, which htop cannot do.

**Correct on macOS.** gander reads the full process table through
`sysctl(KERN_PROC_ALL)` rather than `proc_pidinfo`, which as a normal user sees
only about 60% of it. Run state comes from `proc_taskinfo`, because the `p_stat`
field on current Darwin reports "runnable" for a process blocked in `read(2)`.

**Host throughput in the header.** htop has per-process I/O rate columns, but no
host-wide network or disk meter. gander's `NetworkIO` and `DiskIO` scale their bars
against the largest rate seen so far rather than a fixed ceiling, so they are
legible on a 100 Mbit link and on a 10 Gbit one.

**It tells you *which* container.** htop's `CONTAINER` column flags membership;
gander shows `docker:1f2e3d4c5b6a` or `kubepods:<pod-uid>`. Detection combines the
cgroup path (which names the runtime and id) with the PID namespace (which is the
only reliable membership test — a container with its own cgroup namespace reports
`/` and would otherwise look like the host).

## The two layouts

gander ships two, and `L` switches between them. The choice persists.

**`panels` (default)** — the vertical system column, the process table with
value bars, and the persistent inspector:

```
  CPU    21.5%│ PID  CPU%  MEM%   RES  COMMAND             2032 iTerm2
  ████░░░░░░░░│ 2032  23.9  1.9   152M  /Applications/iTe…  /Applications/iTerm…
  10 cores    │ 24087 11.4  0.1   5.9M  gander              USER   panda
  ▂▂▂▂▃▂▁▁ ·  │ 6511   7.4  3.0   242M  Google Chrome       STATE  S sleeping
  MEM  5.5G/16G│ …                     …                    CPU    23.9%
  SWP  3.9G/5G│                                            MEM    1.9% 114M
  LOAD 2.69…  │                                            TIME   30:09.95
  UP     5h 3m│
  TASKS  552  │
  RUN      1  │
  THR   3370  │
```

The per-core grid paints one cell per core, colour-banded by load, and the
sparkline under it is aggregate CPU over time.

**`htop`** — the classic arrangement, fully supported. Set `layout = "htop"` in
the config to make it the default.

The reasoning, including why the bars are table-relative and what happens on a
narrow terminal, is in [docs/LAYOUT.md](docs/LAYOUT.md).

## Configuration

TOML at `$XDG_CONFIG_HOME/gander/config.toml` (or `$GANDER_CONFIG`). Every screen
tab keeps its own columns and sort, and the header meters and their display modes
are configurable too. Settings are written on a clean exit only, matching htop.

An existing htop setup can be imported:

```bash
gander --import-htoprc ~/.config/htop/htoprc --save
```

Search and filter history lives separately, at
`$XDG_STATE_HOME/gander/{search,filter}_history` — the config is a decision you
copy between machines, history is a byproduct of using this one. `Up` / `Down` in
the `F3` / `F4` prompt walks it, so re-running yesterday's filter is one key.

### Identity columns

Seven columns that describe *which* process this is rather than what it is doing.
All of them work on both platforms.

| Column | Meaning |
|---|---|
| `SESSION` | Login session id (`SID`); `-` when the kernel will not say |
| `TTY` | Controlling terminal, decoded to `pts/3` or `tty1`; `?` when there is none |
| `TPGID` | The foreground job's process group on that terminal |
| `STARTTIME` | Wall-clock start, boot time + the process's start ticks |
| `TGID` | Thread group id: the process a thread row belongs to |
| `SCHED_POLICY` | `other`, `fifo`, `rr`, `batch`, `idle`, `deadline` |
| `PROC_COMM` | The kernel's short command name |
| `PROC_EXE` | Resolved executable path |
| `CWD` | Working directory |

`COMM` and `PROC_COMM` are not the same column, and that is not an accident: `COMM`
is htop's Command column and shows the *command line*, while `PROC_COMM` is the
executable's base name as the kernel holds it. htop makes the same distinction.

`STARTTIME` is shifted by the snapshot's boot time and rendered in **local** time,
so it agrees with the `Clock` in the header instead of silently being UTC.

`TTY` decodes the kernel's `tty_nr` device number rather than printing it: `34816`
is not a terminal anyone recognises, `pts/0` is. `TPGID` renders `-` for the
kernel's `-1`, which means "no foreground job" and is not a process group.

### Container columns (Linux)

`F2` → Columns offers two more on Linux:

- **`CONTAINER`** — the container a process is in, as `runtime:id`, e.g.
  `docker:1f2e3d4c5b6a`, `kubepods:1a2b3c4d-...`, `lxc:web-1`. `-` for a host
  process.
- **`CGROUP`** — the full cgroup path, for when the id is not enough.

Detection uses the cgroup path *and* the PID namespace, because neither is
sufficient: the path names the runtime and the id but reads `/` inside a container
that has its own cgroup namespace, and the namespace says "containerised" without
saying which. A process the namespace flags but the path cannot name shows
`container:?` rather than an invented id.

Press `O` to hide containerised processes and leave the host's own. On macOS there
is no container detection, so the key says so instead of toggling nothing.

### Header meters

`F2` → Header adds and reorders them; `I` cycles the selected meter's mode
(text, bar, graph) and the choice is saved per meter. Beyond the htop set (CPU,
memory, swap, load, uptime, tasks, clock, date, hostname, blank) there are two
throughput meters and four Linux-depth meters:

- **`NetworkIO`** — host receive/transmit rate, loopback excluded.
- **`DiskIO`** — host read/write rate from `/proc/diskstats`, whole disks only
  (partitions and `dm-*` are skipped so nothing is counted twice). macOS has no
  host-wide equivalent without IOKit, so the meter says so there instead of
  showing a permanent `0 B/s`.

Both are rates over cumulative counters, scaled against the largest rate seen so
far rather than a fixed ceiling — a bar pinned to 1 Gbit is an invisible sliver on
a 100 Mbit link. Unlike the container columns these work on macOS too, where
`DiskIO` reports itself unavailable rather than showing zero.

The four Linux-depth meters, all of which say `n/a here` where the subsystem is
absent rather than drawing a permanent empty bar:

- **`PSI`** — pressure stall information: `some` (at least one task waiting) and
  `full` (nothing could run) for CPU, memory, IO and IRQ. This is the one figure
  that separates "busy" from "struggling": 90 % CPU with an empty run queue is
  healthy, 30 % CPU with a 40 % `io full` average is not, and CPU% and load
  average both look fine in the second case.
- **`Zram`** — compressed swap in RAM, as `orig → compressed (ratio)`. A 4:1 ratio
  means 4 GB of swap occupying 1 GB of memory.
- **`Zswap`** — the compressed pool in front of the swap device, as a share of its
  ceiling (`max_pool_percent` × physical memory). This is what says whether the
  next swapped page goes to disk.
- **`HugePages`** — pre-reserved huge pages, `used/total`. Reserved pages are
  unavailable to everything else whether or not a process maps them.

### Linux memory and GPU columns

On Linux, `F2` → Columns also offers the PSS family and the GPU pair:

| Column | Meaning |
|---|---|
| `M_PSS` | Proportional set size: shared pages divided between their sharers |
| `M_EPSS` | PSS plus the swapped-out equivalent (htop calls this `M_PSSWP`) |
| `M_PRIVATE` | `Private_Clean + Private_Dirty`: what exits with the process |
| `M_SWAP` | Memory this process has pushed to swap |
| `GPU%` | Share of GPU time, from `drm-usage-stats` |
| `GPU_TIME` | Cumulative GPU time, summed over every engine class |
| `CPUD%` | Share of this task's life runnable but waiting for a CPU |
| `IOD%` | Share of this task's life waiting for synchronous block I/O |
| `SWPD%` | Share of this task's life waiting for a page to be swapped in |

**Why PSS rather than RES.** RSS counts a shared page once per process, so two
processes sharing 100 MB of libraries both report 100 MB and the `RES` column adds
up to 200 MB that does not exist. PSS divides each shared page by the number of
sharers, which is the only per-process figure that sums correctly across a table.

**Both families are read on demand.** `smaps_rollup` makes the kernel walk every
VMA of the process; `fdinfo` costs a directory walk and a read per descriptor.
Either one, run across a desktop's process table, costs more than the rest of the
scan together. So neither is read unless a column that needs it is actually on
screen — and `gander --sort M_PSS` pays for exactly what it asked for. A row whose
figures were not read shows `-`, never `0`: another user's `smaps_rollup` needs
`CAP_SYS_PTRACE`, and "not measured" is not "zero".

**No vendor SDK.** The GPU figures come from the kernel's own `drm-usage-stats`
keys, so `amdgpu`, `i915`/`xe` and `nouveau` work with nothing installed. A driver
that does not implement them shows dashes.

**The delay columns answer a different question.** `CPU%` says how much work a
process does; `IOD%` says how much of its life it spent unable to do any. A process
at 0.0 % CPU with 40 % `IOD%` is not idle, it is stuck — and nothing else in the
table distinguishes the two. They come from `TASKSTATS` over generic netlink,
speaking the protocol directly rather than linking `libnl3`.

They need `CAP_NET_ADMIN`, which an ordinary process does not have: unprivileged
`gander` gets `EPERM`, stops asking after the first refusal, and renders `-` in all
three columns. That is deliberate — `0.0%` would claim the process never waited, and
the figure was never obtained.

The percentages are **lifetime averages**, not interval rates: the kernel keeps
cumulative counters, so a stall an hour ago still shows. Read them as "how much of
this process's existence was spent waiting", not "what is it doing right now".

### Themes

Eight, chosen with `F2` → Colors (or `color_scheme` in the config):

| | |
|---|---|
| `gander` (the default) | teal on deep navy, purple accents |
| `default` | htop's own palette |
| `monochrome` | no colour at all |
| `midnight` | blue-on-blue, low contrast |
| `classic` | deep navy, one accent, solid blue selection |
| `verdant` | green-leaning |
| `paper` | light background, for a bright terminal |
| `quiet` | desaturated greys |

`paper` is the one that matters structurally: every style sets a foreground and
most leave the background unset, which is invisible on a dark theme and broken on a
light one. The renderer paints the theme background over the whole frame first, so
all four draw paths (htop, panels, setup, dialogs) inherit it.

## Platform support

| | Linux | macOS |
|---|---|---|
| Process list | `/proc` | `sysctl(KERN_PROC_ALL)` + `libproc` |
| Per-process details | `/proc/<pid>` | Mach + `proc_pidinfo` |
| Container columns | cgroup + PID namespace | not available |
| PSS / EPSS / private columns | `smaps_rollup`, read on demand | not available |
| PSI pressure (some/full) | `/proc/pressure` | not available |
| Zram / Zswap / HugePages meters | `/sys/block/zram*`, `meminfo` | not available |
| GPU columns (`GPU%`, `GPU_TIME`) | `drm-usage-stats` in `fdinfo` | not available |
| Delay columns (`CPUD%`, `IOD%`, `SWPD%`) | `TASKSTATS` netlink (needs `CAP_NET_ADMIN`) | not available |
| Threads as rows | yes (`H`) | yes (`H`) |
| CPU affinity (`a`) | read/write | scheduler hint only, explains why |
| I/O priority (`i`) | read/write | not available, explains why |

Both backends are verified against a live OS. Other unix targets compile the
`/proc` backend but have no live data source, and the TUI refuses to start there
rather than showing nothing. Details: [docs/PORTABILITY.md](docs/PORTABILITY.md).

## Build and test

```bash
cargo build --release      # target/release/gander
cargo test                 # 356 on macOS, 366 on Linux
cargo clippy --all-targets # zero warnings
cargo bench -p gander-core # scan timings
```

The test fixtures under `crates/core/tests/fixtures/proc/` are real `/proc`
snapshots, so the Linux read path is testable on any unix.

## Design

gander keeps htop's *feature list* as the requirements document and throws away
its *code structure*: htop is a retained-mode widget toolkit built on ncurses,
while gander is an immediate-mode ratatui front end over a UI-independent core.
The core crate has no UI dependency, which is what makes `--json` nearly free.

| | htop (C) | gander (Rust) |
|---|---|---|
| Render | ncurses, retained-mode panels | ratatui + crossterm, immediate-mode |
| Widgets | `Object` vtable, focus stack | `enum Screen` + per-screen `handle_key()` |
| Data | single-threaded scan | parallel scan (rayon) + background sampler |
| Config | `htoprc`, hand-rolled parser | TOML, plus an `htoprc` importer |
| Platforms | Linux, BSD, macOS | Linux + macOS, same `PlatformSampler` trait |

More: [docs/DESIGN.md](docs/DESIGN.md) for the design,
[docs/ROADMAP.md](docs/ROADMAP.md) for what is done and what is next.

## License

GPL-2.0-or-later, matching htop. Behaviour, keybindings and column semantics are
derived from htop; keep the license compatible and credit it.
