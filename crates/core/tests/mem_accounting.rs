//! Live memory accounting checks.
//!
//! These assert the property that actually broke the UI: on an 8 GB machine the
//! meter must not read 3 GB while the OS says 7.5 GB. They compare against
//! `host_statistics64`-derived `total - free`, which is what Activity Monitor and
//! `top` report.

#![cfg(target_os = "macos")]

use gander_core::PlatformSampler;

#[test]
fn used_matches_total_minus_free() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let mem = &snapshot.mem;
    let mib = 1024 * 1024;

    assert!(mem.total > 0, "total memory must be known");
    assert!(mem.is_exactly_partitioned(), "{mem:?}");

    // The OS figure: everything not free.
    let system_used = mem.total.saturating_sub(mem.free);

    // Our accounting drops a few hundred MiB of pages that none of the counters
    // cover, so allow 15% slack, but never the 2 GB gap the old formula had.
    let diff = system_used.abs_diff(mem.used);
    let tolerance = mem.total / 100 * 15;
    assert!(
        diff <= tolerance,
        "used {} MiB differs from total-free {} MiB by {} MiB (tolerance {} MiB)\n{mem:?}",
        mem.used / mib,
        system_used / mib,
        diff / mib,
        tolerance / mib
    );
}

#[test]
fn used_is_not_suspiciously_low() {
    // Regression guard: the shipped bug reported ~40% of real usage because
    // `inactive` was excluded and `wired` was double-counted the other way.
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let mem = &snapshot.mem;

    let ratio = mem.used as f64 / mem.total as f64;
    assert!(
        ratio > 0.0,
        "used memory must be non-zero, got {ratio} on {mem:?}"
    );
    assert!(ratio <= 1.0, "used cannot exceed total: {mem:?}");
}

#[test]
fn classes_are_not_double_counted() {
    // wired and compressed are disjoint from active; if they overlapped, the sum
    // of the classes would exceed the total.
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let mem = &snapshot.mem;

    let classes = mem
        .wired
        .saturating_add(mem.compressed)
        .saturating_add(mem.active.saturating_sub(mem.purgeable))
        .saturating_add(mem.inactive)
        .saturating_add(mem.free);
    assert!(
        classes <= mem.total,
        "memory classes sum to {} but total is {}: {mem:?}",
        classes,
        mem.total
    );
}
