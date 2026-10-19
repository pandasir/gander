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
    // Linux-only: cgroup membership, and the container derived from it.
    #[cfg(target_os = "linux")]
    m(
        Field::Cgroup,
        "CGROUP",
        " CGROUP ",
        "Full cgroup path",
        false,
        Align::Left,
    ),
    #[cfg(target_os = "linux")]
    m(
        Field::Container,
        "CONTAINER",
        " CONTAINER ",
        "Container id, prefixed with its runtime",
        false,
        Align::Left,
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
    /// The process's cgroup path, e.g. `/system.slice/docker-1f2e….scope`.
    ///
    /// The *path*, not the raw `/proc/<pid>/cgroup` line: the raw line is
    /// `12:cpuset:/docker/abc` and carries the hierarchy id and controller list,
    /// which no column wants. The v2 entry (empty controller list) is preferred
    /// when present, because that is the hierarchy the whole machine shares.
    pub cgroup: Option<Box<str>>,
    /// The container this process belongs to, if any.
    ///
    /// Derived from `cgroup` once per process and cached with it. Linux-only
    /// because the detection is cgroup-based; `None` everywhere else.
    #[cfg(target_os = "linux")]
    pub container: Option<Box<str>>,
    /// The runtime that owns the container (`docker`, `kubepods`, …), kept apart
    /// from `container` so the `CONTAINER` column can show `docker:1f2e…` without
    /// re-parsing the path at render time.
    #[cfg(target_os = "linux")]
    pub container_runtime: Option<Box<str>>,

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
            #[cfg(target_os = "linux")]
            container: None,
            #[cfg(target_os = "linux")]
            container_runtime: None,
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

    /// The executable's base name, and where it sits in [`Self::display_command`].
    ///
    /// Returns `(start, end)` byte offsets into `display_command()` so the renderer
    /// can emphasise those cells without re-deriving the text or allocating.
    ///
    /// # Why this is not just "the last path component"
    ///
    /// The command line is `argv` joined by spaces, so a naive split on `/` finds
    /// a path in an *argument* too (`grep /usr/lib/foo`). The base name is taken
    /// from `exe` when the sampler has it, and located in the command text as a
    /// prefix match on that path; when `exe` is unavailable (another user's
    /// process) it falls back to the first word of the command, which is what
    /// argv[0] is.
    ///
    /// Returns `None` when there is nothing worth highlighting — a kernel thread
    /// in brackets, or a command whose base name is the whole string.
    pub fn base_name_span(&self) -> Option<(usize, usize)> {
        self.base_name_span_in(self.display_command())
    }

    /// [`Self::base_name_span`] against `text` instead of the command column text.
    ///
    /// The renderer prepends the tree branch prefix to the command column, so the
    /// span it needs is offset by that prefix's length. Searching the *finished*
    /// text is simpler and more robust than adding an offset to the span: the
    /// prefix can only appear before the command, and the exe path search below
    /// does not match it.
    pub fn base_name_span_in(&self, text: &str) -> Option<(usize, usize)> {
        if text.is_empty() {
            return None;
        }

        // Prefer the real exe path: a prefix of the command line, when we have it.
        if let Some(exe) = self.exe.as_deref() {
            if !exe.is_empty() {
                if let Some(pos) = text.find(exe) {
                    let start = pos + exe.rfind('/').map_or(0, |i| i + 1);
                    let end = pos + exe.len();
                    if end > start {
                        return Some((start, end));
                    }
                }
            }
        }

        // Fall back to argv[0]: everything up to the first space.
        let end = text.find(' ').unwrap_or(text.len());
        let word = &text[..end];
        // A bracketed name is a kernel thread, and htop does not highlight those.
        if word.starts_with('[') {
            return None;
        }
        let start = word.rfind('/').map_or(0, |i| i + 1);
        // Nothing to gain from highlighting the entire string.
        if start == 0 && end == text.len() {
            return None;
        }
        if end > start {
            Some((start, end))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod base_name_tests {
    use super::*;

    fn row_with(cmdline: &str, exe: Option<&str>) -> Row {
        let mut r = Row::default_for_test(1);
        r.cmdline = Some(cmdline.into());
        r.exe = exe.map(Into::into);
        r
    }

    #[test]
    fn finds_the_basename_inside_a_full_path() {
        let r = row_with(
            "/Applications/iTerm.app/Contents/MacOS/iTerm2 --server",
            Some("/Applications/iTerm.app/Contents/MacOS/iTerm2"),
        );
        let text = r.display_command();
        let (s, e) = r.base_name_span().expect("span");
        assert_eq!(&text[s..e], "iTerm2");
    }

    /// The exe path is authoritative, so an argument that looks like a path must
    /// not win. `grep /usr/lib/foo` should highlight `grep`.
    #[test]
    fn an_argument_path_does_not_win_over_the_exe() {
        let r = row_with("grep /usr/lib/foo", Some("/usr/bin/grep"));
        let text = r.display_command();
        let (s, e) = r.base_name_span().expect("span");
        assert_eq!(&text[s..e], "grep");
    }

    /// Without `exe` (another user's process) fall back to argv[0].
    #[test]
    fn falls_back_to_argv0_without_an_exe() {
        let r = row_with("/usr/bin/python3 script.py", None);
        let text = r.display_command();
        let (s, e) = r.base_name_span().expect("span");
        assert_eq!(&text[s..e], "python3");
    }

    #[test]
    fn a_bare_command_name_gets_no_span() {
        // Nothing to emphasise: the name is the whole string already.
        let r = row_with("launchd", None);
        assert_eq!(r.base_name_span(), None);
    }

    #[test]
    fn a_kernel_thread_gets_no_span() {
        let r = row_with("[kworker/0:1]", None);
        assert_eq!(r.base_name_span(), None);
    }

    #[test]
    fn an_empty_command_gets_no_span() {
        let mut r = Row::default_for_test(1);
        r.cmdline = None;
        r.comm.try_push_str("").unwrap();
        assert_eq!(r.base_name_span(), None);
    }

    /// The renderer passes the *finished* column text, which in tree mode carries
    /// a branch prefix. The span has to move with it.
    #[test]
    fn the_span_tracks_a_prepended_tree_prefix() {
        let r = row_with("/usr/bin/nginx -g daemon", Some("/usr/bin/nginx"));
        let plain = r.display_command();
        let (s, e) = r.base_name_span().expect("span");
        assert_eq!(&plain[s..e], "nginx");

        let prefixed = format!("├─ {plain}");
        let (ps, pe) = r.base_name_span_in(&prefixed).expect("span");
        assert_eq!(&prefixed[ps..pe], "nginx");
        assert_eq!(ps, s + "├─ ".len(), "offset by the prefix");
    }

    /// A base name containing a space would make the argv[0] fallback wrong, but
    /// the exe path still locates it correctly.
    #[test]
    fn an_exe_with_spaces_is_located_by_path_not_by_word() {
        let r = row_with(
            "/Users/x/My App.app/Contents/MacOS/My App --flag",
            Some("/Users/x/My App.app/Contents/MacOS/My App"),
        );
        let text = r.display_command();
        let (s, e) = r.base_name_span().expect("span");
        assert_eq!(&text[s..e], "My App");
    }
}
