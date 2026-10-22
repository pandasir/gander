# Portability

How the three backends differ, what is verified where, and what is honestly
missing. Read [DESIGN.md § 5](DESIGN.md#5-platform-abstraction) for the trait
design.

## Status matrix

| | Linux | macOS | FreeBSD |
|---|---|---|---|
| Process list | `/proc` scan | `proc_listallpids` + `PROC_PIDTASKALLINFO` | `sysctl(KERN_PROC_PROC)` → `kinfo_proc[]` |
| CPU% | tick deltas from `/proc/<pid>/stat` | nanosecond deltas ÷ wall-clock interval | `ki_runtime` microsecond deltas ÷ wall-clock interval |
| Per-core CPU | `/proc/stat` | `host_processor_info` | `kern.cp_times` |
| Memory | `/proc/meminfo` | `host_statistics64` + `hw.memsize` | `vm.stats.vm.v_*` |
| Swap | `/proc/meminfo` | `sysctl(vm.swapusage)` → `struct xsw_usage` | `kvm_getswapinfo(3)` |
| Load average | `/proc/loadavg` | `getloadavg(3)` | `getloadavg(3)` |
| Boot time | `/proc/uptime` | `sysctl(kern.boottime)` | `sysctl(kern.boottime)` |
| Start time | field 22 of `stat` (ticks) | `pbi_start_tvsec/tvusec` | `ki_start` (a `timeval`) |
| cmdline | `/proc/<pid>/cmdline` | `sysctl(KERN_PROCARGS2)` | `sysctl(KERN_PROC_ARGS)` |
| exe | `readlink /proc/<pid>/exe` | `proc_pidpath` | `sysctl(KERN_PROC_PATHNAME)` |
| cwd | `readlink /proc/<pid>/cwd` | `proc_pidinfo(PROC_PIDVNODEPATHINFO)` | `sysctl(KERN_PROC_CWD)` → `struct kinfo_file` |
| uid | `/proc/<pid>/status` | `proc_bsdinfo.pbi_uid` | `kinfo_proc.ki_uid` |
| session | field 4 of `stat` | `getsid(2)` | `kinfo_proc.ki_sid` |
| tty / tpgid | fields 5 and 8 of `stat` | `e_tdev` / `e_tpgid` from `proc_bsdinfo` | `ki_tdev` / `ki_tpgid` |
| threads | `/proc/<pid>/task` | `PROC_PIDLISTTHREADS` + `PROC_PIDTHREADINFO` | `sysctl(KERN_PROC_PID\|KERN_PROC_INC_THREAD)` |
| priority | field 18 of `stat` | `pbi_nice` | `ki_pri.pri_level` |
| per-process disk I/O | `/proc/<pid>/io` — *read by nothing; no column exists* | `proc_pid_rusage` — *readable, not shown* | `ki_rusage` — counts I/O operations, not bytes |
| host network throughput | `/proc/net/dev` | `sysctl(NET_RT_IFLIST2)` → `if_data64` | `sysctl(NET_RT_IFLIST)` → `if_data` |
| host disk throughput | `/proc/diskstats` + `/sys/block` | none without IOKit | `devstat_getdevs(3)` |
| cgroup path | `/proc/<pid>/cgroup` | no cgroup concept | no cgroup concept |
| container detection | cgroup path + PID namespace | no containers | no containers (jails: `ki_jid`, not shown) |
| CPU affinity | `sched_getaffinity` | none (scheduler hint only) | `cpuset_getaffinity` |
| Test coverage | fixtures + live | live only | live only (verified in a VM) |

All three implement `PlatformSampler`, and `gander_core::Sampler` resolves to the
right one at compile time.

## FreeBSD-specific traps (all hit and fixed during development)

The FreeBSD backend was built and verified on FreeBSD 14.5-RELEASE/arm64 in QEMU,
with the test suite run inside the guest. These are the things that were wrong
first:

1. **`kern.proc` with a four-element MIB fails with `ENOTDIR`.** The documented
   spelling is `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_ALL)` — **three** elements.
   A fourth (the `arg`) is only valid for the selectors that take one. Passing it
   anyway produces `ENOTDIR`, which reads like "not a directory" and has nothing
   to do with directories; the size query succeeds with three elements and
   returns 105 KB of records, so the two calls disagree in shape as well as in
   error. Both `KERN_PROC_ALL` and `KERN_PROC_PROC` have this property.
2. **`KERN_PROC_ALL` really is "everything".** It returns per-thread rows *and*
   zombies' thread-group duplicates, and the first record is pid 0 (the kernel's
   thread container) rather than a process. `KERN_PROC_PROC` is the process table:
   one row per process, no pid 0. htop uses `KERN_PROC_PROC` too
   (`freebsd/FreeBSDProcessList.c`).
3. **`KERN_PROC_INC_THREAD` cannot be combined with a bulk selector.** ORing it
   into `KERN_PROC_PROC` fails with `ENOENT`; it only works with a specific pid.
   So threads are one `sysctl` per process, not one extra field on the bulk scan —
   which is exactly why they stay behind `ScanOptions::threads`.
4. **`ki_runtime` is microseconds, not ticks.** Every other CPU-time field on the
   other two platforms is a tick counter with a `HZ` divisor, and `kern.cp_time`
   *is* ticks (100 Hz), so dividing one by the other gives a percentage off by
   four orders of magnitude. `ki_runtime` is also what `ps`'s `TIME` column uses,
   so there is no ambiguity about its units once it is checked against `ps`.
5. **`ki_rssize` is pages; `ki_size` is bytes.** The two look like a pair and are
   not. Forgetting the page size on `ki_rssize` reports a 1.2 GB process as 300 MB
   — plausible enough that only a comparison against `ps -o rss=` catches it.
   `ki_size` needs no scaling, which is the inconsistency.
6. **`ki_stat` is not a state letter and is not even a function of the state.**
   `SIDL` (1) means "being created by `fork`" and `ps` prints `R` for it, not `I`.
   `SSLEEP` (3) splits three ways on `ki_tdflags & TDF_SINTR` and
   `ki_slptime >= MAXSLP`: uninterruptible sleep is `D`, an interruptible sleep
   older than 20 s is `I`, and everything else is `S`. `SLOCK` (7) is `L`.
   Mapping arithmetically (`b'I' + stat - 1`) produced `I` for a brand-new
   process and `X` for a locked one.
7. **`KERN_PROC_CWD` returns a `struct kinfo_file`, not a path.** The mnemonic says
   "cwd" but the payload is the file-descriptor record for the directory, with the
   path at `kf_path`. Reading the buffer as a C string yields a single NUL byte for every
   process — which looks exactly like "the kernel will not tell us", and is why
   the first version of `cwd_path` silently returned `None` for every row.
8. **`KERN_PROC_PATHNAME` and `KERN_PROC_CWD` need a buffer that is large enough
   for their fixed-size structs.** Their size query returns `ENOMEM` rather than a
   length, so the caller has to know the size in advance: `PATH_MAX` for the
   former, `sizeof(struct kinfo_file)` (1392 bytes on this build) for the latter.
   A `PATH_MAX`-sized buffer for CWD fails with `ENOMEM`.
9. **`kvm_getswapinfo` reports pages, not bytes and not kilobytes.** `swapinfo(8)`
   multiplies by `pagesize`. htop's FreeBSD backend multiplies by 1024 instead,
   which reports a 1 GiB swap file as 4 MiB while labelling the figure bytes.
   Doing it the way `swapinfo` does is the only choice that agrees with another
   tool on the same machine.
10. **`hw.physmem` is larger than the memory the VM manages.** On the test VM it
    is 4 265 984 000 bytes while `v_page_count * pagesize` is 4 159 893 504; the
    difference is the kernel's early allocations, which no page queue will ever
    report. Using `hw.physmem` as the bar's denominator leaves it permanently
    ~3 MiB short of full, which reads as a rendering bug on a machine that is
    genuinely out of memory. `v_page_count` is by construction the sum of the
    queues.
11. **`kinfo_proc` is 1088 bytes and `ki_structsize` must be checked, not
    assumed.** `libc`'s struct is compiled into the binary; the kernel's is
    whatever the running system was built with. They agree only if the binary
    was built for an ABI no newer than the kernel's, and a struct that grew makes
    every field after the addition read from the wrong offset — the table looks
    plausible while being wrong. `scan::validate()` compares `ki_structsize`
    against `size_of` and checks that pid 1 is `init`/uid 0 at startup.
12. **`c_long` is not `i64` on every FreeBSD target.** The CPU counters are
    `long[]`, which is 64-bit on amd64/arm64 and 32-bit on i386, so a cast is
    load-bearing on one target and a no-op on the others (and `clippy`'s
    `unnecessary_cast` fires on the 64-bit ones).
13. **A `sysctl` size query is racy by design.** `kern.proc` answers with the size
    *now*, and the table can grow before the read. Three retries with quadratic
    headroom is what `ps(1)` does. The same race is why the live tests compare
    memory counters with a tolerance: each `vm.stats` counter is a separate
    `sysctl`, so six calls see six slightly different moments on a VM whose page
    daemon is running.
14. **`devstat` entries include pass-through devices.** A `pass(4)` device
    observes the same requests as the physical device beneath it, so summing both
    double-counts every byte. `DEVSTAT_TYPE_PASS` marks them. `devstat` also
    needs `devstat_checkversion` before the list is trusted, for the same reason
    `kinfo_proc` needs `ki_structsize`: a struct that changed size would misread
    every counter.
15. **`libc` declares `devstat`/`devinfo` but not `struct statinfo`.** The
    wrapper is 56 bytes of doubles and a pointer that only `devstat_getdevs`
    reads, so it is declared here with the same field order.
16. **`KERN_PROC_FILEDESC`'s records are variable-length and truncated.**
    `libc`'s `struct kinfo_file` is 1392 bytes; the kernel truncates each record
    after `kf_path` and reports 376–408 bytes. Walking by `size_of` runs off the
    first record, and — the natural guard — rejecting records shorter than
    `size_of` rejects *every* one, so `l` showed "cannot read file descriptors"
    on a process whose descriptors were perfectly readable. The stride is each
    record's own `kf_structsize`, and `kf_path` is at a fixed offset of **368**,
    not at `offset_of!(kinfo_file, kf_path)`, because everything past the
    truncation point was never copied out. Verified against `procstat -f`.
17. **`KERN_PROC_FILEDESC` also lists things that are not descriptors.** `kf_fd`
    is `-1` for the cwd, `-2` for the root directory, `-5` for the executable text
    and `-6` for the controlling terminal. `procstat -f` prints all of them, and
    filtering to `kf_fd >= 0` would drop exactly the entries a reader wants when a
    process is stuck in a deleted directory.
18. **`KERN_PROC_ENV` and `KERN_PROC_ARGS` have no size query.** Their size call
    answers `ENOMEM` rather than a length, so the caller has to request `ARG_MAX`
    up front and trust the returned count. `proc_string` does that, bounded, so a
    process with a pathological environment cannot make gander allocate without
    limit.

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
    handle in `Row::pid`. `Row::thread_id` carries it instead; selection follows
    `(Row::identity(), starttime)`. Process actions and tags use `(pid, starttime)`,
    so actions on these thread rows apply to their owning process.
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
3. **A meter taller than the rows left was dropped whole.** `draw_column_indices`
   tested `y + h > area.bottom()` and `break`ed, so on a 128-core machine — where
   the CPU meter wants 65 rows and the header is clamped to the terminal — the
   *first* meter failed the test and the entire header rendered blank. It now
   clips to the rows that remain; the renderers already stop at `area.bottom()`,
   and a one-row slice of a split CPU meter relabels itself `CPU` instead of
   keeping `Avg`, which would read as "the average of what?".

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

## Memory accounting (this was wrong twice)

The macOS memory figure is the single easiest thing to get wrong, because
`vm_statistics64` exposes **two overlapping families of counters**:

- by usage: `active`, `inactive`, `wire`, `compressor`, `free`, `speculative`
- by backing store: `internal` (anonymous), `external` (file-backed)

Summing across families double-counts, so gander builds `used` from the
by-backing-store counter plus the two page categories that family does not cover:

```text
used      = (internal - purgeable) + wired + compressed
available = total - used
```

That is the same number `htop` 3.4.1 prints. htop writes it the long way round
(`darwin/Platform.c`, `Platform_setMemoryValues`):

```c
natural_t used = active + inactive + speculative + wire + compressor
                 - purgeable - external;
MEMORY_METER_USED       = used - compressor;
MEMORY_METER_COMPRESSED = compressor;
```

and `MemoryMeter_updateValues` displays `USED + COMPRESSED`. Since
`internal == active + inactive + speculative - external`, that sum is exactly the
formula above. htop `main` (3.6-dev) states it directly as a class list: `wired`,
`internal - purgeable`, `compressed` are `countsAsUsed`; `speculative`,
`purgeable`, `inactive` are `countsAsCache`. Activity Monitor and `top` agree.

Numbers from one real sample on the 16 GB M-series machine where the two tools
disagreed (page size 16384; GiB):

| counter | GiB |
|---|---|
| wired | 2.28 |
| active | 2.97 |
| inactive | 2.84 |
| internal | 2.85 |
| purgeable | 0.07 |
| compressed | 7.01 |
| free | 0.30 |
| **used** | **13.29** |
| htop 3.4.1 showed | 13.3 |

**The bar segments partition physical memory exactly.** Each page is drawn once:
`wired`, `active`, `compressed`, `purgeable`, `cache` (= `total - used -
purgeable - free`), `free`. `free` is the kernel's own free count, so it cannot
absorb memory some other class is holding.

Mistakes shipped before this landed, each one visible in the meter:

1. `used = active + wired` (htop 3.3.0's formula, compressor ignored) — reported
   **5.3 GB** on this machine while htop reported **13.3 GB**. The whole gap was
   the compressor. htop 3.3.0 has the `MEMORY_METER_COMPRESSED` assignment
   commented out; mimicking that made gander disagree with every other tool.
2. `used = wired + active + compressed` — `inactive` was left out, so an 8 GB
   machine under load displayed ~5.7 GB.
3. `available = total - wired - compressed` while `used` was computed separately,
   so `used + available = 9.3 GB` on an 8 GB machine.
4. The `free` *segment* was derived as `total - used - cache`. Once `used` stopped
   counting the compressor, ~7 GB of compressor pages fell into the free segment:
   the bar claimed 7.8 GB free on a machine with 0.3 GB free.

`MemInfo::is_exactly_partitioned()` encodes the `used`/`available` invariant and
`crates/core/tests/mem_accounting.rs` asserts both that and the segment partition
on every macOS test run.

**Linux is different on purpose.** There `used` follows GNU `free`
(`total - free - cached - sreclaimable - buffers`). That matches htop 3.4.1's
`linux/LinuxMachine.c` verbatim. `available` comes from `MemAvailable`, so the two
are *not* complementary — `MemAvailable` includes reclaimable cache that `used`
also accounts for, and `used + available > total` is correct on Linux. An earlier
revision tried to force the complement and clamped `MemAvailable` down to
`total - used`, understating available memory.

### FreeBSD is a partition, like macOS, but of different things

FreeBSD's VM keeps five mutually exclusive page queues, so the model is exactly
the kind of partition macOS's is — and the [traps](#freebsd-specific-traps-all-hit-and-fixed-during-development)
above explain why getting the denominator right matters:

```text
used      = active + wired + laundry
cache     = inactive + page cache
available = free + cache
total     = v_page_count * pagesize      (not hw.physmem)
```

`laundry` counts as used because those pages are dirty and queued for writeback:
they cannot be reused until the write completes, which is the same reasoning that
puts macOS's compressor in `used`. `active` counts as used because it is the
queue of recently touched pages the page daemon has not yet reclaimed. `inactive`
is the reclaimable half, which is why it is cache and not used.

`total` is the managed page count rather than `hw.physmem` for the reason in trap
10: the kernel's early allocations are not in any queue, so a bar scaled to
`hw.physmem` can never fill.

## Permission model

All three platforms hide information for processes you do not own:

- Linux: `exe`, `smaps_rollup` need `CAP_SYS_PTRACE` (root) to read others'.
- macOS: `proc_pidinfo`, `proc_pidpath`, `proc_pid_rusage`, `KERN_PROCARGS2` return
  `EPERM` for other users' processes.
- FreeBSD: `KERN_PROC_PATHNAME`/`KERN_PROC_CWD`/`KERN_PROC_ARGS` describe other
  users' processes only for root, and `kvm_openfiles` is denied inside a jail —
  which makes `swap` fall back to `vm.swap_total` with `used` unknown rather than
  reporting a machine with no swap.

The UI shows `-` rather than failing. This is correct behaviour, not a bug; it is
also why `htop` runs `sudo htop` when you want full detail.

## Deliberate omissions on macOS

| Feature | Why |
|---|---|
| cgroup, container columns | No cgroup concept. Fields are `#[cfg]`-gated out, so `CGROUP` and `CONTAINER` do not appear in the setup screen at all rather than appearing and rendering `-`. |
| PSI pressure meters | No `/proc/pressure`. The `PSI` meter says `n/a here`. |
| Zram / Zswap / HugePages | Linux-only subsystems. ZFS ARC could be added later. All three meters say `n/a here`. |
| PSS / EPSS | No `smaps_rollup`; `ri_phys_footprint` is the closest analogue and is already available via rusage. The columns are `#[cfg]`-gated out, so they do not appear in the setup screen rather than appearing and rendering `-`. |
| Thread rows | Supported. `PROC_PIDLISTTHREADS` + `PROC_PIDTHREADINFO` are both `libproc`; the earlier note here claiming this needed the deprecated `task_threads` Mach API was wrong. Threads of processes owned by another user are not listable (`proc_pidinfo` returns 0), so those rows simply have none. |
| `a` affinity panel | `thread_policy_set` exists but is per-thread, not per-process; deferred. |
| `i` I/O priority | No `ioprio_set` equivalent. |
| GPU columns | Needs `IOKit`/`IOAccelerator`; a different mechanism from Linux `drm-usage-stats`, and the design forbids a vendor SDK. Gated out of the column list on macOS. |
| Delay accounting | `TASKSTATS` is a Linux generic-netlink family with no Darwin equivalent. The `CPUD%`/`IOD%`/`SWPD%` columns are gated out on macOS. |
| `DiskIO` meter (host-wide) | No host-wide disk byte counter without IOKit. Per-process `rusage` figures exist but summing them double-counts shared I/O. The meter reports `n/a here`. |
| Container detection (`O`, `CONTAINER`) | No cgroup concept. Gated out of the column list, and `O` says so instead of toggling a no-op. |

## Deliberate omissions on FreeBSD

The same rule: a *column* that can only render `-` is `#[cfg]`-gated out, while a
*meter* survives with `n/a here` so a config file still works when moved.

| Feature | Why |
|---|---|
| cgroup, container columns | No cgroup concept. Jails are the nearest analogue and `kinfo_proc` carries `ki_jid`, but a jail is not a container: it shares the kernel, has no image layers and no id beyond a small integer. Listing it as `CONTAINER` would be a false equivalence. |
| PSI pressure meters | No `/proc/pressure` equivalent. |
| Zram / Zswap / HugePages | None of the three subsystems exists. All three meters say `n/a here`. |
| PSS / EPSS / private / swap-per-process | No `smaps_rollup`. FreeBSD has `kinfo_getvmmap(3)`, which reports per-mapping residency, but a `PSS`-equivalent would have to divide shared mappings by their share count, which the kernel does not expose. Gated out. |
| GPU columns | No `drm-usage-stats` in `fdinfo`. FreeBSD's DRM drivers do not export it; a different mechanism (IOKit territory on the Linux side of the fence) would mean a vendor SDK, which the design forbids. |
| Delay accounting | `TASKSTATS` is a Linux generic-netlink family with no FreeBSD equivalent. Gated out. |
| `i` I/O priority | No `ioprio_set`. FreeBSD has `idprio`/`rtprio` for realtime scheduling, which is a different question. The panel says so instead of pretending. |
| `DiskIO` meter (host-wide) | **Supported**, via `devstat_getdevs(3)`. This is the one platform where the per-process and host-wide figures both exist; only the host-wide one is shown, for the same double-counting reason as macOS. |
| Per-process disk I/O columns | `ki_rusage.ru_inblock`/`ru_oublock` are I/O **operation counts**, per `getrusage(2)`: "the number of times the file system had to perform input". Not bytes and not blocks. There is no column for them, and none is planned here — see `docs/ROADMAP.md` § M5 for why the family would have to be absent rather than dashed. |
| `a` affinity panel | **Supported**, via `cpuset_getaffinity`/`cpuset_setaffinity`. |
| `e` environment panel | **Supported.** `sysctl(KERN_PROC_ENV)` is the same NUL-separated payload as `/proc/<pid>/environ`. |
| `l` open files panel | **Supported**, via `sysctl(KERN_PROC_FILEDESC)` — see the truncation trap below. |
| `x` file locks panel | No `/proc/locks` and no `sysctl` for the lock table. The panel says so and points at `lsof`. |

### Jails: detected, not displayed

`kinfo_proc.ki_jid` is read into `RawProc::jail` and is available to a future
`JAIL` column, but nothing renders it yet. The reason is the same one that governs
`CONTAINER`: an identifier that no other column groups on is a number, not
information. `jid 0` means "host", which is the common case, so a column would be
a list of zeroes with a few exceptions — the inverse of useful.

## Network and disk counters

All three backends report host-wide throughput, but not from the same kind of
source, and one obvious choice is wrong on each platform.

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

### FreeBSD disk: `devstat`, and the pass-through trap

`devstat_getdevs(3)` is the kernel's own per-device statistics interface, so
unlike Linux there is no partition-versus-disk ambiguity: each entry is a whole
device and its counters already include everything written through its
partitions.

The trap is `DEVSTAT_TYPE_PASS`. Those are `pass(4)` devices, which see the *same*
requests as the `da`/`ada` device they front; summing both doubles every byte.
`devstat_checkversion` must also pass first, for the same reason `kinfo_proc`'s
`ki_structsize` must: a struct that changed size misreads every counter.

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

## M3: what Linux has that macOS does not, and how

The M3 features are all reads of kernel files that have no Darwin equivalent, so
each one is gated in three places rather than one:

1. **The `Field` variant** is `#[cfg(target_os = "linux")]`, so the column does not
   exist in the setup screen on macOS at all. This is the same treatment `CGROUP`
   got, and for the same reason: a column that can only render `-` is a setup
   entry that does nothing.
2. **The `Row` field** is gated with it, so a macOS build cannot accidentally carry
   a `pss` that is always `None`.
3. **`ScanOptions`** carries the flag anyway, and the macOS sampler accepts it and
   ignores it. That is deliberate: the TUI asks for what it needs in one call
   without naming a backend, and a backend that has nothing to switch on must not
   be a special case at the call site.

The meters are different. `PSI`, `Zram`, `Zswap` and `HugePages` exist on both
platforms and report `n/a here` on macOS, because a *meter name* has to survive a
config file moving between machines. A header edited on a Linux box and copied to a
Mac keeps its layout; the meters that cannot work there say so instead of vanishing
and renumbering everything.

### What the Linux side reads

| Feature | Source | Permission |
|---|---|---|
| PSS / EPSS / private / swap | `/proc/<pid>/smaps_rollup` | own processes; others need `CAP_SYS_PTRACE` |
| PSI | `/proc/pressure/{cpu,memory,io,irq}` | world-readable |
| zram | `/sys/block/zram*/mm_stat` | world-readable |
| zswap | `Meminfo: Zswap`, `/sys/module/zswap/parameters/max_pool_percent` | world-readable |
| huge pages | `Meminfo: HugePages_*`, `Hugepagesize` | world-readable |
| GPU | `/proc/<pid>/fdinfo/*`, `drm-*` keys | own processes |
| Delay accounting | `TASKSTATS` generic netlink | **`CAP_NET_ADMIN`** |

The expensive ones — `smaps_rollup`, `fdinfo` and the `TASKSTATS` round trip — are
only read when a column that needs them is on screen. See [ROADMAP.md § The cost gate](ROADMAP.md#the-cost-gate-and-why-it-is-one-struct).

### The trap in `/proc/meminfo`'s huge page counters

Every other field in `meminfo` is in kB, and `parse_meminfo` multiplies by 1024 for
all of them. `HugePages_Total`, `HugePages_Free`, `HugePages_Rsvd` and
`HugePages_Surp` are **page counts**, not kB. Sending them through the same
multiply reports a 512-page reservation as 512 kB instead of 1 GiB — and 1 GiB is
the number that explains where the memory went. Only `Hugepagesize` is in kB.

## What is verified, and where

All three backends have been exercised against a live OS.

**macOS (arm64, this machine)** — 73 core tests plus the TUI suite, including live
tests that call `proc_pidinfo`, `host_statistics64`, `KERN_PROCARGS2`,
`host_processor_info` and `KERN_PROC_ALL`.

**Linux (arm64, via `rust:1.89-slim` in Docker)** —
`crates/core/tests/linux_live.rs`, twenty-one tests that read the real `/proc` and
compare gander's figures against the same files parsed independently, plus
`m3_fixture.rs`, which exercises the M3 read paths against the committed fixture
tree on any unix:

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
| PSI vs a direct read of `/proc/pressure/io` | within 25 points (it moves between reads) |
| delay percentages | bounded to `0..=100`, all three present or all three absent, and never populated unasked |
| delay accounting without `CAP_NET_ADMIN` | `EPERM` once, then every column `-` — asserted, not just observed |
| huge pages | `used = total - free`, in bytes; page counts not multiplied by 1024 |
| PSS | bounded by RSS, `private <= pss`, `epss == pss + swap` |
| `smaps_rollup` unread by default | every row has `pss == None` |
| GPU percentages | within `0..=100`, and a percentage implies engine time was read |

**FreeBSD (arm64, 14.5-RELEASE in QEMU)** —
`crates/core/tests/freebsd_live.rs`, twenty tests that compare gander's figures
with `sysctl(8)`, `ps(1)`, `swapinfo(8)` and `netstat(1)` *inside one test*, so a
changing system cannot explain a discrepancy and a fixed expectation cannot go
stale. The whole workspace suite (134 core unit tests, 148 TUI tests, 33 config
tests, 11 CLI tests) also runs inside the guest.

| check | result |
|---|---|
| total memory | exactly `v_page_count * pagesize`, not `hw.physmem` |
| page queues vs `vm.stats.vm.v_*` | each within 16 MiB (six separate `sysctl` calls) |
| `used` = `wired + active + laundry` | exact identity |
| bar segments vs total | within 1 % (same reason) |
| swap vs `swapinfo -k` | within 64 KiB |
| core count | `hw.ncpu` + 1 aggregate; no core exceeds the aggregate |
| CPU% over two samples | finite, non-negative, capped at `ncpu * 100` |
| process count vs `ps -ax` | within 5 |
| RSS vs `ps -o rss=` | within 25 % (a pages-vs-bytes error would be off by 4096x) |
| `ELAPSED` vs `ps -o etimes=` | within 2 s, and never older than uptime |
| state letter vs `ps -o stat=` | every non-transitioning process agrees |
| `cmdline`/`exe`/`cwd` vs `ps -o args=` and the cwd | exact |
| `kern.hostname` vs `gethostname(2)` | exact |
| network counters vs `netstat -ibn` | at least their sum (the VM keeps talking) |
| disk counters | monotonic; rates finite and non-negative |
| threads | off by default; every thread row has a unique identity and a process row |
| sort by 16 fields × 2 directions | a permutation, with ties in ascending pid order |
| a process that exits | gone from the next sample |

Four bugs the live run found that the code review had not:

* **`SSLEEP` is three states, not one.** Every sleeping process rendered `S`; the
  `D`/`S` split is `ki_tdflags & TDF_SINTR` and `I` is `ki_slptime >= MAXSLP`.
  `SIDL` was also `I` when `ps` prints `R`. Collapsing `D` into `S` hides exactly
  the state that explains a machine that feels hung.
* **`comm_buf` truncated to *empty*.** `ArrayString::try_push_str` rejects a
  string that does not fit and leaves the buffer as it was, so a long name
  produced a blank `COMMAND` instead of a shortened one. The fully qualified path
  in the test's own argv triggered it on every run.
* **`ELAPSED` subtracted the boot time twice.** It computed `uptime - (start -
  boot)`, which is 0 for every process on a machine whose start is newer than the
  boot — that is, all of them.
* **`cwd` was `None` for every process.** `KERN_PROC_CWD` returns a
  `struct kinfo_file`, and a `PATH_MAX` buffer is too small for it (`ENOMEM`).

The M3 run found a bug of a different kind: the *documentation* was wrong. An
early version of the PSI code asserted that `/proc/pressure/cpu` has no `full`
line, on the reasoning that a CPU stall is inherently per-task. A live test
against a real kernel failed immediately — `cpu` reports both lines, and the code
was already parsing whichever the file contained. The claim is now removed from
`PsiLine`'s doc comment and from `PsiMeter`'s, and `has_full` is read from the file
rather than assumed. Nothing about the runtime behaviour changed; the comments had
simply stated something false, and a test that asserted the false thing would have
passed on the fixture and failed on any real machine.

The original Linux run found a bug the macOS run could not: a container process whose
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

Performance on FreeBSD was not measured: the test VM has 11 processes, which
measures syscall latency rather than a scan. The backend makes **one** `sysctl`
for the whole table — fewer than Linux's one `openat` per process — so it should
be faster per process, but "should be" is not a measurement.

Still unmeasured on all three: render cost, memory footprint, and behaviour above
~1 000 processes. A container or a 4-core VM cannot usefully simulate a busy
server.
