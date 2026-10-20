//! The FreeBSD backend.
//!
//! FreeBSD has no `/proc` by default (`procfs(5)` is deprecated and often not
//! mounted), so the data comes from `sysctl` and `libkvm`, which is the same pair
//! htop uses in `freebsd/`:
//!
//! | Need | Source |
//! |---|---|
//! | Process list | `sysctl(KERN_PROC_ALL)` → `kinfo_proc[]` |
//! | Per-process CPU, memory, faults | the same `kinfo_proc` |
//! | Command line | `sysctl(KERN_PROC_ARGS)` |
//! | Executable path | `sysctl(KERN_PROC_PATHNAME)` |
//! | Working directory | `sysctl(KERN_PROC_CWD)` |
//! | Threads | `kvm_getprocs(KERN_PROC_INC_THREAD)` |
//! | Per-core CPU ticks | `sysctl(KERN_CPTIME)` / `kvm_getpcpu` |
//! | Memory pages | `sysctl(VM_TOTAL)` → `struct vmtotal` |
//! | Swap | `kvm_getswapinfo(3)` |
//! | Load average | `getloadavg(3)` |
//! | Boot time | `sysctl(KERN_BOOTTIME)` |
//! | Network | `sysctl(NET_RT_IFLIST)` → `if_msghdr` |
//! | Disk | `devstat_getdevs(3)` |
//!
//! # Why `kinfo_proc` is used directly here
//!
//! The macOS backend cannot use `kinfo_proc` because `libc` does not expose
//! Darwin's version, so it reads fixed offsets out of raw bytes. `libc` *does*
//! declare FreeBSD's `kinfo_proc`, and the layout is stable within an ABI
//! version, so this backend uses the struct and lets the compiler do the offsets.
//! `kinfo::validate()` still checks the result against `sysctl` at runtime.

pub mod parse;
pub mod scan;
pub mod system;

/// Conversions and constants that are easy to get wrong, in one place.
pub mod units {
    /// FreeBSD's `stathz`/`hz` for CPU time.
    ///
    /// `kinfo_proc.ki_runtime` is **microseconds**, not ticks, so nothing here
    /// scales it; the constant exists because the `TIME` column wants hundredths
    /// of a second and `100` is the divisor for both `ki_runtime` and the
    /// per-thread `ru_utime`.
    pub const USEC_PER_CENTISEC: u64 = 10_000;
}

pub struct FreeBsdPlatform;

impl FreeBsdPlatform {
    /// Clock ticks per second for the `TIME` column.
    ///
    /// Always 100: `ki_runtime` is microseconds, converted to hundredths of a
    /// second (`units::USEC_PER_CENTISEC`). Linux reads the real `_SC_CLK_TCK`.
    pub fn clk_tck() -> u64 {
        100
    }

    /// Logical CPU count, from `hw.ncpu`.
    pub fn cpu_count() -> usize {
        scan::cpu_count()
    }

    /// Memory page size (`getpagesize(3)`).
    pub fn page_size() -> u64 {
        // SAFETY: `getpagesize` takes no arguments and cannot fail.
        let v = unsafe { libc::getpagesize() };
        if v > 0 {
            v as u64
        } else {
            4096
        }
    }
}
