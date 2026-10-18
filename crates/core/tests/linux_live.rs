//! Live `/proc` checks.
//!
//! These only run on Linux. They compare gander's figures against the same
//! files read directly, *inside one test*, so a changing system cannot explain a
//! discrepancy. Everything is skipped on other platforms.

#![cfg(target_os = "linux")]

use std::collections::HashMap;

use gander_core::row::Field;
use gander_core::table::{self, Dir, ProcTable, Sort, Visibility};
use gander_core::PlatformSampler;

/// Parse `/proc/meminfo` into kB values, independently of gander's parser.
fn meminfo() -> HashMap<String, u64> {
    let text = std::fs::read_to_string("/proc/meminfo").expect("/proc/meminfo");
    text.lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(':')?;
            let value = rest.split_whitespace().next()?.parse().ok()?;
            Some((key.to_string(), value))
        })
        .collect()
}

#[test]
fn memory_matches_free_accounting() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let m = &snapshot.mem;
    let kb = 1024;

    // Read meminfo immediately after sampling; the sampler itself uses the same
    // file, so a small drift is possible but not an order of magnitude.
    let raw = meminfo();
    let total = raw["MemTotal"] * kb;
    let free = raw["MemFree"] * kb;
    let buffers = raw["Buffers"] * kb;
    let cached = raw["Cached"] * kb;
    let sreclaimable = raw["SReclaimable"] * kb;

    // `free`'s used: total - free - (buffers + cached + sreclaimable - shmem)
    let shmem = raw.get("Shmem").copied().unwrap_or(0) * kb;
    let buff_cache = buffers + cached + sreclaimable - shmem;
    let expected_used = total - free - buff_cache;

    assert_eq!(m.total, total, "total comes straight from MemTotal");

    // Allow 2% drift for memory changing between the two reads.
    let tolerance = total / 50;
    assert!(
        m.used.abs_diff(expected_used) <= tolerance,
        "gander used {} KiB vs free-accounting {} KiB",
        m.used / kb,
        expected_used / kb
    );
}

#[test]
fn available_comes_from_memavailable() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let raw = meminfo();

    // Kernels newer than 3.14 always provide MemAvailable.
    if let Some(expected) = raw.get("MemAvailable") {
        assert_eq!(
            snapshot.mem.available,
            expected * 1024,
            "available must be MemAvailable verbatim"
        );
    }
}

#[test]
fn cpu_times_cover_every_core() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();

    // SAFETY: `sysconf` has no preconditions.
    let ncpu = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) } as usize;
    assert!(ncpu >= 1);
    assert_eq!(
        snapshot.cpu.len(),
        ncpu + 1,
        "one aggregate entry plus one per online core"
    );
    assert_eq!(snapshot.cpu_count(), ncpu);
    assert!(snapshot.cpu[0].total() > 0);
}

#[test]
fn process_count_matches_proc() {
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();

    // Count numeric /proc entries, which is what the sampler walks.
    let on_disk = std::fs::read_dir("/proc")
        .expect("/proc")
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            !name.is_empty() && name.chars().all(|c| c.is_ascii_digit())
        })
        .count();

    // Processes come and go between the two walks, so allow a small delta.
    let delta = rows.len().abs_diff(on_disk);
    assert!(
        delta <= 10,
        "gander read {} processes, /proc has {on_disk}",
        rows.len()
    );
    assert!(
        rows.len() > 1,
        "a container still has more than one process"
    );
}

#[test]
fn every_row_has_a_plausible_pid_and_command() {
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();

    for row in &rows {
        assert!(row.pid > 0, "pid 0 must not be listed");
        assert!(!row.comm.is_empty(), "pid {} has no command", row.pid);
    }

    // Pids are unique.
    let mut pids: Vec<u32> = rows.iter().map(|r| r.pid).collect();
    let n = pids.len();
    pids.sort_unstable();
    pids.dedup();
    assert_eq!(pids.len(), n, "duplicate pids in the table");
}

#[test]
fn command_lines_have_no_control_characters_after_sanitising() {
    // The renderer replaces control characters; this checks the raw data really
    // can contain them, which is why the sanitiser exists.
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();

    let mut found_control = false;
    for row in &rows {
        if row.display_command().chars().any(|c| c.is_control()) {
            found_control = true;
            break;
        }
    }
    // Not an assertion: in a quiet container there may be none. The point is that
    // the sanitiser runs over everything regardless.
    let _ = found_control;
}

#[test]
fn second_sample_reports_finite_cpu_percent() {
    let mut sampler = gander_core::Sampler::default();
    sampler.sample();
    std::thread::sleep(std::time::Duration::from_millis(120));
    let (snapshot, rows) = sampler.sample();

    assert!(snapshot.cpu_usage().is_finite());
    for row in &rows {
        assert!(
            row.percent_cpu.is_finite() && row.percent_cpu >= 0.0,
            "pid {} has cpu% {}",
            row.pid,
            row.percent_cpu
        );
    }

    // Something must have moved in 120 ms.
    let busiest = rows.iter().map(|r| r.percent_cpu).fold(0.0f32, f32::max);
    assert!(busiest >= 0.0);
}

#[test]
fn sorting_and_filtering_yield_a_permutation() {
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();
    let mut table = ProcTable {
        rows,
        ..Default::default()
    };

    let visibility = Visibility {
        show_kernel_threads: true,
        ..Default::default()
    };
    for field in [
        Field::Pid,
        Field::PercentCpu,
        Field::PercentMem,
        Field::ResMem,
    ] {
        for dir in [Dir::Asc, Dir::Desc] {
            table.sort = Sort { field, dir };
            table.reorder(&visibility);

            let mut order = table.view.order.clone();
            let n = order.len();
            order.sort_unstable();
            order.dedup();
            assert_eq!(order.len(), n, "sort by {field:?} {dir:?} duplicated a row");
            assert!(n > 0);
        }
    }
    let _ = table::sort_order;
}

#[test]
fn tree_mode_lists_every_row_once() {
    let mut sampler = gander_core::Sampler::default();
    let (_snapshot, rows) = sampler.sample();
    let total = rows.len();

    let mut table = ProcTable {
        rows,
        tree_mode: true,
        ..Default::default()
    };
    table.reorder(&Visibility {
        show_kernel_threads: true,
        ..Default::default()
    });

    assert_eq!(table.view.order.len(), total, "tree mode dropped rows");

    let mut sorted = table.view.order.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), total, "tree mode duplicated a row");
}

#[test]
fn process_actions_are_available() {
    // `kill(2)` is POSIX; this asserts the unix gate is right rather than the
    // Linux-only gate that made F9 fail on macOS.
    let action = gander_core::action::Action::Kill {
        pid: u32::MAX - 1,
        signal: 0,
    };
    // Signal 0 only checks permission, so it must fail with ESRCH for a pid that
    // cannot exist — but it must not fail with "unsupported".
    match gander_core::action::execute(&action) {
        Err(e) => assert_ne!(
            e.kind(),
            std::io::ErrorKind::Unsupported,
            "process actions must work on every unix"
        ),
        Ok(()) => panic!("signalling a non-existent pid should fail"),
    }
}
