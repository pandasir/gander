//! Scan benchmarks.
//!
//! Run with: `cargo bench -p gander-core --bench scan`
//!
//! These measure the numbers in `docs/DESIGN.md` §9 so decisions are made from
//! data, not intuition. The most honest metric is syscall count:
//!
//! ```sh
//! strace -c -f -e trace=openat,read ./target/release/gander -n 5
//! ```
//!
//! The bench uses the fixture tree so results are stable across machines, plus a
//! live-`/proc` case to see real-world cost.

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use gander_core::linux::system::Sampler;

fn bench(name: &str, iterations: u32, mut f: impl FnMut()) {
    // Warm up so page/dentry caches are hot, matching steady-state behaviour.
    f();
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    let per = elapsed / iterations;
    println!("{name:<34} {per:>12?} per iteration ({iterations} iterations)");
}

fn main() {
    let fixture = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/proc"));

    if fixture.exists() {
        let mut sampler = Sampler::new(fixture);
        bench("fixture scan (4 procs)", 10_000, || {
            black_box(sampler.sample());
        });
    }

    let live = Path::new("/proc");
    if live.exists() {
        let mut sampler = Sampler::new(live);

        sampler.use_rayon = false;
        bench("live /proc scan (sequential)", 200, || {
            black_box(sampler.sample());
        });

        sampler.use_rayon = true;
        bench("live /proc scan (rayon)", 200, || {
            black_box(sampler.sample());
        });
    } else {
        println!("live /proc scan: skipped (no /proc on this host)");
    }
}
