# Spec Delta

## MODIFIED Requirements

### Requirement: Missing or zero sampling intervals are safe

Without an aggregate CPU tick delta, or when that delta is zero, CPU percentage conversion SHALL produce a safe internal numeric zero rather than NaN, infinity, or a division error, but the interval SHALL be marked unavailable and SHALL NOT be presented as measured zero. If an aggregate delta exists without per-core entries, conversion and normalization SHALL use a one-core fallback. Initial and CPU-topology-reset samples SHALL remain unavailable until a valid interval exists.

#### Scenario: First host sample
- **WHEN** the sampler has no previous host counters
- **THEN** process CPU percentages retain safe internal zero values with unavailable interval status, render as `-`, and export as JSON null if emitted

#### Scenario: No aggregate progress
- **WHEN** the aggregate CPU tick delta is zero
- **THEN** the calculated internal CPU percentage is zero even if a task counter has advanced, and the interval is unavailable rather than a measured zero

#### Scenario: Aggregate-only CPU sample
- **WHEN** an aggregate-only sample advances by 100 ticks and a task advances by 50 ticks
- **THEN** its ordinary CPU percentage is an available 50 using the one-core fallback

#### Scenario: CPU topology changes
- **WHEN** a change in the sampled CPU topology causes the existing host interval to be reset
- **THEN** task CPU percentages are unavailable for that reset sample and resume the corrected convention once a valid interval is available

### Requirement: Output surfaces preserve the ordinary percentage convention

For the same valid sampled process, TUI ordinary CPU%, batch text CPU%, and JSON `cpu_percent` SHALL expose the same ordinary percentage convention, allowing existing rounding differences. Invalid intervals SHALL render as `-` and export as null with additive availability metadata. Configuration field names, JSON metric names, and top-level array framing SHALL remain unchanged; consumers MUST support nullable metrics.

#### Scenario: Single-core load exported as JSON
- **WHEN** a Linux process occupies one core on a two-core host over a valid sampling interval
- **THEN** its JSON `cpu_percent` is approximately 100, consistent with ordinary TUI and text CPU% rather than approximately 50

#### Scenario: Existing configuration remains valid
- **WHEN** a user starts gander with an existing configuration containing `PERCENT_CPU` and `PERCENT_NORM_CPU`
- **THEN** both fields remain available without a configuration migration

#### Scenario: An unmeasured row is exported
- **WHEN** a process lacks a valid interval in an emitted snapshot
- **THEN** `cpu_percent` is null and its availability reason is present without renaming the field or changing the snapshot array framing
