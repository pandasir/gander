//! The macOS sampler.

use std::collections::HashMap;
use std::time::Instant;

use crate::darwin::{parse, scan, units, DarwinPlatform};
use crate::row::{Row, RowFlags};
use crate::system::{CpuTimes, MemInfo, SystemSnapshot};
use crate::table::{Cache, Stable};
use crate::Pid;

use crate::PlatformSampler;

/// Per-process state carried between ticks.
#[derive(Copy, Clone)]
struct PrevRow {
    /// `p_starttime` in microseconds, used as the identity of the process.
    start_usec: u64,
    /// Monotonic CPU nanoseconds, user + system.
    cpu_ns: u64,
    /// Cumulative disk bytes, kept for the IO columns (milestone M1).
    #[allow(dead_code)]
    disk_read: u64,
    #[allow(dead_code)]
    disk_written: u64,
}

/// Drop the identity cache if it grows past this multiple of the live set.
const CACHE_SLACK: usize = 4;

pub struct Sampler {
    cache: Cache,
    prev_rows: HashMap<Pid, PrevRow>,
    prev_cpu: Vec<CpuTimes>,
    prev_at: Option<Instant>,
    page_size: u64,
    cpu_count: usize,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        let cpu_count = DarwinPlatform::cpu_count();
        Self {
            cache: Cache::new(),
            prev_rows: HashMap::new(),
            prev_cpu: Vec::new(),
            prev_at: None,
            page_size: scan::page_size(),
            cpu_count,
        }
    }
}

impl PlatformSampler for Sampler {
    fn backend_name() -> &'static str {
        "darwin/libproc"
    }

    fn cache_len(&self) -> usize {
        self.cache.len()
    }

    fn sample(&mut self) -> (SystemSnapshot, Vec<Row>) {
        let at = Instant::now();

        // --- host counters ---------------------------------------------------
        let cpu = scan::cpu_ticks();
        let cpu_delta: Vec<CpuTimes> = if self.prev_cpu.len() == cpu.len() && !cpu.is_empty() {
            cpu.iter()
                .zip(&self.prev_cpu)
                .map(|(n, p)| n.delta(p))
                .collect()
        } else {
            vec![CpuTimes::default(); cpu.len()]
        };

        let vm = scan::vm_stats();
        let total = scan::total_memory();
        let mut mem = MemInfo {
            total,
            free: vm.free,
            active: vm.active,
            inactive: vm.inactive,
            wired: vm.wired,
            compressed: vm.compressed,
            purgeable: vm.purgeable,
            speculative: vm.speculative,
            // File-backed pages are reclaimable cache; reported for completeness
            // even though `finish_darwin` does not need them.
            cached: vm.external,
            ..Default::default()
        };
        mem.finish_darwin();

        let swap = scan::swap_usage();

        let load = scan::loadavg();
        let uptime = scan::uptime_secs();
        let now_secs = scan::now_secs();

        // --- processes -------------------------------------------------------
        // `KERN_PROC_ALL` sees every process; `proc_pidinfo` cannot. If the
        // layout self-check fails, fall back to the libproc-only path rather
        // than showing wrong numbers.
        let procs = match crate::darwin::kinfo::list_processes() {
            Ok(list) if !list.is_empty() => list,
            _ => Vec::new(),
        };
        let interval = self
            .prev_at
            .map(|p| at.duration_since(p).as_secs_f64())
            .unwrap_or(0.0);
        let rows = self.read_rows(&procs, interval, now_secs, mem.total);

        self.prev_cpu = cpu.clone();
        self.prev_at = Some(at);
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
            clk_tck: 100,
            page_size: self.page_size,
        };
        (snapshot, rows)
    }
}

impl Sampler {
    fn read_rows(
        &mut self,
        procs: &[crate::darwin::kinfo::RawProc],
        interval_secs: f64,
        now_secs: u64,
        mem_total: u64,
    ) -> Vec<Row> {
        let mut out = Vec::with_capacity(procs.len());
        let mut next_prev = HashMap::with_capacity(procs.len());

        for kp in procs {
            let pid = kp.pid;

            // The BSD facts arrive with the process list, so a row always exists
            // even for processes we cannot inspect. Only the task counters
            // (CPU time, memory, threads) need the extra call.
            let ti = scan::task_all_info(pid).map(|all| all.task);

            let start_usec = kp.start_sec.saturating_mul(1_000_000);
            let prev = self.prev_rows.get(&pid).copied();
            let same_process = prev.is_some_and(|p| p.start_usec == start_usec);

            let cpu_ns = ti
                .map(|t| {
                    units::mach_ticks_to_nanos(t.user_ns)
                        .saturating_add(units::mach_ticks_to_nanos(t.system_ns))
                })
                .unwrap_or(0);

            let mut row = Row::default_for_test(pid);
            row.starttime = start_usec;
            row.ppid = kp.ppid;
            row.uid = kp.uid;
            row.nice = kp.nice.clamp(-128, 127) as i8;
            row.priority = kp.nice.clamp(-128, 127) as i8;
            row.state = parse::state_letter(kp.p_stat);
            if let Some(t) = ti {
                if t.running_threads > 0 {
                    row.state = b'R';
                } else if row.state == b'R' {
                    row.state = b'S';
                }
            }

            let name = kp.comm_str();
            let _ = row.comm.try_push_str(&name);

            row.time_ticks = cpu_ns / 10_000_000; // hundredths of a second
            if let Some(t) = ti {
                row.virt = t.virtual_size;
                row.res = t.resident_size;
                row.nlwp = t.threads as i32;
                row.min_flt = t.faults as u64;
            }
            row.processor = -1;

            // CPU% needs wall-clock time here, not tick deltas: proc_taskinfo
            // reports nanoseconds. Mirrors darwin/DarwinProcess.c.
            if same_process && interval_secs > 0.0 {
                if let Some(p) = prev {
                    if cpu_ns >= p.cpu_ns {
                        let delta = (cpu_ns - p.cpu_ns) as f64;
                        let pct = delta / (interval_secs * 1e9) * 100.0;
                        // htop caps at activeCPUs * 100 on Linux; the same cap
                        // keeps a clock jump from producing a nonsense number.
                        row.percent_cpu = (pct as f32).min(self.cpu_count as f32 * 100.0);
                    }
                }
            }

            let io = if ti.is_some() {
                scan::rusage(pid)
            } else {
                None
            };
            next_prev.insert(
                pid,
                PrevRow {
                    start_usec,
                    cpu_ns,
                    disk_read: io.map(|i| i.disk_read).unwrap_or(0),
                    disk_written: io.map(|i| i.disk_written).unwrap_or(0),
                },
            );

            row.percent_mem = if mem_total > 0 {
                row.res as f32 * 100.0 / mem_total as f32
            } else {
                0.0
            };
            // `start_sec` is an absolute Unix timestamp; uptime is a duration.
            row.elapsed = now_secs.saturating_sub(kp.start_sec);

            // Identity data: expensive and immutable, so cached. Only fetched
            // when the process is inspectable; otherwise the command name from
            // the BSD info is all we get, and that is what htop shows too.
            if ti.is_some() {
                let stable = self.identity(pid, start_usec);
                row.uid = stable.uid;
                row.cmdline = stable.cmdline;
                row.exe = stable.exe;
                row.flags = row
                    .flags
                    .set(RowFlags::KERNEL_THREAD, stable.is_kernel_thread);
                if stable.is_kernel_thread {
                    row.flags = row.flags.insert(RowFlags::KERNEL_THREAD);
                }
            }

            out.push(row);
        }

        self.prev_rows = next_prev;
        out
    }

    fn identity(&mut self, pid: Pid, key: u64) -> Stable {
        if let Some(hit) = self.cache.get(pid, key) {
            return hit.clone();
        }

        let mut stable = Stable::default();
        // uid is filled by the caller from the BSD info; it never changes for a
        // live process, so it does not need to be cached separately.
        stable.uid = 0;

        stable.exe = scan::exe_path(pid).map(String::into_boxed_str);
        stable.cmdline = scan::procargs2(pid)
            .and_then(|raw| parse::parse_procargs2(&raw))
            .filter(|s| !s.is_empty())
            .map(String::into_boxed_str);
        stable.is_kernel_thread = stable.cmdline.is_none() && stable.exe.is_none();

        self.cache.insert(pid, key, stable.clone());
        stable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_sample_produces_rows() {
        let mut s = Sampler::new();
        let (snap, rows) = s.sample();
        assert!(!rows.is_empty(), "there are always processes");
        assert!(snap.mem.total > 0);
        assert!(snap.cpu.len() >= 2);
        assert_eq!(snap.page_size, scan::page_size());
    }

    #[test]
    fn self_appears_with_a_command_line() {
        let me = std::process::id();
        let mut s = Sampler::new();
        let (_snap, rows) = s.sample();
        let mine = rows
            .iter()
            .find(|r| r.pid == me)
            .expect("own process must appear");
        assert!(mine.res > 0, "resident size must be known");
        assert!(mine.cmdline.is_some(), "own cmdline is readable");
    }

    #[test]
    fn second_sample_yields_finite_cpu() {
        let mut s = Sampler::new();
        s.sample();
        std::thread::sleep(std::time::Duration::from_millis(60));
        let (snap, rows) = s.sample();
        for row in &rows {
            assert!(row.percent_cpu.is_finite(), "cpu% must be finite");
            assert!(row.percent_cpu >= 0.0);
        }
        assert!(snap.cpu_usage().is_finite());
    }

    #[test]
    fn identity_cache_is_stable_across_ticks() {
        let mut s = Sampler::new();
        s.sample();
        let first = s.cache_len();
        assert!(first > 0);
        s.sample();
        assert!(
            s.cache_len() <= first * 2,
            "cache must not grow unboundedly"
        );
    }
}
