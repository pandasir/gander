# gander — Design

> A Rust reimplementation of htop's feature set. Linux first, others behind a
> platform trait.

Everything below is a design decision with its rationale. When a decision is
revisitable, it is marked **[later]**.

---

## 0. Reference material

We are reimplementing against htop `main` (currently `3.6.0-dev`, GPL-2.0+).

| What | Where |
|---|---|
| Feature list, keybindings, column semantics | `htop.1.in` (man page) |
| Key mapping table | `Action.c` — copy the bindings verbatim |
| Column enum / field IDs | `RowField.h` + `<platform>/ProcessField.h` |
| Platform abstraction shape | `<platform>/Platform.c`, `*Machine.c`, `*ProcessTable.c`, `*Process.c` |
| Meter inventory | `<platform>/Platform.c` → `Platform_meterTypes[]` |
| Meter rendering (bar/graph/text) | `Meter.c`, `CPUMeter.c`, `MemoryMeter.c` |
| GPU without vendor SDK | `linux/GPU.c` (reads `drm-usage-stats` keys from fdinfo) |
| Config format quirks | `Settings.c` |

Prior art in Rust worth reading: **bottom** (`btm`), **zenith**, **`procfs` /
`procfs-core`**.

---

## 1. Scope gates

Do not start milestone *n+1* until *n* is usable end-to-end.

### M0 — usable core (target: 1 week)

The thesis is that the process table + sort + kill is ~80 % of what people
actually do with htop. Ship that first.

- [ ] Scan `/proc`, build a `Row` per PID
- [ ] Columns: PID, COMM/Command, STATE, USER, PRIORITY, NICE, CPU%, MEM%, TIME
- [ ] Sort by any column, asc/desc (`F6`, `<`, `>`)
- [ ] Header row highlighting the active sort column
- [ ] Incremental search (`F3`, `/`)
- [ ] Incremental filter (`F4`, `\`) — fixed strings, `|`-separated, case-insensitive
- [ ] Tree view (`F5`, `t`) with orphan/cycle handling
- [ ] Tag multiple processes (`Space`, `c`, `U`)
- [ ] Kill (`F9`, `k`) with signal picker; nice (`F7`/`F8`)
- [ ] One CPU bar + one memory bar in the header
- [ ] `q` / `F10` quit, clean-exit config save

Explicitly **not** in M0: meters beyond two, tabs, setup screen, colors.

### M1 — feature parity for daily use

- Meter system: CPU (per-core variants), memory, swap, load avg, uptime, tasks,
  clock/date, network IO, disk IO, hostname, blank
- Meter display modes: text / bar / graph; graph history ring buffer
- Full column set available in a setup screen; column reorder
- Themes (color schemes) + monochrome mode
- TOML config, `--import-htoprc`
- Search/filter history in `$XDG_STATE_HOME`

### M2 — power features

- Screen tabs (independent column layout + sort + filter per tab) `Tab`/`Shift-Tab`
- Multi-select bulk actions across the tagged set
- CPU affinity panel (`a`), I/O priority (`i`) — Linux
- cgroup / container columns
- Open files (`l`), env (`e`), command line wrapped (`w`), file locks (`x`),
  strace attach (`s`)
- Display options: hide kernel threads, hide userland threads, highlight
  base name, highlight stale exe

### M3 — Linux depth (the differentiators)

- `smaps_rollup` → PSS, SwapPss, EPSS, private
- PSI pressure meters (`/proc/pressure/{cpu,io,memory,irq}`)
- Zram / Zswap / HugePages meters
- GPU columns via `drm-usage-stats` (no vendor SDK)
- Container detection via cgroup + mountinfo (not heuristics)
- Delay accounting (`TASKSTATS` netlink) — **lowest priority**, needs raw netlink
- Backtrace (libunwind-ptrace) **[later]**, demangling **[later]**

### M4 — portability

Status: **macOS done ahead of schedule** (it is the development machine, and the
cost of retrofitting the abstraction later would have been higher). FreeBSD and
the other BSDs remain.

- `PlatformSampler` trait has two implementations: `/proc` and `darwin`
- Not all fields exist everywhere → `Field` is `#[non_exhaustive]`, and
  `gander_core::capability::*` consts let the UI hide what cannot work
- Platform differences and the traps that were hit: see
  [PORTABILITY.md](PORTABILITY.md)

### Out of scope

- Dynamic/runtime-defined metrics (PCP model)
- Anything requiring a daemon, elevated install, or eBPF in v1

---

## 2. Workspace layout

```
gander/
├── Cargo.toml                  # [workspace]
├── crates/
│   ├── core/                   # NO UI dependency. Hard requirement.
│   │   ├── src/row.rs          Row, Field, FieldMeta, raw values
│   │   ├── src/system.rs       SystemSnapshot (cpu/mem/net/disk)
│   │   ├── src/table.rs        ProcTable: scan, cache, delta compute
│   │   ├── src/sort.rs         sort comparators (numeric, never formatted)
│   │   ├── src/filter.rs       filter predicate
│   │   ├── src/tree.rs         tree build + flatten
│   │   ├── src/action.rs       Action requests (kill/renice/affinity)
│   │   ├── src/format.rs       humanization at render time only
│   │   └── src/linux/          proc parsing, cgroup, gpu, psi
│   ├── tui/                    ratatui widgets, meters, theme, screens
│   ├── config/                 TOML model, htoprc importer
│   └── bin/                    main, arg parsing
├── tests/fixtures/proc/        real /proc snapshots
└── benches/                    criterion
```

**Why `core` must not depend on ratatui:** it makes /proc parsing testable with
golden files, keeps `--json` batch mode trivial, and prevents UI concerns from
leaking into the data model. This mirrors htop's own separation between
`ProcessTable` and `Panel`.

---

## 3. Data layer

### 3.1 Do not use `sysinfo`

```
sysinfo 0.39   convenient  — but cannot reach PSS/EPSS/cgroup/container/GPU/delayacct
procfs  0.18   type-safe   — but allocates heavily per iteration; 4 000 procs drops frames
self-read      htop's approach — fastest, full control
```

Use `procfs-core` as a **reference for field tables and edge cases** (it has
no IO binding), not as a runtime dependency in the hot path.

The differentiator of htop is precisely the columns `sysinfo` cannot provide.
Choosing `sysinfo` would mean reimplementing the missing half anyway, on top of
an abstraction that is in the way.

### 3.2 System call budget per process per tick

```
MUST re-read every tick :  /proc/<pid>/stat            (state, utime, stime, rss, minflt)
                           /proc/<pid>/statm           (only if not in stat)
                           /proc/<pid>/io              (only if IO columns active)
                           /proc/<pid>/schedstat       (only if delay columns active)
                           /proc/<pid>/smaps_rollup    (only if PSS/EPSS active — expensive)

CACHE until exit        :  /proc/<pid>/cmdline
(keyed by                /proc/<pid>/comm
 (pid, starttime))     :  /proc/<pid>/exe  (+ readlink /proc/<pid>/exe)
                           /proc/<pid>/cgroup
                           /proc/<pid>/uid  ← from status/stat
                           /proc/<pid>/environ  (lazy, only for `e` screen)
```

htop re-reads the cached set every tick. Not doing so is a bigger win than any
language choice, and it is only "available" because we are writing new code.

Cache invariants:

- Key `(pid, starttime)`. `starttime` comes from field 22 of `/proc/<pid>/stat`.
  PID reuse then looks like a cache miss, which is correct.
- Evict entries whose `(pid, starttime)` was not seen this tick.
- Bound the cache (e.g. drop everything if it exceeds ~4× current process count).

### 3.3 Parallel enumeration

```rust
let rows: Vec<Row> = pids
    .par_iter()
    .filter_map(|&pid| read_row(pid, &cache, &prev))
    .collect();
```

- Expected **2–3×**, not 8×: the kernel serializes on `tasklist_lock` and dentry
  cache contention, and syscalls do not parallelize for free.
- The win is only meaningful at high process count and high refresh rate. At the
  default 1.5 s / 2 000 procs it is a rounding error.
- Chunk small PID counts to avoid rayon overhead: if `pids.len() < 512`, just
  use a plain loop.
- Do **not** share one scratch buffer across threads — `thread_local!` per worker.

### 3.4 Parsing `/proc/<pid>/stat`

The classic trap: `comm` may contain spaces and both `(` and `)`. Anchor on the
**last** `)`:

```rust
let stat = read(&path)?;                       // thread_local buffer, no String alloc
let l = memchr(b'(', &stat)?;
let r = memrchr(b')', &stat)?;                 // memchr crate
let comm = &stat[l + 1..r];                    // may contain '(' and ')'
let rest = &stat[r + 2..];                     // skip ") "
// tokenize `rest` by whitespace with fixed indices, no Vec allocation:
//   [0]  state            [11] utime (ticks)     [21] rss (pages)
//   [1]  ppid             [12] stime (ticks)     [22] starttime (ticks)
//   [18] priority         [19] nice              [9]  minflt
```

Non-negotiables:

- `_SC_CLK_TCK` via `sysconf`, **never** hardcode 100. Same for `PAGE_SIZE`
  (`sysconf(_SC_PAGESIZE)`).
- `ENOENT` is the normal case, not an error. Processes die between `readdir` and
  `read`. Never `unwrap()`.
- `EACCES` on other users' `smaps_rollup` / `exe` is normal — show `-`.
- Kernel threads have empty `cmdline`; render `[comm]` in brackets.
- Read threads from `/proc/<pid>/task/` only when the "userland threads" option
  is enabled — it multiplies the scan cost. On macOS the equivalent is
  `PROC_PIDLISTTHREADS` + one `PROC_PIDTHREADINFO` per thread; a thread's
  `pth_user_time` is nanoseconds while a process's `pti_total_user` is Mach ticks.
- A thread row needs its own identity. On macOS a thread has no pid at all (its
  row carries the leader's), and the 64-bit `pth` handle is not unique across
  processes, so `Row::thread_id` holds the identity and `Row::identity()` is what
  tags, selection and kill key on. Never key a row by `pid` alone.

### 3.5 Derived metrics

**CPU percent** needs two samples. Store raw tick counters in `Row`:

```rust
struct CpuTimes { total_ticks: u64, cpu_ticks: u64 } // monotonically increasing

// per tick:
let d_total = now.total_ticks - prev.total_ticks;   // system-wide
let d_cpu   = now.cpu_ticks   - prev.cpu_ticks;     // this process
percent = d_cpu as f64 * 100.0 / d_total as f64;    // > 100% on multicore: fine
```

- Guard against `d_total == 0` (sub-tick refresh) and against counter wraparound
  (process replaced → `starttime` changed → treat as new, report 0).
- Divide by `nproc` only if the user chooses "normalized CPU%" (htop has this as
  a separate `PERCENT_NORM_CPU` column).

**Memory** — replicate htop's definition, do not invent one:

```
used = MemTotal - MemAvailable - (SReclaimable - Shmem)     // caches excluded
```

`MemFree` is the wrong number and everyone gets this wrong once.

**RSS pages → bytes**: `rss * page_size`.

---

## 4. Types

### 4.1 `Field`

```rust
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum Field {
    Pid, Comm, State, Ppid, Pgrp, Session, Tty, Tpgid,
    MinFlt, MajFlt, Priority, Nice, StartTime, Processor,
    VirtMem, ResMem, User, PercentCpu, PercentMem, Time,
    Nlwp, Tgid, PercentNormCpu, Elapsed, SchedPolicy,
    ProcComm, ProcExe, Cwd,
    #[cfg(target_os = "linux")]
    Pss, Swap, Private, Epss, Cgroup, Container, IsContainer,
    #[cfg(target_os = "linux")]
    GpuPercent, GpuTime, IoReadRate, IoWriteRate,
    // ...
}
```

Rules:

- Common fields match htop's `RowField.h` meaning; the numeric IDs do not need to
  match (we have no `htoprc` binary compat requirement beyond the importer).
- Use `match` for rendering, **not** `dyn` dispatch. Thousands of rows × dozens of
  columns per frame; dynamic dispatch is measurable and `match` gives exhaustiveness
  checking for free when a new field is added.
- Field metadata is a `const` table:

```rust
pub struct FieldMeta {
    pub name: &'static str,        // "M_PSS" — used by config
    pub title: &'static str,       // " PSS " — header cell
    pub description: &'static str, // shown in setup screen
    pub default_sort_desc: bool,
    pub align: Align,
}
```

### 4.2 `Row`

```rust
pub struct Row {
    pub pid: Pid,
    pub starttime: u64,          // cache key + PID-reuse detector
    pub comm: ArrayString<64>,   // arrayvec — no heap allocation
    pub state: u8,
    pub ppid: Pid,
    pub user: u32,               // uid, name looked up once via cache
    pub priority: i8,
    pub nice: i8,

    // raw counters — formatted at render time only
    pub cpu_ticks: u64,
    pub prev: CpuTimes,
    pub percent_cpu: f32,
    pub rss_pages: u64,
    pub virt_bytes: u64,
    pub percent_mem: f32,
    pub elapsed: u64,

    pub cmdline: Option<Box<str>>,   // lazily fetched, cached
    pub tree: TreeMeta,              // depth, has_children, is_last
    pub flags: RowFlags,             // kernel thread, userland thread, stale exe, ...
}
```

`Row` holds **numbers and interned strings only**. Any `String` produced by
`format!` during a scan is a bug.

### 4.3 `SystemSnapshot`

One immutable value per sample, swapped atomically into the UI:

```rust
pub struct SystemSnapshot {
    pub at: Instant,
    pub cpu: Vec<CpuTimes>,       // per-core + aggregate
    pub mem: MemInfo,
    pub swap: SwapInfo,
    pub load: [f64; 3],
    pub uptime: Duration,
    pub net: Vec<NetIface>,
    pub disk: Vec<DiskIo>,
    pub psi: Option<Psi>,
    pub procs: Arc<[Row]>,        // sorted/filtered view is derived, not stored here
}
```

---

## 5. Platform abstraction

Three traits, matching htop's three platform files. Two traits would be too few
(`Machine` vs `ProcessTable` have different lifetimes), four too many.

```rust
pub trait Platform {
    fn read_system(&mut self) -> SystemSnapshot;
    fn enumerate(&self, ctx: &mut ScanCtx, out: &mut Vec<Row>);
    fn field_meta(f: Field) -> Option<&'static FieldMeta>;
    fn meter_types() -> &'static [&'static dyn MeterSpec];
}
```

Selection is **compile-time**:

```rust
#[cfg(target_os = "linux")]   mod linux;
#[cfg(target_os = "linux")]   pub use linux::LinuxPlatform as Current;
```

No `Box<dyn Platform>`: a single-platform binary has no use for runtime
polymorphism, and the extra indirection in the hot loop is pure cost.

---

## 6. State, update, view

```rust
pub struct App {
    pub screen: Screen,             // ProcessList | Help | Setup(SetupPage) | Info(InfoView)
    pub tabs: Vec<Tab>,
    pub tab_idx: usize,
    pub selected: usize,            // index into `view`, not into `rows`
    pub view: Vec<usize>,           // sorted+filtered indices
    pub rows: Vec<Row>,             // reused across ticks, never cleared
    pub tagged: HashSet<Pid>,
    pub header: HeaderLayout,
    pub theme: Theme,
    pub dirty: Dirty,
}

pub enum Screen {
    ProcessList,
    Help,
    Setup(SetupPage),               // Header | Columns | Display | Colors | Tabs
    Info(InfoView),                 // strace | lsof | env | locks | backtrace
    SignalPicker { pid: Pid, cursor: usize },
}
```

The **index indirection is the central design point**: sorting and filtering
mutate only `Vec<usize>`; process data is updated in place. Tree view does not
reshuffle storage either.

### Three timelines — do not merge them

| Cadence | Work |
|---|---|
| Sampling thread, `1/N` s (default 1.5 s) | `Platform::read_system()` → store `Arc<SystemSnapshot>` |
| Input event | `update(&mut App, Event)` → may set `dirty`, may request recalc |
| Frame | `if dirty { terminal.draw(view) }` |

Non-blocking sampling:

```rust
// sampler thread
let snap = Arc::new(platform.read_system());
shared.store(snap);                 // arc-swap

// UI thread
let snap = shared.load();           // never blocks
```

This is the one place gander can be *qualitatively* better than htop rather
than just "the same but in Rust": htop's UI stalls during recalc, and
`smaps_rollup` across thousands of PIDs is tens of milliseconds.

### Key handling

```rust
impl App {
    fn handle_key(&mut self, key: KeyEvent) -> Reaction; // Continue | Recalc | Quit
}
```

Copy htop's bindings verbatim (`F1`–`F10`, `Tab`, `Space`, `c`, `U`, `t`, `k`,
`s`, `l`, `w`, `x`, `e`, `Z`, `m`, `b`, `H`, `K`, `I`, `+`/`-`/`*`, `u`, `.`,
`<`/`>`, `[`/`]`, `^`, `$`, `PgUp`/`PgDn`, `Home`/`End`, `Ctrl-L`). Migration cost
for a user trying gander should be zero.

---

## 7. UI mapping

| htop | gander |
|---|---|
| `Panel` + focus stack | not needed: `enum Screen` + per-screen `handle_key()` |
| `Meter` vtable | `trait Meter { fn sample(&mut self, &SystemSnapshot); fn render(&self, area, buf, mode); }` |
| `MeterMode` (text/bar/graph) | one `render` with a `match mode`; graph keeps `VecDeque<Vec<f64>>` |
| `FunctionBar` | `fn funcbar(&self) -> [(&str, &str); 10]`, dispatched on `Screen` |
| `CRT.c` color themes | `Theme` struct of `Style`s; one module per theme |
| `HeaderLayout` (3 layouts, %-widths) | `Vec<MeterColumn>` + `Layout::horizontal(Constraint::Percentage(..))` |
| `InfoScreen` (strace/lsof/env) | unified `InfoView { title, lines }` with a swappable source |

### Rendering rules

- `Span`/`Line` construction per frame is fine, but avoid `format!` for values
  that do not change. Prefer a small stack buffer + `write!` into an
  `ArrayString`, or `itoa`/`ryu` for numbers.
- `Style` is `Copy` — but `Span<'a>` borrows; build them as slices where possible.
- Graph characters: hand-rolled `braille`/block rendering beats any crate here and
  is ~50 lines.
- CPU bar segment order and colors must match htop's legend (see `htop.1` METERS
  section): nice / normal / kernel / irq / soft-irq / steal / guest / io-wait.

---

## 8. Configuration

- Native format: TOML at `~/.config/gander/config.toml`, honoring `$XDG_CONFIG_HOME`
  and a `$GANDER_CONFIG` override.
- `--import-htoprc <path>` parses `key=value` lines into the TOML model. htoprc is
  trivially parseable; supporting it is cheap and is a real adoption lever.
- Search/filter history in `$XDG_STATE_HOME/gander/history`, **not** next to the
  config (match htop's newer behavior).
- **Write on clean exit only.** htop documents this deliberately; a signal
  losing config changes is the expected behavior, not a bug.
- Every config field needs a sane hardcoded default, because a first run must work
  with no file present.

---

## 9. Performance

### 9.1 Budget

4 000 processes, 20 Hz, 200×50 terminal:

| Stage | Budget | Bound by |
|---|---|---|
| `readdir /proc` | 1–2 ms | syscall |
| 4 000 × `openat`+`read` `/proc/pid/stat` | 2–4 ms | **syscall + `tasklist_lock`** |
| parse `stat` | ~1.5 ms | CPU |
| sort | <1 ms | CPU |
| render | 1–2 ms | CPU |
| **total** | **~8–12 ms** | |

Note that the CPU-bound portion is under 5 ms total. Even a 30 % advantage there
saves 1.5 ms — imperceptible. **The workload is syscall- and allocation-bound.**

### 9.2 Where a win is actually available

1. **Parallel enumeration** — 2–3×, only meaningful at 10 k+ procs / high refresh.
2. **Caching immutable data** (§3.2) — roughly halves the `openat` count. This is a
   design win, not a language win, and it is the largest single improvement.
3. **Off-thread sampling** — latency win, not throughput. See §6.

### 9.3 Measured on Linux

Numbers from a `rust:1-slim-bookworm` container on Apple Silicon, arm64, using
`cargo bench -p gander-core --bench scan` and `strace -c -e trace=openat`:

| processes | sequential | rayon | speed-up |
|---|---|---|---|
| 405 | 2.25 ms | 1.50 ms | 1.50× |
| 805 | 2.93 ms | 1.91 ms | 1.53× |

The parallel win is real but modest, exactly as predicted: the work is
syscall-bound and the kernel serialises part of it.

**The cache is the bigger win.** `openat` calls per scan:

| scenario | openat total | per process |
|---|---|---|
| cold cache, 405 procs, 1 scan | 1 626 | 4.0 |
| warm cache, 405 procs, 20 scans | 9 397 | 1.16 |

Each process needs `stat`, `status`, `cmdline`, `cgroup` and `exe` on first sight,
then only `stat` afterwards: a 71 % reduction in syscalls. That matches the plan
in § 3.2 and is a design win, not a language win.

Not measured yet: render cost, memory footprint, and behaviour at 10 000+
processes. Those need a host with more processes than a container can usefully
simulate.

### 9.4 Realistic targets

| Scenario | Target |
|---|---|
| 1.5 s refresh, 2 000 procs | parity |
| 100 ms refresh, 10 000 procs | 1.5–2.5× faster |
| Responsiveness during recalc | clearly better |
| Memory footprint | parity or slightly worse (binary is larger) |

### 9.5 Anti-patterns that will make gander slower than htop

| Anti-pattern | Cost |
|---|---|
| Using `sysinfo` | 10–50× slower: extra reads + heavy allocation per process |
| Using `procfs` in the hot path | 5–10× slower, same reason |
| `String::new()` per process per tick | 4 000 × 2 allocs × 20 Hz = 160 k allocs/s ≈ 10–20 ms |
| Sorting on formatted strings | 4 000 × log 4 000 `format!` per tick — catastrophic |
| Rebuilding `Style`/`Span` needlessly | death by a thousand allocations |

Two habits prevent all of it: **`Row` stores raw scalars; formatting happens in
the renderer.** And: **`thread_local!` scratch buffers; `ArrayString` for names.**

### 9.6 Measurement, not guessing

```bash
cargo bench                                    # criterion: full scan p50/p99
strace -c -f ./target/release/gander -n 20     # syscall counts — the honest metric
```

Track `openat` count specifically. If it goes down, wall time goes down, in any
language.

---

## 10. Testing

| Layer | Approach |
|---|---|
| `/proc` parsers | `tests/fixtures/proc/<pid>/{stat,status,smaps_rollup,cgroup,cmdline}` — real snapshots |
| Parser edge cases | comm containing spaces and parens; zombie; kernel thread (empty cmdline); ARM field offsets; truncated file; PID reuse |
| Sort / filter / tree | pure functions; `proptest` invariant: output is a permutation of input with no duplicates |
| CPU% math | inject timestamps; verify delta formula, zero-delta guard, wraparound |
| Tree view | orphan (ppid missing), self-parent cycle, deep chains |
| UI | `ratatui::backend::TestBackend` snapshot tests (CPU bar width == width × percent) |
| Config | round-trip TOML; htoprc import against fixtures |

Rules: parsers take `&[u8]`, not a path, so every test runs on any OS. Fixtures are
committed, never generated at test time from the live system.

---

## 11. Dependencies (planned)

| Crate | Use | Note |
|---|---|---|
| `ratatui` | TUI | immediate-mode rendering |
| `crossterm` | terminal backend, input | |
| `rayon` | parallel scan | gate on PID count |
| `libc` / `nix` | `sysconf`, `kill`, `setpriority`, `sched_setaffinity`, `ioprio_set` | `nix` for safety where it exists |
| `memchr` | fast `stat` tokenizing | |
| `arrayvec` | `ArrayString<64>` for comm | avoids heap |
| `serde` + `toml` | config | |
| `arc-swap` | snapshot handoff | |
| `thiserror` | error types | |
| `proptest`, `criterion` | dev | |

Deliberately absent: `sysinfo`, `procfs` (as runtime deps), any async runtime
(this is a blocking-syscall program, not a server).

---

## 12. First milestone, concretely

Ship this before touching meters:

1. Scan `/proc`, build `Vec<Row>` (single-threaded is fine for v0)
2. Render a table: PID, COMMAND, USER, CPU%, MEM%, TIME
3. Sort by any column; `F6` cycles; header highlights the active sort
4. `/` search, `\` filter, `t` tree
5. `F9` kill, `F7`/`F8` nice
6. One CPU bar + one memory bar
7. Save config on clean exit

Only after this loop works end to end does the abstraction prove itself — and
everything after that is "add a `Field` variant" or "add a `Meter` impl".
