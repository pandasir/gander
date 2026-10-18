//! gander-core — platform data collection and process model.
//!
//! This crate must not depend on any UI crate. That keeps the `/proc` parsers
//! testable against golden fixtures on any host, and makes a `--json` batch mode
//! (and future non-TUI front ends) nearly free.
//!
//! The Linux backend lives in [`linux`]. All of its parsers are pure functions
//! over `&[u8]`, so `cargo test` works on any platform, including macOS and the
//! BSDs, even though those targets have no `/proc`.
//!
//! See `docs/DESIGN.md`.

pub mod action;
pub mod format;
pub mod row;
pub mod system;
pub mod table;
pub mod tree;

/// The `/proc` backend. Its parsers are pure functions over `&[u8]`, and the IO
/// layer is parameterised by a root path, so the whole thing compiles and is
/// testable on any unix (macOS included) even though only Linux has a `/proc`.
#[cfg(unix)]
pub mod linux;

/// Process identifier.
pub type Pid = u32;

/// Whether a working backend exists for this build.
pub const PLATFORM_SUPPORTED: bool = cfg!(target_os = "linux");

/// Re-export of the platform selected at compile time.
#[cfg(unix)]
pub use linux::LinuxPlatform as CurrentPlatform;

/// Platform capabilities that vary per OS, resolved at compile time.
///
/// Front ends use this to hide panes and columns that cannot work here, rather
/// than discovering it at runtime. See `docs/DESIGN.md` milestone M4.
pub mod capability {
    /// CPU affinity (`sched_setaffinity`) can be adjusted.
    pub const AFFINITY: bool = cfg!(target_os = "linux");

    /// I/O priority (`ioprio_set`) can be adjusted.
    pub const IOPRIO: bool = cfg!(target_os = "linux");

    /// Per-process shared/private memory is available (`smaps_rollup`).
    pub const SMAPS: bool = cfg!(target_os = "linux");

    /// cgroup membership is readable (`/proc/<pid>/cgroup`).
    pub const CGROUP: bool = cfg!(target_os = "linux");

    /// Pressure stall information is readable (`/proc/pressure`).
    pub const PSI: bool = cfg!(target_os = "linux");
}
