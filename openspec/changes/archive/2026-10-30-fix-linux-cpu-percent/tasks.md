## 1. Add Numerical Regression Coverage

- [x] 1.1 Add exact tick-based CPU derivation tests in `crates/core/src/table.rs` for CPU counts 1, 2, and 8, fractional load, values above 100%, missing/zero aggregate deltas, the one-core fallback, and saturating task-counter subtraction; verify the multi-core expectations fail against the original arithmetic with `cargo test -p gander-core --lib`.
- [x] 1.2 Extend `crates/core/tests/fixture_scan.rs` with an isolated temporary `/proc` tree and advancing host/process counters; verify the actual Linux sampler's two-core 100%/200% expectations fail before the fix, and include first-sample, zero-interval, and CPU-topology-reset cases without modifying committed fixtures.

## 2. Correct Conversion and Preserve Presentation

- [x] 2.1 Introduce one crate-private integer-delta-to-percentage helper in `crates/core/src/table.rs`, use floating-point intermediate multiplication with an explicit sampled CPU count and zero guard, and route `derive_cpu` plus both Linux process/thread calculation paths through it; verify the new core numerical and fixture tests pass with `cargo test -p gander-core`, including supplied-valid-delta thread conversion, and confirm all three call sites use the same helper.
- [x] 2.2 Add TUI field regression tests in `crates/tui/src/app.rs` for ordinary/normalized pairs of 100/50 and 200/100 on a two-core snapshot, including the existing one-core fallback; verify with `cargo test -p gander-tui` that rendering still divides exactly once and a half-busy host's aggregate usage remains 50%.
- [x] 2.3 Correct only the aggregate-tick CPU formula and its explanation in `docs/DESIGN.md` section 3.5; verify its two-core single-load example evaluates to 100% ordinary and 50% normalized and matches the implemented helper without changing unrelated accounting guidance.

## 3. Verify Integration and Compatibility

- [x] 3.1 Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` on macOS and Linux, plus `rustfmt --edition 2021 --check` on changed Rust files; verify the results are recorded and no macOS/FreeBSD sampler formulas, host meter calculations, configuration schemas, or JSON keys changed. Use the existing FreeBSD CI/cross-check when available and explicitly report any platform checks that cannot be run locally.
- [x] 3.2 Rebuild the current source inside a disposable two-core Linux container and compare gander JSON ordinary CPU% with htop 3.4.1 or newer on the same single-thread and two-compute-thread process workloads, using 500 ms refresh and `hide_userland_threads=1`; discard startup frames, collect at least eight steady samples per tool, and verify ordinary means agree within 10% rather than differing by a factor of two.
- [x] 3.3 In the same live environment, verify gander TUI ordinary/normalized columns approach 100/50 for one busy core and 200/100 for two, batch text ordinary CPU% agrees with JSON, and the host aggregate meter is not multiplied by core count; record the observations and clean up all diagnostic processes and the disposable container.
- [x] 3.4 Review the final diff against `specs/linux-cpu-percent/spec.md` and the design non-goals; verify this change contains only the conversion, regression tests, and formula documentation, and explicitly retain thread-history, identity-cache, guest-accounting, and quota-policy defects as separate work rather than claiming they were fixed.

## Verification Results

- Red phase: three new derivation tests and all three advancing-fixture tests failed against the original formula (including 50 vs 100 and 100 vs 200). Existing macOS core unit tests passed when run outside the process-restricted sandbox.
- Added 13 regression tests: eight core derivation/conversion cases, three advancing Linux fixture cases, and two TUI field-rendering cases.
- macOS: `cargo test --offline --workspace` passed, 447 tests passed and one helper test intentionally ignored. `cargo clippy --offline --workspace --all-targets -- -D warnings` passed.
- Linux arm64: the same workspace test command passed, 452 tests passed and one helper test intentionally ignored. The same strict Clippy command passed.
- FreeBSD: `cargo clippy --offline --workspace --target x86_64-unknown-freebsd --all-targets -- -D warnings` passed on the installed cross target. A live FreeBSD VM test run was not performed.
- `rustfmt --edition 2021 --check` passed for the four changed Rust files; existing nearby formatting in the two core source files was normalized to satisfy that check. `git diff --check` and `openspec validate fix-linux-cpu-percent --strict` passed.
- Live acceptance used gander 0.6.7 built from the modified source, htop 3.4.1, Linux 7.0.14-linuxkit arm64, two logical CPUs, no cgroup CPU quota (`cpu.max = max 100000`), and 500 ms updates. htop used process aggregation (`hide_userland_threads=1`). Startup readings were discarded; each workload had 10 steady JSON/text samples and 12 htop samples.

| Workload | JSON CPU% mean | Text CPU% mean | htop CPU% mean | TUI CPU% mean | TUI NCPU% mean | Host CPU% mean |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| One-core `yes` | 100.21 | 100.30 | 100.00 | 99.68 | 49.84 | 52.35 |
| Two-core process | 197.55 | 197.85 | 196.07 | 196.71 | 98.34 | 99.83 |

- Both JSON/htop mean comparisons passed the 10% tolerance; TUI ordinary/normalized columns, text output, and the unchanged host-wide scale also passed. The host's small additional one-core-case usage includes the concurrently running monitors.
- Raw acceptance samples are retained at `/private/tmp/gander-cpu-linux-acceptance.json`; workspace test logs are at `/private/tmp/gander-cpu-macos-tests.log` and `/private/tmp/gander-cpu-linux-tests.log`.
- All diagnostic workloads exited and the dedicated disposable container was stopped and automatically removed. Docker Desktop remains running; unrelated containers were not touched.
- Final review found only the scoped tick conversion, regression tests, formula documentation, and formatter-only adjustments. No macOS/FreeBSD sampler formulas, host meter accounting, configuration schemas, or JSON keys changed. Thread-history retention, identity-cache keys, guest accounting, and quota policy remain separate unresolved work.
