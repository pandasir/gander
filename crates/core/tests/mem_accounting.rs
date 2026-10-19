//! Live memory accounting checks.
//!
//! These assert the properties that actually broke the UI:
//!
//! 1. The meter must not read ~5 GB while `htop` reads ~13 GB on the same
//!    machine. The missing term was the compressor, which htop 3.4+ counts as
//!    used and htop 3.3 did not.
//! 2. The bar must not draw the compressor as *free* memory. An earlier revision
//!    derived the `free` segment as `total - used - cache`, so ~7 GB of
//!    compressor pages fell through into the free segment on a machine with
//!    0.3 GB actually free.
//!
//! Everything here reads the live machine, so the assertions are invariants
//! rather than exact numbers.

#![cfg(target_os = "macos")]

use gander_core::PlatformSampler;

#[test]
fn used_accounts_for_the_compressor() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let mem = &snapshot.mem;

    assert!(mem.total > 0, "total memory must be known");
    assert!(mem.is_exactly_partitioned(), "{mem:?}");

    // htop 3.4 prints `USED + COMPRESSED`, which is
    // `(internal - purgeable) + wired + compressed`. Nothing may be added twice:
    // `inactive` and `speculative` are already inside `internal`.
    let expected = mem
        .internal
        .saturating_sub(mem.purgeable)
        .saturating_add(mem.wired)
        .saturating_add(mem.compressed)
        .min(mem.total);
    assert_eq!(mem.used, expected, "{mem:?}");

    // The compressor is not optional: on any machine that has been up for a
    // while it is gigabytes, and omitting it is what produced the 5-vs-13 gap.
    assert!(
        mem.used >= mem.compressed,
        "used({}) must include the compressor({}): {mem:?}",
        mem.used,
        mem.compressed
    );
}

#[test]
fn bar_segments_partition_total() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let mem = &snapshot.mem;
    let mib = 1024 * 1024;

    // Each page is drawn exactly once, so the segments must add up to physical
    // memory. Anything else means a class leaked into another (the compressor
    // used to leak into `free`) or was double-counted.
    let classes: u64 = mem
        .classes
        .iter()
        .filter(|c| c.counts_as_total)
        .map(|c| c.bytes)
        .sum();
    assert_eq!(
        classes,
        mem.total,
        "segments sum to {} MiB, total is {} MiB: {mem:?}",
        classes / mib,
        mem.total / mib
    );
}

#[test]
fn free_segment_is_the_kernels_free_pages() {
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let mem = &snapshot.mem;

    let free_class = mem
        .classes
        .iter()
        .find(|c| c.label == "free")
        .expect("a free class is always present");

    // `MemInfo::free` is `vm_stat`'s free-count. The segment may be smaller (the
    // leftover goes to `cache`) but it must never be larger: that is the shape of
    // the old bug, where the compressor was reported as free.
    assert!(
        free_class.bytes <= mem.free,
        "free segment {} exceeds the kernel's free count {}: {mem:?}",
        free_class.bytes,
        mem.free
    );
}

#[test]
fn used_is_not_suspiciously_low() {
    // Regression guard: the shipped bug reported ~5 GB on a 16 GB machine with
    // ~13 GB in use, because the compressor was never counted.
    let mut sampler = gander_core::Sampler::default();
    let (snapshot, _rows) = sampler.sample();
    let mem = &snapshot.mem;

    let ratio = mem.used as f64 / mem.total as f64;
    assert!(
        ratio > 0.0,
        "used memory must be non-zero, got {ratio} on {mem:?}"
    );
    assert!(ratio <= 1.0, "used cannot exceed total: {mem:?}");

    // wired + compressor alone are already in `used`; if the figure were built
    // from `active + wired` alone it could fall below that floor.
    assert!(
        mem.used >= mem.wired.saturating_add(mem.compressed).min(mem.total),
        "used({}) is below wired + compressed: {mem:?}",
        mem.used
    );
}
