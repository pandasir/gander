//! Process actions.
//!
//! `core` never performs a side effect directly: it produces an `Action` that
//! the front end executes, so the same logic can be unit-tested and dry-run.

use crate::Pid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Kill {
        pid: Pid,
        signal: i32,
    },
    Renice {
        pid: Pid,
        nice: i8,
    },
    /// Raw priority adjustment; `Delta` is applied to the current nice value.
    Priority {
        pid: Pid,
        delta: i8,
    },
}

/// Signals offered by the signal picker, in the order htop shows them.
pub const SIGNALS: &[(i32, &str)] = &[
    (1, "SIGHUP"),
    (2, "SIGINT"),
    (3, "SIGQUIT"),
    (4, "SIGILL"),
    (6, "SIGABRT"),
    (9, "SIGKILL"),
    (11, "SIGSEGV"),
    (15, "SIGTERM"),
    (17, "SIGCHLD"),
    (18, "SIGCONT"),
    (19, "SIGSTOP"),
    (20, "SIGTSTP"),
];

/// Executes actions against the real system.
///
/// `kill(2)`, `getpriority(2)` and `setpriority(2)` are POSIX, so this works on
/// every unix. Gating it on Linux meant `F9` silently failed on macOS with
/// "process actions are only implemented for Linux".
#[cfg(unix)]
pub fn execute(action: &Action) -> Result<(), std::io::Error> {
    use std::io;
    match *action {
        Action::Kill { pid, signal } => {
            // SAFETY: `kill` is async-signal-safe and takes only integers.
            let rc = unsafe { libc::kill(pid as libc::pid_t, signal as libc::c_int) };
            if rc == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        Action::Renice { pid, nice } => set_nice(pid, nice as i32),
        Action::Priority { pid, delta } => {
            // Read the current nice value. `-1` is both a valid priority and the
            // documented error return, so errno is cleared first to tell them
            // apart (the classic getpriority trap).
            // SAFETY: both calls take only integers.
            unsafe { set_errno(0) };
            // SAFETY: `getpriority` has no preconditions.
            let cur = unsafe { libc::getpriority(libc::PRIO_PROCESS, who(pid)) };
            // SAFETY: reading errno has no preconditions.
            if cur == -1 && unsafe { errno() } != 0 {
                return Err(io::Error::last_os_error());
            }
            let target = (cur + delta as i32).clamp(-20, 19);
            set_nice(pid, target)
        }
    }
}

/// Read the C `errno`. `libc` exposes it differently per platform.
///
/// SAFETY: no preconditions; the pointer is thread-local and always valid.
#[cfg(unix)]
unsafe fn errno() -> i32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        *libc::__errno_location()
    }
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    {
        *libc::__error()
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd"
    )))]
    {
        0
    }
}

/// Set the C `errno`.
///
/// SAFETY: as above.
#[cfg(unix)]
unsafe fn set_errno(value: i32) {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        *libc::__errno_location() = value;
    }
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    {
        *libc::__error() = value;
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd"
    )))]
    {
        let _ = value;
    }
}

/// The `who` argument for `getpriority`/`setpriority`.
///
/// POSIX types it `id_t`, and FreeBSD's `id_t` is genuinely 64-bit while its
/// `getpriority` still takes an `int`; `libc` reproduces each platform's actual
/// prototype, so the two spellings differ. Naming the type once keeps the call
/// sites identical.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn who(pid: Pid) -> libc::id_t {
    pid as libc::id_t
}

#[cfg(target_os = "freebsd")]
fn who(pid: Pid) -> libc::c_int {
    pid as libc::c_int
}

#[cfg(unix)]
fn set_nice(pid: Pid, nice: i32) -> Result<(), std::io::Error> {
    use std::io;
    // SAFETY: integer arguments only.
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, who(pid), nice) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Non-unix fallback. There is no portable `kill(2)` here, and no supported
/// target needs one yet.
#[cfg(not(unix))]
pub fn execute(_action: &Action) -> Result<(), std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "process actions require a unix target",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_are_comparable() {
        assert_eq!(
            Action::Kill { pid: 1, signal: 9 },
            Action::Kill { pid: 1, signal: 9 }
        );
        assert_ne!(
            Action::Kill { pid: 1, signal: 9 },
            Action::Kill { pid: 1, signal: 15 }
        );
    }

    #[test]
    fn signal_table_is_sorted_and_unique() {
        let mut signals: Vec<i32> = SIGNALS.iter().map(|(n, _)| *n).collect();
        let len = signals.len();
        signals.dedup();
        assert_eq!(signals.len(), len, "duplicate signal numbers");
        signals.sort_unstable();
        assert_eq!(signals.first(), Some(&1));
        assert!(signals.contains(&9) && signals.contains(&15));
    }
}

/// I/O scheduling class, as Linux's `ioprio_set` understands it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum IoClass {
    None,
    Realtime,
    BestEffort,
    Idle,
}

impl IoClass {
    /// The `IOPRIO_CLASS_*` value.
    pub fn bits(self) -> u32 {
        match self {
            IoClass::None => 0,
            IoClass::Realtime => 1,
            IoClass::BestEffort => 2,
            IoClass::Idle => 3,
        }
    }

    pub fn from_bits(bits: u32) -> Option<Self> {
        Some(match bits {
            0 => IoClass::None,
            1 => IoClass::Realtime,
            2 => IoClass::BestEffort,
            3 => IoClass::Idle,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            IoClass::None => "none",
            IoClass::Realtime => "realtime",
            IoClass::BestEffort => "best-effort",
            IoClass::Idle => "idle",
        }
    }

    /// The next class in the cycle the panel walks.
    pub fn next(self) -> Self {
        match self {
            IoClass::None => IoClass::BestEffort,
            IoClass::BestEffort => IoClass::Idle,
            IoClass::Idle => IoClass::Realtime,
            IoClass::Realtime => IoClass::None,
        }
    }
}

/// An I/O priority: a class plus a 0..=7 level within that class.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct IoPriority {
    pub class: IoClass,
    pub level: u8,
}

impl Default for IoPriority {
    fn default() -> Self {
        Self {
            class: IoClass::None,
            level: 0,
        }
    }
}

impl IoPriority {
    pub fn new(class: IoClass, level: u8) -> Self {
        Self {
            class,
            level: level.min(7),
        }
    }

    /// The packed `ioprio` value the syscall takes.
    pub fn to_bits(self) -> i32 {
        (((self.class.bits() as i32) << 13) | self.level as i32) & 0xFFFF
    }

    pub fn from_bits(bits: i32) -> Self {
        let class = IoClass::from_bits(((bits >> 13) & 0x7) as u32).unwrap_or(IoClass::None);
        Self {
            class,
            level: (bits & 0x7) as u8,
        }
    }

    /// `class/level`, e.g. `best-effort/4`.
    pub fn label(self) -> String {
        format!("{}/{}", self.class.name(), self.level)
    }
}

/// Whether this build can read and set I/O priority.
pub fn ioprio_supported() -> bool {
    cfg!(target_os = "linux")
}

/// Read a process's I/O priority.
///
/// Linux only; elsewhere this returns `Err` with an explanation, because unlike
/// CPU affinity there is no partial story to tell.
#[cfg(target_os = "linux")]
pub fn ioprio_get(pid: Pid) -> Result<IoPriority, std::io::Error> {
    use std::io;
    // SAFETY: `ioprio_get` is a plain syscall taking integers.
    let rc = unsafe { libc::syscall(libc::SYS_ioprio_get, 1 /* IOPRIO_WHO_PROCESS */, pid) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(IoPriority::from_bits(rc as i32))
}

#[cfg(not(target_os = "linux"))]
pub fn ioprio_get(_pid: Pid) -> Result<IoPriority, std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "I/O priority is a Linux feature (ioprio_get)",
    ))
}

#[cfg(target_os = "linux")]
pub fn ioprio_set(pid: Pid, priority: IoPriority) -> Result<(), std::io::Error> {
    use std::io;
    // SAFETY: `ioprio_set` is a plain syscall taking integers.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_ioprio_set,
            1, /* IOPRIO_WHO_PROCESS */
            pid,
            priority.to_bits(),
        )
    };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
pub fn ioprio_set(_pid: Pid, _priority: IoPriority) -> Result<(), std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "I/O priority is a Linux feature (ioprio_set)",
    ))
}

#[cfg(test)]
mod ioprio_tests {
    use super::*;

    #[test]
    fn bits_pack_and_unpack() {
        for class in [
            IoClass::None,
            IoClass::Realtime,
            IoClass::BestEffort,
            IoClass::Idle,
        ] {
            for level in 0..=7u8 {
                let p = IoPriority::new(class, level);
                let back = IoPriority::from_bits(p.to_bits());
                assert_eq!(back, p, "{:?}/{}", class, level);
            }
        }
    }

    #[test]
    fn the_default_is_class_none() {
        let p = IoPriority::default();
        assert_eq!(p.class, IoClass::None);
        assert_eq!(p.level, 0);
        assert_eq!(p.to_bits(), 0);
    }

    #[test]
    fn levels_are_clamped_to_seven() {
        let p = IoPriority::new(IoClass::BestEffort, 200);
        assert_eq!(p.level, 7);
    }

    #[test]
    fn classes_cycle_through_every_variant() {
        let mut c = IoClass::None;
        let mut seen = vec![c];
        for _ in 0..3 {
            c = c.next();
            seen.push(c);
        }
        assert_eq!(seen.len(), 4);
        assert_eq!(c.next(), IoClass::None, "the cycle closes");
    }

    #[test]
    fn labels_are_readable() {
        let p = IoPriority::new(IoClass::BestEffort, 4);
        assert_eq!(p.label(), "best-effort/4");
    }

    #[test]
    fn an_out_of_range_class_degrades_to_none() {
        let p = IoPriority::from_bits((7 << 13) | 3);
        assert_eq!(p.class, IoClass::None);
        assert_eq!(p.level, 3);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reading_our_own_ioprio_works() {
        let p = ioprio_get(std::process::id()).expect("ioprio_get on self");
        // Any class is fine; the call must simply succeed.
        assert!(matches!(
            p.class,
            IoClass::None | IoClass::Realtime | IoClass::BestEffort | IoClass::Idle
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn setting_our_own_ioprio_round_trips() {
        let original = ioprio_get(std::process::id()).unwrap();
        let target = IoPriority::new(IoClass::BestEffort, 6);
        if ioprio_set(std::process::id(), target).is_ok() {
            assert_eq!(ioprio_get(std::process::id()).unwrap(), target);
        }
        let _ = ioprio_set(std::process::id(), original);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn unsupported_platforms_explain_themselves() {
        assert!(!ioprio_supported());
        let err = ioprio_get(std::process::id()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
        assert!(err.to_string().contains("Linux"));
    }
}
