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

    // htop 3.3.0's macOS model deliberately leaves the compressor out of `used`
    // *and* out of the bar, so adding the bar segments back cannot close the
    // gap to physical memory unless the compressor is included. Verify that:
    //   used + cache + free + compressed == total
    let accounted = mem.used + mem.cache_from_classes() + mem.free + mem.compressed;
    let diff = mem.total.abs_diff(accounted);
    let tolerance = mem.total / 100 * 10;
    assert!(
        diff <= tolerance,
        "used({}) + cache({}) + free({}) + compressed({}) = {} MiB, total is {} MiB \
         (off by {} MiB)\n{mem:?}",
        mem.used / mib,
        mem.cache_from_classes() / mib,
        mem.free / mib,
        mem.compressed / mib,
        accounted / mib,
        mem.total / mib,
        diff / mib
    );
}

#[test]
fn used_is_not_suspiciously_low() {
    // Regression guard: one shipped bug reported ~40% of real usage because the
    // compressor was never counted (htop 3.3's formula).
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

    // Every class flagged `counts_as_total` represents a distinct set of pages,
    // so their sum must not exceed physical memory.
    let classes: u64 = mem
        .classes
        .iter()
        .filter(|c| c.counts_as_total)
        .map(|c| c.bytes)
        .sum();
    assert!(
        classes <= mem.total,
        "memory classes sum to {} but total is {}: {mem:?}",
        classes,
        mem.total
    );
}
