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
pub mod affinity;
pub mod format;
pub mod row;
pub mod system;
pub mod table;
pub mod tree;
pub mod users;

/// The `/proc` backend. Compiles on any unix because the IO layer is
/// parameterised by a root path; only Linux has a live `/proc` to point it at.
#[cfg(unix)]
pub mod linux;

/// The macOS backend: `libproc` + `sysctl` + Mach.
#[cfg(target_os = "macos")]
pub mod darwin;

/// The FreeBSD backend: `sysctl` + `libkvm` + `devstat`.
#[cfg(target_os = "freebsd")]
pub mod freebsd;

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

    /// Tell the sampler what the front end currently needs.
    ///
    /// Call this before every `sample()`. The default implementation ignores it,
    /// which is the right behaviour for a backend with no expensive reads.
    fn set_scan_options(&mut self, _opts: ScanOptions) {}

    /// Whether this backend can actually enumerate threads.
    fn supports_threads() -> bool {
        false
    }
}

/// What the front end is currently showing, and therefore what the sampler has to
/// pay for.
///
/// # Why this is one struct rather than three setters
///
/// The per-process reads named here are not free:
///
/// * `threads` — an extra `stat` read per thread (~1700 per tick on a desktop).
/// * `smaps` — a kernel walk over every VMA of every process (tens of ms).
/// * `gpu` — a `readdir` of `fdinfo` plus a read per open DRM fd, per process.
///
/// Each one used to be its own `set_*` method on the trait. That worked until the
/// third, at which point a front end had to remember to call every setter on every
/// change, and a forgotten one meant either a wasted scan or a column of dashes.
/// Passing the whole decision as one value makes both failures impossible: there
/// is one call, it carries everything, and a new gated read can only be added by
/// extending this struct — which breaks every call site until it is answered.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanOptions {
    /// Emit one row per thread as well as one per process.
    pub threads: bool,
    /// Read `/proc/<pid>/smaps_rollup` (PSS, EPSS, private, per-process swap).
    pub smaps: bool,
    /// Read `/proc/<pid>/fdinfo` for `drm-*` keys (GPU time and percentage).
    pub gpu: bool,
    /// Read per-task delay counters over `TASKSTATS` generic netlink.
    pub delayacct: bool,
}

impl ScanOptions {
    /// Everything off: the cheap scan, which is what the shipped default column
    /// set and the default header need.
    pub const fn none() -> Self {
        Self {
            threads: false,
            smaps: false,
            gpu: false,
            delayacct: false,
        }
    }

    pub const fn with_threads(mut self, on: bool) -> Self {
        self.threads = on;
        self
    }

    pub const fn with_smaps(mut self, on: bool) -> Self {
        self.smaps = on;
        self
    }

    pub const fn with_gpu(mut self, on: bool) -> Self {
        self.gpu = on;
        self
    }

    pub const fn with_delayacct(mut self, on: bool) -> Self {
        self.delayacct = on;
        self
    }
}

/// Whether a live backend exists for this build.
///
/// Three backends: Linux reads `/proc`, macOS uses `libproc` and Mach, FreeBSD
/// uses `sysctl` and `libkvm`. Everything else compiles but has no sampler, and
/// the front ends refuse to start rather than showing an empty table.
pub const PLATFORM_SUPPORTED: bool = cfg!(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd"
));

/// The sampler for the current target.
#[cfg(target_os = "linux")]
pub type Sampler = linux::system::Sampler;

#[cfg(target_os = "macos")]
pub type Sampler = darwin::system::Sampler;

#[cfg(target_os = "freebsd")]
pub type Sampler = freebsd::system::Sampler;

/// Re-export of the platform selected at compile time.
#[cfg(target_os = "linux")]
pub use linux::LinuxPlatform as CurrentPlatform;

#[cfg(target_os = "macos")]
pub use darwin::DarwinPlatform as CurrentPlatform;

#[cfg(target_os = "freebsd")]
pub use freebsd::FreeBsdPlatform as CurrentPlatform;

/// Platform capabilities that vary per OS, resolved at compile time.
///
/// Front ends use this to hide panes and columns that cannot work here, rather
/// than discovering it at runtime. See `docs/PORTABILITY.md`.
///
/// **A constant here must be read by something.** This module had ten and four of
/// them were referenced by nothing but their own doc comments — among them
/// `PROC_IO`, which claimed per-process I/O counters were available while no such
/// column existed. A capability nothing consults is worse than no capability: it
/// reads like an implemented feature and answers a question nobody asked. They
/// were removed rather than kept "for later"; adding one back is two lines, and
/// the second line is the call site.
///
/// Where the answer is needed by only one module, ask that module instead of
/// growing this list — `affinity::supported()` returns whether a mask can actually
/// be read and written, which is a stricter question than "does the panel open".
pub mod capability {
    /// The `a` affinity panel has something to say on this platform.
    ///
    /// True on macOS as well, where there is no per-process mask: the panel still
    /// opens, and explains that the OS offers only a scheduler hint rather than
    /// silently doing nothing. Whether a mask can actually be **read and
    /// written** is a stricter question, and it is
    /// [`affinity::supported()`](crate::affinity::supported) that answers it.
    pub const AFFINITY: bool = cfg!(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd"
    ));

    /// cgroup membership is readable (`/proc/<pid>/cgroup`).
    ///
    /// Gates the `CGROUP`/`CONTAINER` columns in the setup screen and the `O`
    /// key, so a platform without cgroups offers neither rather than showing a
    /// column of dashes.
    pub const CGROUP: bool = cfg!(target_os = "linux");

    /// Host-wide disk byte counters are available (`/proc/diskstats`,
    /// `devstat_getdevs`).
    ///
    /// macOS is the exception: it exposes per-process `rusage` figures but no
    /// host-wide byte counter that avoids IOKit, and the design forbids a vendor
    /// SDK. The `DiskIO` meter says so there instead of drawing a permanent zero.
    pub const HOST_DISK_IO: bool =
        cfg!(any(target_os = "linux", target_os = "freebsd"));
}
