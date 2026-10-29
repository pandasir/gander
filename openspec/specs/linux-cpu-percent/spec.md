# linux-cpu-percent Specification

## Purpose

Define Linux task CPU percentages so users can compare gander with htop and interpret ordinary and machine-normalized values consistently across the terminal interface and batch exports.

## Requirements

### Requirement: Ordinary CPU percentage uses one core as 100 percent

Given a valid task CPU tick delta, an aggregate CPU tick delta over the same interval, and N sampled logical CPUs, Linux task CPU percentage SHALL equal `100 * N * task_delta / aggregate_delta`. N SHALL be the number of logical CPUs represented in the host CPU sample, not a task's affinity mask or container CPU quota. The same conversion convention SHALL apply to process rows and thread rows supplied with valid deltas. Ordinary process CPU percentage MUST support values above 100 when a process uses multiple cores.

#### Scenario: One occupied core on a two-core host
- **WHEN** a process advances by 100 CPU ticks while a two-core host advances by 200 aggregate CPU ticks
- **THEN** its ordinary CPU percentage is 100, within floating-point rounding tolerance

#### Scenario: Two occupied cores in one process
- **WHEN** a process advances by 200 CPU ticks while a two-core host advances by 200 aggregate CPU ticks
- **THEN** its ordinary CPU percentage is 200 rather than being capped at 100

#### Scenario: CPU count does not reduce the meaning of one core
- **WHEN** a process advances by 100 CPU ticks while an eight-core host advances by 800 aggregate CPU ticks
- **THEN** its ordinary CPU percentage is 100

#### Scenario: Thread conversion uses the same scale
- **WHEN** a thread has a valid 50-tick interval delta while a two-core host advances by 200 aggregate ticks
- **THEN** the conversion yields an ordinary CPU percentage of 50

### Requirement: Normalized CPU percentage divides by CPU count exactly once

The TUI `PERCENT_NORM_CPU` field SHALL display the ordinary task CPU percentage divided by the sampled logical CPU count exactly once. The ordinary `PERCENT_CPU` field SHALL display the task value without machine normalization.

#### Scenario: Single-core load displayed in both columns
- **WHEN** a process has an ordinary CPU percentage of 100 on a two-core host
- **THEN** `PERCENT_CPU` displays 100 percent and `PERCENT_NORM_CPU` displays 50 percent, subject to existing display precision

#### Scenario: Whole-machine load displayed in both columns
- **WHEN** a process has an ordinary CPU percentage of 200 on a two-core host
- **THEN** `PERCENT_CPU` displays 200 percent and `PERCENT_NORM_CPU` displays 100 percent

### Requirement: Missing or zero sampling intervals are safe

Without an aggregate CPU tick delta, or when that delta is zero, CPU percentage conversion SHALL produce zero rather than NaN, infinity, or a division error. If an aggregate delta exists without per-core entries, conversion and normalization SHALL use a one-core fallback. The initial host sample and the existing CPU-topology-change baseline reset SHALL continue to report zero CPU percentage until a valid interval is available.

#### Scenario: First host sample
- **WHEN** the sampler has no previous host counters
- **THEN** process CPU percentages are zero

#### Scenario: No aggregate progress
- **WHEN** the aggregate CPU tick delta is zero
- **THEN** the calculated CPU percentage is zero even if a task counter has advanced

#### Scenario: Aggregate-only CPU sample
- **WHEN** an aggregate-only sample advances by 100 ticks and a task advances by 50 ticks
- **THEN** its ordinary CPU percentage is 50 using the one-core fallback

#### Scenario: CPU topology changes
- **WHEN** a change in the sampled CPU topology causes the existing host interval to be reset
- **THEN** task CPU percentages are zero for that reset sample and resume the corrected convention once a valid interval is available

### Requirement: Output surfaces preserve the ordinary percentage convention

For the same sampled process, TUI ordinary CPU%, batch text CPU%, and JSON `cpu_percent` SHALL expose the same ordinary percentage convention, allowing their existing rounding differences. The fix MUST NOT rename configuration fields or JSON keys, add required configuration, or change the JSON structure.

#### Scenario: Single-core load exported as JSON
- **WHEN** a Linux process occupies one core on a two-core host over a valid sampling interval
- **THEN** its JSON `cpu_percent` is approximately 100, consistent with ordinary TUI and text CPU% rather than approximately 50

#### Scenario: Existing configuration remains valid
- **WHEN** a user starts gander with an existing configuration containing `PERCENT_CPU` and `PERCENT_NORM_CPU`
- **THEN** both fields remain available without a configuration migration

### Requirement: Other platform sampling and host CPU meters retain their conventions

The correction SHALL preserve macOS and FreeBSD sampler CPU percentage conventions and SHALL NOT rescale host-wide CPU usage meters using the process percentage correction.

#### Scenario: macOS wall-clock calculation
- **WHEN** a macOS process consumes one CPU-second over a one-second interval
- **THEN** its ordinary CPU percentage remains approximately 100

#### Scenario: Linux host meter on a half-busy machine
- **WHEN** one core is busy and one is idle over a two-core host interval
- **THEN** the aggregate host CPU usage remains approximately 50 percent while the busy process has approximately 100 ordinary CPU percent
