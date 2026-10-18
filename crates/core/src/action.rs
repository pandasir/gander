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
#[cfg(target_os = "linux")]
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
            // PRIO_PROCESS with a zero argument means "read the current value".
            // SAFETY: as above.
            let cur = unsafe { libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t) };
            let target = (cur + delta as i32).clamp(-20, 19);
            set_nice(pid, target)
        }
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(not(target_os = "linux"))]
pub fn execute(_action: &Action) -> Result<(), std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "process actions are only implemented for Linux",
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
