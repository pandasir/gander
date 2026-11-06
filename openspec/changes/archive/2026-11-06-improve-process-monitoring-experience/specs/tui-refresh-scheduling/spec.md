# Spec Delta

## Purpose

Keep the terminal interface responsive to input and new measurements while avoiding redraw work when no visible state has changed.

## ADDED Requirements

### Requirement: Drawing follows visible invalidation

The interface SHALL draw initially and after new visible samples, relevant input, resize events, or scheduled changes to time-dependent content. It SHALL NOT redraw an unchanged idle view solely because a periodic poll elapsed. CPU and meter histories SHALL advance from measurements, not redraw frequency.

#### Scenario: Idle view with a one-second sampler
- **WHEN** the interface receives ten samples over ten seconds with no input, resize, or independently scheduled content changes
- **THEN** draw count is at most the initial frame plus one per adopted sample, rather than approximately 300 frames

#### Scenario: Search input between samples
- **WHEN** the user types before the next sampling deadline
- **THEN** the search/filter state is rendered promptly without waiting for a new measurement

#### Scenario: Time-dependent meter
- **WHEN** a visible clock requires an update without a new process sample
- **THEN** a render is scheduled for that content deadline without reintroducing unconditional frame-rate redraws

### Requirement: Input and sampling remain independently responsive

New completed samples SHALL notify the interface without blocking input on scanning. In deterministic event-loop tests, input-to-draw and completed-sample-to-draw delays SHALL each be at most 100 ms under no artificial rendering load. A burst of updates SHALL be coalesced safely to the latest snapshot. Quit and sampling-speed changes SHALL remain responsive at a 10000 ms sampling interval.

#### Scenario: Slow sampling with fast input
- **WHEN** sampling is configured at 10000 ms and the user moves selection
- **THEN** the changed selection can be drawn within 100 ms without forcing a scan

#### Scenario: Many samples arrive before a draw
- **WHEN** multiple completed samples arrive before the interface next renders
- **THEN** the newest snapshot can replace older pending snapshots without unbounded buffering

#### Scenario: Quit during sampler wait
- **WHEN** the user quits while the sampler is waiting for its next long interval
- **THEN** shutdown does not wait for the entire sampling interval and terminal state is restored

### Requirement: Performance claims use repeatable measurements

Performance acceptance SHALL record before/after release-build measurements with the same viewport, sampling period, thread visibility, process scope, and workload. After warm-up, at least three 30-second windows SHALL measure process CPU time, RSS, terminal output bytes, and interaction latency. Completion SHALL include deterministic redraw-count evidence and a reduction in median process CPU cost on the reference host, without latency regression.

#### Scenario: Validate the optimization
- **WHEN** the change is benchmarked at 160x32 with one-second sampling and a full process list
- **THEN** results include the baseline and changed revisions, workload/process count, repeated measurements, median comparison, and the redraw-count test result

#### Scenario: Compare against htop
- **WHEN** htop is included in the benchmark
- **THEN** equivalent settings and independent OS CPU accounting are used, and its displayed process CPU values are not assumed to be ground truth
