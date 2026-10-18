//! The Linux sampler: turns `/proc` into a `SystemSnapshot`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::row::{CpuTicks, Row, RowFlags};
use crate::system::{CpuTimes, MemInfo, SwapInfo, SystemSnapshot};
use crate::table::Cache;
use crate::Pid;

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
    pub use_rayon: bool,
    /// Whether to emit one row per thread in addition to one per process.
    ///
    /// Off by default: `/proc/<pid>/task` costs an extra `stat` read per thread,
    /// which on a busy desktop is roughly 1700 extra file reads per tick. htop
    /// makes the same trade (`hide_userland_threads`).
    show_threads: bool,
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
            use_rayon: true,
            show_threads: false,
        }
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
            tasks: task_stats(&rows),
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
            row.flags = row.flags.insert(RowFlags::USERLAND_THREAD);
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

            row.elapsed = if self.clk_tck > 0 {
                uptime.saturating_sub(starttime / self.clk_tck)
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
        out
    }
}

/// Summarise the process table for the `Tasks` meter.
fn task_stats(rows: &[Row]) -> crate::system::TaskStats {
    crate::system::TaskStats {
        total: rows.len(),
        running: rows.iter().filter(|r| r.state == b'R').count(),
        threads: rows.iter().map(|r| r.nlwp.max(1) as usize).sum(),
    }
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

    fn set_show_threads(&mut self, on: bool) {
        self.show_threads = on;
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
