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
        }
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    pub fn clk_tck(&self) -> u64 {
        self.clk_tck
    }

    /// Sample everything: system counters plus one row per process.
    pub fn sample(&mut self) -> (SystemSnapshot, Vec<Row>) {
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
        let rows = self.read_rows(&pids, &cpu_delta, uptime, mem.total);

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
        };
        (snapshot, rows)
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
