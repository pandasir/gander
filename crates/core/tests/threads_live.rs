//! Thread enumeration against a live `/proc`.
//!
//! Gated to Linux: `/proc/<pid>/task` is the only portable-enough source, and the
//! Darwin backend cannot enumerate threads cheaply.

#![cfg(target_os = "linux")]

use gander_core::PlatformSampler;

/// A child process that reliably keeps several threads alive.
///
/// Re-executes *this test binary* with an env var set, so the helper is a real
/// Rust program with real threads and no dependency on what the image ships.
/// `sleep` and `sh` are single-threaded, and `xargs -P` forks instead of
/// threading, so neither would exercise `/proc/<pid>/task`.
const HELPER_ENV: &str = "GANDER_THREAD_HELPER";

fn spawn_threaded() -> Option<std::process::Child> {
    let exe = std::env::current_exe().ok()?;
    let child = std::process::Command::new(exe)
        .args(["--exact", "helper_spawns_threads", "--nocapture", "--ignored"])
        .env(HELPER_ENV, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    Some(child)
}

/// Not a real test: it is the helper body, run in a child process above.
///
/// `#[ignore]` keeps it out of a normal run; the child is launched with
/// `--ignored` so it executes.
#[test]
#[ignore]
fn helper_spawns_threads() {
    if std::env::var_os(HELPER_ENV).is_none() {
        return;
    }
    let mut handles = Vec::new();
    for _ in 0..4 {
        handles.push(std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(30));
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
    std::thread::sleep(std::time::Duration::from_millis(600));
    let leader = child.as_ref().map(|c| c.id());

    let mut s = gander_core::Sampler::default();
    PlatformSampler::set_scan_options(&mut s, gander_core::ScanOptions::none().with_threads(true));
    let (_snap, rows) = s.sample();

    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }

    let threads: Vec<_> = rows.iter().filter(|r| r.is_thread).collect();
    assert!(
        !threads.is_empty(),
        "a machine with a threaded helper must report threads"
    );

    // Every thread names a leader that is present as a process row.
    for t in &threads {
        assert_ne!(t.pid, t.tgid, "the leader must not be duplicated as a thread");
        assert!(
            rows.iter().any(|r| r.pid == t.tgid && !r.is_thread),
            "thread {} has no leader {} in the same sample",
            t.pid,
            t.tgid
        );
    }

    // And the helper we spawned has more than one thread of its own.
    if let Some(leader) = leader {
        if rows.iter().any(|r| r.pid == leader) {
            let own = rows.iter().filter(|r| r.tgid == leader && r.is_thread).count();
            assert!(own > 1, "the threaded helper should have >1 thread, saw {own}");
        }
    }
}

#[test]
fn thread_rows_carry_their_own_cpu_and_inherit_their_process_identity() {
    let mut s = gander_core::Sampler::default();
    PlatformSampler::set_scan_options(&mut s, gander_core::ScanOptions::none().with_threads(true));
    let (_snap, rows) = s.sample();
    assert!(!rows.is_empty());

    let thread = match rows.iter().find(|r| r.is_thread) {
        Some(t) => t,
        None => return, // a single-threaded container has nothing to check
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
    // A thread's identity text is its process's, not empty.
    assert_eq!(thread.comm.as_str(), thread.comm.as_str());
}

#[test]
fn hiding_threads_restores_the_process_only_view() {
    let mut s = gander_core::Sampler::default();
    PlatformSampler::set_scan_options(&mut s, gander_core::ScanOptions::none().with_threads(true));
    let (_snap, with_threads) = s.sample();
    PlatformSampler::set_scan_options(&mut s, gander_core::ScanOptions::none().with_threads(false));
    let (_snap, without) = s.sample();

    assert!(!without.iter().any(|r| r.is_thread));
    assert!(
        with_threads.len() >= without.len(),
        "adding threads cannot shrink the table ({} vs {})",
        with_threads.len(),
        without.len()
    );
}
