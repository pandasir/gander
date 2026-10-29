## Context

See `proposal.md` for motivation and `specs/linux-cpu-percent/spec.md` for the behavior contract. A design is warranted because the same arithmetic appears in Linux process sampling, Linux thread sampling, and a public tick-based table helper, while the TUI applies a separate normalization step.

Observed implementation:

- `linux::system::Sampler::read_rows` and `read_threads` divide task tick deltas by the aggregate delta from `cpu_delta[0]` without multiplying by CPU count.
- `table::derive_cpu` repeats that convention. No current repository call sites were found, but leaving its behavior inconsistent would preserve another route to the bug.
- `SystemSnapshot::cpu_count()` derives the logical CPU count from aggregate-plus-per-core samples. Linux builds the corresponding `cpu_delta` vector with the same shape, resetting it to zero when the CPU sample length changes.
- TUI `render_field` already divides `row.percent_cpu` by `snapshot.cpu_count().max(1)` for `PERCENT_NORM_CPU`. Batch text and JSON serialize the ordinary sampled value.
- macOS and FreeBSD calculate task CPU usage from elapsed wall-clock time, not this aggregate tick conversion.
- `docs/DESIGN.md` section 3.5 describes a one-core-equals-100 convention but prints the faulty aggregate formula. Existing fixture CPU counters do not advance, and the Linux live test checks only finiteness and non-negativity.

Pre-fix observations from Linux arm64, two logical CPUs, `cpu.max = max 100000`, gander 0.6.7, htop 3.4.1, 500 ms intervals:

| Workload | gander ordinary mean | htop ordinary mean |
| --- | ---: | ---: |
| Single-thread `yes` | 50.33% | 100.83% |
| One process with two compute threads | 99.35% | 198.75% |

The TUI independently displayed approximately 50% ordinary / 25% normalized for single-core load. htop must have `hide_userland_threads=1` to compare process totals; with threads expanded, its Python main-thread row was sleeping and was not a comparable measurement.

## Goals / Non-Goals

**Goals:**

- Establish one reusable aggregate-tick conversion with explicit CPU count.
- Preserve the boundary between ordinary sampled CPU% and normalized presentation.
- Make the numerical contract testable without busy loops, scheduler timing, or htop in CI.
- Keep Linux fixture tests runnable on macOS, as the existing path-parameterized backend permits.

**Non-Goals:**

- Do not change host CPU totals, guest accounting, host meter segmentation, or memory derivation.
- Do not revise new-process/PID-reuse baselines, thread-history ownership, or thread identity caching. The existing thread-history overwrite is a separate defect; this change must not be described as making end-to-end thread CPU sampling fully correct.
- Do not normalize to cgroup quotas, affinity masks, or CPU shares, and do not add a new configuration option or dependency.
- Do not modify macOS or FreeBSD sampler formulas or broadly refactor sampling.

## Decisions

### 1. Correct the tick conversion in core, not in the renderer

Use the equivalent of:

```text
ordinary_percent = 100 * task_delta * max(logical_cpu_count, 1) / aggregate_delta
normalized_percent = ordinary_percent / max(logical_cpu_count, 1)
```

Introduce a small crate-private pure helper near the existing tick-based derivation in `table.rs`, accepting integer deltas and CPU count. Route the Linux process calculation, Linux thread calculation, and `table::derive_cpu` through it. Keep counter subtraction saturating at the callers. Convert operands before multiplication, preferably using `f64` intermediate arithmetic and the existing `f32` output, so integer multiplication cannot overflow and an early `f32` conversion does not discard counter precision.

Return zero for a zero aggregate delta. Read Linux CPU count from the current aggregate-plus-per-core delta vector, with a one-core fallback; the table helper uses the corresponding snapshot count. Preserve first-sample and topology-reset behavior. Do not cap ordinary CPU% at 100.

Alternatives rejected:

- Multiplying only in TUI rendering would leave JSON and text output incorrect and would alter platforms that already supply correct ordinary CPU%.
- Rewriting Linux to use wall-clock intervals would change more of the sampler than necessary and would abandon the existing host tick interval.
- Copying corrected arithmetic into each caller would retain three implementations that can drift.

### 2. Preserve existing presentation and host accounting

Keep the TUI's single division for `PERCENT_NORM_CPU`, retain JSON keys and text formats, and leave sorting comparisons alone. Add rendering tests to prove that corrected ordinary values produce the expected ordinary and normalized columns. Any formatting edits must be limited to what those tests demonstrate is necessary.

Do not multiply `CpuTimes::total()`, change `SystemSnapshot::cpu_usage()`, or alter per-core counters to achieve task scaling: those values also drive host meters. The host aggregate remains a machine-wide fraction, while task CPU% uses a single-core basis.

### 3. Separate deterministic regression coverage from live acceptance

Numerical tests cover CPU counts 1, 2, and 8, fractional load, multi-core values above 100, zero/missing aggregate deltas, the aggregate-only fallback, and saturating task-counter subtraction. Exercise `table::derive_cpu` as well as the shared conversion.

Extend Linux fixture coverage with an isolated temporary `/proc` tree whose host and process counters advance across samples. Assert the actual sampler produces 100% for `task_delta=100`, `aggregate_delta=200`, and two cores, then 200% for a two-core process load. Also cover zero intervals and the existing topology-change reset. Never modify committed fixture files in place. Use fixtures with zero guest counters to isolate this correction.

Thread conversion tests cover the arithmetic for supplied valid deltas and verify the thread path uses the shared conversion. They are not acceptance claims about the independently broken thread-history lifecycle.

TUI tests use explicit snapshots to assert ordinary/normalized pairs of 100/50 and 200/100 on a two-core host, plus unchanged aggregate host usage. Existing macOS and FreeBSD coverage remains the compatibility check for their samplers.

Live acceptance repeats the two-core Docker experiment with the current source, no extra CPU quota, matching 500 ms refresh, identical target PIDs, and htop process aggregation. Discard startup frames and collect at least eight steady samples per tool. Under stable load, ordinary gander and htop means must differ by no more than 10%; expect roughly 100% for one core and 200% for two. Independently inspect TUI ordinary/normalized columns, JSON `cpu_percent`, and batch text. If the environment cannot sustain the workload, report that limitation rather than treating a timing-sensitive comparison as a deterministic unit test.

### 4. Correct documentation without expanding the CPU accounting audit

Update only the relevant formula and explanation in `docs/DESIGN.md` section 3.5 so the denominator is clearly aggregate host ticks and the numerator includes CPU count. Preserve unrelated historical notes and leave the separately identified guest and thread-history work for later changes.

## Risks / Trade-offs

- [Linux numeric values increase by N relative to previous releases] -> This restores the existing documented convention; retain schemas and field names and explain the correction in the formula documentation.
- [Scaling the wrong shared quantity changes host meters or other platforms] -> Keep conversion at task derivation call sites, test host usage separately, and leave platform sampler formulas untouched.
- [Thread-history defects obscure thread live results] -> Scope thread verification to valid-delta conversion and perform live acceptance on process totals with thread rows hidden.
- [Host scheduling or tick granularity causes transient values above ideal load] -> Use exact fixtures for CI and averages/tolerance for live comparisons; do not hide discrepancies with a 100% clamp.
- [Future CPU-count policy changes drift between sampler and UI] -> Use the sampled host topology and the same one-core fallback, not an independent `sysconf` or cgroup-derived count.

## Migration Plan

No data or configuration migration is required. Land the core correction, regression tests, and formula documentation together; run the existing workspace checks and the live comparison before release. Rollback consists of reverting that change set; it restores the old numerical behavior without requiring users to rewrite configuration or stored data.
