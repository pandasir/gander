# gander

> A cross-platform interactive process viewer in Rust — `take a gander` at your system.

`gander` is a functional reimplementation of [htop](https://github.com/htop-dev/htop)'s
*candidate feature set*, built with an immediate-mode TUI (ratatui) and an
Elm-style state/update/view loop instead of htop's retained-mode Panel stack.

**Status: M0 skeleton.** Compiles, 122 tests pass, runs on **Linux and macOS**.
See [docs/DESIGN.md](docs/DESIGN.md) for the design,
[docs/ROADMAP.md](docs/ROADMAP.md) for progress, and
[docs/PORTABILITY.md](docs/PORTABILITY.md) for the per-platform details.

## Why not a 1:1 port?

htop is a retained-mode widget toolkit (`Object` → `Panel` with a focus stack)
rasterizing into a ncurses window. ratatui is immediate-mode: you rebuild the
frame each tick and it diffs cells for you. Translating htop's `Panel` /
`ScreenManager` / `Meter` vtable design onto ratatui fights the framework and
buys nothing.

So: **keep htop's feature list as the requirements document, throw away its
code structure.** Keep its *keybindings* though — zero-cost migration for users.

| | htop (C) | gander (Rust) |
|---|---|---|
| Render | ncurses, retained-mode panels | ratatui + crossterm, immediate-mode |
| Widgets | `Object` vtable, focus stack | `enum Screen` + per-screen `handle_key()` |
| Data | single-threaded `/proc` scan | parallel scan (rayon) + background sampler |
| Platforms | Linux, BSD, macOS | Linux + macOS, same `PlatformSampler` trait |
| Config | `htoprc`, hand-rolled parser | TOML, with `--import-htoprc` |
| Tests | luajit spec, valgrind scripts | fixtures + proptest + `TestBackend` snapshots |

## Scope

| Milestone | Contents |
|---|---|
| **M0** | Linux only. Process table, sort, filter, tree view, kill / renice |
| **M1** | Meter system (CPU / memory / swap / load / net / disk IO), configurable columns, config persistence |
| **M2** | Screen tabs, incremental search, multi-select bulk actions, affinity / ioprio, cgroup columns |
| **M3** | High-value Linux depth: PSS/EPSS via `smaps_rollup`, PSI, Zram/Zswap, GPU, container detection |
| **M4** | macOS / BSD behind a `Platform` trait |

Explicitly out of scope for v1: a `pcp-htop`-style dynamic metric engine
(runtime-defined columns/meters/screens driven by a metrics API). That is a
separate product.

## Deliberate deviations from htop

1. **Never block the UI.** Sampling runs off-thread; the renderer reads an
   `Arc` snapshot. htop stalls while `smaps_rollup` reads a few thousand PIDs.
2. **`--json` output.** The core crate has no UI dependency, so a batch mode
   that emits JSON is nearly free — and integration with monitoring systems is
   something htop fundamentally cannot offer.
3. **Cache immutable per-process data.** `cmdline`, `exe`, `comm`, `cgroup`,
   `uid` do not change for a live process; keyed by `(pid, starttime)`.
4. **TOML config + htoprc importer**, not htoprc as the native format.
5. **Save on clean exit only**, matching htop (`htop.1`: any signal loses
   config). This is intentional and must not be "improved".

## Expectations on performance

Language choice alone buys nothing here — the workload is syscall- and
allocation-bound, not CPU-bound. Realistic targets:

| Scenario | Target vs htop |
|---|---|
| Default 1.5 s refresh, 2 000 procs | **parity** (±1 ms is noise) |
| 100 ms refresh, 10 000 procs | **1.5–2.5× faster** (rayon + cache + off-thread) |
| Input latency during recalc | **noticeably better** (non-blocking) |

Details and the budget breakdown: [docs/DESIGN.md § 9 Performance](docs/DESIGN.md#9-performance).

## Build

```bash
cargo build --release      # target/release/gander
cargo test                 # 122 tests
cargo clippy --all-targets # zero warnings
cargo bench -p gander-core # scan timings
```

Works on Linux (`/proc`) and macOS (`libproc` + `sysctl` + Mach). Other unix
targets compile the `/proc` backend but have no live source; the TUI refuses to
start there. Test fixtures under `crates/core/tests/fixtures/proc/` are real
`/proc` snapshots, so the Linux read path is testable on any unix.

## Try it

```bash
cargo run --release                          # the TUI
cargo run --release -- --json --limit 10     # batch mode
cargo run --release -- -n 5 -d 1000          # five samples, one per second
cargo run --release -- --import-htoprc ~/.config/htop/htoprc --save
```

Keys are identical to htop: [docs/KEYS.md](docs/KEYS.md).

## License

GPL-2.0-or-later, matching htop. Behavior, keybindings and column semantics are
derived from htop; keep the license compatible and credit it.
