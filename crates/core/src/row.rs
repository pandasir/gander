//! Field identifiers and their presentation metadata.

use crate::Pid;

/// A displayable per-process column.
///
/// Variants mirror htop's `RowField.h` / `<platform>/ProcessField.h` semantics.
/// The numeric IDs deliberately do *not* match htop: only the `htoprc` importer
/// needs name-level compatibility.
///
/// Render with `match`, never `dyn` dispatch — thousands of rows times dozens of
/// columns per frame makes dynamic dispatch measurable.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Field {
    Pid,
    Comm,
    State,
    Ppid,
    Pgrp,
    Session,
    Tty,
    Tpgid,
    MinFlt,
    MajFlt,
    Priority,
    Nice,
    StartTime,
    Processor,
    VirtMem,
    ResMem,
    User,
    PercentCpu,
    PercentMem,
    Time,
    Nlwp,
    Tgid,
    PercentNormCpu,
    Elapsed,
    SchedPolicy,
    ProcComm,
    ProcExe,
    Cwd,

    // Linux-only fields (see docs/DESIGN.md, milestone M3).
    #[cfg(target_os = "linux")]
    Pss,
    #[cfg(target_os = "linux")]
    Swap,
    #[cfg(target_os = "linux")]
    Private,
    #[cfg(target_os = "linux")]
    Epss,
    #[cfg(target_os = "linux")]
    Cgroup,
    #[cfg(target_os = "linux")]
    Container,
    #[cfg(target_os = "linux")]
    GpuPercent,
    #[cfg(target_os = "linux")]
    GpuTime,
    #[cfg(target_os = "linux")]
    IoReadRate,
    #[cfg(target_os = "linux")]
    IoWriteRate,
}

/// Horizontal alignment of a column.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Right,
}

/// Static description of a field, used by the setup screen and by the renderer.
pub struct FieldMeta {
    pub field: Field,
    /// Stable identifier used in config files (`"M_PSS"`).
    pub name: &'static str,
    /// Header cell text (`" PSS "`).
    pub title: &'static str,
    /// Human description shown in the setup screen.
    pub description: &'static str,
    /// Whether this column sorts descending by default.
    pub default_sort_desc: bool,
    pub align: Align,
}

impl Field {
    pub fn meta(self) -> Option<&'static FieldMeta> {
        FIELD_META.iter().find(|m| m.field == self)
    }

    /// Stable config name, e.g. `PercentCpu` -> `"PERCENT_CPU"`.
    pub fn name(self) -> &'static str {
        self.meta().map_or("UNKNOWN", |m| m.name)
    }

    /// Header text, e.g. `PercentCpu` -> `" CPU% "`.
    pub fn title(self) -> &'static str {
        self.meta().map_or(" ? ", |m| m.title)
    }

    pub fn default_sort_desc(self) -> bool {
        self.meta().is_some_and(|m| m.default_sort_desc)
    }

    /// All fields available on this platform, in setup-screen order.
    pub fn all() -> impl Iterator<Item = Field> {
        FIELD_META.iter().map(|m| m.field)
    }

    pub fn from_name(name: &str) -> Option<Field> {
        FIELD_META.iter().find(|m| m.name == name).map(|m| m.field)
    }
}

const fn m(
    field: Field,
    name: &'static str,
    title: &'static str,
    description: &'static str,
    default_sort_desc: bool,
    align: Align,
) -> FieldMeta {
    FieldMeta {
        field,
        name,
        title,
        description,
        default_sort_desc,
        align,
    }
}

const FIELD_META: &[FieldMeta] = &[
    m(
        Field::Pid,
        "PID",
        "   PID ",
        "Process ID",
        false,
        Align::Right,
    ),
    m(
        Field::Comm,
        "COMM",
        " COMMAND ",
        "Command name from /proc/<pid>/comm",
        false,
        Align::Left,
    ),
    m(
        Field::State,
        "STATE",
        " S ",
        "Process state",
        false,
        Align::Left,
    ),
    m(
        Field::Ppid,
        "PPID",
        "  PPID ",
        "Parent process ID",
        false,
        Align::Right,
    ),
    m(
        Field::Pgrp,
        "PGRP",
        "  PGRP ",
        "Process group ID",
        false,
        Align::Right,
    ),
    m(
        Field::User,
        "USER",
        " USER ",
        "Effective user",
        false,
        Align::Left,
    ),
    m(
        Field::Priority,
        "PRIORITY",
        " PRI ",
        "Kernel scheduling priority",
        false,
        Align::Right,
    ),
    m(
        Field::Nice,
        "NICE",
        " NI ",
        "Nice value",
        false,
        Align::Right,
    ),
    m(
        Field::PercentCpu,
        "PERCENT_CPU",
        " CPU% ",
        "CPU usage in percent",
        true,
        Align::Right,
    ),
    m(
        Field::PercentMem,
        "PERCENT_MEM",
        " MEM% ",
        "Memory usage in percent",
        true,
        Align::Right,
    ),
    m(
        Field::PercentNormCpu,
        "PERCENT_NORM_CPU",
        " NCPU% ",
        "CPU usage normalized by core count",
        true,
        Align::Right,
    ),
    m(
        Field::Time,
        "TIME",
        "  TIME+ ",
        "Cumulative CPU time",
        true,
        Align::Right,
    ),
    m(
        Field::Elapsed,
        "ELAPSED",
        " ELAPSED ",
        "Time since the process started",
        false,
        Align::Right,
    ),
    m(
        Field::ResMem,
        "M_RESIDENT",
        "  RES ",
        "Resident set size",
        true,
        Align::Right,
    ),
    m(
        Field::VirtMem,
        "M_VIRT",
        "  VIRT ",
        "Virtual memory size",
        true,
        Align::Right,
    ),
    m(
        Field::Nlwp,
        "NLWP",
        " NLWP ",
        "Number of threads",
        false,
        Align::Right,
    ),
    m(
        Field::Processor,
        "PROCESSOR",
        " CPU ",
        "Last used CPU",
        false,
        Align::Right,
    ),
    m(
        Field::MinFlt,
        "MINFLT",
        " MINFLT ",
        "Minor page faults",
        false,
        Align::Right,
    ),
    m(
        Field::MajFlt,
        "MAJFLT",
        " MAJFLT ",
        "Major page faults",
        false,
        Align::Right,
    ),
];

/// Names of the fields shown in the default M0 layout.
pub const DEFAULT_COLUMNS: &[Field] = &[
    Field::Pid,
    Field::User,
    Field::Priority,
    Field::Nice,
    Field::PercentCpu,
    Field::PercentMem,
    Field::Time,
    Field::Comm,
];

/// A process name, bounded to avoid any heap allocation during a scan.
pub type CommBuf = arrayvec::ArrayString<64>;

/// Raw, monotonically increasing CPU tick counters for one process.
///
/// CPU percent can only be derived from two samples; see `docs/DESIGN.md` §3.5.
#[derive(Copy, Clone, Default, Debug)]
pub struct CpuTicks {
    /// `utime + stime` of the process, in clock ticks.
    pub proc: u64,
    /// System-wide total ticks across all CPUs at the same instant.
    pub total: u64,
}

bitflags_like::flags! {
    /// Properties of a row that affect rendering and filtering.
    pub struct RowFlags: u16 {
        const KERNEL_THREAD = 1 << 0;
        const USERLAND_THREAD = 1 << 1;
        const STALE_EXE = 1 << 2;
        const STALE_LIB = 1 << 3;
        const NEW = 1 << 4;
    }
}

/// Internal shim: a tiny bitflags implementation, to avoid a dependency.
mod bitflags_like {
    macro_rules! flags {
        (
            $(#[$meta:meta])*
            pub struct $name:ident: $ty:ty {
                $(
                    const $flag:ident = $value:expr;
                )*
            }
        ) => {
            $(#[$meta])*
            #[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
            pub struct $name(pub $ty);

            impl $name {
                $(pub const $flag: $name = $name($value);)*

                pub const fn empty() -> Self { $name(0) }

                pub const fn contains(self, other: Self) -> bool {
                    (self.0 & other.0) == other.0
                }

                pub const fn intersects(self, other: Self) -> bool {
                    (self.0 & other.0) != 0
                }

                pub const fn insert(self, other: Self) -> Self {
                    $name(self.0 | other.0)
                }

                pub const fn remove(self, other: Self) -> Self {
                    $name(self.0 & !other.0)
                }

                pub const fn set(self, other: Self, on: bool) -> Self {
                    if on { self.insert(other) } else { self.remove(other) }
                }
            }
        };
    }

    pub(crate) use flags;
}

/// One process, as sampled during a single tick.
///
/// **Invariant: `Row` stores raw scalars and interned strings only.** Any
/// `String` produced by `format!` during a scan is a bug — see
/// `docs/DESIGN.md` §9.4. Human-readable text is produced by the renderer
/// through `crate::format`.
#[derive(Clone, Debug)]
pub struct Row {
    pub pid: Pid,
    pub ppid: Pid,
    pub pgrp: Pid,
    pub session: Pid,
    pub tty_nr: i64,
    /// Field 22 of `/proc/<pid>/stat`: cache key and PID-reuse detector.
    pub starttime: u64,
    /// Thread group id: the pid of the process this row belongs to.
    ///
    /// For a process this equals `pid`. For a thread it is the leader's pid,
    /// which is what lets the table nest threads under their process and keep
    /// the selection stable while thread enumeration is toggled.
    pub tgid: Pid,
    /// True when this row is a thread rather than a process.
    ///
    /// Kept separate from `RowFlags::USERLAND_THREAD` because that flag answers
    /// "is this a userland thread" for display purposes, while this answers a
    /// structural question the table has to get right.
    pub is_thread: bool,
    /// Identity of a thread row, unique across the whole table.
    ///
    /// For a process row this is `None` and `pid` is the identity. For a thread it
    /// is `Some`, because on macOS a thread has no pid of its own: `pth` handles
    /// are 64-bit (95% exceed `u32`) and are *not* globally unique (every process
    /// reports the same handle for its main thread). Linux threads do have a tid
    /// that fits in `pid`, but it can coincide with an unrelated process's pid, so
    /// thread rows use this on both platforms.
    pub thread_id: Option<u64>,

    /// `ArrayString<64>`: no heap allocation per scanned process.
    pub comm: CommBuf,
    pub state: u8,

    pub uid: u32,
    pub user: Option<u32>, // index into the user-name table

    pub priority: i8,
    pub nice: i8,
    pub processor: i32,
    pub nlwp: i32,
    pub sched_policy: u8,

    /// Raw tick counters, current and previous sample.
    pub ticks: CpuTicks,
    pub prev_ticks: CpuTicks,

    pub min_flt: u64,
    pub maj_flt: u64,

    /// Virtual and resident size in bytes.
    pub virt: u64,
    pub res: u64,

    /// Derived values, computed at sample time (not at render time).
    pub percent_cpu: f32,
    pub percent_mem: f32,
    /// `utime + stime` in ticks, kept for the TIME column.
    pub time_ticks: u64,
    /// Seconds since process start.
    pub elapsed: u64,

    /// Lazily loaded, cached until the process exits.
    pub cmdline: Option<Box<str>>,
    pub exe: Option<Box<str>>,
    pub cgroup: Option<Box<str>>,

    /// Tree bookkeeping, filled by `tree::build`.
    pub depth: u16,
    pub has_children: bool,
    pub is_last_child: bool,

    pub flags: RowFlags,
}

impl Row {
    /// Construct a minimal row, used by tests and by the tree helpers.
    pub fn default_for_test(pid: Pid) -> Self {
        Row {
            pid,
            ppid: 0,
            pgrp: 0,
            session: 0,
            tty_nr: 0,
            starttime: 0,
            tgid: pid,
            is_thread: false,
            thread_id: None,
            comm: CommBuf::new(),
            state: b'R',
            uid: 0,
            user: None,
            priority: 20,
            nice: 0,
            processor: 0,
            nlwp: 1,
            sched_policy: 0,
            ticks: CpuTicks::default(),
            prev_ticks: CpuTicks::default(),
            min_flt: 0,
            maj_flt: 0,
            virt: 0,
            res: 0,
            percent_cpu: 0.0,
            percent_mem: 0.0,
            time_ticks: 0,
            elapsed: 0,
            cmdline: None,
            exe: None,
            cgroup: None,
            depth: 0,
            has_children: false,
            is_last_child: false,
            flags: RowFlags::empty(),
        }
    }

    /// Major page faults.
    ///
    /// Exposed as a method so sort keys and renderers read symmetrically.
    pub const fn maj_flt_count(&self) -> u64 {
        self.maj_flt
    }

    /// Total CPU ticks consumed by the process.
    pub const fn time_ticks_total(&self) -> u64 {
        self.time_ticks
    }

    pub fn is_kernel_thread(&self) -> bool {
        self.flags.contains(RowFlags::KERNEL_THREAD)
    }

    /// The value that identifies this row across samples.
    ///
    /// `pid` for a process, a thread-specific id for a thread. Used wherever a
    /// row must be told apart from another row with the same `pid`, which on
    /// macOS every thread of a process shares.
    pub fn identity(&self) -> u64 {
        match self.thread_id {
            Some(id) => id,
            None => self.pid as u64,
        }
    }

    /// Text for the command column: `cmdline` if known, else `comm`, with
    /// kernel threads bracketed per htop's convention.
    pub fn display_command(&self) -> &str {
        match &self.cmdline {
            Some(c) if !c.is_empty() => c,
            _ => self.comm.as_str(),
        }
    }
}
