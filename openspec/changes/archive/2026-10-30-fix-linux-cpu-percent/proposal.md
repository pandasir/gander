## Why

Linux process CPU percentages use the sum of all cores as their denominator but are exposed as htop-style percentages where one saturated core is 100%. A live two-core Docker comparison confirmed approximately 50% versus htop's 100% for one busy core, and 100% versus htop's 200% for two busy cores; the TUI's normalized column divides this already-normalized result again.

## What Changes

- Correct the conversion from Linux task tick deltas and aggregate CPU tick deltas to a percentage with one core equal to 100%, for both process and thread calculation paths.
- Keep normalization at the presentation boundary: `PERCENT_NORM_CPU` divides the corrected percentage by the sampled logical CPU count exactly once. JSON `cpu_percent` and text output retain the ordinary, non-normalized convention.
- Align the existing tick-based `table::derive_cpu` helper with the same convention, without changing macOS or FreeBSD sampler calculations.
- Add deterministic numerical regression tests and repeat the Linux gander/htop comparison with matched process aggregation settings.
- Correct the conflicting CPU formula in `docs/DESIGN.md`.

## Capabilities

### New Capabilities

- `linux-cpu-percent`: Establish the previously undocumented OpenSpec contract for Linux ordinary and normalized task CPU percentages, safe zero-delta handling, and consistent output surfaces. This specifies an existing intended behavior rather than introducing a new user-facing feature.

### Modified Capabilities

None. The main OpenSpec specification directory contains no existing capabilities.

## Impact

- Core: `crates/core/src/linux/system.rs` and `crates/core/src/table.rs`; their tick-based calculations will share a consistent conversion.
- Tests: core numerical and Linux fixture coverage, plus TUI ordinary/normalized field rendering coverage. Batch output uses the corrected sampled value without changing its schema.
- Documentation: `docs/DESIGN.md` section 3.5, whose current formula contradicts its stated single-core convention.
- Compatibility: Linux numerical CPU values increase by the sampled CPU count relative to the faulty calculation; field names, configuration, JSON structure, sorting behavior, and dependencies remain unchanged. No changes to macOS or FreeBSD sampler formulas or host CPU meter accounting.
- Out of scope: guest-time accounting, thread-history retention, thread identity-cache keys, process metadata freshness, cgroup quota/affinity normalization, and unrelated rendering or performance refactors. Thread conversion is corrected for a supplied valid delta; the separate thread-history defect is not resolved by this change.
