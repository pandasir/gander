//! The Linux sampler: turns `/proc` into a `SystemSnapshot`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::row::{CpuTicks, Row, RowFlags};
use crate::system::{CpuTimes, DiskCounters, MemInfo, NetCounters, SwapInfo, SystemSnapshot};
use crate::table::Cache;
use crate::Pid;

#[cfg(target_os = "linux")]
use super::delayacct::{DelayAcct, DelayStats};
use super::scan;
use super::LinuxPlatform;

/// Below this many processes, rayon's overhead exceeds the gain.
const PAR_THRESHOLD: usize = 512;

/// Drop the identity cache if it grows past this multiple of the live set.
const CACHE_SLACK: usize = 4;

/// Per-process state that must survive between ticks to compute deltas.
#[derive(Copy, Clone)]
struct PrevRow {
    starttime: u64,
    proc_ticks: u64,
}

pub struct Sampler {
    root: PathBuf,
    clk_tck: u64,
    page_size: u64,
    cache: Cache,
    /// Previous aggregate + per-core counters, for delta computation.
    prev_cpu: Vec<CpuTimes>,
    /// Minimum per-process state needed for the next tick's delta.
    prev_rows: HashMap<Pid, PrevRow>,
    /// Previous host counters, for the network/disk throughput meters.
    ///
    /// Held as `Option` so the first sample can tell "no previous sample" from
    /// "a previous sample that happened to be zero", which decides whether a
    /// rate is meaningful or is simply 0 B/s.
    prev_net: Option<(NetCounters, std::time::Instant)>,
    prev_disk: Option<(DiskCounters, std::time::Instant)>,
    pub use_rayon: bool,
    /// Whether to emit one row per thread in addition to one per process.
    ///
    /// Off by default: `/proc/<pid>/task` costs an extra `stat` read per thread,
    /// which on a busy desktop is roughly 1700 extra file reads per tick. htop
    /// makes the same trade (`hide_userland_threads`).
    show_threads: bool,
    /// Whether a column needing `/proc/<pid>/smaps_rollup` is on screen.
    ///
    /// The kernel walks every VMA to produce that file, so reading it for every
    /// process costs tens of milliseconds — more than the rest of the scan put
    /// together. Set from the front end whenever the visible column set changes;
    /// when false the sampler never opens the file at all.
    read_smaps: bool,
    /// Whether a GPU column is on screen (reads each process's `fdinfo`).
    read_gpu: bool,
    /// Whether a delay-accounting column is on screen.
    ///
    /// Costs one generic-netlink round trip per process per tick, which is more
    /// expensive than a `/proc` read: every request is a syscall pair plus a
    /// kernel lookup. Off unless a `CPUD%`/`IOD%`/`SWPD%` column is visible.
    #[cfg(target_os = "linux")]
    read_delayacct: bool,
    /// The `TASKSTATS` socket, opened on first use.
    ///
    /// `Option` because opening it can fail in two ordinary ways (no
    /// `CONFIG_TASKSTATS`, or the socket cannot be created), and the sampler must
    /// keep working when it does. Opened lazily rather than in `new` so a run
    /// that never shows a delay column never creates a socket.
    #[cfg(target_os = "linux")]
    delayacct: Option<DelayAcct>,
    /// Previous per-task delay counters, for the percentage delta.
    ///
    /// Keyed by pid and holding `starttime` alongside: a recycled pid must not
    /// inherit its predecessor's counters, the same hazard the CPU% path solves
    /// with `PrevRow`.
    #[cfg(target_os = "linux")]
    prev_delay: HashMap<Pid, (u64, DelayStats)>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new(Path::new("/proc"))
    }
}

impl Sampler {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            clk_tck: LinuxPlatform::clk_tck(),
            page_size: LinuxPlatform::page_size(),
            cache: Cache::new(),
            prev_cpu: Vec::new(),
            prev_rows: HashMap::new(),
            prev_net: None,
            prev_disk: None,
            use_rayon: true,
            show_threads: false,
            read_smaps: false,
            read_gpu: false,
            #[cfg(target_os = "linux")]
            read_delayacct: false,
            #[cfg(target_os = "linux")]
            delayacct: None,
            #[cfg(target_os = "linux")]
            prev_delay: HashMap::new(),
        }
    }

    /// Read delay counters for `pid`, opening the netlink socket on first use.
    ///
    /// Returns `None` when the data cannot be read: no `CAP_NET_ADMIN`, no
    /// `CONFIG_TASKSTATS`, or the process exited between `readdir` and now. All
    /// three are ordinary, and all three must render as `-` rather than `0.0%`.
    ///
    /// `starttime` is not used to compute anything; it is recorded so a recycled
    /// pid can be detected. The lifetime percentage needs no delta, but a pid
    /// reused by a new process must not keep the old one's numbers, and the
    /// stored `starttime` is what makes that distinguishable.
    #[cfg(target_os = "linux")]
    fn delay_for(&mut self, pid: Pid, starttime: u64) -> Option<DelayStats> {
        let acct = match self.delayacct {
            Some(ref mut a) => a,
            None => self.delayacct.insert(DelayAcct::open()?),
        };
        let stats = acct.read(pid)?;
        // Forget counters for pids that have been reused, so the map cannot grow
        // without bound and a recycled pid cannot inherit stale values.
        if let Some(&(prev_start, _)) = self.prev_delay.get(&pid) {
            if prev_start != starttime {
                self.prev_delay.remove(&pid);
            }
        }
        self.prev_delay.insert(pid, (starttime, stats));
        Some(stats)
    }

    pub fn clk_tck(&self) -> u64 {
        self.clk_tck
    }

    /// Sample everything: system counters plus one row per process.
    fn sample_inner(&mut self) -> (SystemSnapshot, Vec<Row>) {
        let at = std::time::Instant::now();

        let cpu = scan::read_proc_stat(&self.root);
        let cpu_delta: Vec<CpuTimes> = if self.prev_cpu.len() == cpu.len() && !cpu.is_empty() {
            cpu.iter()
                .zip(&self.prev_cpu)
                .map(|(n, p)| n.delta(p))
                .collect()
        } else {
            // First sample, or a core came online: no delta available yet.
            vec![CpuTimes::default(); cpu.len()]
        };

        let mut mem = MemInfo::default();
        let mut swap = SwapInfo::default();
        scan::read_meminfo(&self.root, &mut mem, &mut swap);

        let load = scan::read_loadavg(&self.root).unwrap_or([0.0; 3]);
        let uptime = scan::read_uptime(&self.root).unwrap_or(0);

        // Pressure stall information, zram devices, and the zswap pool ceiling.
        // All three are Linux-only subsystems and all three are absent on kernels
        // built without them, so each reports `None`/empty rather than zeroes that
        // would render as an idle machine.
        let psi = scan::read_psi(&self.root);
        let zram = scan::read_zram(&self.root);
        // `max_pool_percent` is a percentage of physical memory, so it can only be
        // resolved once `MemTotal` is known.
        if let Some(percent) = scan::read_zswap_max_percent(&self.root) {
            mem.zswap_max = mem.total.saturating_mul(percent.min(100)) / 100;
        }

        // Network and disk throughput. Both are cumulative counters, so the rate
        // needs the previous sample *and* the interval between the two; the
        // interval is measured here rather than taken from the config, because a
        // slow scan makes the real gap longer than the configured delay.
        let net = scan::read_net_counters(&self.root);
        let disk = scan::read_disk_counters(&self.root);
        let net_rate = match self.prev_net {
            Some((prev, at_prev)) => net.per_sec(&prev, at.duration_since(at_prev).as_secs_f64()),
            None => crate::system::Rate::default(),
        };
        let disk_rate = match self.prev_disk {
            Some((prev, at_prev)) => {
                disk.per_sec(&prev, at.duration_since(at_prev).as_secs_f64())
            }
            None => crate::system::Rate::default(),
        };
        self.prev_net = Some((net, at));
        self.prev_disk = Some((disk, at));

        let pids = scan::list_pids(&self.root).unwrap_or_default();
        let mut rows = self.read_rows(&pids, &cpu_delta, uptime, mem.total);
        if self.show_threads {
            let threads = self.read_threads(&pids, &cpu_delta);
            rows.extend(threads);
        }

        self.prev_cpu = cpu.clone();
        self.cache.retain_live(
            &rows
                .iter()
                .map(|r| (r.pid, r.starttime))
                .collect::<Vec<_>>(),
        );
        if self.cache.len() > rows.len().saturating_mul(CACHE_SLACK) + 64 {
            self.cache.clear();
        }

        let snapshot = SystemSnapshot {
            at,
            cpu,
            cpu_delta,
            mem,
            swap,
            load,
            uptime,
            clk_tck: self.clk_tck,
            page_size: self.page_size,
            net,
            net_rate,
            disk,
            disk_rate,
            tasks: task_stats(&rows),
            psi,
            zram,
        };
        (snapshot, rows)
    }

    /// One extra row per thread, with `tgid` pointing at its process.
    ///
    /// The thread-group leader is skipped: it is already present as the process
    /// row, and duplicating it would make the tree show every process twice.
    /// Threads inherit the process's uid, command line and cgroup, so only the
    /// thread's own `stat` (and therefore its CPU%) is read here.
    fn read_threads(&mut self, pids: &[Pid], cpu_delta: &[CpuTimes]) -> Vec<Row> {
        let root = self.root.clone();
        let page_size = self.page_size;
        let total_delta = cpu_delta.first().map(|d| d.total()).unwrap_or(0) as f32;

        // Collect (pid, tid) pairs first so the parallel part only reads files.
        let mut pairs: Vec<(Pid, Pid)> = Vec::new();
        for &pid in pids {
            let Ok(tids) = scan::list_tids(&root, pid) else {
                continue;
            };
            for tid in tids {
                if tid != pid {
                    pairs.push((pid, tid));
                }
            }
        }
        if pairs.is_empty() {
            return Vec::new();
        }

        let read_one = |&(pid, tid): &(Pid, Pid)| {
            scan::read_thread_stat(&root, pid, tid).map(|(comm, fields)| (pid, tid, comm, fields))
        };

        let stats: Vec<(Pid, Pid, crate::row::CommBuf, super::parse::StatFields)> =
            if self.use_rayon && pairs.len() >= PAR_THRESHOLD {
                use rayon::prelude::*;
                pairs.par_iter().filter_map(read_one).collect()
            } else {
                pairs.iter().filter_map(read_one).collect()
            };

        let mut out = Vec::with_capacity(stats.len());
        for (pid, tid, comm, fields) in stats {
            let starttime = fields.starttime;
            let mut row = Row::default_for_test(tid);
            row.pid = tid;
            // The tid is already unique across the system on Linux, but it is
            // stored through `thread_id` as well so that `Row::identity` means the
            // same thing on both backends and a thread row can never be confused
            // with a process that happens to have the same pid.
            row.thread_id = Some(tid as u64);
            row.tgid = pid;
            row.is_thread = true;
            row.ppid = pid;
            row.comm = comm;

            let prev_thread_ticks = match self.prev_rows.get(&tid) {
                Some(p) if p.starttime == starttime => p.proc_ticks,
                _ => 0,
            };
            self.prev_rows.insert(
                tid,
                PrevRow {
                    starttime,
                    proc_ticks: fields.cpu_ticks(),
                },
            );

            scan::apply_stat(&mut row, &fields, page_size);

            let delta = fields.cpu_ticks().saturating_sub(prev_thread_ticks) as f32;
            row.percent_cpu = if total_delta > 0.0 {
                delta * 100.0 / total_delta
            } else {
                0.0
            };

            // Identity is the process's, and already cached under the leader's pid.
            let stable = scan::read_identity(&self.root, pid, starttime, &mut self.cache);
            row.uid = stable.uid;
            row.cmdline = stable.cmdline.clone();
            row.exe = stable.exe.clone();
            row.cgroup = stable.cgroup.clone();
            #[cfg(target_os = "linux")]
            {
                row.container = stable.container.clone();
                row.container_runtime = stable.container_runtime.clone();
            }
            row.flags = row
                .flags
                .insert(RowFlags::USERLAND_THREAD)
                .set(RowFlags::STALE_EXE, stable.exe_deleted);
            row.nlwp = 1;

            out.push(row);
        }
        out
    }

    /// Read `stat` for every pid, in parallel when the set is large enough.
    fn read_rows(
        &mut self,
        pids: &[Pid],
        cpu_delta: &[CpuTimes],
        uptime: u64,
        mem_total: u64,
    ) -> Vec<Row> {
        let root = &self.root;
        let page_size = self.page_size;

        // Phase 1: the per-process `stat` read. Read-only, so parallel-safe.
        let stats: Vec<(Pid, crate::row::CommBuf, super::parse::StatFields)> =
            if self.use_rayon && pids.len() >= PAR_THRESHOLD {
                use rayon::prelude::*;
                pids.par_iter()
                    .filter_map(|&pid| scan::read_stat(root, pid).map(|(c, f)| (pid, c, f)))
                    .collect()
            } else {
                pids.iter()
                    .filter_map(|&pid| scan::read_stat(root, pid).map(|(c, f)| (pid, c, f)))
                    .collect()
            };

        // Phase 2: identity (uid, cmdline, exe) — cached, so mostly a hash hit.
        let mut out = Vec::with_capacity(stats.len());
        let mut next_prev = HashMap::with_capacity(stats.len());
        let total_delta = cpu_delta.first().map(|d| d.total()).unwrap_or(0) as f32;

        for (pid, comm, fields) in stats {
            let starttime = fields.starttime;
            let mut row = Row::default_for_test(pid);

            // Same (pid, starttime) means same process; otherwise this is either
            // a new process or a reused PID and the tick history is invalid.
            let prev_proc_ticks = match self.prev_rows.get(&pid) {
                Some(p) if p.starttime == starttime => p.proc_ticks,
                _ => 0,
            };
            next_prev.insert(
                pid,
                PrevRow {
                    starttime,
                    proc_ticks: fields.cpu_ticks(),
                },
            );

            scan::apply_stat(&mut row, &fields, page_size);
            row.comm = comm;

            // The PSS family, only when the front end says a column needs it.
            // `None` (not zero) when unread: another user's process refuses, and a
            // zero would read as "this process uses no memory".
            #[cfg(target_os = "linux")]
            if self.read_smaps {
                if let Some(rollup) = super::fsops::read_smaps_rollup(&self.root, pid) {
                    row.pss = Some(rollup.pss);
                    row.epss = Some(rollup.epss());
                    row.private = Some(rollup.private());
                    row.swap_bytes = Some(rollup.swap);
                }
            }
            // GPU counters from `drm-usage-stats`, gated the same way and for the
            // same reason: one `readdir` plus a read per descriptor, per process.
            #[cfg(target_os = "linux")]
            if self.read_gpu {
                if let Some(gpu) = super::scan::read_gpu_usage(&self.root, pid) {
                    row.gpu_percent = gpu.percent();
                    row.gpu_ns = Some(gpu.gpu_ns);
                }
            }
            // Delay accounting, over generic netlink. Also gated: a netlink round
            // trip per process per tick is the most expensive read in the scan.
            //
            // The percentage is a lifetime average (the kernel keeps no interval
            // counter), so it needs no delta: the first sample already has one.
            #[cfg(target_os = "linux")]
            if self.read_delayacct {
                if let Some(stats) = self.delay_for(pid, starttime) {
                    row.cpu_delay_percent = stats.cpu_percent();
                    row.io_delay_percent = stats.blkio_percent();
                    row.swap_delay_percent = stats.swapin_percent();
                }
            }

            let d_proc = fields.cpu_ticks().saturating_sub(prev_proc_ticks) as f32;
            row.percent_cpu = if total_delta > 0.0 {
                // May exceed 100% on multicore: correct, and matches htop.
                d_proc * 100.0 / total_delta
            } else {
                0.0
            };

            let stable = scan::read_identity(&self.root, pid, starttime, &mut self.cache);
            row.uid = stable.uid;
            row.flags = row
                .flags
                .set(RowFlags::KERNEL_THREAD, stable.is_kernel_thread);
            row.cmdline = stable.cmdline;
            row.exe = stable.exe;
            row.cgroup = stable.cgroup;
            // A deleted or replaced executable is highlighted, matching htop's EXE
            // column behaviour. Threads inherit the process's exe, so they inherit
            // the flag below too.
            row.flags = row
                .flags
                .set(RowFlags::STALE_EXE, stable.exe_deleted);
            #[cfg(target_os = "linux")]
            {
                row.container = stable.container;
                row.container_runtime = stable.container_runtime;
            }

            row.elapsed = if self.clk_tck > 0 {
                uptime.saturating_sub(starttime.checked_div(self.clk_tck).unwrap_or(0))
            } else {
                0
            };
            row.percent_mem = if mem_total > 0 {
                row.res as f32 * 100.0 / mem_total as f32
            } else {
                0.0
            };

            row.ticks = CpuTicks {
                proc: fields.cpu_ticks(),
                total: total_delta as u64,
            };
            row.prev_ticks = CpuTicks {
                proc: prev_proc_ticks,
                total: 0,
            };

            out.push(row);
        }

        self.prev_rows = next_prev;
        // Drop delay counters for processes that have exited. Without this the map
        // grows with every process the machine has ever run, which on a desktop
        // is thousands of entries in a few minutes.
        #[cfg(target_os = "linux")]
        {
            let live: std::collections::HashSet<Pid> =
                out.iter().filter(|r| !r.is_thread).map(|r| r.pid).collect();
            self.prev_delay.retain(|pid, _| live.contains(pid));
        }
        out
    }
}

/// Summarise the process table for the `Tasks` meter.
///
/// Only process rows are counted. Thread rows are extra rows for the same
/// process, so counting every row would report `total` and `threads` roughly
/// twice over: the leader's `nlwp` already counts its threads, and each thread
/// row contributes one more.
fn task_stats(rows: &[Row]) -> crate::system::TaskStats {
    let procs = rows.iter().filter(|r| !r.is_thread);
    let mut stats = crate::system::TaskStats::default();
    for row in procs {
        stats.total += 1;
        if row.state == b'R' {
            stats.running += 1;
        }
        stats.threads += row.nlwp.max(1) as usize;
    }
    stats
}

/// The unified sampler interface.
///
/// This was only implemented for the Darwin backend, so on Linux the trait was
/// declared but never satisfied: `gander_core::PlatformSampler` described an
/// interface that did not exist on the platform where it mattered most, and
/// every `use gander_core::PlatformSampler` in a Linux-only test was dead
/// (the inherent `sample` shadowed it). `sample` and `cache_len` now exist *only*
/// on the trait, on both platforms, so a caller cannot accidentally depend on a
/// method that one backend happens to have and the other does not.
impl crate::PlatformSampler for Sampler {
    fn sample(&mut self) -> (SystemSnapshot, Vec<Row>) {
        self.sample_inner()
    }

    fn cache_len(&self) -> usize {
        self.cache.len()
    }

    fn backend_name() -> &'static str {
        "linux (/proc)"
    }

    fn set_scan_options(&mut self, opts: crate::ScanOptions) {
        self.show_threads = opts.threads;
        self.read_smaps = opts.smaps;
        self.read_gpu = opts.gpu;
        #[cfg(target_os = "linux")]
        {
            self.read_delayacct = opts.delayacct;
            // Drop the socket when the last delay column goes away, so a session
            // that showed one briefly does not hold a netlink socket open for the
            // rest of the run.
            if !opts.delayacct {
                self.delayacct = None;
                self.prev_delay.clear();
            }
        }
    }

    fn supports_threads() -> bool {
        true
    }
}

/// The Linux sampler must satisfy the shared trait. Gated to Linux because the
/// `linux` module compiles on every unix (its IO layer is path-parameterised),
/// but only Linux has a `/proc` to sample.
#[cfg(all(test, target_os = "linux"))]
mod platform_trait_tests {
    use crate::PlatformSampler;

    #[test]
    fn the_linux_sampler_satisfies_the_shared_trait() {
        assert!(<crate::Sampler as PlatformSampler>::backend_name().contains("linux"));
        let mut s = crate::Sampler::default();
        let (_snap, rows) = PlatformSampler::sample(&mut s);
        assert!(!rows.is_empty(), "/proc must yield processes");
        assert_eq!(PlatformSampler::cache_len(&s), <crate::Sampler>::cache_len(&s));
    }
}
