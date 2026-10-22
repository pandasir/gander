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
- [x] `PlatformSampler` is implemented by all three backends (Linux `/proc`,
      `darwin`, `freebsd`); `sample`/`cache_len` live only on the trait, so no
      platform can grow a method the others lack
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

Complete. All five items shipped: four in v0.4.0, delay accounting in v0.5.0.

| Area | State |
|---|---|
| `M_PSS` / `M_EPSS` / `M_PRIVATE` / `M_SWAP` columns from `smaps_rollup` | done, read lazily |
| PSI pressure: `PSI` meter, and a pressure block in the `panels` column | done |
| `Zram` / `Zswap` / `HugePages` meters | done |
| GPU columns (`GPU%`, `GPU_TIME`) via `drm-usage-stats` in `fdinfo` | done |
| Delay accounting: `CPUD%` / `IOD%` / `SWPD%` over `TASKSTATS` netlink | done, needs `CAP_NET_ADMIN` |

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

### Delay accounting, and the two ways it is absent

`CPUD%`, `IOD%` and `SWPD%` are the share of a task's life spent *waiting*:
runnable but not on a CPU, blocked on synchronous block I/O, and waiting for a page
to be swapped back in. CPU% says how much work a process does; these say how much
of its time was spent unable to do any. A process at 0.0 % CPU with 40 % `IOD%` is
not idle, it is stuck, and no other column in the table can tell those apart.

The data comes from `TASKSTATS` over generic netlink, which htop reaches through
`libnl3` — `dlopen`ed and called through function pointers, so a missing library
degrades rather than fails. gander speaks the protocol directly instead. It is four
messages, and it keeps the dependency list at `libc` only.

**It needs `CAP_NET_ADMIN`.** `TASKSTATS_CMD_GET` is not readable by an ordinary
process, which is the normal case: an unprivileged `gander` gets `EPERM` on every
request. The sampler records the first refusal and stops asking — one wasted
syscall rather than one per process per tick — and every row renders `-`. That is
the same rule the PSS columns follow: `-` is "not measured", `0.0%` is a
measurement, and only one of them is true.

`CONFIG_TASKSTATS=n` is the other absence: the generic-netlink family does not
exist, `DelayAcct::open()` returns `None`, and the columns stay empty. Both cases
look identical from the table, which is correct — the user cannot act on the
difference.

**The percentage is a lifetime average**, not an interval rate. The kernel keeps
cumulative `*_delay_total` counters in nanoseconds and `ac_etime` in microseconds;
`delay / (etime × 1000)` is the share of the task's whole life spent waiting. htop
computes the same figure from the same two numbers. It does not respond to a change
in behaviour within one tick the way `CPU%` does — a process that stalled for a
minute an hour ago carries that minute for the next hour. Reading it as a
current-state gauge would be wrong, which is why the description says "share of
this task's life" rather than "waiting now".

`cpu_delay_total` is documented as updated non-atomically with respect to
`ac_etime`, so a sample can show more delay than elapsed time. The percentage is
clamped to 100: the honest reading is "essentially all of it", and an unclamped
value would sort above a process that really is stalled all the time.

## Post-M3 fixes

Three discrepancies between what the code did and what the repository said, found
by auditing the two against each other rather than by a failing test.

### `s` did not exist

`docs/KEYS.md` and `docs/DESIGN.md` both listed "strace attach (`s`)" as an M2
feature, and `handle_list_key` had no `s` arm. Pressing it did nothing: no screen,
no status line, no error. Every other unimplemented key (`Z`, `b`) had been given an
explanation screen for exactly this reason, which is what made the omission easy to
miss — the pattern *looked* followed.

`s` now opens a panel that says why attaching a tracer is a terminal handover and
not a pane (strace writes a live stream the alternate screen would swallow), and
prints `strace -p <pid>` and the ptrace-permission caveat. A test asserts the panel
mentions both the obstacle and the workaround, because "not implemented yet." alone
teaches the reader nothing.

The `strace` attach itself remains unbuilt, and the docs now say so.

### Nine `Field` variants were unreachable

`Session`, `Tty`, `Tpgid`, `StartTime`, `Tgid`, `SchedPolicy`, `ProcComm`, `ProcExe`
and `Cwd` were declared in the `Field` enum with **no `FieldMeta` entry**. Three
consequences, none of which failed loudly:

- `Field::name()` returned `"UNKNOWN"`. The `htoprc` importer maps htop ids onto
these variants, so `fields=0 5 2` imported to `["PID", "UNKNOWN", "STATE",
  "COMM"]`, and `TabConfig::fields()`'s `filter_map` silently dropped the entry. An
  import that lost a column looked exactly like an import that worked.
- `sort_key=` on one of them fell through to `PERCENT_CPU`, because
  `TabConfig::sort` uses `Field::from_name(..).unwrap_or(Field::PercentCpu)`.
- They never appeared in the `F2` → Columns list, which iterates `Field::all()`.

All nine now have metadata, a renderer arm and a `sort_order` comparator. Two other
variants (`IoReadRate`, `IoWriteRate`) were deleted rather than given metadata: they
had no implementation, and their htop field ids could not be verified against
`ProcessField.h`, so naming them would have invented config keys.

A test (`every_variant_has_metadata`) now walks every variant and asserts it has a
name, a title, a description, and that the name round-trips through `from_name`. It
is a written-out list rather than a derived one, because `Field` has no enumeration
macro and adding one for a single test is not worth the dependency — so a new
variant still needs the list edited, which is the point.

### `TTY` and `TPGID` were also unparsed

Both were in the enum and neither was read from `/proc/<pid>/stat`, so even with
metadata they would have rendered a constant. `tty_nr` was parsed and unused;
`tpgid` was not parsed at all. Both are now filled, and `TTY` decodes the device
number to `pts/N` instead of printing `34816`.

`session` was in the same position on macOS: `proc_bsdinfo` carries no session id
and the `kinfo_proc` field that does is not among the offsets `darwin::kinfo`
decodes. It now comes from `getsid(2)`, cached with the process identity so the
syscall is paid once per process rather than once per tick.

### Smaller fixes in the same pass

The in-app help screen and `--help` listed a fraction of the bindings: `L`, `H`,
`O`, `Z`, `b`, `Tab`, `u`, and the whole navigation group (`PgUp`/`PgDn`, `g`/`G`,
`Left`/`Right`) were missing, so the only way to discover them was `docs/KEYS.md`
— which is not on the machine the user is looking at. Both lists are now complete.

The `docs/KEYS.md` "Acting on processes" table was broken by a paragraph in the
middle, leaving eight keys outside the table; it is now two tables.

## M4 — portability

Released as **v0.6.0**. FreeBSD is the milestone's own work; macOS landed ahead of
schedule.

| Area | State |
|---|---|
| macOS backend (`libproc` + `sysctl` + Mach) | done, ahead of schedule |
| FreeBSD backend (`sysctl` + `libkvm` + `devstat`) | done, verified in a live VM |
| Three `PlatformSampler` implementations behind one compile-time selection | done |
| Per-platform `Field` gating, so a column that cannot work is absent rather than `-` | done |
| CPU affinity on Linux and FreeBSD; macOS says why it has none | done |
| Host disk throughput on Linux and FreeBSD; macOS says why it has none | done |
| Other BSDs (OpenBSD, NetBSD, DragonFly) | not planned — see below |

M4 is complete for Linux, macOS and FreeBSD. The FreeBSD backend was written and
verified on **FreeBSD 14.5-RELEASE/arm64 in QEMU**: the full workspace suite runs
inside the guest, plus `crates/core/tests/freebsd_live.rs`, twenty tests that
compare gander's figures against `sysctl(8)`, `ps(1)`, `swapinfo(8)` and
`netstat(1)` in the same test. The TUI was started in the guest and rendered the
`panels` layout correctly.

### The four bugs only a live FreeBSD run could find

Every one of these passed review and passed the other platforms' tests, because
each is a place where two plausible readings of the same field differ:

**`SSLEEP` is three states, not one.** `ki_stat` is not a state letter and is not
even a function of the state. The first version mapped `1..7` onto `I R S T Z W ?`
arithmetically, which reported `I` for a process that had just been forked
(`SIDL`) and `S` for every sleeper. `ps`'s `print.c` is the authority: `SIDL` and
`SRUN` are `R`; `SSLEEP` splits into `D` (uninterruptible, `ki_tdflags &
TDF_SINTR` clear), `I` (interruptible and asleep for `MAXSLP` = 20 s), and `S`;
`SLOCK` is `L`. Collapsing `D` into `S` hides exactly the state that explains a
machine that feels hung — a process blocked on disk cannot be signalled away. The
live test compares every process's letter against `ps -axo pid,stat`.

**`comm_buf` truncated to empty.** `ArrayString::try_push_str` *rejects* a string
that does not fit and leaves the buffer as it was. So a name longer than 64 bytes
produced a blank `COMMAND`, not a shortened one — and a blank command name looks
exactly like a process the sampler could not identify. The test's own fully
qualified path triggered it on every run.

**`ELAPSED` subtracted the boot time twice.** The first version computed
`uptime - (start - boot)`. On a machine whose clock is newer than its boot — that
is, all of them — `start - boot` is the process's age, so every process reported
`0`. `now - start` is the whole formula; the boot time is irrelevant once `start`
is absolute.

**`cwd` was `None` for every process.** `KERN_PROC_CWD` returns a
`struct kinfo_file`, not a path, and a `PATH_MAX`-sized buffer is too small for it
(`ENOMEM`). Reading the buffer as a C string yields a single NUL byte, for every process,
which is indistinguishable from "the kernel will not tell us" — so the failure
looked like a permission model rather than a parsing bug.

**`KERN_PROC_FILEDESC`'s records are variable-length.** The `l` panel said
"cannot read file descriptors" for a process whose descriptors were perfectly
readable. `libc`'s `struct kinfo_file` is 1392 bytes; the kernel truncates each
record after `kf_path` and reports 376–408. The guard written was the natural one
— *skip a record whose `kf_structsize` is smaller than the struct we know about*
— which rejects every record the kernel sends. The stride is the record's own
size, and `kf_path` sits at a fixed offset of 368 because everything past the
truncation point was never copied out.

Four more were found by comparing against the system's own tools rather than by
reviewing the code: `hw.physmem` is larger than `v_page_count * pagesize` by the
kernel's early allocations, so using it as the bar's denominator leaves the bar
permanently short of full; `kvm_getswapinfo` reports pages, not bytes; `devstat`'s
pass-through devices double-count every byte; and `KERN_PROC_FILEDESC` lists the
cwd, root, text and controlling terminal as negative `kf_fd` entries rather than
leaving them out.

### Why not the other BSDs

OpenBSD and NetBSD have neither `kinfo_proc` nor `libkvm` in the same shape, and
DragonFly has diverged from FreeBSD since 2003. Each would be a new backend
written against a system this project has no way to test — and the M3 lesson is
that "it compiles" is worth very little: every trap in
[PORTABILITY.md](PORTABILITY.md#freebsd-specific-traps-all-hit-and-fixed-during-development)
produced a plausible number. A backend that cannot be run is a backend that
cannot be trusted.

`crates/core/src/lib.rs` is written so adding one is mechanical — a new `mod`, a
`PlatformSampler` impl, a `type Sampler` arm — but nobody should write it without
a machine to run it on.

## M5 — per-process I/O, and the promises the repo makes

Not started. Ships as **v0.7.0**. One of its five items (release notes) already
landed in v0.6.0, because it was cheap and its absence was actively misleading.

M0–M4 left one column family unbuilt and a set of claims that are no longer true.
Both are the same kind of defect: something this repository says is not something
the code does. A missing column is visible; a stale `DESIGN.md` is not, and the
last milestone's audit (see [Post-M3 fixes](#post-m3-fixes)) found four of those.

| Area | State |
|---|---|
| Per-process I/O: `RCHAR` / `WCHAR` / `SYSCR` / `SYSCW` / `RBYTES` / `WBYTES` / `CNCLWB` | not started |
| Per-process I/O rates: `IO_READ_RATE` / `IO_WRITE_RATE` / `IO_RATE` | not started |
| `s` attaches a real tracer instead of explaining itself | not started |
| `DESIGN.md` and `PORTABILITY.md` match the code that exists | **done in v0.6.1–v0.6.3** |
| Per-release `CHANGELOG.md`, and release notes that say what changed | **done in v0.6.0** |
| Dead `capability` constants and the `#[allow(dead_code)]` I/O fields | **done in v0.6.1–v0.6.3** |

This milestone is **wanted, not required**. Nothing in `README.md` or `KEYS.md`
names an I/O column, so its absence breaks no promise; `s` currently opens a panel
that explains the obstacle and prints the command to run instead. It is listed here
because htop has the columns and this project's stated goal is feature parity for
daily use — not because the tool is incomplete without it. The two items already
done were the ones that were actively misleading.

### Per-process I/O, and why it is not just three more columns

**The dangling promise is already cleaned up.** This section used to open by
pointing at `capability::PROC_IO` (declared since M0, read by nothing) and at
`disk_read`/`disk_written` in Darwin's `PrevRow` sitting behind
`#[allow(dead_code)]` with the comment "kept for the IO columns (milestone M1)" —
a promise from three milestones ago, kept alive by an attribute that suppressed the
warning that would have noticed it.

Both are now gone. The two fields were removed **along with the
`proc_pid_rusage` call that filled them**, which was one syscall per process per
tick on the development platform with its result dropped on the floor. The
`PROC_IO` constant went with them, as did four other `capability` constants that
nothing consulted (`IOPRIO`, `SMAPS`, `PSI`, `DELAYACCT`, `THREADS`, `LIVE`): a
capability nothing asks about is not a capability, and `PROC_IO == true` actively
lied, since no I/O column existed to be gated by it. The module doc now carries the
rule — *a constant here must be read by something*.

`darwin::scan::rusage()` is deliberately kept and still tested, because it is
verified against a live kernel and a future `IO_READ_RATE` column would otherwise
be rewritten from a man page by someone who cannot run macOS. Its doc comment says
it is unwired.

What remains below is the design work for when the columns are wanted.

| | Linux | macOS | FreeBSD |
|---|---|---|---|
| bytes and syscalls | `/proc/<pid>/io` | — | — |
| bytes actually hitting the block layer | `read_bytes` / `write_bytes` | `ri_diskio_bytesread/written` | `ki_rusage.ru_inblock/ru_oublock` — a *count*, see below |
| rates | delta ÷ **measured** interval | same | — |

Four things decide whether the numbers are right, and each has a wrong answer that
looks plausible:

**`RCHAR` is not `RBYTES`.** `rchar` counts bytes returned by `read(2)` and
`wchar` by `write(2)`, including page-cache hits, pipes and ttys. `read_bytes`
counts what actually reached the block layer, and a process reading a cached file
has a large `rchar` and a `read_bytes` of zero. htop's title for `RBYTES` is
`IO_R` and its description says "Bytes of read(2) I/O", which is the wrong way
round for the name and the right way round for the number. Both columns ship, with
titles that say which is which.

**A rate needs a measured interval, not the configured delay.** Same lesson as the
`NetworkIO` and `DiskIO` meters in M1: a slow scan makes the real gap longer than
`delay_ms`, so dividing by the configured value overstates the rate by exactly the
amount the last scan was late.

**The first sample has no rate.** It has a counter and no previous one, so the
rate is `None`, not `0 B/s` — the same distinction the PSS family and the delay
columns already make, and the reason the meters carry `prev_net` as an `Option`.

**FreeBSD has the field and not the figure.** `getrusage(2)` there defines
`ru_inblock` and `ru_oublock` as "the number of times the file system had to
perform input/output" — an operation *count*, not a byte total and not even a
block count. There is no way to turn it into bytes that does not need a per-device
block size and an assumption that every operation moved one block.  htop's FreeBSD
backend has no I/O columns at all, so there is nothing to agree or disagree with.
The family is therefore **absent** on FreeBSD — `Field` gating already exists for
exactly this, and a column that cannot work should not be in the setup screen at
all rather than render `-` for every row.

**Cost.** One `openat` + `read` per process, which is nothing next to
`smaps_rollup`, but 4 000 of them per tick is not nothing either. `ScanOptions`
gains a fifth flag, derived the same way as the other four — from the active tab's
visible columns — so a tab without an I/O column pays nothing.

**Batch mode.** `--json`'s extras object is `#[cfg(target_os = "linux")]`
`memory_json`, holding the PSS and GPU values. I/O exists on macOS too, so it needs
a sibling rather than a fifth arm inside a Linux-only function; a JSON consumer on
macOS currently has no way to ask for these at all.

### `s`, for real

`s` has been a screen that explains why tracing is a terminal handover since the
post-M3 audit. The explanation is correct and stays true — `strace` writes a live
stream the alternate screen would swallow — but the conclusion does not follow.
The answer is a pane that reads a **pipe**, which is what htop does
(`TraceScreen.c`): `fork` + `pipe`, the child `dup2`s onto stdout/stderr and
`exec`s the tracer, the parent reads non-blocking on the event loop's own tick and
appends to the same scrollable buffer the other info screens use. `Esc` kills the
child and reaps it.

| | tracer | absence |
|---|---|---|
| Linux | `strace -T -tt -s 512 -p <pid>` | `ptrace` may be denied; see below |
| FreeBSD | `truss -s 512 -p <pid>` | in the base system |
| macOS | `dtruss -p <pid>` | needs SIP disabled; the panel must say so |

**`ptrace_scope` is the thing that actually stops people.** The current panel warns
about "the ptrace-permission caveat" in the abstract. On most distributions
`/proc/sys/kernel/yama/ptrace_scope` is `1`, which forbids attaching to anything
that is not a child — so the key fails for every process on the machine, for a
reason the user cannot see and might reasonably blame on gander. The panel reads
that file and prints its value, because it is the difference between "this is not
implemented" and "this is set to 1".

`b` (backtrace) stays a panel. `libunwind-ptrace` is marked **[later]** in
[DESIGN.md](DESIGN.md) and unwinding a live process is a different problem from
reading a pipe; the honest alternative — deleting the key — is worse, because htop
has one and a missing key reads as a missing feature rather than an unbuilt one.

### Documentation that is no longer true

Found by reading the docs against the source, not by a failing test. Every item is
a sentence a user could act on and be wrong:

- **`DESIGN.md` §4.1 lists `IoReadRate, IoWriteRate`** in the `Field` enum. They
  were deleted in the post-M3 audit (their htop ids could not be verified against
  `ProcessField.h`, so naming them would have invented config keys). The section
  is an example of what a derived column looks like, so it cannot simply be
  trimmed to match — M5 gives those two variants real metadata and a real
  implementation, and the snippet becomes true by construction. The same goes for
  the nine identity columns, which the snippet still omits.
- **`DESIGN.md` §2 lists files that do not exist**: `core/src/sort.rs`,
  `core/src/filter.rs` (sorting and filtering live in `table.rs`), and
  `benches/ criterion` when there is no criterion dependency — the benchmark is a
  hand-timed `benches/scan.rs`, and says so in its own header. The tree also omits
  `darwin/` and `freebsd/` entirely, i.e. two thirds of the backends.
- **`README.md` and `README.zh-CN.md` said the identity columns "work on both
  platforms"**. There are three. Fixed in v0.6.0: the English sentence, and the
  missing 身份列 section in the Chinese file, which had none at all.
- **`README.zh-CN.md`'s platform table had two columns and said "两个后端"**, and its
  "其他 unix 目标" paragraph predated the FreeBSD backend. Fixed in v0.6.0 by
  rewriting it from the English table (three columns, `DiskIO` row included), along
  with the English "both platforms" sentence and the missing 身份列 section. The
  remaining instances of "both" in this file are M0-era history and have been left
  where they describe that milestone, corrected where they claimed a current state.
- **`DESIGN.md` §3's memory snippet says "the two platforms genuinely disagree"**
  and gives two formulas. `freebsd/system.rs` is a third model entirely
  (`used = active + wired + laundry`, with the page queues checked against
  `v_page_count`), and it has no arm in that snippet — so the one document that
  says *how memory is computed* omits a third of the answer. The formula is also
  duplicated in `system.rs`'s own doc comments, which is why it drifted.
- **`ROADMAP.md`'s M0 checklist says `PlatformSampler` "is implemented by both
  backends"**, in a milestone section whose own text now says three.
- **`PORTABILITY.md`'s per-process I/O row** is the "*not yet*" this milestone
  removes.

The fix for the class of problem, not just the instances: **a fact should live in
one place**. The `Field` enum's example in `DESIGN.md` duplicates `row.rs` and
drifted; the workspace tree duplicates `Cargo.toml` and drifted. Both become a
pointer to the source of truth plus the reasoning that is *not* in the code —
why `core` has no UI dependency, why the cost gates sit on `Field`. A doc that
restates code is a doc that will be wrong within one milestone.

### Releases that say what they contain — **done**

Every release's notes were a static body in `.github/workflows/release.yml`: the same
six lines for every tag, so the Releases page said nothing about what v0.5.0
changed, while the tag message that explained it well was visible only to `git`.
The v0.6.0 release made the cost concrete — it listed
`x86_64-unknown-freebsd` in its platform table for a milestone whose FreeBSD
archive the workflow never attached.

There is still no hand-maintained changelog, deliberately. A file edited by hand at
release time is a file that gets forgotten, and a hand-written changelog is a third
copy of facts that already live in the tag and in this document.

**The annotated tag is the only input.** It is written by whoever knows what
changed, at the moment they know it, and reviewers already read it through
`git log`.

- `.github/scripts/build-release-body.sh <tag>` builds the release body from the
  tag: headline, then the notes verbatim, then install instructions and a compare
  link. It exits non-zero for a missing tag, a lightweight tag, or an empty body,
  so a release cannot ship with blank notes — the failure mode that started this.
- `.github/scripts/gen-changelog.sh --write` regenerates `CHANGELOG.md` from every
  annotated tag in version order. It is idempotent, so re-running it produces no
  diff.
- The `release` job also compares `$GITHUB_REF_NAME` against
  `[workspace.package] version` and fails if they differ. `--version` reads the
  manifest, so a tag without a bump would publish binaries claiming the previous
  version under the new version's filename.
- The procedure, including how to fix a bad release (delete and re-tag rather than
  patch over it) is [RELEASING.md](RELEASING.md).

### Done when

- [ ] `RCHAR`, `WCHAR`, `SYSCR`, `SYSCW`, `RBYTES`, `WBYTES`, `CNCLWB` on Linux,
      with the byte and syscall families' descriptions naming which is which
- [ ] `IO_READ_RATE`, `IO_WRITE_RATE`, `IO_RATE` on Linux and macOS, over a
      measured interval, `None` on the first sample and after a failed read
- [ ] The I/O family absent rather than dashed on FreeBSD, and PORTABILITY.md says
      why (`ru_inblock` counts I/O operations)
- [x] `capability::PROC_IO` deleted, along with five other unreferenced
      `capability` constants; the `#[allow(dead_code)]` fields in
      `darwin::system::PrevRow` removed together with the `proc_pid_rusage` call
      that fed them (v0.6.1 cleanup)
- [ ] `ScanOptions` carries `proc_io`, derived from the visible columns, exercised
      by a fixture test that asserts an I/O column does *not* open
      `/proc/<pid>/io` when it is off screen
- [ ] `--json` emits the I/O figures on every platform that reads them
- [ ] `s` runs a tracer in a pane, kills it on `Esc`, and prints the actual
      `ptrace_scope` value; `truss` on FreeBSD, `dtruss`-with-SIP on macOS
- [x] `DESIGN.md`'s tree no longer lists files that do not exist (`sort.rs`,
      `filter.rs`, `criterion`) and now shows all three backends; §4.1 points at
      `row.rs` as the authority instead of restating a drifted enum, and lists all
      three `needs_*` gates rather than two; §3 covers the FreeBSD memory model.
      README says three platforms in both languages; the M0 checklist says three
      backends. (v0.6.1)
- [ ] `CHANGELOG.md` regenerated at release time (the mechanism exists; the item is
      the habit, not the script)
- [ ] Tests on all three platforms, zero clippy warnings, and the Linux container
      check extended to the new read path

### Deliberately out of M5

- **Man page and distro packaging** (a `gander.1`, a Homebrew formula, a
  `cargo-binstall` metadata block). Worth a milestone of its own, because each is
  a separate build pipeline to keep honest and none of them is a code change.
- **macOS `IOKit` disk throughput for the `DiskIO` meter.** The per-process figures
  are what M5 is for; a host-wide counter needs a whole framework to answer a
  question the meter already declines to fake.
- **FreeBSD jails in a column.** `ki_jid` is read and not shown; `jid 0` is the
  common case and a bare integer nobody groups on is a number, not information.
  Showing it needs an aggregate view, not a column.

## Deliberately not planned

- A PCP-style dynamic metric engine (runtime-defined columns/meters).
- Any daemon, setuid helper, or eBPF dependency.
- Windows support.
- OpenBSD, NetBSD and DragonFly backends — see above.

## Known limitations

- On macOS the per-process state is only trustworthy for `stopped` and `zombie`.
  Current Darwin reports `SRUN` for a process blocked in `read(2)`, so runnability
  comes from `proc_taskinfo.pti_numrunning` instead. Processes owned by another
  user cannot be inspected that way, and are shown as `?` rather than guessed at.
- macOS has no per-process CPU affinity mask and no I/O priority; both panels say
  so instead of failing silently.
- FreeBSD has no I/O priority panel (`ioprio_set` is a Linux syscall; FreeBSD's
  `rtprio`/`idprio` answer a different question), and the panel says so.
- FreeBSD jails are read (`kinfo_proc.ki_jid`) but not displayed: a bare integer
  that nothing groups on is a number, not information, and `jid 0` is the common
  case.
- FreeBSD's `kern.proc` size query is racy, so a machine forking thousands of
  processes a second can lose a row between the size query and the read. There is
  no atomic "read the whole table" interface; three retries with headroom is what
  `ps(1)` does.
- FreeBSD's memory is read from six separate `sysctl` calls rather than one
  snapshot, so the counters are milliseconds apart on a busy VM. The bar can be
  off by a few hundred pages under load; reading them atomically would need a
  subtree walk for no visible gain.
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
- Delay accounting needs `CAP_NET_ADMIN`, so an unprivileged `gander` shows `-` in
  `CPUD%`, `IOD%` and `SWPD%`. That is the kernel's permission model, not a
  limitation of the read.
- The delay percentages are lifetime averages rather than interval rates, because
  the kernel exposes no interval counter. A process that stalled once an hour ago
  still shows that stall.
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
