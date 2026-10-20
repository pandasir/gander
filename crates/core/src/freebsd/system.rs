//! The FreeBSD sampler.
//!
//! # Why CPU% divides by wall-clock time
//!
//! `kinfo_proc.ki_runtime` is cumulative **microseconds**, not a tick counter, so
//! there is no system-wide tick total to difference against the way Linux's
//! `/proc/stat` provides one. The denominator has to be the interval between two
//! samples, exactly as on macOS — and `kern.cp_time` is *also* in ticks (100 Hz),
//! so mixing the two would give a percentage off by four orders of magnitude.
//!
//! # What is free and what is not
//!
//! One `sysctl(KERN_PROC_PROC)` returns the whole table with CPU time, memory,
//! thread count and the BSD identity fields, so the default column set costs
//! exactly one syscall. The expensive reads are gated:
//!
//! * command line, exe path and cwd — three more syscalls per *process lifetime*,
//!   cached with the identity (they do not change);
//! * threads — one sysctl per process, only when thread rows are on screen.

use std::collections::HashMap;
use std::time::Instant;

use crate::freebsd::{parse, scan, FreeBsdPlatform};
use crate::row::{Row, RowFlags};
use crate::system::{CpuTimes, DiskCounters, MemInfo, NetCounters, SwapInfo, SystemSnapshot};
use crate::table::{Cache, Stable};
use crate::Pid;

/// Per-process state that must survive between ticks.
#[derive(Copy, Clone)]
struct PrevRow {
    /// Start time in microseconds: the PID-reuse key, exactly as on the other two
    /// backends.
    start_usec: u64,
    /// Cumulative CPU microseconds.
    runtime_us: u64,
}

/// Per-thread state, keyed by `(pid, tid)`.
///
/// A tid on FreeBSD is unique across the system the way Linux's is, so the pid is
/// not strictly needed — but it costs nothing and it keeps the key shape the same
/// as macOS's, where the handle is only unique *within* a process.
#[derive(Copy, Clone)]
struct PrevThread {
    runtime_us: u64,
}

const CACHE_SLACK: usize = 4;

pub struct Sampler {
    cache: Cache,
    prev_rows: HashMap<Pid, PrevRow>,
    prev_threads: HashMap<(Pid, i32), PrevThread>,
    prev_cpu: Vec<CpuTimes>,
    prev_at: Option<Instant>,
    prev_net: Option<(NetCounters, Instant)>,
    prev_disk: Option<(DiskCounters, Instant)>,
    page_size: u64,
    cpu_count: usize,
    show_threads: bool,
    /// Whether the dashboard has ever needed the identity strings.
    ///
    /// `COMM` alone is satisfied by `ki_comm`, so a batch run that asks only for
    /// pids and CPU must not spend three syscalls per process on paths nobody
    /// reads. The front end sets this from the visible columns.
    read_identity: bool,
    /// Cached `kern.boottime`, which never changes for the life of an uptime.
    boot_time: Option<i64>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            cache: Cache::new(),
            prev_rows: HashMap::new(),
            prev_threads: HashMap::new(),
            prev_cpu: Vec::new(),
            prev_at: None,
            prev_net: None,
            prev_disk: None,
            page_size: FreeBsdPlatform::page_size(),
            cpu_count: FreeBsdPlatform::cpu_count(),
            show_threads: false,
            read_identity: true,
            boot_time: scan::boot_time(),
        }
    }

    /// Boot time, from `kern.boottime`.
    ///
    /// Read once and kept: a machine's boot time does not move. Re-reading it per
    /// tick would be one syscall per frame for a constant, and the drift between
    /// reads (a second here or there) would make `ELAPSED` jitter.
    pub fn boot_time(&self) -> Option<i64> {
        self.boot_time
    }

    /// One extra row per thread, with `tgid` pointing at its process.
    ///
    /// The thread-group leader is skipped by *tid == pid*, which is what the
    /// kernel guarantees for the leader and is checkable, unlike macOS's
    /// position-based rule.
    fn read_threads(
        &mut self,
        procs: &[scan::RawProc],
        proc_rows: &[Row],
        interval_secs: f64,
    ) -> Vec<Row> {
        let mut out = Vec::new();
        let mut next_prev = HashMap::with_capacity(self.prev_threads.len());
        // Threads share the address space, so memory and elapsed time come from
        // the process row rather than from the thread's own (identical) figure.
        //
        // Indexed once rather than searched per thread: with 40 kernel threads
        // per process a linear `find` inside the thread loop is quadratic in the
        // process count, and on a busy machine that is the whole tick.
        let mem_of: HashMap<Pid, (u64, u64, u64, f32)> = proc_rows
            .iter()
            .map(|r| (r.pid, (r.res, r.virt, r.elapsed, r.percent_mem)))
            .collect();

        for kp in procs {
            let pid = kp.pid;
            if pid == 0 {
                // Pid 0 is the kernel's thread container: no address space, no
                // user, no command line. Listing 40 rows called "kernel" teaches
                // nothing and inflates every count.
                continue;
            }
            let Ok(threads) = scan::list_threads(pid) else {
                continue;
            };
            if threads.len() < 2 {
                continue; // nothing beyond the leader
            }
            let stable = self.identity(kp, kp.start_sec, kp.start_usec);

            for thread in threads {
                if thread.tid == pid as i32 {
                    continue; // the leader is already the process row
                }
                let mut row = Row::default_for_test(pid);
                row.thread_id = Some(thread.tid as u64);
                row.tgid = pid;
                row.is_thread = true;
                row.ppid = kp.ppid;
                row.pgrp = kp.pgid;
                row.session = kp.sid;
                // A thread has no terminal of its own; it inherits the process's,
                // which is what `ps -H` shows too.
                row.tpgid = kp.tpgid as i64;
                row.tty_nr = normalise_tty(kp.tty_dev);
                row.starttime = (kp.start_sec.max(0) as u64).saturating_mul(1_000_000)
                    + kp.start_usec.max(0) as u64;
                row.uid = kp.uid;
                row.nice = kp.nice;
                row.priority = thread.pri_level as i8;
                row.sched_policy = parse::sched_policy(thread.pri_class);
                row.state = parse::state_letter(thread.stat, thread.tdf_intr, thread.slptime);
                row.nlwp = 1;
                row.processor = thread.oncpu;
                // Faults belong to the address space, so a thread gets the
                // process's figures — the same rule memory and elapsed time
                // follow, and the same thing Linux's `task/<tid>/stat` reports.
                row.min_flt = kp.min_flt;
                row.maj_flt = kp.maj_flt;

                // Prefer the thread's own name, which is what `top -H` prints,
                // and fall back to the process name so the row is never blank —
                // an unnamed kernel thread reports the process's name anyway.
                let name = thread.tdname_str();
                let comm = if name.is_empty() {
                    thread.comm_str()
                } else {
                    name
                };
                row.comm = parse::comm_buf(&comm);

                let cpu_us = thread.runtime_us;
                let prev = self.prev_threads.get(&(pid, thread.tid)).copied();
                if let Some(p) = prev {
                    if interval_secs > 0.0 && cpu_us >= p.runtime_us {
                        let delta = (cpu_us - p.runtime_us) as f64 / 1e6;
                        row.percent_cpu =
                            (delta / interval_secs * 100.0) as f32;
                        let cap = self.cpu_count as f32 * 100.0;
                        row.percent_cpu = row.percent_cpu.min(cap);
                    }
                }
                next_prev.insert((pid, thread.tid), PrevThread { runtime_us: cpu_us });

                row.time_ticks = cpu_us / 10_000; // hundredths of a second
                row.cmdline = stable.cmdline.clone();
                row.exe = stable.exe.clone();
                row.cwd = stable.cwd.clone();
                row.flags = row
                    .flags
                    .insert(RowFlags::USERLAND_THREAD)
                    .set(RowFlags::KERNEL_THREAD, thread.is_kernel_thread());

                if let Some(&(res, virt, elapsed, mem_pct)) = mem_of.get(&pid) {
                    row.res = res;
                    row.virt = virt;
                    row.elapsed = elapsed;
                    row.percent_mem = mem_pct;
                }
                out.push(row);
            }
        }

        self.prev_threads = next_prev;
        out
    }

    /// Immutable per-process identity: command line, exe, cwd.
    ///
    /// Three syscalls per process per *lifetime*, not per tick. They cannot
    /// change without the process being replaced (the exe can be deleted
    /// underneath it, which this does not detect — FreeBSD has no `/proc/<pid>/exe`
    /// symlink whose target carries a ` (deleted)` suffix, so the `STALE_EXE`
    /// flag stays clear here rather than being guessed at).
    fn identity(&mut self, kp: &scan::RawProc, start_sec: i64, start_usec: i64) -> Stable {
        let key = (start_sec.max(0) as u64).saturating_mul(1_000_000) + start_usec.max(0) as u64;
        if let Some(hit) = self.cache.get(kp.pid, key) {
            return hit.clone();
        }
        let mut stable = Stable::default();
        if self.read_identity {
            stable.cmdline = scan::cmdline(kp.pid).map(String::into_boxed_str);
            stable.exe = scan::exe_path(kp.pid).map(String::into_boxed_str);
            stable.cwd = scan::cwd_path(kp.pid).map(String::into_boxed_str);
        }
        // Session, pgrp, tpgid and the tty arrive in the process list, so they
        // need no caching; they are here only because `Stable` is the shared
        // shape and the thread pass reads them from it.
        stable.session = kp.sid;
        stable.pgrp = kp.pgid;
        stable.tpgid = kp.tpgid as i64;
        stable.tty_dev = normalise_tty(kp.tty_dev);
        stable.is_kernel_thread = kp.is_kernel_thread();
        self.cache.insert(kp.pid, key, stable.clone());
        stable
    }

    fn sample_inner(&mut self) -> (SystemSnapshot, Vec<Row>) {
        let at = Instant::now();

        // --- host counters ---------------------------------------------------
        let cpu = self.read_cpu();
        let cpu_delta: Vec<CpuTimes> = if self.prev_cpu.len() == cpu.len() && !cpu.is_empty() {
            cpu.iter().zip(&self.prev_cpu).map(|(n, p)| n.delta(p)).collect()
        } else {
            vec![CpuTimes::default(); cpu.len()]
        };

        let mut mem = self.read_mem();
        mem.finish_freebsd();
        let swap = self.read_swap();
        let load = self.read_loadavg();
        let uptime = self.uptime();

        let net = scan::net_counters();
        let net_rate = match self.prev_net {
            Some((prev, at_prev)) => net.per_sec(&prev, at.duration_since(at_prev).as_secs_f64()),
            None => crate::system::Rate::default(),
        };
        self.prev_net = Some((net, at));

        let disk = scan::disk_counters();
        let disk_rate = match self.prev_disk {
            Some((prev, at_prev)) => {
                disk.per_sec(&prev, at.duration_since(at_prev).as_secs_f64())
            }
            None => crate::system::Rate::default(),
        };
        self.prev_disk = Some((disk, at));

        // --- processes -------------------------------------------------------
        let procs = scan::list_processes().unwrap_or_default();
        let interval = self
            .prev_at
            .map(|p| at.duration_since(p).as_secs_f64())
            .unwrap_or(0.0);
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut rows = self.read_rows(&procs, interval, mem.total, now_secs);
        if self.show_threads {
            let threads = self.read_threads(&procs, &rows, interval);
            rows.extend(threads);
        }

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
            clk_tck: FreeBsdPlatform::clk_tck(),
            page_size: self.page_size,
            net,
            net_rate,
            disk,
            disk_rate,
            tasks: task_stats(&rows),
            // No `/proc/pressure` and no zram on FreeBSD.
            psi: None,
            zram: crate::system::ZramInfo::default(),
        };
        (snapshot, rows)
    }

    fn read_cpu(&self) -> Vec<CpuTimes> {
        // Index 0 is the aggregate, 1..=n the per-core counters, matching the
        // shape both other backends produce.
        let mut out = Vec::with_capacity(self.cpu_count + 1);
        out.push(
            scan::cp_time()
                .map(|v| parse::cpu_times(&v))
                .unwrap_or_default(),
        );
        match scan::cp_times() {
            Some(per) if !per.is_empty() => {
                out.extend(per.iter().map(parse::cpu_times));
            }
            _ => {
                // No per-core counters: fall back to `ncpu` copies of the
                // aggregate so the meter still draws the right number of bars.
                let agg = out[0];
                out.extend(std::iter::repeat_n(agg, self.cpu_count));
            }
        }
        out
    }

    /// The memory counters, and why the denominator is `v_page_count`.
    ///
    /// `hw.physmem` is the machine's physical memory, which on this VM is
    /// 4 265 984 000 bytes — larger than `v_page_count * pagesize`
    /// (4 159 893 504) by 876 pages. Those pages are the kernel's early
    /// allocations, which the VM subsystem never manages, so no queue will ever
    /// account for them. Using `hw.physmem` as the bar's denominator therefore
    /// leaves it permanently ~3 MiB short of full, which reads as a rendering bug
    /// on a machine that is genuinely out of memory.
    ///
    /// `v_page_count` is by definition the sum of the queues, so the segments
    /// partition the bar exactly. `hw.physmem` is still used when `v_page_count`
    /// is missing, because a total of zero is worse than a bar that does not quite
    /// reach the end.
    fn read_mem(&self) -> MemInfo {
        let page = self.page_size;
        let pages = |name: &std::ffi::CStr| -> u64 {
            scan::sysctl_u64(name).unwrap_or(0).saturating_mul(page)
        };
        let managed = scan::sysctl_u64(c"vm.stats.vm.v_page_count").unwrap_or(0);
        let total = if managed > 0 {
            managed.saturating_mul(page)
        } else {
            scan::sysctl_u64(c"hw.physmem")
                .or_else(|| scan::sysctl_u64(c"hw.realmem"))
                .unwrap_or(0)
        };
        MemInfo {
            total,
            free: pages(c"vm.stats.vm.v_free_count"),
            active: pages(c"vm.stats.vm.v_active_count"),
            inactive: pages(c"vm.stats.vm.v_inactive_count"),
            wired: pages(c"vm.stats.vm.v_wire_count"),
            laundry: pages(c"vm.stats.vm.v_laundry_count"),
            file_cache: pages(c"vm.stats.vm.v_cache_count"),
            managed_pages: managed,
            ..Default::default()
        }
    }

    fn read_swap(&self) -> SwapInfo {
        // `kvm_getswapinfo` needs a `kvm_t`, which a jailed root cannot open. The
        // total is still readable through `vm.swap_total`, so fall back to that
        // with `used` unknown (= 0) rather than reporting "no swap at all".
        if let Some(s) = scan::swap_info() {
            return s;
        }
        let total = scan::sysctl_u64(c"vm.swap_total").unwrap_or(0);
        SwapInfo {
            total,
            used: 0,
            free: total,
        }
    }

    fn read_loadavg(&self) -> [f64; 3] {
        let mut load = [0.0f64; 3];
        // SAFETY: `load` is three `f64`s and 3 is passed as the element count.
        let rc = unsafe { libc::getloadavg(load.as_mut_ptr(), 3) };
        if rc == 3 {
            load
        } else {
            [0.0; 3]
        }
    }

    fn uptime(&self) -> u64 {
        let Some(boot) = self.boot_time else {
            return 0;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        now.saturating_sub(boot).max(0) as u64
    }

    fn read_rows(
        &mut self,
        procs: &[scan::RawProc],
        interval_secs: f64,
        mem_total: u64,
        now_secs: u64,
    ) -> Vec<Row> {
        let mut out = Vec::with_capacity(procs.len());
        let mut next_prev = HashMap::with_capacity(procs.len());
        let cap = self.cpu_count as f32 * 100.0;

        for kp in procs {
            // Pid 0 is the kernel's thread container, not a process: it has no
            // address space, no user and no command line. htop lists its threads
            // separately and so does this backend, via `read_threads`; as a
            // *process* row it would be a 0-byte entry counted in `Tasks`.
            if kp.pid == 0 {
                continue;
            }

            let start_usec = (kp.start_sec.max(0) as u64).saturating_mul(1_000_000)
                + kp.start_usec.max(0) as u64;
            let prev = self.prev_rows.get(&kp.pid).copied();
            let same_process = prev.is_some_and(|p| p.start_usec == start_usec);

            let mut row = Row::default_for_test(kp.pid);
            row.starttime = start_usec;
            row.ppid = kp.ppid;
            row.pgrp = kp.pgid;
            row.session = kp.sid;
            row.tpgid = kp.tpgid as i64;
            row.tty_nr = normalise_tty(kp.tty_dev);
            row.uid = kp.uid;
            row.nice = kp.nice;
            row.priority = kp.pri_level as i8;
            row.sched_policy = parse::sched_policy(kp.pri_class);
            row.state = parse::state_letter(kp.stat, kp.tdf_intr, kp.slptime);
            row.nlwp = kp.num_threads.max(1);
            // `NOCPU` (-1) means "not on a CPU right now", which is the honest
            // answer for a sleeping process. Falling back to `lastcpu` the way
            // htop does would claim a sleeping process is *on* the CPU it last
            // used, and the column exists to answer "where is it running".
            row.processor = kp.oncpu;
            row.min_flt = kp.min_flt;
            row.maj_flt = kp.maj_flt;
            row.comm = parse::comm_buf(&kp.comm_str());

            row.time_ticks = kp.runtime_us / 10_000; // hundredths of a second
            row.virt = kp.vsize;
            // `ki_rssize` is in pages; scaling by the page size is the whole
            // difference between "1.2G resident" and "300M resident".
            row.res = (kp.rss_pages.max(0) as u64).saturating_mul(self.page_size);

            if same_process {
                if let Some(p) = prev {
                    if interval_secs > 0.0 && kp.runtime_us >= p.runtime_us {
                        let delta = (kp.runtime_us - p.runtime_us) as f64 / 1e6;
                        row.percent_cpu = ((delta / interval_secs * 100.0) as f32).min(cap);
                    }
                }
            }
            next_prev.insert(
                kp.pid,
                PrevRow {
                    start_usec,
                    runtime_us: kp.runtime_us,
                },
            );

            row.percent_mem = if mem_total > 0 {
                row.res as f32 * 100.0 / mem_total as f32
            } else {
                0.0
            };
            // Elapsed is `now - start`, both absolute. Subtracting the boot time
            // first (as an earlier draft did) turns a start time into an age and
            // then subtracts uptime from it, which reports 0 for every process on
            // a machine whose clock is newer than its start.
            row.elapsed = now_secs.saturating_sub(kp.start_sec.max(0) as u64);

            let stable = self.identity(kp, kp.start_sec, kp.start_usec);
            row.cmdline = stable.cmdline;
            row.exe = stable.exe;
            row.cwd = stable.cwd;
            row.flags = row
                .flags
                .set(RowFlags::KERNEL_THREAD, stable.is_kernel_thread);

            out.push(row);
        }

        self.prev_rows = next_prev;
        out
    }
}

/// `ki_tdev`'s "no terminal" value is all-ones, where Linux uses `0` and macOS
/// normalises to `0` as well. One rule for the renderer, so `?` means the same
/// thing on all three.
fn normalise_tty(dev: u64) -> i64 {
    if dev == u64::MAX {
        0
    } else {
        dev as i64
    }
}

/// Summarise the process table for the `Tasks` meter.
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

impl crate::PlatformSampler for Sampler {
    fn sample(&mut self) -> (SystemSnapshot, Vec<Row>) {
        self.sample_inner()
    }

    fn cache_len(&self) -> usize {
        self.cache.len()
    }

    fn backend_name() -> &'static str {
        "freebsd/sysctl"
    }

    fn set_scan_options(&mut self, opts: crate::ScanOptions) {
        self.show_threads = opts.threads;
        // `smaps`, `gpu` and `delayacct` are Linux-only reads; FreeBSD has no
        // `/proc/<pid>/smaps_rollup`, no `drm-usage-stats` in `fdinfo` and no
        // `TASKSTATS` family, so a request for them changes nothing here rather
        // than being half-honoured.
    }

    fn supports_threads() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformSampler;

    #[test]
    fn a_live_sample_produces_rows() {
        let mut s = Sampler::new();
        let (snap, rows) = s.sample();
        assert!(!rows.is_empty(), "there are always processes");
        assert!(snap.mem.total > 0, "physical memory must be known");
        assert!(snap.cpu.len() >= 2, "aggregate plus per-core");
        assert_eq!(snap.clk_tck, 100);
    }

    #[test]
    fn self_appears_with_identity_and_a_sane_size() {
        let me = std::process::id();
        let mut s = Sampler::new();
        let (_snap, rows) = s.sample();
        let mine = rows
            .iter()
            .find(|r| r.pid == me)
            .expect("own process must appear");
        assert!(mine.res > 0, "resident size must be non-zero");
        assert!(mine.virt > 0, "virtual size must be non-zero");
        assert!(mine.cmdline.is_some(), "own cmdline is readable");
        assert!(mine.cwd.is_some(), "own cwd is readable");
        assert_eq!(mine.uid, unsafe { libc::getuid() });
        assert!(!mine.is_thread);
    }

    #[test]
    fn second_sample_yields_finite_cpu() {
        let mut s = Sampler::new();
        s.sample();
        std::thread::sleep(std::time::Duration::from_millis(80));
        let (snap, rows) = s.sample();
        for row in &rows {
            assert!(row.percent_cpu.is_finite(), "cpu% must be finite");
            assert!(row.percent_cpu >= 0.0, "cpu% must not be negative");
            assert!(row.percent_mem.is_finite());
        }
        assert!(snap.cpu_usage().is_finite());
        assert!((0.0..=1.0).contains(&snap.cpu_usage()), "aggregate is a share");
    }

    /// The bar segments must add up to the total, so nothing is drawn twice and
    /// nothing falls off the end.
    ///
    /// The tolerance is not slack for a wrong formula: each `vm.stats` counter is
    /// a **separate `sysctl` call**, so six calls see six slightly different
    /// moments in a running VM with a page daemon. On a quiet machine they agree
    /// exactly; under load a few hundred pages move between two of the reads.
    /// A strict equality assertion would be a flaky test that teaches nothing,
    /// and reading them atomically would need a subtree walk for no gain.
    #[test]
    fn memory_segments_partition_the_total() {
        let mut s = Sampler::new();
        let (snap, _rows) = s.sample();
        let m = &snap.mem;
        assert!(m.total > 0);
        assert!(m.used <= m.total, "used {} > total {}", m.used, m.total);
        assert!(m.available <= m.total);

        let segments: u64 = m
            .classes
            .iter()
            .filter(|c| c.counts_as_total)
            .map(|c| c.bytes)
            .sum();
        let tolerance = (m.total / 100).max(4 * 1024 * 1024); // 1 %, minimum 4 MiB
        assert!(
            segments.abs_diff(m.total) <= tolerance,
            "bar segments sum to {segments}, total is {} (tolerance {tolerance})",
            m.total
        );
        assert_eq!(
            m.used_from_classes(),
            m.used,
            "the classes are the authority on `used`"
        );
    }

    #[test]
    fn the_identity_cache_does_not_grow_without_bound() {
        let mut s = Sampler::new();
        s.sample();
        let first = s.cache_len();
        assert!(first > 0);
        s.sample();
        assert!(s.cache_len() <= first * 2);
    }

    #[test]
    fn threads_are_listed_only_when_asked_for() {
        let mut s = Sampler::new();
        let (_snap, rows) = s.sample();
        assert!(rows.iter().all(|r| !r.is_thread), "threads are off by default");

        s.set_scan_options(crate::ScanOptions::none().with_threads(true));
        let (_snap, rows) = s.sample();
        // This test binary is single-threaded, but the machine has kernel
        // threads; what matters is that no thread row claims to be a process.
        let threads: Vec<&Row> = rows.iter().filter(|r| r.is_thread).collect();
        for t in &threads {
            assert!(t.thread_id.is_some(), "a thread row needs its own identity");
            assert_ne!(t.thread_id, Some(t.pid as u64), "the leader is not a thread");
        }
    }

    #[test]
    fn boot_time_is_stable_between_samples() {
        let mut s = Sampler::new();
        let first = s.boot_time();
        s.sample();
        s.sample();
        assert_eq!(first, s.boot_time(), "boot time cannot change");
    }

    #[test]
    fn uptime_grows_between_samples() {
        let mut s = Sampler::new();
        let (a, _) = s.sample();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let (b, _) = s.sample();
        assert!(b.uptime >= a.uptime);
    }
}
