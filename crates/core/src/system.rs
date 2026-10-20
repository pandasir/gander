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

    // --- M3: zswap and huge pages -----------------------------------------
    /// Bytes currently held in the zswap pool (`Meminfo: Zswap`).
    ///
    /// Not part of the memory *bar*: zswap pages are anonymous pages that have
    /// been compressed, so they are already counted in `used`. Reporting them
    /// again would double-count. They get their own meter instead.
    pub zswap: u64,
    /// Bytes swapped out to zswap over the machine's lifetime (`Zswapped`).
    pub zswapped: u64,
    /// The zswap pool's ceiling, from
    /// `/sys/module/zswap/parameters/max_pool_percent` times physical memory.
    /// `0` when the parameter is unreadable.
    pub zswap_max: u64,
    /// Huge page counters, in *pages* (`Meminfo: HugePages_*`).
    pub huge_pages_total: u64,
    pub huge_pages_free: u64,
    pub huge_pages_rsvd: u64,
    pub huge_pages_surplus: u64,
    /// Bytes per huge page (`Hugepagesize`), `0` when huge pages are disabled.
    pub huge_page_size: u64,
    // --- FreeBSD -----------------------------------------------------------
    /// Pages that are dirty and queued for writeback (`v_laundry_count`).
    ///
    /// FreeBSD counts these separately from `active` and `inactive`, and they
    /// cannot be reused without a write first, so they belong in `used` rather
    /// than in cache.
    pub laundry: u64,
    /// The page cache (`v_cache_count`), in bytes rather than pages.
    ///
    /// Modern FreeBSD folds the unified buffer cache into the inactive queue and
    /// reports 0 here; it is kept because older kernels and `vfs.bufspace` still
    /// populate it, and silently dropping it would understate reclaimable memory
    /// on exactly those systems.
    pub file_cache: u64,
    /// `v_page_count`, the number of pages the VM subsystem manages.
    ///
    /// Used as the denominator for the partition check: `active + inactive +
    /// laundry + wired + free` must equal it, which is what makes the bar
    /// segments add up to physical memory.
    pub managed_pages: u64,

    /// `internal_page_count`: anonymous pages, the "by backing store" family.
    ///
    /// macOS reports memory twice over, once by usage (`active`, `inactive`, …)
    /// and once by backing store (`internal`, `external`). Only this one is
    /// needed for accounting, because `internal` is exactly
    /// `active + inactive + speculative - external` and therefore counts each
    /// anonymous page once instead of re-adding overlapping usage classes.
    pub internal: u64,
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

    /// macOS accounting, matching **htop 3.4 and later**.
    ///
    /// ```text
    /// used = (internal - purgeable) + wired + compressed
    /// ```
    ///
    /// htop 3.4.1 (`darwin/Platform.c`, `Platform_setMemoryValues`) says it the
    /// long way round:
    ///
    /// ```c
    /// natural_t used = vm->active_count + vm->inactive_count +
    ///                  vm->speculative_count + vm->wire_count +
    ///                  vm->compressor_page_count - vm->purgeable_count -
    ///                  vm->external_page_count;
    /// mtr->values[MEMORY_METER_USED]       = (double)(used - vm->compressor_page_count) * page_K;
    /// mtr->values[MEMORY_METER_COMPRESSED] = (double)vm->compressor_page_count * page_K;
    /// ```
    ///
    /// and `MemoryMeter_updateValues` prints `USED + COMPRESSED`. That sum is
    /// `internal + wired + compressed`, because `internal_page_count` *is*
    /// `active + inactive + speculative - external`. htop `main` (3.6-dev) states
    /// the same figure directly, as a class list marked `countsAsUsed`: `wired`,
    /// `internal - purgeable`, `compressed`. One formula therefore matches both,
    /// and it matches Activity Monitor and `top` as well.
    ///
    /// Consequences, all intentional:
    /// * The compressor **is** counted as used. On a Mac that has been running a
    ///   while it is the largest single term — 7 GB on a 16 GB machine is normal
    ///   — and leaving it out made gander report ~5 GB where htop reported ~13 GB.
    /// * `inactive` and `speculative` are **not** added on top. They are part of
    ///   `internal` (or are file cache), so adding them double-counts; that was
    ///   the other shipped mistake, and it is why the by-backing-store counter is
    ///   used here instead of the by-usage ones.
    /// * `free` is `vm_stat`'s own free count. Deriving it as `total - used -
    ///   cache` is what made the bar claim 7.8 GB free on a machine with 0.3 GB
    ///   free: the compressor pages that `used` omitted fell through into the
    ///   free segment.
    /// * `available = total - used`, so the two figures partition memory exactly,
    ///   and so do the bar segments.
    ///
    /// The old htop-3.3.0 formula (`used = active + wired`, compressor ignored)
    /// is gone: it was the reason the meter disagreed with every other tool by
    /// the size of the compressor.
    pub fn finish_darwin(&mut self) {
        // Clamp every class against what is left of physical memory, so a kernel
        // that reports overlapping or absurd counters cannot overflow the bar.
        let wired = self.wired.min(self.total);
        let compressed = self.compressed.min(self.total - wired);
        // Anonymous pages, minus the ones the OS may discard on demand.
        let internal = self.internal.min(self.total - wired - compressed);
        let purgeable = self.purgeable.min(internal);
        // htop's "active" is the anonymous figure net of purgeable pages; that is
        // exactly what `MEMORY_METER_USED` reports on 3.4, and what htop `main`
        // calls the `active` class.
        let active = internal - purgeable;

        self.used = wired + compressed + active;

        // Free comes from the kernel, not from arithmetic. Everything left over is
        // reclaimable: file-backed pages (`external`), `speculative`, and the page
        // categories neither counter describes.
        let free = self.free.min(self.total - self.used - purgeable);
        let cache = self.total - self.used - purgeable - free;
        self.available = self.total - self.used;

        // Order matches htop's `MEMORY_1..6`: hot classes first, then the
        // reclaimable ones, then free. The segments partition the bar — each page
        // is counted exactly once, so they add up to `total`.
        self.classes = vec![
            MemoryClass {
                label: "wired",
                bytes: wired,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 0,
            },
            MemoryClass {
                label: "active",
                bytes: active,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 1,
            },
            MemoryClass {
                label: "compressed",
                bytes: compressed,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 2,
            },
            MemoryClass {
                label: "purgeable",
                bytes: purgeable,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 4,
            },
            MemoryClass {
                label: "cache",
                bytes: cache,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 3,
            },
            MemoryClass {
                label: "free",
                bytes: free,
                counts_as_used: false,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 5,
            },
        ];
    }

    /// FreeBSD accounting.
    ///
    /// FreeBSD's VM keeps five mutually exclusive page queues, so the model is a
    /// partition rather than a formula:
    ///
    /// ```text
    /// used      = active + wired + laundry
    /// cache     = inactive + page cache
    /// available = free + cache
    /// ```
    ///
    /// Every page is in exactly one queue (`v_page_count` is their sum), which is
    /// why the bar segments can be checked against physical memory the way
    /// macOS's can. `laundry` counts as used because those pages cannot be
    /// reused until their write completes — the same reasoning that puts
    /// macOS's compressor in `used`.
    ///
    /// `free` is the kernel's own free count, never `total - used`: deriving it
    /// is what made the macOS bar claim 7.8 GB free on a machine with 0.3 GB.
    pub fn finish_freebsd(&mut self) {
        let wired = self.wired.min(self.total);
        let compressed = 0; // no compressor on FreeBSD
        let _ = compressed;
        let active = self.active.min(self.total - wired);
        let laundry = self.laundry.min(self.total - wired - active);
        let used = wired + active + laundry;

        let inactive = self.inactive.min(self.total - used);
        let file_cache = self.file_cache.min(self.total - used - inactive);
        let free = self.free.min(self.total - used - inactive - file_cache);

        self.used = used;
        self.cached = inactive + file_cache;
        self.available = (free + self.cached).min(self.total);

        self.classes = vec![
            MemoryClass {
                label: "wired",
                bytes: wired,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 0,
            },
            MemoryClass {
                label: "active",
                bytes: active,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 1,
            },
            MemoryClass {
                label: "laundry",
                bytes: laundry,
                counts_as_used: true,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 2,
            },
            MemoryClass {
                label: "inactive",
                bytes: inactive,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 3,
            },
            MemoryClass {
                label: "cache",
                bytes: file_cache,
                counts_as_used: false,
                counts_as_cache: true,
                counts_as_total: true,
                color_slot: 4,
            },
            MemoryClass {
                label: "free",
                bytes: free,
                counts_as_used: false,
                counts_as_cache: false,
                counts_as_total: true,
                color_slot: 5,
            },
        ];
    }

    /// Huge pages in use, in bytes.
    ///
    /// `total - free` rather than `total - free - rsvd`: reserved pages are
    /// already committed to a process that has not faulted them in yet, so a
    /// `free` figure that includes them overstates what is actually available.
    /// htop prints `used/total` with the same subtraction.
    pub fn huge_used_bytes(&self) -> u64 {
        let used_pages = self.huge_pages_total.saturating_sub(self.huge_pages_free);
        used_pages.saturating_mul(self.huge_page_size)
    }

    pub fn huge_total_bytes(&self) -> u64 {
        self.huge_pages_total.saturating_mul(self.huge_page_size)
    }

    /// Whether the kernel has huge pages configured at all.
    ///
    /// A machine with `HugePages_Total` of 0 has the feature compiled in but
    /// nothing reserved; a meter for it would be a permanent empty bar, so the
    /// meter says `n/a here` instead. Same reasoning as `DiskIO` on macOS.
    pub fn huge_pages_configured(&self) -> bool {
        self.huge_pages_total > 0 && self.huge_page_size > 0
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

/// One PSI resource's `some` and `full` averages.
///
/// `/proc/pressure/<resource>` reports two lines:
///
/// ```text
/// some avg10=0.00 avg60=0.00 avg300=0.00 total=0
/// full avg10=0.00 avg60=0.00 avg300=0.00 total=0
/// ```
///
/// `some` is the share of wall time during which *at least one* task was
/// stalled; `full` is the share during which *every* runnable task was stalled,
/// i.e. the machine genuinely had nothing to run.
///
/// **All four resources report both lines**, `cpu` included. An earlier revision
/// of this comment claimed `cpu` had no `full` line — that is wrong, and checking
/// a real `/proc/pressure/cpu` (Linux 6.x) shows both. The two lines mean slightly
/// different things for `cpu` (a task runnable-but-not-running is `some`; the
/// whole CPU idle while tasks wait is `full`), which is exactly why the kernel
/// reports both. Code here must not assume `has_full` is false for any resource.
#[derive(Copy, Clone, Default, Debug, PartialEq)]
pub struct PsiLine {
    /// Percentage of wall time stalled, averaged over 10 s.
    pub avg10: f32,
    /// Percentage of wall time stalled, averaged over 60 s.
    pub avg60: f32,
    /// Percentage of wall time stalled, averaged over 300 s.
    pub avg300: f32,
    /// Total microseconds stalled since boot. Cumulative, so it can be used for
    /// a rate the same way the network counters are.
    pub total_us: u64,
}

/// Pressure stall information for one resource.
#[derive(Copy, Clone, Default, Debug, PartialEq)]
pub struct PsiResource {
    /// Readable at all. `false` means the file was missing.
    pub present: bool,
    /// The `some` line.
    pub some: PsiLine,
    /// The `full` line. Left at zero when the file has none.
    pub full: PsiLine,
    /// Whether this resource's file carried a `full` line.
    ///
    /// True for every resource on current kernels, `cpu` included. Kept because
    /// it is the file that decides, not us: a future kernel is free to drop a
    /// line, and a meter that assumed one was present would draw a fabricated
    /// zero instead of leaving the value out.
    pub has_full: bool,
}

/// Host pressure stall information.
///
/// # Why this exists on top of CPU% and load average
///
/// Neither of those answers "is the machine actually struggling". A box at 90 %
/// CPU with nothing waiting is busy but healthy; a box at 30 % CPU with an `io`
/// `full` average of 40 % is making every process wait on the disk, and both
/// CPU% and load average look unremarkable while it happens. PSI is the kernel's
/// own answer to "how much time did work actually lose", and it is the one
/// number that distinguishes the two cases.
#[derive(Copy, Clone, Default, Debug, PartialEq)]
pub struct Psi {
    pub cpu: PsiResource,
    pub memory: PsiResource,
    pub io: PsiResource,
    pub irq: PsiResource,
}

impl Psi {
    /// Whether any resource was readable. A `Psi` where nothing is present is
    /// worse than `None`: it draws three empty bars that look like an idle
    /// machine instead of an absent feature.
    pub fn any_present(&self) -> bool {
        self.cpu.present || self.memory.present || self.io.present || self.irq.present
    }

    /// The resources in display order, with their labels.
    pub fn rows(&self) -> [(&'static str, PsiResource); 4] {
        [
            ("CPU", self.cpu),
            ("MEM", self.memory),
            ("IO", self.io),
            ("IRQ", self.irq),
        ]
    }
}

/// One compressed RAM block device (`/sys/block/zram<N>`).
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct ZramDevice {
    pub name: String,
    /// Uncompressed size of the data held, in bytes.
    pub orig_data_size: u64,
    /// Size actually occupied in RAM after compression, in bytes.
    pub compr_data_size: u64,
    /// Bytes of RAM the device has taken from the allocator.
    pub mem_used_total: u64,
}

impl ZramDevice {
    /// Compression ratio, `orig / compressed`. `1.0` when nothing is stored, so
    /// a caller never divides by zero and an empty device does not claim an
    /// infinite ratio.
    pub fn ratio(&self) -> f32 {
        if self.compr_data_size == 0 {
            1.0
        } else {
            self.orig_data_size as f32 / self.compr_data_size as f32
        }
    }
}

/// Every zram device on the machine, and their totals.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct ZramInfo {
    pub devices: Vec<ZramDevice>,
}

impl ZramInfo {
    pub fn present(&self) -> bool {
        !self.devices.is_empty()
    }

    pub fn orig_total(&self) -> u64 {
        self.devices.iter().map(|d| d.orig_data_size).sum()
    }

    pub fn compr_total(&self) -> u64 {
        self.devices.iter().map(|d| d.compr_data_size).sum()
    }

    pub fn mem_total(&self) -> u64 {
        self.devices.iter().map(|d| d.mem_used_total).sum()
    }

    /// Aggregate ratio across all devices. Computed from the totals rather than
    /// averaged over the devices, so a 1 MB and a 1 GB device do not count
    /// equally.
    pub fn ratio(&self) -> f32 {
        let compr = self.compr_total();
        if compr == 0 {
            1.0
        } else {
            self.orig_total() as f32 / compr as f32
        }
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
    /// Pressure stall information. `None` where the kernel does not provide it
    /// (macOS, or a Linux kernel older than 4.20).
    pub psi: Option<Psi>,
    /// Compressed RAM block devices. Empty when the machine has none.
    pub zram: ZramInfo,
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
            psi: None,
            zram: ZramInfo::default(),
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

    /// Real `vm_statistics64` numbers from a 16 GB Apple Silicon Mac (page size
    /// 16384) at the moment htop 3.4.1 displayed `13.3G/16.0G`, while gander
    /// still showed `5.3G/16.0G`.
    ///
    /// The gap was exactly the compressor: htop 3.4.1 prints
    /// `USED + COMPRESSED` (`MemoryMeter_updateValues`), which simplifies to
    /// `internal + wired + compressed`. The old `used = active + wired` formula
    /// left out both the compressor and `internal - active`, i.e. most of what
    /// the machine was actually using.
    #[test]
    fn darwin_used_matches_htop_341() {
        const PAGE: u64 = 16 * 1024;
        let mut mem = MemInfo {
            total: 16 * 1024 * 1024 * 1024,
            // vm_stat, verbatim.
            internal: 301_497 * PAGE,
            active: 219_954 * PAGE,
            inactive: 216_885 * PAGE,
            speculative: 2_022 * PAGE,
            wired: 136_897 * PAGE,
            compressed: 429_207 * PAGE,
            purgeable: 0,
            free: 4_007 * PAGE,
            ..Default::default()
        };
        mem.finish_darwin();

        // htop's two printed terms: its USED value, which excludes the
        // compressor, plus its COMPRESSED value.
        let htop_used_excl_compressor = 301_497 * PAGE + 136_897 * PAGE;
        let htop_compressed = 429_207 * PAGE;
        assert_eq!(
            mem.used,
            htop_used_excl_compressor + htop_compressed,
            "gander's used must equal htop's USED + COMPRESSED"
        );
        assert_eq!(mem.used_from_classes(), mem.used);
        assert!(mem.is_exactly_partitioned());

        // The compressor is no longer the missing 7 GB.
        assert_eq!(
            mem.used - (mem.internal - mem.purgeable) - mem.wired,
            htop_compressed,
            "the remainder after active+wired is the compressor"
        );

        // htop 3.4 also counts inactive/speculative as cache, not as used. They
        // must not be added on top here, or used would be ~3.4 GB too high.
        assert!(mem.used < mem.total);

        let labels: Vec<&str> = mem.classes.iter().map(|c| c.label).collect();
        assert_eq!(
            labels,
            vec![
                "wired",
                "active",
                "compressed",
                "purgeable",
                "cache",
                "free"
            ]
        );
    }

    /// The bar must not claim free memory the compressor is holding.
    ///
    /// This is the second half of the same bug: the old formula derived the
    /// `free` segment as `total - used - purgeable - inactive`, so the ~7 GB the
    /// compressor occupied landed in the free segment. A machine with 0.3 GB
    /// free drew a bar that was nearly half empty.
    #[test]
    fn darwin_free_segment_is_not_the_compressor() {
        const PAGE: u64 = 16 * 1024;
        let mut mem = MemInfo {
            total: 16 * 1024 * 1024 * 1024,
            internal: 301_497 * PAGE,
            wired: 136_897 * PAGE,
            compressed: 429_207 * PAGE,
            purgeable: 0,
            free: 4_007 * PAGE,
            ..Default::default()
        };
        mem.finish_darwin();

        let free_class = mem
            .classes
            .iter()
            .find(|c| c.label == "free")
            .expect("a free class is always present");
        // At most ~130 MiB of free memory exists on this sample; anything in the
        // gigabytes means the compressor fell through into the wrong segment.
        assert!(
            free_class.bytes < 1024 * 1024 * 1024,
            "free segment is {} MiB, which is the compressor leaking in",
            free_class.bytes / (1024 * 1024)
        );
    }

    #[test]
    fn darwin_bar_segments_partition_total() {
        // Every class flagged `counts_as_total` is a distinct set of pages, so the
        // segments must add up to physical memory exactly -- after clamping, on
        // absurd input included.
        for (internal, wired, compressed, purgeable) in [
            (0u64, 0u64, 0u64, 0u64),
            (500, 300, 200, 0),
            (500, 300, 200, 700),  // purgeable > internal: must not underflow
            (400, 300, 300, 0),    // sums to total already
            (4000, 3000, 3000, 0), // over-subscribed: must clamp
        ] {
            let mut mem = MemInfo {
                total: 1000,
                internal,
                wired,
                compressed,
                purgeable,
                ..Default::default()
            };
            mem.finish_darwin();
            let classes: u64 = mem
                .classes
                .iter()
                .filter(|c| c.counts_as_total)
                .map(|c| c.bytes)
                .sum();
            assert_eq!(classes, mem.total, "{mem:?}");
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
