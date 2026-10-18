//! The macOS backend.
//!
//! Darwin has no `/proc`. The data comes from three places, mirroring what htop
//! does in `darwin/`:
//!
//! | Need | Source |
//! |---|---|
//! | Process list | `sysctl(KERN_PROC_ALL)` → `kinfo_proc[]` |
//! | Per-process CPU, memory, threads | `proc_pidinfo(PROC_PIDTASKINFO)` |
//! | uid, ppid, state, start time | `kinfo_proc` |
//! | Executable path | `proc_pidpath` |
//! | Command line | `sysctl(KERN_PROCARGS2)` |
//! | Disk I/O | `proc_pid_rusage(RUSAGE_INFO_V2)` |
//! | Per-core CPU ticks | `host_processor_info(PROCESSOR_CPU_LOAD_INFO)` |
//! | Memory pages | `host_statistics64(HOST_VM_INFO64)` |
//! | Swap | `sysctl(vm.swapusage)` |
//! | Load average | `getloadavg(3)` |
//! | Boot time | `sysctl(kern.boottime)` |
//!
// `libc` marks its Mach bindings as deprecated in favour of the `mach2` crate.
// They are correct and stable; four functions do not justify a dependency, so the
// warnings are silenced here once, with intent.
#![allow(deprecated)]

//! CPU% needs two samples, and the denominator is wall-clock time, not tick
//! deltas: `proc_taskinfo` reports nanoseconds. htop does the same:
//! `darwin/DarwinProcess.c` divides the nanosecond delta by the sampling
//! interval.

pub mod parse;
pub mod scan;
pub mod system;

/// Conversions that are easy to get wrong, in one place.
pub mod units {
    /// `mach_timebase_info`-derived scale factor, cached after the first call.
    ///
    /// On Intel this is 1/1; on Apple Silicon it is 125/3 — hardcoding either is
    /// a bug that silently makes CPU% wrong by 40x on the other architecture.
    pub fn mach_ticks_to_nanos(ticks: u64) -> u64 {
        use std::sync::OnceLock;
        static SCALE: OnceLock<(u64, u64)> = OnceLock::new();
        let (numer, denom) = *SCALE.get_or_init(|| {
            let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
            // SAFETY: `mach_timebase_info` fills the struct; it has no
            // preconditions and cannot fail in practice.
            let rc = unsafe { libc::mach_timebase_info(&mut info) };
            if rc == 0 && info.numer != 0 && info.denom != 0 {
                (u64::from(info.numer), u64::from(info.denom))
            } else {
                (1, 1)
            }
        });
        if denom == 0 {
            return ticks;
        }
        // Divide first to avoid overflow on long-running processes, then add the
        // remainder's contribution back.
        ticks / denom * numer + (ticks % denom) * numer / denom
    }
}

pub struct DarwinPlatform;

impl DarwinPlatform {
    /// Nominal clock-tick rate used by the TIME column.
    ///
    /// Mach time is converted to nanoseconds, then to hundredths of a second, so
    /// the effective rate is always 100. Linux reads the real `_SC_CLK_TCK`.
    pub fn clk_tck() -> u64 {
        100
    }

    /// Logical CPU count.
    pub fn cpu_count() -> usize {
        crate::darwin::scan::cpu_count()
    }

    /// Nominal memory page size.
    pub fn page_size() -> u64 {
        // SAFETY: `vm_page_size` is a plain static filled in by dyld.
        let v = unsafe { libc::vm_page_size };
        if v > 0 {
            v as u64
        } else {
            16_384
        }
    }
}
