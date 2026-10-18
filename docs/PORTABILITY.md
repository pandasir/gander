# Portability

How the two backends differ, what is verified where, and what is honestly
missing. Read [DESIGN.md § 5](DESIGN.md#5-platform-abstraction) for the trait
design.

## Status matrix

| | Linux | macOS |
|---|---|---|
| Process list | `/proc` scan | `proc_listallpids` + `PROC_PIDTASKALLINFO` |
| CPU% | tick deltas from `/proc/<pid>/stat` | nanosecond deltas ÷ wall-clock interval |
| Per-core CPU | `/proc/stat` | `host_processor_info` |
| Memory | `/proc/meminfo` | `host_statistics64` + `hw.memsize` |
| Swap | `/proc/meminfo` | `sysctl(vm.swapusage)` → `struct xsw_usage` |
| Load average | `/proc/loadavg` | `getloadavg(3)` |
| Boot time | `/proc/uptime` | `sysctl(kern.boottime)` |
| Start time | field 22 of `stat` (ticks) | `pbi_start_tvsec/tvusec` |
| cmdline | `/proc/<pid>/cmdline` | `sysctl(KERN_PROCARGS2)` |
| exe | `readlink /proc/<pid>/exe` | `proc_pidpath` |
| uid | `/proc/<pid>/status` | `proc_bsdinfo.pbi_uid` |
| threads | `/proc/<pid>/task` *(not yet)* | `pti_threadnum` |
| disk I/O | `/proc/<pid>/io` *(not yet)* | `proc_pid_rusage` *(parsed, not shown)* |
| Test coverage | fixtures + live | live only (no fixture format to replay) |

Both backends implement `PlatformSampler`, and `gander_core::Sampler` resolves to
the right one at compile time.

## Darwin-specific traps (all hit and fixed during development)

These are the ones a naive implementation gets wrong, recorded so they are not
reintroduced:

1. **`mach_timebase_info` is not 1:1.** On Apple Silicon it reports 125/3. Treating
   `pti_total_user` as nanoseconds makes CPU time 40x too small. Always scale
   through `units::mach_ticks_to_nanos`.
2. **`vm.swapusage` via `sysctlbyname` returns `struct xsw_usage`, not text.**
   `sysctl -n vm.swapusage` prints a formatted string, which tempts you into
   parsing text; you get binary garbage. Read the struct.
3. **`pti_*_size` is bytes, not pages.** Linux's `rss` is pages and needs
   `* page_size`; Darwin's does not.
4. **`KERN_BOOTTIME` is absolute; `pbi_start_tvsec` is absolute too.** Elapsed time
   is `now - start`, never `uptime - start` (that yields 0).
5. **`p_stat` is mostly `SRUN`.** Use `pti_numrunning` for R-vs-S, keeping
   `SSTOP`/`SZOMB` from `p_stat`. htop does this in `darwin/DarwinProcess.c`.
6. **CPU% divides by wall-clock time, not tick deltas.** `proc_taskinfo` reports
   nanoseconds; there is no tick counter to difference.
7. **`proc_listallpids` returns a count, not bytes**, and the size can grow between
   calls. Retry with headroom when the buffer comes back full.
8. **`host_processor_info` allocates a Mach array.** Not calling `vm_deallocate`
   leaks a page per sample.
9. **`libc` has no Darwin `kinfo_proc`.** `PROC_PIDTASKALLINFO` provides both the
   BSD and task structures in one call, which is fewer syscalls than htop's
   `sysctl(KERN_PROC_ALL)` + `proc_pidinfo` pair.

## Seeing the whole process table

`proc_pidinfo` only describes processes owned by the calling user, so
`PROC_PIDTASKALLINFO` alone sees roughly 60% of the table: 270 of 456 processes on
a typical desktop. The first version silently dropped the rest, which is why the
process count read 284 while `htop` showed 472.

`sysctl(KERN_PROC_ALL)` is not restricted the same way, and it is what htop uses
(`darwin/DarwinProcessTable.c: ProcessTable_getKInfoProcs`). `libc` does not
expose Darwin's `kinfo_proc`, so `darwin/kinfo.rs` reads the fixed 648-byte
records and extracts fields by offset rather than redeclaring a large,
version-sensitive struct.

Those offsets were not guessed. Each was verified against `ps(1)` on arm64:

| field | offset | how it was verified |
|---|---|---|
| `p_starttime.tv_sec` | 0 | plausible Unix seconds for every row |
| `p_stat` | 32 | histogram matches `ps -o stat=` (4 = SSLEEP dominates) |
| `p_pid` | 40 | matches `getpid()` and `ps -p` |
| `p_nice` | 242 | `renice -n 10` a child, find the byte that becomes 10 |
| `p_comm` | 243 | `"launchd"` for pid 1 |
| `e_ppid` | 560 | matches `getppid()` |
| `e_ucred.cr_uid` | 392 | matches `getuid()`, 0 for pid 1 |

`kinfo::validate()` re-runs the pid-1 and self checks at startup, and
`kinfo::tests` asserts them on every test run, so a layout change fails loudly
instead of producing silent nonsense.

The resulting split: **every** process gets a row from `KERN_PROC_ALL`; the task
counters (CPU time, memory, threads) are added only where `proc_pidinfo` permits.
Processes owned by other users therefore show `-` for CPU/memory, exactly as htop
does — but they are listed.

## CPU core display

htop's default header splits the CPU meters in half above four cores
(`Settings.c`: `LeftCPUs` / `RightCPUs`), one row per core, so an 8-core machine
shows rows `0..3` in the left column and `4..7` in the right. gander now does the
same: `CpuMeter::left(0..half)` and `CpuMeter::right(half..n)`.

Two implementation notes that were bugs first:

1. **The header height was hard-coded to four rows**, so every core past the third
   was clipped. `App::header_height()` now sums `Meter::rows()` per column and
   takes the taller one.
2. **The column split was inferred from a meter's name.** With the CPU meters
   ordered `[left, mem, swap, right, tasks, ...]`, splitting at `Tasks` put the
   right-hand CPU block in the *left* column and duplicated it. Meters now declare
   `Meter::column()` explicitly, mirroring htop's `hColumns[0]` / `hColumns[1]`.

Known difference from htop: htop interleaves cores across the two halves
(`0,4,1,5,2,6,3,7`); gander splits them contiguously (`0,1,2,3` / `4,5,6,7`).
Cosmetic, not a data error.

## Memory accounting (this was wrong once)

The macOS memory figure is the single easiest thing to get wrong, because
`vm_statistics64` exposes **two overlapping families of counters**:

- by usage: `active`, `inactive`, `wire`, `compressor`, `free`, `speculative`
- by backing store: `internal` (anonymous), `external` (file-backed)

Summing across families double-counts. The correct derivation, verified against
`top` and Activity Monitor on an 8 GB M-series machine:

```text
used = (active - purgeable) + wired + compressed + inactive
available = total - used
```

Numbers from one real sample (MiB):

| counter | value |
|---|---|
| active | 1878 |
| inactive | 1778 |
| wired | 1631 |
| compressed | 2187 |
| purgeable | 99 |
| free | 156 |
| **used** | **7375** |
| `top` reports | 7257–7491 |

Two mistakes were shipped before this landed, both visible in the meter:

1. `used = wired + active + compressed` (**5590 MiB**) — `inactive` was left out,
   so an 8 GB machine under load displayed ~5.7 GB.
2. `available = total - wired - compressed` while `used` was computed separately,
   so `used + available = 9.3 GB` on an 8 GB machine. That contradiction is what
   prompted the fix.

`MemInfo::is_exactly_partitioned()` encodes the invariant and is asserted by
`crates/core/tests/mem_accounting.rs` on every macOS test run.

**Linux is different on purpose.** There `used` follows GNU `free`
(`total - free - cached - sreclaimable - buffers`) and `available` comes from
`MemAvailable`. They are *not* complementary — `MemAvailable` includes
reclaimable cache that `used` also accounts for — so `used + available > total`
is correct on Linux. An earlier revision tried to force the complement and
clamped `MemAvailable` down to `total - used`, understating available memory.

## Permission model

Both platforms hide information for processes you do not own:

- Linux: `exe`, `smaps_rollup` need `CAP_SYS_PTRACE` (root) to read others'.
- macOS: `proc_pidinfo`, `proc_pidpath`, `proc_pid_rusage`, `KERN_PROCARGS2` return
  `EPERM` for other users' processes.

The UI shows `-` rather than failing. This is correct behaviour, not a bug; it is
also why `htop` runs `sudo htop` when you want full detail.

## Deliberate omissions on macOS

| Feature | Why |
|---|---|
| cgroup, container columns | No cgroup concept. Fields are `#[cfg]`-gated out. |
| PSI pressure meters | No `/proc/pressure`. |
| Zram / Zswap | Linux-only subsystems. ZFS ARC could be added later. |
| PSS / EPSS | No `smaps_rollup`; `ri_phys_footprint` is the closest analogue and is already available via rusage. |
| Thread rows | `pti_threadnum` gives the count; listing threads needs the deprecated `task_threads` Mach API. Planned with the same gate as Linux's `/proc/<pid>/task`. |
| `a` affinity panel | `thread_policy_set` exists but is per-thread, not per-process; deferred. |
| `i` I/O priority | No `ioprio_set` equivalent. |
| GPU columns | Needs `IOKit`/`IOAccelerator`; different mechanism from Linux `drm-usage-stats`. |

## What is verified, and where

- **Everything compiles and its unit tests pass on this macOS/arm64 machine**
  (116 tests), including 20+ live tests that read real `proc_pidinfo`,
  `host_statistics64`, `KERN_PROCARGS2` and `host_processor_info`.
- **The Linux read path is verified only against fixtures** on this machine: the
  parsers and sampler are exercised with `tests/fixtures/proc`, but the live
  `/proc` code path cannot run here.
- **The TUI render path is smoke-tested against live Darwin data** through
  `TestBackend` (`app::tests::draw_with_a_live_sample`).
- **Neither backend has been benchmarked on real hardware yet.** The numbers in
  DESIGN.md § 9 are budget targets, not measurements.

Before trusting a release: run `cargo test` and `cargo bench` on a Linux host with
several thousand processes, plus `strace -c -f` to confirm the syscall count.
