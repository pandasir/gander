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
            let cur = unsafe { libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t) };
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

#[cfg(unix)]
fn set_nice(pid: Pid, nice: i32) -> Result<(), std::io::Error> {
    use std::io;
    // SAFETY: integer arguments only.
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) };
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
