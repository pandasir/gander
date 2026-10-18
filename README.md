# gander

**English** | [简体中文](README.zh-CN.md)

> A cross-platform interactive process viewer in Rust — `take a gander` at your system.

gander shows you what is running on your machine and what it is costing. It runs
on **Linux and macOS**, keeps htop's keybindings, and is built on a different
screen layout than htop — one that spends its space on the process list instead
of on horizontal bars.

## Install

Requires a Rust toolchain (**1.75+**). Nothing else: no libcurses, no ncurses
development headers, no C compiler beyond what `rustc` already needs.

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

There are no prebuilt binaries or distro packages yet; building from source is
the only supported way to install today.

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

Run `gander --help` for the full list. Every keybinding is htop's, so there is
nothing new to learn: [docs/KEYS.md](docs/KEYS.md). The only additions are `L`
(switch layout) and `H` (show one row per thread, Linux).

## Why gander over htop

**The process list gets the vertical space.** htop draws its meters horizontally,
and its height grows with the core count: one row per core. gander puts the system
state in a vertical side column whose height barely changes, with per-core load in
a grid that fits 16 cores in two rows. Measured on this 8-core machine in a 120x24
terminal: htop shows **13** process rows, gander shows **21** — and the gap widens
on machines with more cores.

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

## The two layouts

gander ships two, and `L` switches between them. The choice persists.

**`panels` (default)** — the vertical system column, the process table with
value bars, and the persistent inspector:

```
  CPU    21.5%│ PID  CPU%  MEM%   RES  COMMAND             2032 iTerm2
  ████░░░░░░░░│ 2032  23.9  1.9   152M  /Applications/iTe…  /Applications/iTerm…
  8 cores     │ 24087 11.4  0.1   5.9M  gander              USER   panda
  ▂▂▂▂▃▂▁▁    │ 6511   7.4  3.0   242M  Google Chrome       STATE  S sleeping
  MEM  3.7G/8G│ …                     …                    CPU    23.9%
  SWP 168M/1G │                                            MEM    1.9% 114M
  LOAD 2.69…  │                                            TIME   30:09.95
```

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

## Platform support

| | Linux | macOS |
|---|---|---|
| Process list | `/proc` | `sysctl(KERN_PROC_ALL)` + `libproc` |
| Per-process details | `/proc/<pid>` | Mach + `proc_pidinfo` |
| Threads as rows | yes (`H`) | not yet |
| CPU affinity (`a`) | read/write | scheduler hint only, explains why |
| I/O priority (`i`) | read/write | not available, explains why |

Both backends are verified against a live OS. Other unix targets compile the
`/proc` backend but have no live data source, and the TUI refuses to start there
rather than showing nothing. Details: [docs/PORTABILITY.md](docs/PORTABILITY.md).

## Build and test

```bash
cargo build --release      # target/release/gander
cargo test                 # 233 on macOS, 216 on Linux
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
