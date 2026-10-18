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
| Batch mode: `-n`, `--json`, `--filter`, `--sort`, `--limit` | done |
| Kill / renice | done |
| Identical keybindings to htop | done |
| macOS backend: `libproc` + `sysctl` + Mach | done, see [PORTABILITY.md](PORTABILITY.md) |
| Correct memory accounting on both platforms (see PORTABILITY.md) | done |
| Full process table on macOS via `KERN_PROC_ALL` | done |
| 157 tests, zero clippy warnings | done |

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
- [x] `F2` setup screen: Columns, Display, Colors, About
- [x] `Z` security context and `b` backtrace explain themselves instead of
      silently doing nothing

Remaining:

- [ ] Screen tabs (`Tab` / `Shift+Tab`) with per-tab columns and sort
- [ ] Header editing in the setup screen
- [ ] Affinity panel (`a`) and I/O priority (`i`)
- [ ] Thread enumeration as rows


## Then: M1 / M2 / M3

See [DESIGN.md § Scope gates](DESIGN.md#1-scope-gates). The short version:

1. Meter display-mode cycling per meter (currently global).
2. cgroup / container columns — the parsers already exist in `linux/fsops.rs`.
3. PSS / EPSS columns, loaded lazily and only when visible.
4. PSI pressure meters.
5. Zram / Zswap / HugePages meters.
6. GPU columns via `drm-usage-stats` (no vendor SDK).
7. Delay accounting via `TASKSTATS` netlink — lowest priority.

## Deliberately not planned

- A PCP-style dynamic metric engine (runtime-defined columns/meters).
- Any daemon, setuid helper, or eBPF dependency.
- Windows support.

## Known limitations

- `LOCAL_OFFSET_SECS` is `0`, so `Clock`/`Date` meters display UTC. Wire up
  `localtime_r` before shipping.
- Threads are not enumerated yet on either platform; `NLWP` is displayed but the
  thread list is not.
- Rayon parallelism is only exercised at 512+ processes; the 4-process fixture
  path is sequential by design.
- **The Linux live path is unverified on real hardware**: parsers and sampler are
  fixture-tested, but only macOS has been exercised against a live OS. Run
  `cargo test` on Linux before trusting it.
- The macOS disk I/O columns are parsed (`proc_pid_rusage`) but not displayed yet.
- The function bar truncates its labels on terminals narrower than ~50 columns,
  though the key names stay intact.
- Scrolling uses minimal movement rather than centring the selection, so the
  cursor can sit at the very top or bottom edge of the list.
- macOS `used` omits a few hundred MiB of pages that no `vm_statistics64`
  counter covers. It reads slightly below `top` by design; the alternative
  (`total - free`) also counts pages the kernel has not classified yet.
