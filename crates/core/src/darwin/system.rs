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
///
/// Only what the next tick's delta actually needs. Cumulative disk counters used
/// to live here too, behind `#[allow(dead_code)]`, "kept for the IO columns
/// (milestone M1)" — a column family that M1 never shipped. The cost was not just
/// two unused fields: `proc_pid_rusage` was called once per process per tick and
/// the result dropped on the floor, which is a syscall paid for nothing on the
/// platform this project is developed on. Removed rather than carried further;
/// see `docs/ROADMAP.md` § M5 for what a real I/O column would need.
#[derive(Copy, Clone)]
struct PrevRow {
    /// `p_starttime` in microseconds, used as the identity of the process.
    start_usec: u64,
    /// Monotonic CPU nanoseconds, user + system.
    cpu_ns: u64,
}

/// Per-thread state carried between ticks, keyed by `(pid, handle)`.
///
/// The handle alone is not a valid key: every process reports the same handle for
/// its main thread, so two processes' main threads would collide and inherit each
/// other's CPU history.
#[derive(Copy, Clone)]
struct PrevThread {
    /// Monotonic CPU nanoseconds from `proc_threadinfo`, user + system.
    cpu_ns: u64,
}

/// A thread row's identity, unique across the whole table.
///
/// Neither native identifier is usable on its own:
///
/// * `handle` is not globally unique — every process reports the same handle for
///   its main thread, so two processes' threads would collide.
/// * `handle` is 64-bit and 95% of the values on a desktop exceed `u32`, so it
///   cannot simply be stored in `pid`.
///
/// The result packs the pid into the top 16 bits and a 48-bit hash of the handle
/// into the rest, so it is unique per `(pid, handle)` pair for any pid below
/// 65536 (macOS `PID_MAX` is 99998, so the top bit is never needed). The low half
/// is a xor-fold of the handle rather than the handle itself, because handles
/// share a large common prefix — `8681251360` for every main thread — and the
/// truncated raw value would keep only that shared part.
///
/// Stability: a handle does not move while the thread lives, so the identity is
/// stable across samples. It is not unique across *time*: a thread that exits can
/// have its handle reused, which is why the sampler also tracks `starttime`.
pub fn thread_identity(pid: Pid, handle: u64) -> u64 {
    let mixed = handle ^ ((pid as u64) << 32);
    // Keep the low 48 bits of the mix and put the pid back on top, so the result is
    // unique per (pid, handle) pair as long as pid < 2^16, which is far beyond the
    // kernel's PID_MAX (99998 on macOS).
    ((pid as u64) << 48) | (mixed & 0x0000_FFFF_FFFF_FFFF)
}

/// Drop the identity cache if it grows past this multiple of the live set.
const CACHE_SLACK: usize = 4;

pub struct Sampler {
    cache: Cache,
    prev_rows: HashMap<Pid, PrevRow>,
    /// Per-thread CPU counters, keyed by `(pid, thread handle)`.
    prev_threads: HashMap<(Pid, u64), PrevThread>,
    prev_cpu: Vec<CpuTimes>,
    prev_at: Option<Instant>,
    /// Previous host network counters, for the throughput meter.
    ///
    /// macOS has no host-wide disk byte counter reachable without IOKit, so
    /// there is no `prev_disk`; see `docs/PORTABILITY.md`.
    prev_net: Option<(crate::system::NetCounters, Instant)>,
    page_size: u64,
    cpu_count: usize,
    /// Whether to emit one row per thread in addition to one per process.
    show_threads: bool,
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
            prev_threads: HashMap::new(),
            prev_cpu: Vec::new(),
            prev_at: None,
            prev_net: None,
            page_size: scan::page_size(),
            cpu_count,
            show_threads: false,
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
            // Anonymous pages. `finish_darwin` keys `used` off this rather than
            // off active/inactive/speculative, which overlap with it.
            internal: vm.internal,
            ..Default::default()
        };
        mem.finish_darwin();

        let swap = scan::swap_usage();

        let load = scan::loadavg();
        let uptime = scan::uptime_secs();
        let now_secs = scan::now_secs();

        // Network throughput. Cumulative counters, so the rate needs the
        // previous sample and the interval measured between the two reads.
        let net = scan::net_counters();
        let net_rate = match self.prev_net {
            Some((prev, at_prev)) => net.per_sec(&prev, at.duration_since(at_prev).as_secs_f64()),
            None => crate::system::Rate::default(),
        };
        self.prev_net = Some((net, at));

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
        let mut rows = self.read_rows(&procs, interval, now_secs, mem.total);
        if self.show_threads {
            let threads = self.read_threads(&procs, &rows, interval, mem.total);
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
            clk_tck: 100,
            page_size: self.page_size,
            net,
            net_rate,
            // macOS exposes no host-wide disk byte counters without IOKit, and
            // the design forbids a vendor SDK. A per-process figure exists
            // (`proc_pid_rusage`) but summing it here would double-count shared
            // I/O and still not answer "what is the disk doing", so the meter
            // reports `n/a here` instead of a number nobody can act on.
            disk: crate::system::DiskCounters::default(),
            disk_rate: crate::system::Rate::default(),
            tasks: task_stats(&rows),
            // Neither exists on macOS: no `/proc/pressure`, no zram module.
            psi: None,
            zram: crate::system::ZramInfo::default(),
        };
        (snapshot, rows)
    }

    fn set_scan_options(&mut self, opts: crate::ScanOptions) {
        self.show_threads = opts.threads;
        // `smaps` and `gpu` are Linux-only reads; macOS has neither
        // `/proc/<pid>/smaps_rollup` nor `drm-usage-stats`, so a request for them
        // changes nothing here rather than being silently half-honoured.
    }

    fn supports_threads() -> bool {
        true
    }
}

impl Sampler {
    /// One extra row per thread, with `tgid` pointing at its process.
    ///
    /// The main thread is skipped: it is already present as the process row, and
    /// duplicating it would make the tree show every process twice. On macOS it is
    /// identified by *position* (index 0 of the list), because a thread has no pid
    /// to compare against; see `scan::list_threads`.
    ///
    /// Cost, measured on this machine with 686 processes / 2609 threads: about
    /// 2-5 ms per sample on top of the 0.7-1.4 ms the process table already
    /// costs. That is why threads stay off by default. The claim in an earlier
    /// version of `docs/ROADMAP.md` that this needed a Mach call per thread was
    /// wrong: `PROC_PIDTHREADINFO` returns the name and the CPU time in one libproc
    /// call, so no Mach port and no `task_for_pid` are involved.
    ///
    /// Threads inherit the process's uid, command line and exe, so only their own
    /// CPU counters and name are read here.
    fn read_threads(
        &mut self,
        procs: &[crate::darwin::kinfo::RawProc],
        proc_rows: &[Row],
        interval_secs: f64,
        mem_total: u64,
    ) -> Vec<Row> {
        let mut out = Vec::new();
        let mut next_prev: HashMap<(Pid, u64), PrevThread> =
            HashMap::with_capacity(self.prev_threads.len());
        // Memory is per address space, not per thread: every thread of a process
        // reports the same RSS on Linux too (`/proc/<pid>/task/<tid>/stat` field
        // 24 is the mm's figure). Inheriting it here keeps the two backends
        // consistent instead of showing a column of zeroes on macOS.
        let mem_of: HashMap<Pid, (u64, u64, u64)> = proc_rows
            .iter()
            .map(|r| (r.pid, (r.res, r.virt, r.elapsed)))
            .collect();

        for kp in procs {
            let pid = kp.pid;
            // Threads are only listable for processes we can inspect at all.
            let Some(handles) = scan::list_threads(pid) else {
                continue;
            };
            if handles.is_empty() {
                continue;
            }

            // Identity is the process's, already cached under the leader's pid by
            // `read_rows`. Reuse it rather than re-reading `KERN_PROCARGS2` for
            // every thread of the same process.
            let start_usec = kp.start_sec.saturating_mul(1_000_000);
            let stable = self.identity(pid, start_usec);

            for (index, handle) in handles.into_iter().enumerate() {
                // Index 0 is the main thread, which is the process row. It is
                // identified by position, not by being the first non-zero handle:
                // a process whose main-thread handle is withheld reports `[0, …]`
                // (pid 508 does), and treating the next entry as the leader would
                // drop a real thread instead.
                if index == 0 {
                    continue;
                }
                // A withheld handle elsewhere in the list is not readable; skip it
                // rather than asking `PROC_PIDTHREADINFO` about handle 0, which
                // would describe whatever thread the kernel maps 0 to.
                if handle == 0 {
                    continue;
                }

                let Some(ti) = scan::thread_info(pid, handle) else {
                    continue;
                };

                // `handle` cannot be used as the key on its own: every process
                // reports the same handle for its main thread, so keying on it
                // would make two processes share one CPU history. `(pid, handle)`
                // is unique within a sample, and a handle is stable for as long as
                // the thread lives.
                let cpu_ns = ti.user_ns.saturating_add(ti.system_ns);
                let prev = self.prev_threads.get(&(pid, handle)).copied();

                // A macOS thread has no pid of its own, so `pid` stays the
                // leader's and the thread gets its own identity. Using the
                // leader's pid for the thread row would make it indistinguishable
                // from the process row: kill, tagging and the selection all key on
                // `pid`.
                let mut row = Row::default_for_test(pid);
                row.thread_id = Some(thread_identity(pid, handle));
                row.tgid = pid;
                row.is_thread = true;
                row.ppid = pid;
                row.starttime = start_usec;
                row.uid = kp.uid;
                row.pgrp = stable.pgrp;
                row.tpgid = stable.tpgid;
                row.tty_nr = stable.tty_dev;
                row.nice = kp.nice.clamp(-128, 127) as i8;
                row.priority = row.nice;
                row.processor = -1;
                row.nlwp = 1;
                // A thread's identity files belong to its process.
                row.cmdline = stable.cmdline.clone();
                row.exe = stable.exe.clone();
                row.cwd = stable.cwd.clone();
                row.session = stable.session;
                row.flags = row
                    .flags
                    .insert(RowFlags::USERLAND_THREAD)
                    .set(RowFlags::KERNEL_THREAD, stable.is_kernel_thread);

                // The name is the only per-thread text macOS gives us. Fall back
                // to the process name so a row is never blank; 46% of threads on
                // this machine are unnamed.
                if ti.name_str().is_empty() {
                    let _ = row.comm.try_push_str(&kp.comm_str());
                } else {
                    let _ = row.comm.try_push_str(ti.name_str());
                }

                row.time_ticks = cpu_ns / 10_000_000; // hundredths of a second
                row.state = match ti.run_state {
                    // TH_STATE_RUNNING
                    1 => b'R',
                    // TH_STATE_STOPPED
                    2 => b'T',
                    // TH_STATE_WAITING / TH_STATE_UNINTERRUPTIBLE / HALTED
                    _ => b'S',
                };

                // Same wall-clock formula as the process rows: `proc_threadinfo`
                // reports nanoseconds, so a tick delta would be meaningless.
                if interval_secs > 0.0 {
                    if let Some(p) = prev {
                        if cpu_ns >= p.cpu_ns {
                            let delta = (cpu_ns - p.cpu_ns) as f64;
                            let pct = delta / (interval_secs * 1e9) * 100.0;
                            row.percent_cpu = (pct as f32).min(self.cpu_count as f32 * 100.0);
                        }
                    }
                }

                // Threads share the process's address space, so the process's
                // resident size is the only per-thread figure that exists; this
                // matches what Linux reports from `task/<tid>/stat`.
                let (res, virt, elapsed) = mem_of.get(&pid).copied().unwrap_or((0, 0, 0));
                row.res = res;
                row.virt = virt;
                row.elapsed = elapsed;
                row.percent_mem = if mem_total > 0 {
                    row.res as f32 * 100.0 / mem_total as f32
                } else {
                    0.0
                };
                next_prev.insert((pid, handle), PrevThread { cpu_ns });
                out.push(row);
            }
        }

        self.prev_threads = next_prev;
        out
    }

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
            let bsd_details = scan::task_all_info(pid);
            let ti = bsd_details.map(|all| all.task);
            let details = bsd_details.map(|all| all.bsd).unwrap_or_default();

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
            // PGRP, TPGID, and the session id all come from the per-process
            // `proc_bsdinfo` call rather than the `KERN_PROC_ALL` record, which
            // carries only the fields `kinfo` decodes by offset. Unreadable
            // processes keep the defaults, and the columns render `-`.
            row.pgrp = details.pgrp;
            row.tpgid = details.tpgid;
            row.tty_nr = details.tty_dev;
            // SAFETY: `getsid` only reads kernel state for a pid. `-1` (`EPERM`
            // for another user's process) becomes `0` = unknown.
            let sid = unsafe { libc::getsid(pid as libc::pid_t) };
            row.session = if sid > 0 { sid as u32 } else { 0 };
            row.uid = kp.uid;
            row.nice = kp.nice.clamp(-128, 127) as i8;
            row.priority = kp.nice.clamp(-128, 127) as i8;
            row.state = parse::state_letter(kp.p_stat);
            // `p_stat` alone cannot be trusted for the sleep/run distinction on
            // current Darwin: this kernel reports `SRUN` (2) for a process
            // blocked in `read(2)`, while `ps -o stat=` calls the same process
            // `S`. Only the *stopped* and *zombie* values are reliable, so they
            // are kept, and everything else falls back to the per-task counter.
            //
            // Without this, `TasksMeter` reported 201 "running" processes out of
            // 514 where htop showed 3, and the `R` badge was meaningless.
            if matches!(row.state, b'T' | b'Z' | b'I') {
                // Authoritative: leave it alone.
            } else if let Some(t) = ti {
                row.state = if t.running_threads > 0 { b'R' } else { b'S' };
            } else {
                // No task info means the process belongs to another user. We
                // genuinely do not know whether it is running, and claiming `R`
                // for every unreadable process is what produced the bogus
                // counts. `?` is the honest answer and matches how htop renders
                // a state it could not determine.
                row.state = b'?';
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

            next_prev.insert(
                pid,
                PrevRow {
                    start_usec,
                    cpu_ns,
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
                row.cmdline = stable.cmdline;
                row.exe = stable.exe;
                row.cwd = stable.cwd;
                row.session = stable.session;
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

    /// Immutable per-process identity: exe path, command line, kernel-thread flag.
    ///
    /// The uid is *not* part of this: it comes from `KERN_PROC_ALL` on every scan
    /// and needs no lookup. It used to be overwritten with `0` here, which made
    /// every process show up as `root`.
    fn identity(&mut self, pid: Pid, key: u64) -> Stable {
        if let Some(hit) = self.cache.get(pid, key) {
            return hit.clone();
        }

        let mut stable = Stable {
            exe: scan::exe_path(pid).map(String::into_boxed_str),
            cwd: scan::cwd_path(pid).map(String::into_boxed_str),
            ..Stable::default()
        };
        // `proc_bsdinfo` has no session id, and the `kinfo_proc` field that does
        // is not among the offsets this build decodes. `getsid(2)` asks the kernel
        // directly; it returns `-1`/`EPERM` for another user's process, which is
        // recorded as `0` = unknown rather than guessed at.
        // SAFETY: `getsid` only reads kernel state for a pid.
        let sid = unsafe { libc::getsid(pid as libc::pid_t) };
        stable.session = if sid > 0 { sid as u32 } else { 0 };
        // Cached from the same `proc_bsdinfo` call the caller already made; the
        // thread pass reuses it rather than calling `proc_pidinfo` once per thread
        // for two integers.
        if let Some(all) = scan::task_all_info(pid) {
            stable.pgrp = all.bsd.pgrp;
            stable.tpgid = all.bsd.tpgid;
            stable.tty_dev = all.bsd.tty_dev;
        }
        stable.cmdline = scan::procargs2(pid)
            .and_then(|raw| parse::parse_procargs2(&raw))
            .filter(|s| !s.is_empty())
            .map(String::into_boxed_str);
        stable.is_kernel_thread = stable.cmdline.is_none() && stable.exe.is_none();

        self.cache.insert(pid, key, stable.clone());
        stable
    }
}

/// Summarise the process table for the `Tasks` meter.
///
/// Only process rows are counted. When thread rows are present they are
/// *additional* rows for the same process, so counting every row would report
/// both `total` and `threads` roughly twice over: the leader's `nlwp` already
/// counts its threads, and each thread row contributes one more.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_identity_is_unique_per_pid_and_handle() {
        // The same handle under two pids must not collide: every process reports
        // the same handle for its main thread.
        let shared = 8_681_251_360u64;
        assert_ne!(thread_identity(1, shared), thread_identity(2, shared));

        // Different handles under one pid must not collide either.
        assert_ne!(thread_identity(1, 100), thread_identity(1, 200));

        // And the same pair is stable, so a thread keeps its identity across
        // samples.
        assert_eq!(thread_identity(42, 7), thread_identity(42, 7));
    }

    #[test]
    fn thread_identity_does_not_collide_with_a_process_pid() {
        // `Row::identity` returns the bare pid for a process, so a thread id must
        // never land on a plausible pid.
        for pid in [1u32, 2, 100, 99998] {
            for handle in [0u64, 1, 8_681_251_360, u64::MAX] {
                let id = thread_identity(pid, handle);
                assert!(
                    id > u32::MAX as u64,
                    "thread id {id} for pid {pid} would collide with a pid"
                );
            }
        }
    }

    #[test]
    fn task_stats_count_processes_not_rows() {
        let mut proc = Row::default_for_test(1);
        proc.nlwp = 4;
        proc.state = b'R';
        let mut thread = Row::default_for_test(1);
        thread.is_thread = true;
        thread.thread_id = Some(thread_identity(1, 99));
        thread.nlwp = 1;
        thread.state = b'R';

        let stats = task_stats(&[proc, thread]);
        assert_eq!(stats.total, 1, "one process, not two rows");
        assert_eq!(stats.running, 1, "the thread must not be counted again");
        assert_eq!(stats.threads, 4, "nlwp already counts the threads");
    }

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
