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

In progress. Screen tabs, bulk actions, the affinity/I/O panels and the modal
screens were already done ahead of schedule during M0/M1; the cgroup columns are
the first item that had to wait for this milestone.

| Area | State |
|---|---|
| Screen tabs with per-tab columns and sort | done (ahead of schedule) |
| CPU affinity (`a`) and I/O priority (`i`) panels | done (ahead of schedule) |
| Open files (`l`), env (`e`), command line (`w`), file locks (`x`) | done (ahead of schedule) |
| `CGROUP` column: the process's cgroup path | done |
| `CONTAINER` column: runtime + container id | done |
| Multi-select bulk actions across the tagged set | not started |
| Hide-containerised filter (htop's `O`) | not started |
| Highlight base name / stale exe | config exists, rendering not wired |

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

## Then: M3

See [DESIGN.md § Scope gates](DESIGN.md#1-scope-gates). The short version:

1. PSS / EPSS columns, loaded lazily and only when visible.
2. PSI pressure meters.
3. Zram / Zswap / HugePages meters.
4. GPU columns via `drm-usage-stats` (no vendor SDK).
5. Delay accounting via `TASKSTATS` netlink — lowest priority.

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
