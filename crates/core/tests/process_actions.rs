#![cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]

use gander_core::action::{execute_checked, Action, ProcessTarget, SIGNALS};
use gander_core::PlatformSampler;

struct Child(std::process::Child);

impl Child {
    fn spawn() -> Self {
        Self(
            std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap(),
        )
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn signal_names_match_platform_constants() {
    let expected = [
        (libc::SIGHUP, "SIGHUP"),
        (libc::SIGINT, "SIGINT"),
        (libc::SIGQUIT, "SIGQUIT"),
        (libc::SIGILL, "SIGILL"),
        (libc::SIGABRT, "SIGABRT"),
        (libc::SIGKILL, "SIGKILL"),
        (libc::SIGSEGV, "SIGSEGV"),
        (libc::SIGTERM, "SIGTERM"),
        (libc::SIGCHLD, "SIGCHLD"),
        (libc::SIGCONT, "SIGCONT"),
        (libc::SIGSTOP, "SIGSTOP"),
        (libc::SIGTSTP, "SIGTSTP"),
    ];
    assert_eq!(SIGNALS, expected);
}

#[test]
fn sampled_identity_matches_live_identity() {
    let pid = std::process::id();
    let (_, rows) = gander_core::Sampler::default().sample();
    let row = rows.iter().find(|r| r.pid == pid && !r.is_thread).unwrap();
    assert_eq!(
        ProcessTarget::from(row),
        ProcessTarget::capture(pid).unwrap()
    );
}

#[test]
fn stale_identity_cannot_signal_or_renice_a_live_child() {
    let mut child = Child::spawn();
    let target = ProcessTarget::capture(child.0.id()).unwrap();
    let stale = ProcessTarget {
        starttime: target.starttime.wrapping_add(1),
        ..target
    };
    assert!(execute_checked(
        stale,
        &Action::Kill {
            pid: target.pid,
            signal: libc::SIGTERM
        }
    )
    .is_err());
    assert!(execute_checked(
        stale,
        &Action::Priority {
            pid: target.pid,
            delta: 1
        }
    )
    .is_err());
    assert!(child.0.try_wait().unwrap().is_none());
    execute_checked(
        target,
        &Action::Kill {
            pid: target.pid,
            signal: 0,
        },
    )
    .unwrap();
}

#[test]
fn checked_signal_terminates_only_the_captured_child() {
    let mut child = Child::spawn();
    let target = ProcessTarget::capture(child.0.id()).unwrap();
    execute_checked(
        target,
        &Action::Kill {
            pid: target.pid,
            signal: libc::SIGTERM,
        },
    )
    .unwrap();
    assert!(!child.0.wait().unwrap().success());
}

#[test]
fn exited_target_and_mismatched_action_are_rejected() {
    let mut child = Child::spawn();
    let target = ProcessTarget::capture(child.0.id()).unwrap();
    assert!(execute_checked(
        target,
        &Action::Kill {
            pid: std::process::id(),
            signal: 0
        }
    )
    .is_err());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(execute_checked(
        target,
        &Action::Kill {
            pid: target.pid,
            signal: 0
        }
    )
    .is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn checked_signals_also_support_non_leader_thread_ids() {
    let (send_tid, tid) = std::sync::mpsc::channel();
    let (release, finished) = std::sync::mpsc::channel::<()>();
    let thread = std::thread::spawn(move || {
        // SAFETY: gettid takes no arguments.
        let tid = unsafe { libc::syscall(libc::SYS_gettid) } as u32;
        send_tid.send(tid).unwrap();
        let _ = finished.recv();
    });
    let tid = tid.recv().unwrap();
    let target = ProcessTarget::capture(tid).unwrap();
    let result = execute_checked(
        target,
        &Action::Kill {
            pid: tid,
            signal: 0,
        },
    );
    drop(release);
    thread.join().unwrap();
    result.unwrap();
}

#[test]
fn process_group_pids_are_rejected() {
    for pid in [0, u32::MAX] {
        assert!(ProcessTarget::capture(pid).is_err());
        assert!(gander_core::action::execute(&Action::Kill { pid, signal: 0 }).is_err());
    }
}
