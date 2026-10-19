# Roadmap

What is done, what is next, and what is deliberately not planned. The full
design lives in [DESIGN.md](DESIGN.md).

## Done (M0 skeleton)

M0 is complete on both platforms. The vertical slice compiles, is tested, and runs
on Linux **and macOS**.

| Area | State |
|---|---|
| Workspace split (`core` / `config` / `tui` / `bin`) | done |
| `/proc` parsing: `stat`, `status`, `meminfo`, `stat`, `loadavg`, `uptime`, `smaps_rollup`, `cgroup` | done, fixture-tested |
| Two-sample CPU% delta, memory accounting, PID-reuse detection | done |
| Immutable-data cache keyed by `(pid, starttime)` | done |
| Parallel enumeration (rayon, gated on process count) | done |
| Row model, field metadata, human formatting | done |
| Sort, filter (`|`-separated), incremental search | done |
| Tree build + htop-style `├─ └─ │` prefixes | done |
| Process table rendering, header meters, function bar | done |
| Meters: CPU (bar/text/graph), memory, swap, load, uptime, tasks, clock, date, hostname, blank | done |
| Themes: `default`, `monochrome`, `midnight` | done |
| Config: TOML load/save, `htoprc` importer with the `id - 1` offset handled | done |
| Config: per-meter header layout with per-meter display mode | done |
| Batch mode: `-n`, `--json`, `--filter`, `--sort`, `--limit` | done |
| Kill / renice | done |
| htop keybindings for implemented features (`a`, `i` and `Delete` now bound) | done |
| macOS backend: `libproc` + `sysctl` + Mach | done, see [PORTABILITY.md](PORTABILITY.md) |
| Correct memory accounting on both platforms (see PORTABILITY.md) | done |
| Full process table on macOS via `KERN_PROC_ALL` | done |
| 300 tests on macOS, 279 on Linux, zero clippy warnings on both | done |
| Live Linux verification in a container, incl. affinity/ioprio syscalls | done, see [PORTABILITY.md](PORTABILITY.md) |

## The interface

The default layout is **`panels`**: a vertical system column, a leaner process
table with in-table value bars, and a persistent inspector that shows the command
line and exact numbers without leaving the list. `L` switches to the classic htop
arrangement, and `layout = "htop"` makes that permanent.

The reasoning is in [LAYOUT.md](LAYOUT.md). In short: htop puts its meters on the
scarce axis (vertical), makes you leave the list to learn anything about a
process, and spends a row per core.

| Area | State |
|---|---|
| `panels` layout: system column, table, inspector | done |
| Vertical system column (aggregate CPU, core grid, mem/swap, load, uptime, tasks, sparkline) | done |
| Persistent inspector: wrapped command line, state in words, exact numbers | done |
| In-table value bars behind `CPU%` / `MEM%`, scaled table-wide | done |
| Lean default columns for the narrow table, without overriding an explicit `F2` choice | done |
| Graceful degradation down to a 30x4 terminal, falling back to the htop layout | done |
| `L` to switch, persisted in the config | done |
| Classic htop layout still available and fully supported | done |

## Next: finish M0

Done:

- [x] `F9` signal picker actually sends the selected signal
- [x] Viewport-aware paging (`PgUp`/`PgDn`)
- [x] Horizontal scroll (`Left`/`Right` clip columns instead of being dropped)
- [x] Scroll-following so the selection stays on screen
- [x] `Tasks` meter shortens itself to fit narrow headers
- [x] Function bar separates key and label, drops keys instead of cutting words
- [x] `u` user filter (cycles through the uids present)
- [x] `USER` column shows names, not numeric uids
- [x] `F2` setup screen: Header, Columns, Tabs, Display, Colors, About
- [x] `Z` security context and `b` backtrace explain themselves instead of
      silently doing nothing
- [x] Screen tabs (`Tab` / `Shift+Tab`) with per-tab columns and sort, plus a
      Tabs page in the setup screen (add/select/delete)
- [x] Header editing in the setup screen (a Header page: add, delete, reorder
      and set each meter's mode)
- [x] Affinity panel (`a`): real read/write on Linux via
      `sched_getaffinity`/`sched_setaffinity`; macOS explains that it only has a
      scheduler hint
- [x] I/O priority panel (`i`): real read/write on Linux via
      `ioprio_get`/`ioprio_set`; unsupported platforms say why
- [x] Per-meter display mode: `I` cycles each meter independently, and the choice
      is persisted per meter in the config
- [x] `runtime.rs` moves the sample out of the `ArcSwap` instead of cloning the
      row vector, so a sample no longer copies thousands of `Box<str>`
- [x] `Clock` / `Date` use the real local time zone (`localtime_r`), not UTC
- [x] `Hostname` uses `gethostname(2)` on macOS, where `/proc` does not exist
- [x] htoprc import handles `column_meters_N`, `column_meter_modes_N` and
      `screen:NAME=` tab sections
- [x] Per-meter cursor in the header (`Shift+Left`/`Shift+Right`), so `I` changes
      only the selected meter, and the selection is underlined
- [x] `PlatformSampler` is implemented by both backends; `sample`/`cache_len` live
      only on the trait, so neither platform can grow a method the other lacks
- [x] Settings changed inside the TUI are actually saved on exit (the exit path
      used to write back the config it had *started* with)
- [x] Linux verified in a Debian container against a real `/proc`, including the
      affinity and ioprio syscalls
- [x] Thread enumeration as rows on Linux (`H` / the Display page), with threads
      nested under their process in tree view and their own per-thread CPU%
- [x] Thread enumeration on macOS, via `PROC_PIDLISTTHREADS` +
      `PROC_PIDTHREADINFO`. Both are `libproc` calls; there is no Mach port and no
      `task_for_pid` involved, and one call per thread returns both the name and
      the CPU time. Measured at 2–5 ms per sample for 2609 threads. See
      [PORTABILITY.md](PORTABILITY.md) for the nanoseconds-vs-Mach-ticks trap.

Remaining:

- [ ] Nothing. M0 is complete on both platforms.

Two pre-existing bugs were found and fixed while adding macOS threads, both of
which also affected Linux:

- `TaskStats` summed `nlwp` over *every row*, so with thread rows visible the
  `Tasks` meter reported roughly twice the real process and thread counts.
- `tree::build` keyed its parent map by `pid` across all rows. A thread's tid can
  equal an unrelated process's pid, which silently re-parented that process; on
  macOS, where a thread row shares its leader's pid outright, the collision was
  guaranteed.

Thread rows also gained `Row::thread_id`, because a macOS thread has no pid of its
own: `pth` handles are 64-bit (95% exceed `u32`) and are *not* globally unique —
every process reports the same handle for its main thread. Tags, selection and
kill now key on `Row::identity()` rather than `pid`.


## M1 — feature parity for daily use

Complete. What was missing at the end of M0 was the two throughput meters and
search/filter history; both are done, along with two bugs found on the way.

| Area | State |
|---|---|
| Meter system: CPU (per-core variants), memory, swap, load avg, uptime, tasks, clock/date, hostname, blank | done (M0) |
| Meter display modes: text / bar / graph, per-meter, persisted | done (M0) |
| Full column set in a setup screen, with column reorder | done (M0) |
| Themes + monochrome mode | done (M0) |
| TOML config, `--import-htoprc` | done (M0) |
| `NetworkIO` meter: host receive/transmit rate | done |
| `DiskIO` meter: host read/write rate | done, Linux only (see below) |
| Search/filter history in `$XDG_STATE_HOME`, with `Up`/`Down` recall | done |

### How the throughput meters work

Both are *rate* meters over cumulative kernel counters, not counters. A meter
showing "1.9 GB" answers how much the machine has ever transferred, which nobody
asks; the question is what it is doing now. So the sampler keeps the previous
counters plus the `Instant` of the previous sample, and the snapshot carries the
delta divided by the **measured** interval — not the configured delay, because a
slow scan makes the real gap longer.

The bar is scaled against the largest combined rate observed so far rather than
against a fixed ceiling. A 1 Gbit and a 100 Mbit link differ by an order of
magnitude; a bar pinned to 1 Gbit would be an invisible sliver on the slower one,
and one pinned to 100 Mbit would sit saturated on the faster one.

| | Source | Notes |
|---|---|---|
| Linux network | `/proc/net/dev` | Loopback excluded; sums every other interface |
| Linux disk | `/proc/diskstats` | Whole disks only, via `/sys/block`, sectors × 512 |
| macOS network | `sysctl(NET_RT_IFLIST2)` | `if_data64`, because `getifaddrs`'s counters are 32-bit |
| macOS disk | — | No host-wide counter without IOKit; the meter says `n/a here` |

`capability::HOST_DISK_IO` gates the disk meter, so macOS explains itself instead
of drawing a permanent `0 B/s` that looks like a dead disk.

### Bugs found while doing it

- **`Esc` could never clear a filter.** `handle_input_key` reset `self.input` and
  *then* asked `self.input.kind`, which by then was `None`. The only way out of a
  filter was to erase it one character at a time.
- **The search/filter prompt was never drawn.** `F3`, `F4` and every typed
  character went into `App::input`, the filter applied live, and the screen showed
  nothing — so a filter that hid every row looked like a sampler bug, and there
  was no way to see that a filter was active. It now occupies the table's header
  row in both layouts, which is where htop puts it.

## M2 — power features

Complete. Screen tabs, the affinity/I/O panels and the modal screens landed ahead
of schedule during M0/M1; the cgroup columns, bulk renice, base-name highlighting
and the `O` filter are this milestone's own work.

| Area | State |
|---|---|
| Screen tabs with per-tab columns and sort | done (ahead of schedule) |
| CPU affinity (`a`) and I/O priority (`i`) panels | done (ahead of schedule) |
| Open files (`l`), env (`e`), command line (`w`), file locks (`x`) | done (ahead of schedule) |
| `CGROUP` column: the process's cgroup path | done |
| `CONTAINER` column: runtime + container id | done |
| Bulk actions across the tagged set (`F9` kill, `F7`/`F8` renice) | done |
| Hide-containerised filter (htop's `O`) | done, Linux only |
| Highlight base name / stale exe | done |
| Stable list: PID-ascending default, deterministic tie-break | done |

### Why the default sort is PID, not CPU%

htop opens sorted by CPU% descending. On a busy machine that reorders the list on
every sample, so the row you are looking at moves out from under the cursor before
you can act on it. Measured here on a 10-core Mac: of the 12 visible rows, **10
changed position within 1.5 s**. A list that reshuffles that fast cannot be read as
a list, and it makes tagging or killing the right process a race.

PID order is stable: a process keeps its position for as long as it lives. It is
ascending, so the long-lived system processes sit at the top and new ones appear at
the bottom, where they are easy to spot. `F6` / `<` `>` still reaches CPU% in one
keypress, and anyone who wants htop's behaviour can set `field = "PERCENT_CPU"`,
`desc = true` in the config.

Two bugs were in the way of this, both of which made *any* sort look unstable:

- **`App::new` overwrote the configured direction.** Three lines set
  `dir = Dir::Desc` unconditionally, to give CPU% its natural order. The effect was
  that `desc = false` in the config was ignored for every field, so asking for
  ascending PID order silently produced descending, with no fix short of editing
  the source.
- **Ties were reversed and unordered.** The sort built an ascending order and then
  `reverse()`d the whole vector for a descending sort, which reversed every *tie*
  too: on a quiet machine, "CPU% descending" listed pids as 10, 9, 8, … Separately,
  `sort_unstable_by` leaves ties unordered, so equal rows could swap between
  samples. Both now break ties by pid, ascending, in either direction.

### M2's last three items

**Bulk renice.** `F9` already sent a signal to every tagged process; `F7`/`F8`
reniced only the row under the cursor. Two keys side by side where one honours tags
and the other silently ignores them is worse than not supporting bulk renice at
all, because it teaches the wrong expectation. Both now share the same target
selection: the tagged set if anything is tagged, otherwise the selection.

**Base-name highlighting.** `highlight_base_name` and `highlight_deleted_exe` were
in the config, in the setup screen, and in the htoprc importer, and read by
nothing. `Row::base_name_span` now locates the executable's base name and the
renderer patches its cells with `Style::patch`, so the row's own state (selected,
tagged, kernel thread) still shows through.

The span is found from `exe` when available rather than by splitting the command on
`/`, because `grep /usr/lib/foo` has a path in an *argument*. Without `exe`
(another user's process) it falls back to argv[0]. A deleted executable is detected
from the ` (deleted)` suffix the kernel appends to the `/proc/<pid>/exe` link
target — the only signal available, since comparing mtimes needs the file to still
exist.

**`O` hides containerised processes.** Gated on `capability::CGROUP`: on macOS the
key says "container detection is Linux-only" instead of flipping a toggle that
cannot change anything. Verified against a real nested PID namespace in a
container: five rows before, four after, and the hidden one was the containerised
process.

### A bug found while doing this: the status line was never drawn

`App::status` is written by a dozen actions — renice results, kill failures,
"showing threads", the reason `O` does nothing on macOS — and **no code ever drew
it**. Every message was invisible. The existing tests passed because they read the
field rather than the screen, which is exactly the gap that hid it.

It is now painted right-aligned on the function bar, capped at half its width so
`F1 Help` and the left-hand keys stay visible, and cleared by the next action that
has nothing to say — otherwise `K` after `O` would leave "container detection is
Linux-only" on screen as if `K` had produced it.

### How container detection works

Two signals, because neither is sufficient on its own.

**The cgroup path is the informative one.** `/proc/<pid>/cgroup` names the runtime
and the container, so it is what the column displays:

| runtime | example path | shown as |
|---|---|---|
| Docker (v1) | `/docker/1f2e3d4c…` | `docker:1f2e3d4c5b6a` |
| Docker (v2, systemd) | `/system.slice/docker-1f2e3d4c….scope` | `docker:1f2e3d4c5b6a` |
| Podman | `/machine.slice/libpod-1f2e….scope` | `podman:1f2e3d4c5b6a` |
| Kubernetes | `…/kubepods-burstable-pod<uid>.slice/cri-containerd-<id>.scope` | `kubepods:<uid>` |
| LXC | `/lxc.payload.<name>` | `lxc:<name>` |
| systemd-nspawn | `/machine.slice/machine-<name>.scope` | `nspawn:<name>` |

The bare marker (`container_from_cgroup` used to return `"docker"`) is not enough:
every containerised process on the machine then renders the same word, which is a
boolean dressed up as data. The id is what distinguishes rows. Kubernetes is
tested before the plain runtimes because its leaf is `cri-containerd-<id>.scope` —
testing `containerd` first loses the pod identity, and the pod is the thing that
stays put across a container restart.

**The PID namespace is the membership test.** A container with its own cgroup
namespace sees `/` in `/proc/<pid>/cgroup`, so the path says nothing and a
cgroup-only check reports "no container" for a process that is plainly inside one.
Comparing `/proc/<pid>/ns/pid` against **our own** `/proc/self/ns/pid` answers the
question the column asks. Anchoring on `/proc/1` looks equivalent and is not:
inside a container pid 1 *is* the container's init, so every process compares equal
and nothing is ever flagged. htop does the same thing (`LinuxProcessTable.c` reads
`PROCDIR "/self/ns/pid"` into `rootPidNs`).

When the namespace says "containerised" but the cgroup path could not name it, the
column says `container:?` rather than inventing an identifier. A namespace inode
is not a container id, and printing one would look like an answer.

### Two bugs found here

- **`row.cgroup` held the raw `/proc/<pid>/cgroup` line**, so the column would have
  shown `0::/init.scope` — hierarchy id and an empty controller list — for every
  process. It now holds the path, preferring the unified (empty-controller) entry.
  The field had been loaded since M0 and never displayed, which is why nobody
  noticed.
- **A new `Field` with no arm in `sort_order` silently sorts by pid.** The `_` arm
  is the pid comparator, so "sort by CONTAINER" reordered by pid and looked like a
  sort that did nothing on an already pid-sorted list. Both new columns have their
  own comparator, and there is a test for each.

## M3 — Linux depth

Shipped in v0.4.0. Four of the five items are done; delay accounting is the
remainder and stays last, because it needs raw `TASKSTATS` netlink and the payoff is
small next to the four that landed. It is the only unchecked item in the table and
the milestone is otherwise closed.

| Area | State |
|---|---|
| `M_PSS` / `M_EPSS` / `M_PRIVATE` / `M_SWAP` columns from `smaps_rollup` | done, read lazily |
| PSI pressure: `PSI` meter, and a pressure block in the `panels` column | done |
| `Zram` / `Zswap` / `HugePages` meters | done |
| GPU columns (`GPU%`, `GPU_TIME`) via `drm-usage-stats` in `fdinfo` | done |
| Delay accounting (`TASKSTATS` netlink) | not started |

### The cost gate, and why it is one struct

Two of the four new features are expensive per process per tick, in different ways:

| Read | What it costs | Why |
|---|---|---|
| `/proc/<pid>/smaps_rollup` | a kernel walk over every VMA | tens of ms across a desktop's table |
| `/proc/<pid>/fdinfo/*` | one `readdir` plus a read per descriptor | a few thousand small reads |

Both are therefore off unless a column that needs them is on screen. That decision
used to be a pair of `set_*` methods on the trait, which worked until there were
three: the front end had to remember to call every setter on every change, and a
forgotten one produced either a wasted scan or a column of dashes. It is now a
single `ScanOptions` value (`threads`, `smaps`, `gpu`), passed in one call, packed
into one `AtomicU64` for the sampler thread so it can never observe a half-updated
combination, and derived in one place from the active tab's visible columns rather
than from the config as a whole — a PSS column on a tab nobody is looking at costs
nothing.

`Field::needs_smaps_rollup()` and `Field::needs_gpu_fdinfo()` live next to the
`Field` enum, so a future derived column cannot be added without answering the
question. `gander --sort M_PSS` pays for what it asks for, because batch mode drives
the same switch from the sort field.

### What PSS is for, and what RSS gets wrong

RSS counts a shared page once per process. Two processes sharing 100 MB of
libraries both report 100 MB, and summing the `RES` column says the machine uses
200 MB it does not. PSS divides each shared page by the number of sharers, which is
the only per-process figure that adds up across the table. `M_PRIVATE`
(`Private_Clean + Private_Dirty`) is the complement: what would actually be freed
if the process exited. `M_EPSS` adds the swapped-out equivalent, which is why it
and `M_PSS` differ on a machine that has been swapping.

`None` means *not read*, and it renders `-`. It never means zero: another user's
`smaps_rollup` needs `CAP_SYS_PTRACE`, and a column the sampler was not asked to
fill is equally unknown. `Some(0)` renders `0B`, because that is a measurement.

### PSI: the one figure that says "struggling" rather than "busy"

CPU% and load average both describe how much work there is. Neither says whether
work is *waiting*. A box at 90 % CPU with an empty run queue is healthy; a box at
30 % CPU whose `io full` average is 40 % is making every process block on the disk,
and both of the familiar numbers look unremarkable while it happens.

`/proc/pressure/<resource>` reports two lines:

```text
some avg10=0.00 avg60=0.00 avg300=0.00 total=0
full avg10=0.00 avg60=0.00 avg300=0.00 total=0
```

`some` is the share of wall time at least one task was stalled; `full` is the share
where *every* runnable task was stalled. The meter shows both — `some` as the bar
and `full` as a brighter overlay in bar mode, `full` as the number — because high
`some` with low `full` is contention and high `full` is a real stall.

**Every resource reports both lines, `cpu` included.** An early draft of this
milestone asserted otherwise, and a live test against a real `/proc/pressure/cpu`
caught it. The parser reads `has_full` from the file rather than from a table of
assumptions about which resources have one.

PSI needs Linux 4.20+ and `CONFIG_PSI=y`. When the files are absent the sampler
reports `psi: None` and the meter says `n/a here` — three empty bars would be
indistinguishable from a machine under no pressure at all.

### zram, zswap, huge pages

Each answers a question the memory bar cannot.

`Zram` shows `orig → compressed (ratio)`. A 4:1 ratio means 4 GB of swap is
occupying 1 GB of RAM; printing only the compressed figure understates what is
stored, printing only the uncompressed one overstates what it costs. The bar is
drawn against the compressed figure as a share of physical memory, because the
ratio itself has no natural scale.

`Zswap` shows bytes in the pool against the pool's ceiling, which comes from
`max_pool_percent` (a percentage of physical memory — resolved after `MemTotal` is
known). That is the figure that answers "will the next page spill to disk".

`HugePages` shows `used/total` in bytes. The counters in `/proc/meminfo` are in
*pages*, not kB, so they must not go through the `× 1024` path every other field
uses; a 512-page reservation would read as 512 kB instead of 1 GiB. `used` is
`total - free` rather than `total - free - rsvd`, because reserved pages are
already committed to a process that has not faulted them in yet.

All three report `n/a here` when the subsystem is absent — no zram devices, zswap
disabled, or no huge pages reserved — rather than drawing a permanent empty bar.
That is the same rule `DiskIO` follows on macOS.

### GPU without a vendor SDK

The kernel exports `drm-usage-stats` keys in each process's `fdinfo`:

```text
drm-driver:     amdgpu
drm-engine-gfx: 5000000000 ns
drm-engine-compute: 1000000000 ns
drm-busy:       25000000000 ns
```

Two details decide whether the reading is right:

- **Engine classes are summed by prefix**, not by name. They vary by driver
  (`gfx`, `render`, `video`, `compute`, …), and a process that only uses video
  decode must not read as "no GPU".
- **The percentage is engine time over `drm-busy`, not over the sample interval.**
  `drm-busy` is the time the GPU was busy *while this process had work queued*, so
  it is the only correct denominator. A driver that reports engine time without
  `drm-busy` yields `None`, not a guess.

A process with no DRM descriptor renders `-`, not `0.0`: it is not using zero
percent of the GPU, it is not using the GPU at all, and those read differently in a
sorted list.

## Deliberately not planned

- A PCP-style dynamic metric engine (runtime-defined columns/meters).
- Any daemon, setuid helper, or eBPF dependency.
- Windows support.

## Known limitations

- On macOS the per-process state is only trustworthy for `stopped` and `zombie`.
  Current Darwin reports `SRUN` for a process blocked in `read(2)`, so runnability
  comes from `proc_taskinfo.pti_numrunning` instead. Processes owned by another
  user cannot be inspected that way, and are shown as `?` rather than guessed at.
- macOS has no per-process CPU affinity mask and no I/O priority; both panels say
  so instead of failing silently.
- Rayon parallelism is only exercised at 512+ processes; the 4-process fixture
  path is sequential by design.
- The Linux path is verified in a Docker container (see PORTABILITY.md), but not
  on bare metal with thousands of processes.
- The GPU columns report what the driver exposes. `amdgpu`, `i915`/`xe` and
  `nouveau` implement `drm-usage-stats`; the proprietary NVIDIA driver does so only
  partially, and older drivers not at all. A machine where nothing implements it
  shows a column of dashes rather than an error, which is correct but not helpful —
  there is no per-driver detection to say *why*.
- `Zram`, `Zswap` and `HugePages` read `/sys`, so a container with a masked `/sys`
  sees no devices even when the host has them. The meter says `n/a here`, the same
  answer it gives on a host that genuinely has none.
- The zswap pool ceiling is `max_pool_percent × MemTotal`, resolved once per
  sample. It is the configured ceiling, not what the kernel will actually allow
  right now: zswap also refuses to grow past
  `totalram - totalhigh - 1/16 totalram`, so a machine with a high
  `max_pool_percent` will see the bar stop short of full.
- `M_PSS` and friends need `CAP_SYS_PTRACE` to read another user's process, which
  is the normal Linux permission model. Those rows show `-`.
- Delay accounting (item 5 of M3) is not implemented.
- macOS has no host-wide disk throughput counter without IOKit, so the `DiskIO`
  meter reports `n/a here` there. The per-process figures are parsed
  (`proc_pid_rusage`) but not displayed yet.
- The `DiskIO` meter reads `/proc/diskstats`, which counts I/O to real block
  devices. Filesystem-level I/O through a page-cache hit is not disk I/O and is
  correctly absent; `dm-*` and `loop*` are skipped so a device-mapper setup is not
  counted twice.
- Thread rows are only listable for processes owned by the current user. On macOS
  `proc_pidinfo` returns 0 for another user's process, so those rows show the
  `NLWP` count but cannot be expanded; on Linux `/proc/<pid>/task` is readable and
  the limit does not apply. Even for an inspectable process, macOS can withhold a
  single thread's handle, so a process may show one fewer row than its `NLWP`.
- macOS thread names are only present when the program set one with
  `pthread_setname_np`; 46% of the threads on a desktop are unnamed and fall back
  to the process name.
- A macOS thread row reports its process's `RES`, because memory belongs to the
  address space rather than to a thread. The same is true of Linux's
  `task/<tid>/stat`, so the two agree.
- macOS threads cannot be signalled or reniced individually: tagging a thread row
  resolves to its process before the action is sent.
- The function bar truncates its labels on terminals narrower than ~50 columns,
  though the key names stay intact.
- Scrolling uses minimal movement rather than centring the selection, so the
  cursor can sit at the very top or bottom edge of the list.
- macOS `used` omits a few hundred MiB of pages that no `vm_statistics64`
  counter covers. It reads slightly below `top` by design; the alternative
  (`total - free`) also counts pages the kernel has not classified yet.
