//! System-wide snapshots: CPU, memory, swap, load, uptime.
//!
//! One immutable value per sample, handed to the UI atomically (`docs/DESIGN.md` §6).

use std::time::Instant;

#[derive(Copy, Clone, Default, Debug)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
    pub guest: u64,
    pub guest_nice: u64,
}

impl CpuTimes {
    /// Total non-idle-plus-idle ticks. This is the denominator for process CPU%.
    pub fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
            + self.guest
            + self.guest_nice
    }

    /// Everything that is not idle. Split into the segments htop's CPU bar draws.
    pub fn busy(&self) -> u64 {
        self.total().saturating_sub(self.idle)
    }

    /// Tick-wise difference against an earlier sample.
    pub fn delta(&self, prev: &CpuTimes) -> CpuTimes {
        CpuTimes {
            user: self.user.saturating_sub(prev.user),
            nice: self.nice.saturating_sub(prev.nice),
            system: self.system.saturating_sub(prev.system),
            idle: self.idle.saturating_sub(prev.idle),
            iowait: self.iowait.saturating_sub(prev.iowait),
            irq: self.irq.saturating_sub(prev.irq),
            softirq: self.softirq.saturating_sub(prev.softirq),
            steal: self.steal.saturating_sub(prev.steal),
            guest: self.guest.saturating_sub(prev.guest),
            guest_nice: self.guest_nice.saturating_sub(prev.guest_nice),
        }
    }
}

/// One named class of memory, as htop models it.
///
/// `counts_as_used` adds it to the "used" figure; `counts_as_cache` marks it as
/// reclaimable. Modeling this as data (which is what htop 3.6 does in
/// `MemoryClass[]`) keeps the formula reviewable instead of hiding it in
/// arithmetic.
#[derive(Copy, Clone, Debug)]
pub struct MemoryClass {
    pub label: &'static str,
    pub bytes: u64,
    pub counts_as_used: bool,
    pub counts_as_cache: bool,
    pub counts_as_total: bool,
    /// Color slot, 0-5, matching htop's `MEMORY_1..6` palette order.
    pub color_slot: u8,
}

/// The memory breakdown a platform reports.
///
/// Every backend fills in `total` and `classes`, then calls
/// `finish_linux` or `finish_darwin` exactly once to derive `used`/`available`.
///
/// The classes are the *authority* on what counts as used: `finish_*` only sums
/// the flags. This matters because the platforms genuinely disagree, and every
/// attempt to hardcode a formula produced a number that contradicted either the
/// class sum or the operating system (`docs/PORTABILITY.md`).
#[derive(Clone, Default, Debug)]
pub struct MemInfo {
    pub total: u64,
    /// Memory that can be handed to a new allocation, including reclaimable
    /// cache. Derived by the `finish_*` methods.
    pub available: u64,
    /// Derived: the sum of every class flagged `counts_as_used`.
    pub used: u64,
    /// Free pages. Not a class of its own on macOS, where `free` is tiny and the
    /// page cache holds the slack.
    pub free: u64,
    /// The ordered class list. Order is the bar-segment order.
    pub classes: Vec<MemoryClass>,

    // Raw fields kept for platform-specific reporting and diagnostics.
    pub buffers: u64,
    pub cached: u64,
    pub sreclaimable: u64,
    pub shmem: u64,
    pub wired: u64,
    pub compressed: u64,
    pub active: u64,
    pub inactive: u64,
    pub purgeable: u64,
    pub speculative: u64,
}

impl MemInfo {
    /// Sum the classes tagged `counts_as_used`.
    pub fn used_from_classes(&self) -> u64 {
        self.classes
            .iter()
            .filter(|c| c.counts_as_used)
            .map(|c| c.bytes)
            .sum()
    }

    /// Sum the classes tagged `counts_as_cache`.
    pub fn cache_from_classes(&self) -> u64 {
        self.classes
            .iter()
            .filter(|c| c.counts_as_cache)
            .map(|c| c.bytes)
            .sum()
    }

    /// Linux/GNU `free` accounting, matching `linux/LinuxMachine.c`.
    ///
    /// `used = total - free - cached - sreclaimable - buffers`, with `shmem`
    /// subtracted from the cached total first so it is not counted twice.
    /// `available` is `MemAvailable` clamped to `total`, falling back to `free`
    /// on kernels older than 3.14 that do not provide it.
    ///
    /// Do not try to make `used + available == total` here: they measure
    /// different things, and forcing it clamps `MemAvailable` down to
    /// `total - used`, which is a much smaller (wrong) number.
    pub fn finish_linux(&mut self) {
        // `used = total - free - cached - sreclaimable - buffers`, which is
        // exactly `free(1)`'s `total - free - buff/cache`.
        let used_diff = self.free + self.cached + self.sreclaimable + self.buffers;
        self.used = if self.total >= used_diff {
            self.total - used_diff
        } else {
            self.total.saturating_sub(self.free)
        };
        // Report `cached` the way `free` does: Meminfo's `Cached` plus
        // `SReclaimable`, minus `Shmem` which is already counted in `Cached`.
        // Overwriting the field keeps one meaning from here on; computing `used`
        // above used the raw values.
        self.cached = self.cached + self.sreclaimable - self.shmem;
        self.available = if self.available != 0 {
            self.available.min(self.total)
        } else {
            self.free
        };

        self.classes = vec![
            MemoryClass {
                label: "used",
                bytes: self.used,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 0,
            },
            MemoryClass {
                label: "buffers",
                bytes: self.buffers,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 1,
            },
            MemoryClass {
                label: "cache",
                bytes: self.cached,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 2,
            },
            MemoryClass {
                label: "free",
                bytes: self.free,
                counts_as_used: false,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 5,
            },
        ];
    }

    /// macOS accounting, matching **htop 3.3.0** exactly.
    ///
    /// `used = active + wired`
    ///
    /// This is deliberately the same formula as `darwin/Platform.c` in htop
    /// 3.3.0:
    ///
    /// ```c
    /// mtr->values[MEMORY_METER_USED] = (vm->active_count + vm->wire_count) * page_K;
    /// mtr->values[MEMORY_METER_CACHE] = vm->inactive_count * page_K;
    /// ```
    ///
    /// The point is that `gander` and the `htop` next to it must agree; a
    /// different (even "better") formula makes the two impossible to compare.
    ///
    /// Consequences, all intentional:
    /// * `compressed` is **not** counted as used. htop 3.3.0 shows it as a
    ///   separate segment, so it appears in the bar but not in the number. On a
    ///   machine with 2 GB in the compressor this is the bulk of the gap to
    ///   `top`.
    /// * `inactive` is cache, not used.
    /// * `available = total - used`, so the two figures partition memory exactly.
    ///
    /// Newer htop (3.4+) changed the formula. When matching a different version
    /// is wanted, see [`MemInfo::finish_darwin_36`].
    pub fn finish_darwin(&mut self) {
        self.used = self.active.saturating_add(self.wired).min(self.total);
        self.available = self.total.saturating_sub(self.used);

        // htop 3.3.0 on macOS sets only four values in
        // `Platform_setMemoryValues` (darwin/Platform.c:293): USED, BUFFERS
        // (purgeable), CACHE (inactive). COMPRESSED and AVAILABLE are explicitly
        // left commented out, so the bar has four segments and nothing more.
        // Reproducing that exactly is the point; see `finish_darwin_36` for the
        // newer formula.
        self.classes = vec![
            MemoryClass {
                label: "used",
                bytes: self.used,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 0,
            },
            MemoryClass {
                label: "buffers",
                bytes: self.purgeable,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 1,
            },
            MemoryClass {
                label: "cache",
                bytes: self.inactive,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 2,
            },
            MemoryClass {
                label: "free",
                bytes: self
                    .total
                    .saturating_sub(self.used)
                    .saturating_sub(self.purgeable)
                    .saturating_sub(self.inactive),
                counts_as_used: false,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 5,
            },
        ];
    }

    /// macOS accounting matching htop 3.6-dev: count the compressor as used.
    ///
    /// Kept as a named alternative rather than a config knob, because the default
    /// must match the htop a user is most likely to have installed (3.3.x).
    pub fn finish_darwin_36(&mut self) {
        self.used = self
            .active
            .saturating_add(self.wired)
            .saturating_add(self.compressed)
            .min(self.total);
        self.available = self.total.saturating_sub(self.used);

        self.classes = vec![
            MemoryClass {
                label: "active",
                bytes: self.active,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 1,
            },
            MemoryClass {
                label: "wired",
                bytes: self.wired,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 0,
            },
            MemoryClass {
                label: "compressed",
                bytes: self.compressed,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 2,
            },
            MemoryClass {
                label: "inactive",
                bytes: self.inactive,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 5,
            },
            MemoryClass {
                label: "purgeable",
                bytes: self.purgeable,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 4,
            },
            MemoryClass {
                label: "free",
                bytes: self.free,
                counts_as_used: false,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 5,
            },
        ];
    }

    /// Backwards-compatible alias used by the Linux parser.
    pub fn compute_used(&mut self) {
        self.finish_linux();
    }

    /// Whether every derived figure is within physical bounds.
    pub fn is_consistent(&self) -> bool {
        self.used <= self.total && self.available <= self.total
    }

    /// Stronger check for platforms that derive `available` from `used`.
    pub fn is_exactly_partitioned(&self) -> bool {
        self.used <= self.total && self.used + self.available == self.total
    }
}

/// Process-table counts, so meters can summarise processes without reaching into
/// the table themselves.
///
/// htop prints `Tasks: 472, 1664 thr, 1 running`; these are those numbers.
#[derive(Copy, Clone, Default, Debug)]
pub struct TaskStats {
    /// Rows in the process table.
    pub total: usize,
    /// Processes in state `R`.
    pub running: usize,
    /// Sum of `NLWP` across the table.
    pub threads: usize,
}

#[derive(Copy, Clone, Default, Debug)]
pub struct SwapInfo {
    pub total: u64,
    pub free: u64,
    pub used: u64,
}

/// Host-wide network byte counters, cumulative since boot.
///
/// Loopback is excluded by both backends: `lo` traffic never reaches a wire, and
/// counting it would both inflate the figure and double every byte (once as
/// received, once as transmitted). A container or a busy local database would
/// otherwise report hundreds of MB/s of "network" while the NIC is idle.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct NetCounters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

impl NetCounters {
    /// Counter delta against an earlier sample, saturating on reset.
    ///
    /// A counter can go *backwards*: an interface is destroyed and recreated, or
    /// the kernel wraps the counter on 32-bit `if_data`. Reporting a negative
    /// rate (or a 4 GiB spike) for one tick is worse than reporting zero, so a
    /// decreasing counter contributes nothing.
    pub fn delta(&self, prev: &NetCounters) -> NetCounters {
        NetCounters {
            rx_bytes: self.rx_bytes.saturating_sub(prev.rx_bytes),
            tx_bytes: self.tx_bytes.saturating_sub(prev.tx_bytes),
        }
    }

    /// Bytes per second between two samples `secs` apart.
    pub fn per_sec(&self, prev: &NetCounters, secs: f64) -> Rate {
        let d = self.delta(prev);
        Rate::from_bytes(d.rx_bytes, d.tx_bytes, secs)
    }
}

/// Host-wide disk byte counters, cumulative since boot.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct DiskCounters {
    pub read_bytes: u64,
    pub write_bytes: u64,
}

impl DiskCounters {
    pub fn delta(&self, prev: &DiskCounters) -> DiskCounters {
        DiskCounters {
            read_bytes: self.read_bytes.saturating_sub(prev.read_bytes),
            write_bytes: self.write_bytes.saturating_sub(prev.write_bytes),
        }
    }

    pub fn per_sec(&self, prev: &DiskCounters, secs: f64) -> Rate {
        let d = self.delta(prev);
        Rate::from_bytes(d.read_bytes, d.write_bytes, secs)
    }
}

/// A throughput in bytes per second, split into its two directions.
///
/// Kept as `f64` rather than `u64` because a 1.5 s interval divides a small
/// delta into a fraction; truncating there made a steady 3 B/s read show as
/// alternating 0 and 6.
#[derive(Copy, Clone, Default, Debug, PartialEq)]
pub struct Rate {
    /// Received (net) or read (disk).
    pub inbound: f64,
    /// Transmitted (net) or written (disk).
    pub outbound: f64,
}

impl Rate {
    fn from_bytes(inbound: u64, outbound: u64, secs: f64) -> Self {
        if secs <= 0.0 || secs.is_nan() {
            // First sample: there is no interval to divide by. Zero, not NaN, so
            // a meter never has to special-case its first frame.
            return Self::default();
        }
        Self {
            inbound: inbound as f64 / secs,
            outbound: outbound as f64 / secs,
        }
    }

    /// Combined throughput, for scaling a single bar or graph.
    pub fn total(&self) -> f64 {
        self.inbound + self.outbound
    }
}

/// One immutable sample handed from the sampler thread to the renderer.
#[derive(Clone, Debug)]
pub struct SystemSnapshot {
    pub at: Instant,
    /// Index 0 is the aggregate; indices 1..=n are per-core.
    pub cpu: Vec<CpuTimes>,
    /// Same shape as `cpu`, holding the delta against the previous sample.
    pub cpu_delta: Vec<CpuTimes>,
    pub mem: MemInfo,
    pub swap: SwapInfo,
    pub load: [f64; 3],
    pub uptime: u64,
    pub clk_tck: u64,
    pub page_size: u64,
    /// Cumulative host network counters, loopback excluded.
    pub net: NetCounters,
    /// Throughput against the previous sample. Zero on the first sample.
    pub net_rate: Rate,
    /// Cumulative host disk counters.
    pub disk: DiskCounters,
    /// Throughput against the previous sample. Zero on the first sample.
    pub disk_rate: Rate,
    /// Filled in by the sampler once the process table has been read.
    pub tasks: TaskStats,
}

impl Default for SystemSnapshot {
    fn default() -> Self {
        Self {
            at: Instant::now(),
            cpu: Vec::new(),
            cpu_delta: Vec::new(),
            mem: MemInfo::default(),
            swap: SwapInfo::default(),
            load: [0.0; 3],
            uptime: 0,
            clk_tck: 100,
            page_size: 4096,
            net: NetCounters::default(),
            net_rate: Rate::default(),
            disk: DiskCounters::default(),
            disk_rate: Rate::default(),
            tasks: TaskStats::default(),
        }
    }
}

impl SystemSnapshot {
    pub fn cpu_count(&self) -> usize {
        self.cpu.len().saturating_sub(1)
    }

    /// Aggregate CPU usage in `0.0..=1.0`, including all busy segments.
    pub fn cpu_usage(&self) -> f32 {
        let d = match self.cpu_delta.first() {
            Some(d) => d,
            None => return f32::NAN,
        };
        let total = d.total();
        if total == 0 {
            return 0.0;
        }
        d.busy() as f32 / total as f32
    }

    pub fn mem_usage(&self) -> f32 {
        if self.mem.total == 0 {
            return f32::NAN;
        }
        self.mem.used as f32 / self.mem.total as f32
    }

    pub fn swap_usage(&self) -> f32 {
        if self.swap.total == 0 {
            return 0.0;
        }
        self.swap.used as f32 / self.swap.total as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_is_tick_wise() {
        let a = CpuTimes {
            user: 10,
            idle: 90,
            ..Default::default()
        };
        let b = CpuTimes {
            user: 30,
            idle: 170,
            ..Default::default()
        };
        let d = b.delta(&a);
        assert_eq!(d.user, 20);
        assert_eq!(d.idle, 80);
        assert_eq!(d.total(), 100);
        assert_eq!(d.busy(), 20);
    }

    #[test]
    fn delta_never_wraps() {
        let a = CpuTimes {
            user: 30,
            ..Default::default()
        };
        let b = CpuTimes {
            user: 10,
            ..Default::default()
        };
        assert_eq!(b.delta(&a).user, 0);
    }

    #[test]
    fn linux_accounting_matches_free() {
        let mut mem = MemInfo {
            total: 1000,
            free: 100,
            buffers: 50,
            cached: 200,
            sreclaimable: 100,
            shmem: 20,
            available: 400,
            ..Default::default()
        };
        mem.finish_linux();
        // used = total - free - cached - sreclaimable - buffers
        assert_eq!(mem.used, 1000 - 100 - 200 - 100 - 50);
        // cached is reported net of shmem
        assert_eq!(mem.cached, 280);
        assert!(mem.is_consistent(), "{mem:?}");
    }

    #[test]
    fn linux_accounting_keeps_the_invariant_on_bad_input() {
        // A kernel that reports available > total must not break the invariant
        // nor overflow.
        let mut mem = MemInfo {
            total: 100,
            available: 500,
            free: 10,
            ..Default::default()
        };
        mem.finish_linux();
        assert!(mem.is_consistent(), "{mem:?}");
        assert!(mem.used <= mem.total);
        assert!(mem.available <= mem.total);
    }

    /// Real `vm_statistics64` numbers from an 8 GB Apple Silicon Mac, taken at
    /// the same moment htop 3.3.0 displayed 3.43 GB.
    ///
    /// htop 3.3.0 computes `used = active + wire` on macOS and leaves the
    /// compressor out entirely (`darwin/Platform.c:299`, with the COMPRESSED
    /// assignment commented out at :301). gander must produce the same number,
    /// otherwise the two cannot be compared side by side.
    #[test]
    fn darwin_used_matches_htop_330() {
        let mib = 1024 * 1024;
        let mut mem = MemInfo {
            total: 8 * 1024 * mib,
            free: 175 * mib,
            active: 1895 * mib,
            inactive: 1772 * mib,
            wired: 1607 * mib,
            compressed: 2182 * mib,
            purgeable: 31 * mib,
            speculative: 85 * mib,
            ..Default::default()
        };
        mem.finish_darwin();

        // 1895 + 1607 = 3502 MiB, matching htop 3.3.0.
        assert_eq!(mem.used, 3502 * mib);
        assert_eq!(mem.used_from_classes(), mem.used);
        assert!(mem.is_exactly_partitioned());

        // The compressor is deliberately *not* part of `used` here.
        assert!(mem.used < mem.active + mem.wired + mem.compressed);

        // htop 3.3.0 on macOS sets only USED / BUFFERS / CACHE, so the bar has
        // four segments and no compressed segment.
        let labels: Vec<&str> = mem.classes.iter().map(|c| c.label).collect();
        assert_eq!(labels, vec!["used", "buffers", "cache", "free"]);
    }

    #[test]
    fn htop_36_formula_is_available_separately() {
        let mib = 1024 * 1024;
        let mut mem = MemInfo {
            total: 8 * 1024 * mib,
            active: 1895 * mib,
            wired: 1607 * mib,
            compressed: 2182 * mib,
            inactive: 1772 * mib,
            purgeable: 31 * mib,
            ..Default::default()
        };
        mem.finish_darwin_36();
        assert_eq!(mem.used, (1895 + 1607 + 2182) * mib);
        assert!(mem.is_exactly_partitioned());
    }

    #[test]
    fn darwin_used_and_available_always_sum_to_total() {
        for (active, inactive, wired, compressed, purgeable) in [
            (0u64, 0u64, 0u64, 0u64, 0u64),
            (500, 100, 300, 200, 0),
            (1000, 0, 0, 0, 500),
            (10, 10, 10, 10, 100), // purgeable > active: must not underflow
            (1000, 1000, 1000, 1000, 0), // over-subscribed: must clamp to total
        ] {
            let mut mem = MemInfo {
                total: 1000,
                active,
                inactive,
                wired,
                compressed,
                purgeable,
                ..Default::default()
            };
            mem.finish_darwin();
            assert!(mem.is_exactly_partitioned(), "{mem:?}");
            assert!(mem.used <= mem.total, "{mem:?}");
        }
    }

    #[test]
    fn zero_delta_is_not_nan() {
        let snap = SystemSnapshot {
            cpu_delta: vec![CpuTimes::default()],
            ..Default::default()
        };
        assert_eq!(snap.cpu_usage(), 0.0);
    }
}
