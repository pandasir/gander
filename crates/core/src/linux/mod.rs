//! Linux `/proc` and `/sys` access.
//!
//! Split into two layers:
//!  * `parse` — pure functions over `&[u8]`, unit-testable against fixtures
//!  * `scan`  — the IO layer, using thread-local scratch buffers
//!
//! Both are configured by a `root` path so tests can point at a fixture tree
//! instead of the live `/proc`.

pub mod delayacct;
pub mod fsops;
pub mod parse;
pub mod scan;
pub mod system;

pub struct LinuxPlatform;

impl LinuxPlatform {
    /// Clock ticks per second (`_SC_CLK_TCK`). Never hardcode 100.
    pub fn clk_tck() -> u64 {
        // SAFETY: sysconf is thread-safe and has no preconditions.
        let v = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        if v > 0 {
            v as u64
        } else {
            100
        }
    }

    /// Memory page size (`_SC_PAGESIZE`).
    pub fn page_size() -> u64 {
        // SAFETY: as above.
        let v = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if v > 0 {
            v as u64
        } else {
            4096
        }
    }
}

/// Number of online CPUs, used to normalize CPU%.
pub fn cpu_count() -> usize {
    // SAFETY: `sysconf(_SC_NPROCESSORS_ONLN)` has no preconditions.
    let v = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if v > 0 {
        v as usize
    } else {
        1
    }
}
