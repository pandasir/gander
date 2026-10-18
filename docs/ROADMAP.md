# Roadmap

What is done, what is next, and what is deliberately not planned. The full
design lives in [DESIGN.md](DESIGN.md).

## Done (M0 skeleton)

The vertical slice compiles, is tested, and runs on Linux **and macOS**.

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
| 207 tests on macOS, zero clippy warnings | done |
| Live Linux verification in a container | done, see [PORTABILITY.md](PORTABILITY.md) |

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

Remaining:

- [ ] Thread enumeration as rows (macOS has no cheap per-thread enumeration; the
      Linux side needs a `Task`-style secondary list)
- [ ] Per-meter cursor in the header, so `Left`/`Right` can select the meter that
      `I` operates on without cycling all of them


## Then: M1 / M2 / M3

See [DESIGN.md § Scope gates](DESIGN.md#1-scope-gates). The short version:

1. cgroup / container columns — the parsers already exist in `linux/fsops.rs`.
2. PSS / EPSS columns, loaded lazily and only when visible.
3. PSI pressure meters.
4. Zram / Zswap / HugePages meters.
5. GPU columns via `drm-usage-stats` (no vendor SDK).
6. Delay accounting via `TASKSTATS` netlink — lowest priority.

## Deliberately not planned

- A PCP-style dynamic metric engine (runtime-defined columns/meters).
- Any daemon, setuid helper, or eBPF dependency.
- Windows support.

## Known limitations

- Threads are not enumerated yet on either platform; `NLWP` is displayed but the
  thread list is not.
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
- The macOS disk I/O columns are parsed (`proc_pid_rusage`) but not displayed yet.
- The function bar truncates its labels on terminals narrower than ~50 columns,
  though the key names stay intact.
- Scrolling uses minimal movement rather than centring the selection, so the
  cursor can sit at the very top or bottom edge of the list.
- macOS `used` omits a few hundred MiB of pages that no `vm_statistics64`
  counter covers. It reads slightly below `top` by design; the alternative
  (`total - free`) also counts pages the kernel has not classified yet.
