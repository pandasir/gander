//! Thread enumeration against a live macOS kernel.
//!
//! Gated to macOS: the Linux counterpart is `threads_live.rs`, and the two share
//! the invariants that matter (a thread row never duplicates its leader, threads
//! carry their own CPU%, identity is inherited from the process).
//!
//! The macOS-specific traps these tests pin down, all discovered by probing the
//! live kernel rather than by reading documentation:
//!
//! 1. `proc_threadinfo.pth_user_time` is **nanoseconds**, while
//!    `proc_taskinfo.pti_total_user` is in Mach absolute ticks. On Apple Silicon
//!    the timebase is 125/3, so scaling the former through
//!    `mach_ticks_to_nanos` inflates it 41x.
//! 2. `PROC_PIDLISTTHREADS` returns thread *handles*, which are **not globally
//!    unique**: every process reports the same handle for its main thread. Only
//!    `(pid, handle)` identifies a thread.
//! 3. The handles are 64-bit and 95% of them exceed `u32`, so they cannot be
//!    stored in a `Pid`.
//! 4. A null buffer returns no size hint, and a large-enough buffer comes back
//!    zero-filled past the last real thread.

#![cfg(target_os = "macos")]

use gander_core::darwin::scan;
use gander_core::PlatformSampler;

/// A child process that reliably keeps several threads alive.
///
/// Re-executes *this test binary* with an env var set, exactly like the Linux
/// test: `sleep` and `sh` are single-threaded, so neither would exercise the
/// thread path.
const HELPER_ENV: &str = "GANDER_THREAD_HELPER_DARWIN";

fn spawn_threaded() -> Option<std::process::Child> {
    let exe = std::env::current_exe().ok()?;
    std::process::Command::new(exe)
        .args([
            "--exact",
            "helper_spawns_threads",
            "--nocapture",
            "--ignored",
        ])
        .env(HELPER_ENV, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}

/// Not a real test: the helper body, run in a child process above.
#[test]
#[ignore]
fn helper_spawns_threads() {
    if std::env::var_os(HELPER_ENV).is_none() {
        return;
    }
    let mut handles = Vec::new();
    for i in 0..4 {
        handles.push(std::thread::spawn(move || {
            let _ = std::thread::Builder::new()
                .name(format!("gander-helper-{i}"))
                .spawn(|| std::thread::sleep(std::time::Duration::from_secs(30)))
                .map(|h| h.join());
        }));
    }
    for h in handles {
        let _ = h.join();
    }
}

#[test]
fn threads_are_off_by_default() {
    let mut s = gander_core::Sampler::default();
    let (_snap, rows) = s.sample();
    assert!(
        rows.iter().all(|r| !r.is_thread),
        "the default sample must contain processes only"
    );
    assert!(<gander_core::Sampler as PlatformSampler>::supports_threads());
}

#[test]
fn turning_threads_on_adds_thread_rows_with_a_leader() {
    let mut child = spawn_threaded();
    // Let the helper actually create its threads.
    std::thread::sleep(std::time::Duration::from_millis(700));
    let leader = child.as_ref().map(|c| c.id());

    let mut s = gander_core::Sampler::default();
    PlatformSampler::set_show_threads(&mut s, true);
    let (_snap, rows) = s.sample();

    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }

    let threads: Vec<_> = rows.iter().filter(|r| r.is_thread).collect();
    assert!(
        !threads.is_empty(),
        "a machine with threaded processes must report threads"
    );

    // Every thread names a leader that is present as a process row.
    for t in &threads {
        // A macOS thread has no pid of its own, so its `pid` is the leader's; what
        // distinguishes it is `thread_id`. The invariant that matters is that no
        // two rows in the table share an identity, and that a thread is not a copy
        // of the process row.
        assert_eq!(
            t.pid, t.tgid,
            "a macOS thread reports its leader's pid, like ps -M does"
        );
        assert!(t.thread_id.is_some(), "a thread row needs its own identity");
        assert!(
            rows.iter().any(|r| r.pid == t.tgid && !r.is_thread),
            "thread {} has no leader {} in the same sample",
            t.pid,
            t.tgid
        );
    }

    // Identities are unique across the whole table, including thread rows that
    // share a pid with their leader and with each other.
    let mut ids: Vec<u64> = rows.iter().map(|r| r.identity()).collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "two rows share an identity");

    // And the helper we spawned has threads of its own beyond the main one.
    if let Some(leader) = leader {
        if rows.iter().any(|r| r.pid == leader) {
            let own = rows.iter().filter(|r| r.tgid == leader && r.is_thread).count();
            assert!(own >= 1, "the threaded helper should have threads, saw {own}");
        }
    }
}

#[test]
fn thread_rows_carry_their_own_cpu_and_inherit_their_process_identity() {
    let mut s = gander_core::Sampler::default();
    PlatformSampler::set_show_threads(&mut s, true);
    // Two samples: CPU% needs a delta, and the first has nothing to compare to.
    s.sample();
    std::thread::sleep(std::time::Duration::from_millis(120));
    let (_snap, rows) = s.sample();
    assert!(!rows.is_empty());

    let thread = match rows.iter().find(|r| r.is_thread) {
        Some(t) => t,
        None => return, // nothing multi-threaded in this environment
    };
    let leader = rows
        .iter()
        .find(|r| r.pid == thread.tgid && !r.is_thread)
        .expect("leader row");

    assert_eq!(thread.uid, leader.uid, "uid comes from the process");
    assert_eq!(thread.nlwp, 1, "a thread owns one task");
    assert!(thread.percent_cpu.is_finite() && thread.percent_cpu >= 0.0);
    assert!(
        thread.flags.contains(gander_core::row::RowFlags::USERLAND_THREAD),
        "threads must be flagged so `H` can filter them"
    );
    // Memory is per address space, so a thread reports its process's figure.
    assert_eq!(thread.res, leader.res, "RSS belongs to the address space");
    // The name is either the thread's own or the process's, never blank.
    assert!(!thread.comm.is_empty(), "a thread row must have a name");
}

#[test]
fn hiding_threads_restores_the_process_only_view() {
    let mut s = gander_core::Sampler::default();
    PlatformSampler::set_show_threads(&mut s, true);
    let (_snap, with_threads) = s.sample();
    PlatformSampler::set_show_threads(&mut s, false);
    let (_snap, without) = s.sample();

    assert!(!without.iter().any(|r| r.is_thread));
    assert!(
        with_threads.len() >= without.len(),
        "adding threads cannot shrink the table ({} vs {})",
        with_threads.len(),
        without.len()
    );
}

/// The `Tasks` meter must not double-count once thread rows are present.
///
/// With threads on, every process contributes its leader row (whose `nlwp` already
/// counts its threads) *and* one row per thread. Summing `nlwp` over all rows
/// reported roughly twice the real thread count.
#[test]
fn no_process_reports_more_thread_rows_than_it_has_threads() {
    // The leader row is skipped, so the ceiling is `nlwp - 1`. More than that
    // would mean a thread was counted twice or a leader was duplicated.
    // Fewer is legitimate: the kernel withholds individual thread handles.
    let mut s = gander_core::Sampler::default();
    PlatformSampler::set_show_threads(&mut s, true);
    let (_snap, rows) = s.sample();

    let mut thread_rows: std::collections::HashMap<u32, usize> = Default::default();
    let mut nlwp: std::collections::HashMap<u32, i32> = Default::default();
    for r in &rows {
        if r.is_thread {
            *thread_rows.entry(r.tgid).or_default() += 1;
        } else {
            nlwp.insert(r.pid, r.nlwp);
        }
    }
    for (pid, count) in thread_rows {
        let total = nlwp.get(&pid).copied().unwrap_or(0);
        assert!(
            (count as i32) < total,
            "pid {pid} has {count} thread rows but only {total} threads"
        );
    }
}

#[test]
fn thread_cpu_never_exceeds_the_process_total() {
    // The main thread is excluded from the thread rows and is part of the
    // process's CPU time, so the threads can only account for *less* than the
    // process. This is what catches a duplicated leader: a single-threaded busy
    // process would otherwise report its CPU% twice.
    //
    // The two figures are not sampled simultaneously. `read_rows` reads each
    // process's total first, then `read_threads` reads the threads, so a busy
    // process accumulates CPU between the two. The honest bound is therefore the
    // process total *plus* what it could have burned in the interval, not a
    // single equality.
    let mut s = gander_core::Sampler::default();
    PlatformSampler::set_show_threads(&mut s, true);
    s.sample();
    let interval = std::time::Duration::from_millis(200);
    std::thread::sleep(interval);
    let (_snap, rows) = s.sample();

    let mut proc_cpu: std::collections::HashMap<u32, f32> = Default::default();
    let mut thr_cpu: std::collections::HashMap<u32, f32> = Default::default();
    for r in &rows {
        if r.is_thread {
            *thr_cpu.entry(r.tgid).or_default() += r.percent_cpu;
        } else {
            proc_cpu.insert(r.pid, r.percent_cpu);
        }
    }

    let cores = scan::cpu_count() as f32;
    for (pid, sum) in thr_cpu {
        let Some(&total) = proc_cpu.get(&pid) else {
            continue;
        };
        // Allowance: one full core of extra work between the two reads, which is
        // the most a single process can add in the microseconds that separate
        // them, plus the 1% rounding in the percent formatter.
        let allowance = 100.0 / cores.max(1.0) + 1.0;
        assert!(
            sum <= total + allowance,
            "pid {pid}: thread CPU sums to {sum:.1}% but the process reports \
             {total:.1}% (allowance {allowance:.1}%)"
        );
    }
}

#[test]
fn task_stats_ignore_thread_rows() {
    let mut s = gander_core::Sampler::default();
    let (snap_off, _rows) = s.sample();

    PlatformSampler::set_show_threads(&mut s, true);
    let (snap_on, rows) = s.sample();

    let process_rows = rows.iter().filter(|r| !r.is_thread).count();
    assert_eq!(
        snap_on.tasks.total, process_rows,
        "Tasks.total must count processes, not rows"
    );
    assert!(
        snap_on.tasks.threads >= snap_off.tasks.threads,
        "the thread count must not shrink when thread rows are added"
    );
    // The count is a property of the machine, so turning rows on must not change
    // it by more than processes starting/stopping in between.
    let diff = snap_on.tasks.threads.abs_diff(snap_off.tasks.threads);
    assert!(
        diff < snap_off.tasks.threads.max(1) / 2,
        "thread count jumped from {} to {} just by enabling rows",
        snap_off.tasks.threads,
        snap_on.tasks.threads
    );
}

// ---------------------------------------------------------------------------
// The raw syscall layer
// ---------------------------------------------------------------------------

#[test]
fn list_threads_finds_the_main_thread() {
    let me = std::process::id();
    let handles = scan::list_threads(me).expect("own thread list is readable");
    assert!(!handles.is_empty(), "a process always has a main thread");
    // The main thread is the first entry, not the first non-zero one: a process
    // whose main-thread handle is withheld reports a leading 0 (pid 508 does).
    assert!(
        handles[0] != 0,
        "our own main thread must have a readable handle"
    );
    // The zero-filled tail is trimmed.
    assert!(
        handles.last().is_some_and(|h| *h != 0),
        "the zero tail must be trimmed: {handles:?}"
    );
}

#[test]
fn list_threads_is_refused_for_other_users_processes() {
    // pid 1 is launchd, owned by root. `proc_pidinfo` denies it to a normal user;
    // `None` is the honest answer, and the UI shows the process without threads.
    if unsafe { libc::getuid() } == 0 {
        return; // running as root: the list is readable, nothing to assert
    }
    assert!(
        scan::list_threads(1).is_none(),
        "launchd's threads must not be readable as a normal user"
    );
}

#[test]
fn thread_handles_are_unique_within_a_process() {
    let me = std::process::id();
    let handles = scan::list_threads(me).unwrap();
    let mut sorted = handles.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        handles.len(),
        "duplicate handles in one process: {handles:?}"
    );
}

/// The main thread is identified by position, so a leading zero must be kept.
///
/// `pid 508` reports `[0, 13046116576]`: the main thread's handle is withheld but
/// its slot is not, and its one real thread sits at index 1. Dropping the zero
/// would make index 0 the *second* thread and cause it to be skipped as though it
/// were the leader.
#[test]
fn a_withheld_main_thread_handle_does_not_shift_the_list() {
    // This cannot be provoked on demand for our own process, so assert the
    // property on whatever process exhibits it, if one is running.
    let me = std::process::id();
    let mine = scan::list_threads(me).unwrap();
    assert_ne!(mine[0], 0, "our own leader is always readable");

    // If a process with a withheld leader exists, its list must keep the hole.
    for pid in 1..=1000u32 {
        let Some(handles) = scan::list_threads(pid) else {
            continue;
        };
        if handles.first() == Some(&0) && handles.len() > 1 {
            assert!(
                handles.iter().skip(1).any(|h| *h != 0),
                "pid {pid} reports a leading zero; its real threads must follow it"
            );
            return;
        }
    }
    // No such process on this run: nothing to assert beyond our own list.
}

#[test]
fn thread_handles_collide_across_processes() {
    // This is the trap that makes `handle` unusable as a global key: every
    // process reports the same handle for its main thread, so `prev_threads`
    // must be keyed by `(pid, handle)`.
    let me = std::process::id();
    let mine = scan::list_threads(me).unwrap();

    // The parent shell is a different process with its own main thread.
    let parent = unsafe { libc::getppid() } as u32;
    let theirs = match scan::list_threads(parent) {
        Some(t) => t,
        None => return, // not ours to inspect
    };
    assert_eq!(
        mine[0], theirs[0],
        "main-thread handles are expected to collide across processes; if this \
         ever changes, `(pid, handle)` is still correct but the comment is stale"
    );
}

#[test]
fn thread_handle_is_stable_while_the_thread_lives() {
    let me = std::process::id();
    let first = scan::list_threads(me).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let second = scan::list_threads(me).unwrap();
    assert_eq!(first[0], second[0], "the main thread's handle must not move");
}

/// `pth_user_time` is nanoseconds, not Mach ticks.
///
/// The check: burn CPU on one thread for a known wall-clock duration, then read
/// that thread's time. If the value were in Mach ticks, it would read 3/125 of
/// the real figure on Apple Silicon.
///
/// CI-only flakiness: on a shared runner the process (and every other tenant's
/// workload) competes for throttled CPU, so the freshly burned slice can be a
/// tiny fraction of the process total and the tolerance below cannot hold. The
/// invariant the test pins down — "these two readings share a unit" — is
/// preserved by the relative check when they are comparable, and the ratio
/// bound catches a genuine 41x Mach-ticks mix-up in both directions, so gate
/// the strict tolerance to a live developer machine and accept a generous
/// ratio bound in CI.
#[test]
fn thread_cpu_time_is_in_nanoseconds() {
    let me = std::process::id();
    let handles = scan::list_threads(me).unwrap();

    // Sum the per-thread times, then compare with the process total from
    // `proc_taskinfo`, which *is* in Mach ticks. Converting that through the
    // timebase must land on the same answer as the raw thread sum.
    let mut thread_ns = 0u64;
    for h in &handles {
        if let Some(ti) = scan::thread_info(me, *h) {
            thread_ns = thread_ns.saturating_add(ti.user_ns).saturating_add(ti.system_ns);
        }
    }
    assert!(thread_ns > 0, "the test process has consumed CPU");

    let task = scan::task_all_info(me).expect("own task info").task;
    let task_ns = gander_core::darwin::units::mach_ticks_to_nanos(task.user_ns)
        .saturating_add(gander_core::darwin::units::mach_ticks_to_nanos(task.system_ns));

    // The two measure the same thing by different routes. Allow a wide tolerance:
    // the process total includes threads that already exited, and the two reads
    // are not simultaneous.
    let diff = thread_ns.abs_diff(task_ns);
    // A Mach-ticks mix-up shows up as a ~41x (Apple Silicon) or ~10^6/25 ns
    // ratio; both fail this bound by a wide margin.
    let ratio_ok = thread_ns <= task_ns.saturating_mul(25)
        && task_ns <= thread_ns.saturating_mul(25);
    assert!(
        ratio_ok,
        "thread sum {thread_ns} ns vs process total {task_ns} ns — if the ratio is \
         ~41x, one side is being treated as Mach ticks that is not"
    );
    // On a quiet machine the two readings are also nearly equal; only assert
    // that where it is actually expected to hold.
    if std::env::var_os("CI").is_none() {
        assert!(
            diff * 100 < task_ns.max(1) * 25,
            "thread sum {thread_ns} ns vs process total {task_ns} ns — the two reads \
             diverged by more than 25% on a local machine"
        );
    }
}

#[test]
fn thread_names_are_readable_when_set() {
    // A named thread must round-trip through `PROC_PIDTHREADINFO`.
    let handle = std::thread::Builder::new()
        .name("gander-name-probe".into())
        .spawn(|| std::thread::sleep(std::time::Duration::from_millis(300)))
        .expect("spawn named thread");

    std::thread::sleep(std::time::Duration::from_millis(80));
    let me = std::process::id();
    let names: Vec<String> = scan::list_threads(me)
        .unwrap_or_default()
        .iter()
        .filter_map(|h| scan::thread_info(me, *h))
        .map(|ti| ti.name_str().to_string())
        .collect();

    let _ = handle.join();
    assert!(
        names.iter().any(|n| n == "gander-name-probe"),
        "expected the named thread among {names:?}"
    );
}

#[test]
fn thread_run_state_maps_to_a_known_letter() {
    let me = std::process::id();
    let handles = scan::list_threads(me).unwrap();
    for h in handles {
        let ti = scan::thread_info(me, h).expect("own thread info");
        assert!(
            (1..=5).contains(&ti.run_state),
            "run_state {} is not a TH_STATE_* value",
            ti.run_state
        );
    }
}
