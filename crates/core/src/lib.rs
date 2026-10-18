//! gander-core — platform data collection and process model.
//!
//! This crate must not depend on any UI crate. That keeps the parsers testable
//! against fixtures on any host, and makes a `--json` batch mode (and future
//! non-TUI front ends) nearly free.
//!
//! Two backends exist:
//!  * [`linux`] — reads `/proc` and `/sys`. Parametrised by a root path, so the
//!    entire read path is testable from fixtures on any unix.
//!  * [`darwin`] — uses `libproc`, `sysctl` and Mach host statistics.
//!
//! Both produce the same [`row::Row`] / [`system::SystemSnapshot`] types, so the
//! UI and configuration layers are platform-independent.
//!
//! See `docs/DESIGN.md` and `docs/PORTABILITY.md`.

pub mod action;
pub mod format;
pub mod row;
pub mod system;
pub mod table;
pub mod tree;

/// The `/proc` backend. Compiles on any unix because the IO layer is
/// parameterised by a root path; only Linux has a live `/proc` to point it at.
#[cfg(unix)]
pub mod linux;

/// The macOS backend: `libproc` + `sysctl` + Mach.
#[cfg(target_os = "macos")]
pub mod darwin;

/// Process identifier.
pub type Pid = u32;

/// A unified sampler interface so the front end does not name a backend.
///
/// `docs/DESIGN.md` §5: selection happens at compile time, so this trait exists
/// for genericity in tests and for documentation, not for runtime dispatch in
/// the hot loop.
pub trait PlatformSampler: Default {
    /// Take one sample: host counters plus one row per process.
    fn sample(&mut self) -> (system::SystemSnapshot, Vec<row::Row>);

    /// Number of cached per-process identity records, for tests and diagnostics.
    fn cache_len(&self) -> usize;

    /// A user-facing name for the backend.
    fn backend_name() -> &'static str;
}

/// Whether a live backend exists for this build.
pub const PLATFORM_SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "macos"));

/// The sampler for the current target.
#[cfg(target_os = "linux")]
pub type Sampler = linux::system::Sampler;

#[cfg(target_os = "macos")]
pub type Sampler = darwin::system::Sampler;

/// Re-export of the platform selected at compile time.
#[cfg(target_os = "linux")]
pub use linux::LinuxPlatform as CurrentPlatform;

#[cfg(target_os = "macos")]
pub use darwin::DarwinPlatform as CurrentPlatform;

/// Platform capabilities that vary per OS, resolved at compile time.
///
/// Front ends use this to hide panes and columns that cannot work here, rather
/// than discovering it at runtime. See `docs/PORTABILITY.md`.
pub mod capability {
    /// CPU affinity can be adjusted.
    pub const AFFINITY: bool = cfg!(any(target_os = "linux", target_os = "macos"));

    /// I/O priority can be adjusted (`ioprio_set`).
    pub const IOPRIO: bool = cfg!(target_os = "linux");

    /// Per-process shared/private memory is available (`smaps_rollup`).
    pub const SMAPS: bool = cfg!(target_os = "linux");

    /// cgroup membership is readable (`/proc/<pid>/cgroup`).
    pub const CGROUP: bool = cfg!(target_os = "linux");

    /// Pressure stall information is readable (`/proc/pressure`).
    pub const PSI: bool = cfg!(target_os = "linux");

    /// Per-process disk I/O counters are available.
    pub const PROC_IO: bool = cfg!(any(target_os = "linux", target_os = "macos"));

    /// Threads can be enumerated as rows.
    pub const THREADS: bool = cfg!(target_os = "linux");

    /// Sampling is backed by a live OS source (not just fixtures).
    pub const LIVE: bool = crate::PLATFORM_SUPPORTED;
}
