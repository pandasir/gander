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
        let cached = self.cached + self.sreclaimable - self.shmem;
        let used_diff = self.free + self.cached + self.sreclaimable + self.buffers;
        self.used = if self.total >= used_diff {
            self.total - used_diff
        } else {
            self.total.saturating_sub(self.free)
        };
        self.cached = cached;
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

    /// macOS accounting, following htop's `darwin/Platform.c`.
    ///
    /// `used = wired + active + compressed`
    ///
    /// * `wired` and `compressed` are disjoint from `active`, so all three add.
    ///   htop 3.3.0 omitted `compressed`; on a machine with 2 GB in the
    ///   compressor that understates usage by 2 GB, and upstream fixed it.
    /// * `inactive` is **cache**, not used: the page counters partition physical
    ///   memory, so counting `inactive` as used both overshoots the total and
    ///   contradicts its own role as reclaimable.
    /// * `available = total - used`, so the two figures partition memory
    ///   exactly, which is a useful invariant to test.
    pub fn finish_darwin(&mut self) {
        self.used = self
            .wired
            .saturating_add(self.active)
            .saturating_add(self.compressed)
            .min(self.total);
        self.available = self.total.saturating_sub(self.used);

        // Order matches htop's bar: used classes first, then cache, then free.
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

#[derive(Copy, Clone, Default, Debug)]
pub struct SwapInfo {
    pub total: u64,
    pub free: u64,
    pub used: u64,
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
    /// the same moment htop 3.3.0 displayed 3.45 GB and htop 3.6-dev 5.64 GB.
    ///
    /// We follow 3.6: `wired + active + compressed`. 3.3.0 omitted the
    /// compressor, which is 2 GB on this machine.
    #[test]
    fn darwin_used_matches_htop_36() {
        let mib = 1024 * 1024;
        let mut mem = MemInfo {
            total: 8 * 1024 * mib,
            free: 225 * mib,
            active: 1710 * mib,
            inactive: 1628 * mib,
            wired: 1823 * mib,
            compressed: 2243 * mib,
            purgeable: 133 * mib,
            speculative: 85 * mib,
            ..Default::default()
        };
        mem.finish_darwin();

        // 1710 + 1823 + 2243 = 5776 MiB, which is what htop 3.6 reports.
        assert_eq!(mem.used, 5776 * mib);
        assert_eq!(mem.used_from_classes(), mem.used);
        assert!(mem.is_exactly_partitioned());

        // htop 3.3.0's formula, for contrast: it drops the 2243 MiB compressor.
        let htop_330 = (mem.active + mem.wired) / mib;
        assert_eq!(htop_330, 3533);
        assert!(
            htop_330 + 2000 < mem.used / mib,
            "htop 3.3 understates by the compressor size, which is why we do not copy it"
        );
        assert!(
            mem.is_exactly_partitioned(),
            "used {} + available {} != total {}",
            mem.used,
            mem.available,
            mem.total
        );
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
