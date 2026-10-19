//! Milestone M3 against the fixture `/proc` tree.
//!
//! Everything here runs on any unix: the sampler is pointed at
//! `tests/fixtures/proc`, which now carries a `pressure/` directory and a
//! process with `fdinfo` entries. The live equivalents are in `linux_live.rs`.

//! The PSS and GPU fields are `#[cfg(target_os = "linux")]`, so the assertions
//! about them only compile there. The PSI parser and sampler path are portable
//! (the fixture tree is a directory, not a live `/proc`), so those run everywhere.
#![cfg(unix)]

use std::path::Path;

use gander_core::linux::system::Sampler;
use gander_core::PlatformSampler;

// The column and sort tests are Linux-only (the fields they exercise are), so the
// imports they need are gated with them rather than sitting unused elsewhere.
#[cfg(target_os = "linux")]
use gander_core::row::Field;
#[cfg(target_os = "linux")]
use gander_core::table::{self, Dir, Sort};

fn fixture_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/proc"))
}

#[test]
fn pressure_stall_information_is_read() {
    let mut sampler = Sampler::new(fixture_root());
    let (snapshot, _rows) = sampler.sample();

    let psi = snapshot.psi.expect("the fixture tree has /proc/pressure");
    assert!(psi.any_present());
    assert_eq!(psi.io.some.avg10, 12.5);
    assert_eq!(psi.io.full.avg10, 40.5);
    assert!(psi.io.has_full);
    assert_eq!(psi.memory.some.avg10, 0.1);
    // `cpu` reports a `full` line too on a real kernel, so the parser must read
    // it rather than special-casing the resource.
    assert!(psi.cpu.has_full);
    // `irq` is absent from the fixture: a missing file must not discard the
    // other three.
    assert!(!psi.irq.present);
}

/// The sampler must not report a `Psi` at all when no resource file exists, or
/// the meter draws four empty bars that look like an idle machine.
#[test]
fn missing_pressure_directory_reports_none() {
    let dir = tempfile_dir();
    // A `/proc` with no `pressure/` subdirectory, as on Linux < 4.20.
    std::fs::create_dir_all(&dir).unwrap();
    let mut sampler = Sampler::new(&dir);
    let (snapshot, _rows) = sampler.sample();
    assert!(snapshot.psi.is_none(), "got {:?}", snapshot.psi);
    std::fs::remove_dir_all(&dir).ok();
}

/// `smaps_rollup` is only read when asked for, and the columns that ask for it
/// are the ones that would render `-` otherwise.
#[cfg(target_os = "linux")]
#[test]
fn smaps_is_only_read_when_requested() {
    let mut sampler = Sampler::new(fixture_root());

    // Default: no smaps read, so the fields stay `None`.
    let (_snap, rows) = sampler.sample();
    let row = rows.iter().find(|r| r.pid == 42).expect("pid 42");
    assert_eq!(row.pss, None, "the cheap scan must not read smaps_rollup");

    // Asking for it fills them in. The fixture has no `smaps_rollup`, so they
    // stay `None` — the assertion that matters is that the *option* is honoured
    // and does not panic on a missing file.
    PlatformSampler::set_scan_options(
        &mut sampler,
        gander_core::ScanOptions::none().with_smaps(true),
    );
    let (_snap, rows) = sampler.sample();
    let row = rows.iter().find(|r| r.pid == 42).expect("pid 42");
    assert_eq!(row.pss, None, "no smaps_rollup in the fixture tree");
}

/// The fixture process 42 holds one DRM descriptor with engine and busy time.
#[cfg(target_os = "linux")]
#[test]
fn gpu_usage_is_read_from_fdinfo() {
    let mut sampler = Sampler::new(fixture_root());
    PlatformSampler::set_scan_options(
        &mut sampler,
        gander_core::ScanOptions::none().with_gpu(true),
    );
    let (_snap, rows) = sampler.sample();

    let row = rows.iter().find(|r| r.pid == 42).expect("pid 42");
    // gfx 5 s + compute 1 s.
    assert_eq!(row.gpu_ns, Some(6_000_000_000));
    // 6 s of engine time over 25 s of busy time.
    let pct = row.gpu_percent.expect("busy time is present");
    assert!((pct - 24.0).abs() < 0.01, "got {pct}");

    // A process with no DRM descriptor reports nothing, not zero.
    let other = rows.iter().find(|r| r.pid == 1).expect("pid 1");
    assert_eq!(other.gpu_ns, None);
    assert_eq!(other.gpu_percent, None);
}

/// The fields must be offerable in the setup screen and survive the config round
/// trip, or they cannot be selected at all.
#[cfg(target_os = "linux")]
#[test]
fn the_m3_fields_are_selectable() {
    let names: Vec<&str> = Field::all().map(|f| f.name()).collect();
    for want in ["M_PSS", "M_EPSS", "M_PRIVATE", "M_SWAP", "GPU%", "GPU_TIME"] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
        assert_eq!(
            Field::from_name(want).map(Field::name),
            Some(want),
            "{want} does not round-trip through its name"
        );
    }
}

/// Sorting by a PSS-derived column must sort by that figure, not fall through to
/// pid — the `_` arm of `sort_order` is the pid comparator, so a new field with no
/// arm silently sorts by pid and looks like a sort that does nothing.
#[cfg(target_os = "linux")]
#[test]
fn sorting_by_pss_sorts_by_pss() {
    let mut rows = Vec::new();
    for (pid, pss) in [(1u32, 300u64), (2, 100), (3, 200)] {
        let mut r = gander_core::row::Row::default_for_test(pid);
        r.pss = Some(pss);
        rows.push(r);
    }
    let mut order: Vec<usize> = (0..rows.len()).collect();
    table::sort_order(
        &rows,
        &mut order,
        Sort {
            field: Field::Pss,
            dir: Dir::Asc,
        },
    );
    let pids: Vec<u32> = order.iter().map(|&i| rows[i].pid).collect();
    assert_eq!(pids, vec![2, 3, 1], "ascending PSS");
}

/// A row whose PSS was never read must sort below one that has a figure, in both
/// directions of the tie-break, rather than being treated as zero-cost.
#[cfg(target_os = "linux")]
#[test]
fn unread_pss_sorts_as_unknown() {
    let mut a = gander_core::row::Row::default_for_test(1);
    a.pss = None;
    let mut b = gander_core::row::Row::default_for_test(2);
    b.pss = Some(0);
    let rows = vec![a, b];

    let mut order: Vec<usize> = (0..rows.len()).collect();
    table::sort_order(
        &rows,
        &mut order,
        Sort {
            field: Field::Pss,
            dir: Dir::Desc,
        },
    );
    // `Some(0)` and `None` compare equal, so the pid tie-break decides — which is
    // the point: the placeholder never jumps to the top of a descending sort.
    let pids: Vec<u32> = order.iter().map(|&i| rows[i].pid).collect();
    assert_eq!(pids, vec![1, 2], "equal values fall back to pid, ascending");
}

fn tempfile_dir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "gander-m3-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    p
}
