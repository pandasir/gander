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

#[derive(Copy, Clone, Default, Debug)]
pub struct MemInfo {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub buffers: u64,
    pub cached: u64,
    pub sreclaimable: u64,
    pub shmem: u64,
    pub used: u64,
}

impl MemInfo {
    /// Memory actually in use, using htop's definition.
    ///
    /// `used = total - available - (sreclaimable - shmem)`
    ///
    /// `MemFree` is the wrong number here; see `docs/DESIGN.md` §3.5.
    pub fn compute_used(&mut self) {
        let reclaimable = self.sreclaimable.saturating_sub(self.shmem);
        self.used = self
            .total
            .saturating_sub(self.available)
            .saturating_sub(reclaimable);
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
    fn used_excludes_reclaimable() {
        let mut mem = MemInfo {
            total: 1000,
            available: 400,
            sreclaimable: 100,
            shmem: 20,
            ..Default::default()
        };
        mem.compute_used();
        // 1000 - 400 - (100 - 20) = 520
        assert_eq!(mem.used, 520);
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
