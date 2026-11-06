# process-metric-validity Specification

## Purpose

Make process resource measurements trustworthy by distinguishing real zero values from unavailable data across platforms, presentation surfaces, and sort orders.

## Requirements

### Requirement: Resource availability is explicit per field

Process and thread CPU percentage, memory percentage, RSS, virtual size, CPU time, priority, and thread count SHALL distinguish a measured value from unavailable data. Reasons SHALL distinguish `warming_up`, `permission_denied`, `unsupported`, and `read_error`. A failed read SHALL NOT invalidate independently readable identity or metrics, or produce a fabricated zero measurement.

#### Scenario: A process cannot be inspected
- **WHEN** a process is enumerated but the operating system denies its task-information read
- **THEN** its identity remains visible and affected metrics are marked `permission_denied`, rather than zero

#### Scenario: A measured idle process
- **WHEN** a readable process consumes no CPU over a valid interval
- **THEN** CPU is available and its measured value is numeric zero

#### Scenario: Partial information is available
- **WHEN** resource counters fail but UID, PID, command name, and nice are readable
- **THEN** readable facts remain available and only affected resource measurements are unavailable

#### Scenario: Counters reset
- **WHEN** counters regress, a task identity changes, or a baseline is reset
- **THEN** the affected interval measurement is unavailable until a new valid interval exists

### Requirement: Output surfaces preserve missing-value semantics

TUI and batch text SHALL display unavailable metrics as `-` and SHALL NOT draw value bars for them. JSON SHALL use `null` for unavailable existing numeric fields and add an `availability` object keyed by the corresponding exported metric names. Available entries SHALL have status `available`; unavailable entries SHALL identify their reason. Known zero SHALL remain numeric zero.

#### Scenario: Unreadable memory exported
- **WHEN** RSS and memory percentage cannot be read due to permissions
- **THEN** JSON has `rss: null` and `mem_percent: null`, with both entries marked `permission_denied` in `availability`

#### Scenario: CPU baseline is missing
- **WHEN** a readable row has no valid CPU interval because it has not yet established a sampling baseline
- **THEN** TUI ordinary and normalized CPU show `-` and JSON has `cpu_percent: null` with availability `warming_up`

#### Scenario: Genuine zero exported
- **WHEN** a metric was measured as zero
- **THEN** its JSON value is zero and its availability is `available`

### Requirement: Unknown measurements do not rank as measured values

Sorting on an affected metric SHALL place unavailable values after available values for both ascending and descending orders. Equal available values and unavailable groups SHALL have deterministic identity ordering. Memory and CPU value-bar scaling SHALL exclude unavailable values.

#### Scenario: Ascending RSS with unreadable processes
- **WHEN** rows contain measured RSS values zero and 100 MiB plus an unreadable RSS
- **THEN** ascending order is measured zero, measured 100 MiB, then unreadable

#### Scenario: Descending CPU with a new process
- **WHEN** rows contain available CPU 90 and 0 plus a warming-up process
- **THEN** descending order is 90, 0, then warming-up

### Requirement: macOS priority is independent of nice

On macOS, an available process scheduling priority SHALL reflect native task priority, not a copy of its nice adjustment. Thread rows SHALL use their native thread priority when readable. Nice SHALL retain its existing adjustment semantics. Unavailable priority SHALL follow the same missing-value contract as other metrics.

#### Scenario: Native priority differs from nice
- **WHEN** the system reports task priority 31 and nice zero
- **THEN** gander reports priority 31 and nice zero in the relevant columns and JSON fields

#### Scenario: Process priority is unavailable
- **WHEN** task information is denied but nice is independently readable
- **THEN** priority is unavailable without replacing it with nice, while nice remains available
