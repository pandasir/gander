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
| threads | `/proc/<pid>/task` | `PROC_PIDLISTTHREADS` + `PROC_PIDTHREADINFO` |
| per-process disk I/O | `/proc/<pid>/io` *(not yet)* | `proc_pid_rusage` *(parsed, not shown)* |
| host network throughput | `/proc/net/dev` | `sysctl(NET_RT_IFLIST2)` → `if_data64` |
| host disk throughput | `/proc/diskstats` + `/sys/block` | none without IOKit |
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
10. **`pth_user_time` is nanoseconds; `pti_total_user` is Mach ticks.** The two
    look alike and measure the same thing in different units. Verified by burning
    CPU for two seconds on one thread: the per-thread sum came to 1.141 s while
    `pti_total_user * 125/3` came to the same 1.141 s. Running the thread value
    through `units::mach_ticks_to_nanos` inflates it 41x on Apple Silicon.
11. **`PROC_PIDLISTTHREADS` returns thread *handles*, not tids, and they are not
    globally unique.** Every process reports the same handle for its main thread
    (`8681251360` on this machine) — the shell, `fork` children and Chrome alike.
    A handle is stable while the thread lives, but it only identifies a thread
    within its own process, so per-thread state must be keyed by `(pid, handle)`.
12. **Thread handles are 64-bit and mostly do not fit in a pid.** 94.9% of the
    handles on a desktop exceed `u32::MAX`, so a thread row cannot store its
    handle in `Row::pid`. `Row::thread_id` carries it instead, and `Row::identity`
    is the key for tags, selection and kill.
13. **`PROC_PIDLISTTHREADS` has no size query, zero-fills the tail, and can leave
    holes in the middle.** A null buffer returns 0 rather than a byte count, and a
    buffer larger than the thread count comes back with the extra entries zeroed
    while the return value is the *buffer* size. Separately, a process can report
    `pti_threadnum = N` while the list has only `N - 1` usable handles: one slot is
    `0` because the kernel withheld that thread's handle. `pid 508`
    (`CarbonComponentScannerXPC`) reports `[0, 13046116576, 0]` with
    `pti_threadnum == 3`. Only the zero *tail* is trimmed, so the main thread's
    position is preserved.
14. **A thread has no pid of its own on macOS.** `ps -M` prints the process's pid
    in every thread row for the same reason. The main thread is therefore skipped
    by *position* (index 0), not by comparing a tid to a pid as Linux does, and not
    by "the first non-zero entry" — a withheld main-thread handle puts a real
    thread at index 1, and treating it as the leader would silently drop it.
15. **The number of thread rows can be less than `NLWP - 1`.** When a handle is
    withheld (see 13), that thread cannot be described at all, so the table shows
    one fewer row than the process's thread count claims. This is a kernel
    permission limit, not a counting bug; it affected 54 of 709 processes on a
    desktop.

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

## Readable units in the header

Two meters copied htop's compact notation, which is fine in a dense table but not
in a header where there is room to spell things out:

| meter | before | after |
|---|---|---|
| Uptime | `Uptime 4-23:13:30` | `Uptime 4d 23h` |
| Tasks | `Tasks -` (never populated) | `Tasks 484 procs, 1 running, 1824 threads` |

`format::duration_verbose` keeps the two most significant units and drops the
rest: seconds are omitted once days appear, because nobody reading a process
viewer needs second-resolution uptime measured in months. `duration_human` is
unchanged and still used for the `ELAPSED` table column, where fixed width
matters more than immediate legibility.

`Tasks` was a hard-coded `-` because `Meter::sample` only received the host
snapshot and the process counts live in the table. `SystemSnapshot` now carries
`TaskStats { total, running, threads }`, filled by the sampler from the rows it
just produced, so the meter has real data without the trait needing a second
parameter.

## Process ownership

The uid comes from `KERN_PROC_ALL` (`e_ucred.cr_uid` at offset 392) and is
displayed as a login name resolved through `getpwuid_r`.

Two bugs lived here:

1. The `USER` column printed the raw numeric uid, because no name lookup existed
   at all. `core::users` now wraps `getpwuid_r` with a cache — a scan asks for the
   same handful of uids thousands of times, and directory-service lookups are not
   cheap.
2. The sampler overwrote the uid it had just read with `0` while fetching the
   per-process identity, so every process displayed as `root`. The identity cache
   does not carry uid any more; it comes straight from the process list.

`kinfo::tests::uid_is_the_real_owner` asserts both that the calling process's uid
matches `getuid()` and that the table contains more than one distinct uid on a
non-root account, which is what would have caught bug 2.

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
| Thread rows | Supported. `PROC_PIDLISTTHREADS` + `PROC_PIDTHREADINFO` are both `libproc`; the earlier note here claiming this needed the deprecated `task_threads` Mach API was wrong. Threads of processes owned by another user are not listable (`proc_pidinfo` returns 0), so those rows simply have none. |
| `a` affinity panel | `thread_policy_set` exists but is per-thread, not per-process; deferred. |
| `i` I/O priority | No `ioprio_set` equivalent. |
| GPU columns | Needs `IOKit`/`IOAccelerator`; different mechanism from Linux `drm-usage-stats`. |
| `DiskIO` meter (host-wide) | No host-wide disk byte counter without IOKit. Per-process `rusage` figures exist but summing them double-counts shared I/O. The meter reports `n/a here`. |

## Network and disk counters

Both backends report host-wide throughput, but not from the same kind of source,
and one obvious choice is wrong on each platform.

### macOS network: `NET_RT_IFLIST2`, not `getifaddrs`

`getifaddrs` works and is what most code uses. On 64-bit Darwin it hands back
`struct if_data`, whose `ifi_ibytes` / `ifi_obytes` are **32-bit**. A machine that
has moved more than 4 GiB wraps those counters constantly, and a wrap is
indistinguishable from a genuine reset — so on any active link the meter would
drop to zero every few seconds. It also cannot be corrected for after the fact:
the true value is gone.

`sysctl(NET_RT_IFLIST2)` returns `if_msghdr2` carrying `struct if_data64` with
64-bit counters. Two traps in that reply:

- It is a **routing-socket dump**, a sequence of variable-length messages, not an
  array. The walk has to advance by `ifm_msglen`, and a zero length must be
  treated as the end or the loop never terminates.
- The messages are **unaligned**: `if_msghdr2` is `repr(packed(4))` and successive
  messages are not 8-byte aligned. Casting a `&buf[offset..]` to a reference is
  UB on a strict-alignment target, so the header is copied out with
  `copy_nonoverlapping` before being read.

Verified against `netstat -ib` on this machine: summing every non-`lo0` interface
gives the same byte counts.

### Linux network: `/proc/net/dev` has no space after the colon

The interface name and the first counter share one colon-separated field:

```text
  eth0:1234 12 0 0 ...
```

Splitting on whitespace first therefore puts `eth0:1234` in the name slot and
*shifts every counter by one*, so the receive byte count silently becomes the
packet count. Split on `:` first, then whitespace. Loopback is excluded: it never
reaches a wire, and counting it would double every local byte (once received, once
transmitted) — a container running a local database would otherwise report
hundreds of MB/s on an idle NIC.

### Linux disk: `/proc/diskstats` mixes disks and partitions

Fields 6 and 10 (1-based) are sectors read and written, always 512 bytes per
sector regardless of the device's physical sector size. The trap is scope: the
file lists partitions next to their parent disks, and a partition's counters are
*already included* in the disk's. Summing everything doubles every byte.

The authoritative test is whether `/sys/block/<name>` exists. `dm-*` and `loop*`
are skipped too: a device-mapper device wraps the same I/O as the disk beneath it,
and `zram` would double-count swap. Where `/sys` is unavailable (a fixture tree, a
container with a masked `/sys`), a name heuristic covers it — `nvme0n1` is a disk
while `nvme0n1p1` and `sda1` are partitions.

### Counters can go backwards

An interface destroyed and recreated, or a 32-bit counter wrapping, makes the
"delta" negative. A negative rate — or a 4 GiB spike from an unsigned underflow —
for one tick is worse than zero, so `delta` saturates. A zero or negative interval
(first sample, or a clock jump) also yields zero rather than NaN, so no meter has
to special-case its first frame.

## What is verified, and where

Both backends have now been exercised against a live OS.

**macOS (arm64, this machine)** — 73 core tests plus the TUI suite, including live
tests that call `proc_pidinfo`, `host_statistics64`, `KERN_PROCARGS2`,
`host_processor_info` and `KERN_PROC_ALL`.

**Linux (arm64, via `rust:1-slim-bookworm` in Docker)** —
`crates/core/tests/linux_live.rs`, thirteen tests that read the real `/proc` and
compare gander's figures against the same files parsed independently:

| check | result |
|---|---|
| `MemTotal` | exact match |
| `MemAvailable` | used verbatim |
| `used` vs `free`'s `total - free - buff/cache` | within 2 % (Shmem-sized gap) |
| core count | `sysconf(_SC_NPROCESSORS_ONLN)` + 1 aggregate |
| process count vs `read_dir("/proc")` | within 10 |
| sort/filter/tree | permutations, no duplicates, tree keeps every row |
| CPU% over two samples | finite, non-negative |
| `kill(2)` availability | not `Unsupported` on unix |
| network counters vs `/proc/net/dev` | matches a direct sum of non-`lo` interfaces |
| disk counters vs `/proc/diskstats` | matches a direct sum over `/sys/block` disks |
| throughput after two samples | finite, non-negative, counters monotonic |

The Linux run found a bug the macOS run could not: a container process whose
`cmdline` contained newlines made `ratatui` panic with *"control character passed
to cell_width without filtering"*. Process names and command lines are arbitrary
bytes, so `meters::put_str` now sanitises every character it writes (tabs become
spaces, other control characters become `?`).

Performance, measured in that container (405 and 805 processes):

| processes | sequential | rayon |
|---|---|---|
| 405 | 2.25 ms | 1.50 ms |
| 805 | 2.93 ms | 1.91 ms |

`openat` per process per scan: 4.0 cold, 1.16 warm — the identity cache removes
71 % of the syscalls (DESIGN.md § 9.3).

Still unmeasured: render cost, memory footprint, and behaviour above ~1 000
processes. A container cannot usefully simulate a busy server.
