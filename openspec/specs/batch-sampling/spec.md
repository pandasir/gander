# batch-sampling Specification

## Purpose

Provide meaningful process measurements from the first batch output while preserving predictable sampling counts and the existing script-facing framing.

## Requirements

### Requirement: Batch output starts after a measurement interval

Text and JSON batch execution SHALL take an unreported baseline, wait one configured clamped sampling interval, and then emit the first requested sample. CPU values SHALL only be reported for valid intervals belonging to the same process or thread identity. Processes first encountered after the baseline SHALL have unavailable CPU rather than measured zero.

#### Scenario: Single JSON sample of a busy process
- **WHEN** a continuously busy single-core process exists throughout the baseline and measured interval and the user runs `--json`
- **THEN** one JSON array is emitted with measured ordinary CPU approximately 100 percent under a sustained workload, rather than an unmeasured zero

#### Scenario: New process appears after baseline
- **WHEN** a process is first encountered in the first emitted sample
- **THEN** that row remains present with `cpu_percent: null` and availability `warming_up`

#### Scenario: Sampling interval is clamped
- **WHEN** the requested delay is below 100 ms or above 10000 ms
- **THEN** warm-up uses the same 100 ms to 10000 ms clamp as normal sampling

### Requirement: Iterations count emitted samples

The `-n N` count SHALL count emitted snapshots, not baseline scans. A single sampler SHALL retain interval history throughout the invocation. Existing handling of a zero iteration request as one emitted sample SHALL remain unchanged.

#### Scenario: Two requested outputs
- **WHEN** the user requests `--json -n 2`
- **THEN** two measured snapshots are emitted after the unreported baseline, with no third output for warm-up

#### Scenario: Process identity is reused
- **WHEN** a PID is reused between emitted samples
- **THEN** the replacement process does not inherit its predecessor's CPU history

### Requirement: Batch framing remains compatible

A single JSON snapshot SHALL remain a top-level array of process objects; repeated snapshots SHALL retain the existing sequence-of-arrays framing. Existing field names, filter semantics, limits, escaping, and broken-pipe success behavior SHALL remain intact. Nullable metrics and additive availability metadata SHALL be documented as a value-contract change.

#### Scenario: Single snapshot consumer
- **WHEN** a user parses one `--json --limit 3` output as JSON
- **THEN** the result is an array containing at most three rows, not a new envelope object

#### Scenario: Repeated snapshots
- **WHEN** a user requests multiple JSON snapshots
- **THEN** each emitted snapshot remains a complete array in the existing stream format

### Requirement: Batch sorting respects metric validity

Filtering and sorting SHALL operate on the measured snapshot before applying the limit. Unknown sort values SHALL follow measured values in either direction, using stable identity tie-breaking instead of treating unknown values as zero.

#### Scenario: CPU top list after warm-up
- **WHEN** a sustained busy process and an idle process are both measurable and the user requests CPU descending with limit one
- **THEN** the busy process is emitted ahead of idle or unmeasured processes
