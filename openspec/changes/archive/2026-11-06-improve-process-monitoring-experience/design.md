# Design

## Context

See `proposal.md` for the ten agreed improvements. This design is necessary because they cross the core sampling model, JSON contract, persistent configuration, process identity handling, and terminal event loop.

Observed implementation at `6fb015b`:

- `Row` stores scalar resource values without validity for basic CPU/memory fields; optional Linux metrics already distinguish missing data. Darwin task-info failures leave zero scalars, and process priority is assigned from nice even though `TaskInfo.priority` is available.
- `batch::run` emits its first scan immediately. CPU sampling needs a prior scan. JSON uses a manual formatter; a snapshot is an array, and repeated snapshots are separate arrays.
- `linux-cpu-percent` explicitly requires initial zero presentation and forbids JSON structure changes. Its delta deliberately changes these two contracts, not the valid-interval arithmetic.
- `App::cycle_sort` only rotates columns and resets default direction. `I` changes meter modes; `+/-` change sampling speed regardless of tree mode. Tree construction is already cycle-safe and separates process/thread identity, but has no fold state.
- `app_safety_tests.rs` already protects selection identity, PID reuse, tags, and frozen signal targets. These protections must survive the new navigation paths.
- The inspector calculates a four-row command limit but does not enforce it. The command modal pre-wraps at 80 characters, then renders without wrapping in the center panel. `draw_panels` leaves sidebar tracks reserved for secondary screens.
- Config defaults have one Main tab in PID order. Config serialization is atomic, and the htop importer constructs its own tab list from a default config.
- `runtime.rs` draws every approximately 33 ms. Sampling already runs off-thread with a latest-snapshot `ArcSwap` slot and interruptible sleeping.

The macOS live comparison found accurate steady-state gander process CPU against independent OS accounting, not against htop's displayed CPU. The observed 1.88% versus 0.39% monitor CPU costs were one eight-second window and are only motivation for a controlled benchmark.

## Goals / Non-Goals

**Goals:**

- Carry validity alongside raw values without putting presentation or serialization dependencies into core.
- Reuse the existing table, tree, setup, history, and process-action machinery; add small screen variants and explicit geometry rather than replacing the UI architecture.
- Define deterministic tests for contracts that would otherwise depend on permissions, scheduler timing, or terminal size.
- Keep old explicit configurations usable and document intentional JSON/key compatibility changes.

**Non-Goals:**

- No daemon, automatic sudo, tracing/backtrace implementation, new per-process I/O family, or change to host memory accounting.
- No changes to valid CPU scaling, normalized CPU definitions, CPU quotas, affinity normalization, or unrelated thread identity/history algorithms.
- No versioned JSON envelope, new repeated-output framing, configurable keymap framework, or migration of every optional Linux metric to a new schema.
- No promise to outperform htop on every machine; require a repeatable improvement over the same gander baseline.

## Decisions

### 1. Keep scalar storage and add compact per-field availability

Add a small core metric identifier/status representation next to `Row`. Statuses are `available`, `warming_up`, `permission_denied`, `unsupported`, and `read_error`. Cover ordinary/normalized CPU, resident and virtual memory, memory percentage, CPU time, scheduling priority, and thread count. Normalized CPU inherits ordinary validity; memory percentage inherits readable RSS and a valid total-memory denominator. Identity, UID, nice, and independently readable fields are not invalidated by a task-info failure.

Retain numeric storage and safe internal zero baselines so existing arithmetic cannot produce NaN or infinity. Production row construction must explicitly set validity from read outcomes rather than infer it from nonzero values. Test row constructors can create explicitly available fixture values to keep intentional numerical tests readable. Do not silently default production rows to all-available.

At each backend, preserve the information needed to distinguish denied, unsupported, and failed reads. Where an existing `Option` helper erases errno, add a narrowly scoped checked result for the affected reads rather than refactoring unrelated scanning. A first interval, a counter regression, a changed identity, or a host baseline reset invalidates only the interval metric. Cumulative CPU time can remain available on the same row. Thread-inherited memory also inherits its source validity.

Compare validity before reversing the numeric comparison so unknown values stay last in both sort directions. Keep deterministic identity tie-breaking, including start time and thread identity where needed. Exclude unavailable values from value-bar maxima. All rendering and batch paths consult the same validity policy.

Alternative rejected: replacing every raw scalar with `Option` would cause wider arithmetic churn and still not explain why a measurement is absent. A single row-level readable flag would lose partial-read information.

### 2. Warm up once and serialize the nullable contract deliberately

Use one sampler per batch invocation. Apply scan options, obtain an unreported baseline, wait `config.delay()`, and then begin the existing emitted-iteration loop. Count only outputs toward `-n`; retain the current zero-count clamp, filters, limits, ordering, and broken-pipe behavior. Do not introduce an immediate-output flag in this change. First output latency increases by one configured clamped interval.

A task that appears only in the measured snapshot is included but has unavailable CPU until a later valid interval. PID reuse never inherits a prior task's history. Use backend-measured intervals, not requested sleep duration, for percentage calculation.

Keep existing JSON keys and array framing. The affected exported keys are `cpu_percent`, `mem_percent`, `rss`, `vsize`, `priority`, and `threads`; each can become null and receives an entry in `availability`. For example, an unreadable RSS is paired with `"availability":{"rss":"permission_denied",...}`. Do not add a JSON CPU-time field solely for this work. Existing nested Linux `memory` fields keep their established optional semantics.

Use a small bin-local serialization DTO with the workspace's existing `serde` and a scoped `serde_json` dependency. Core remains serialization-free. This makes escaping and nullable structured metadata parser-backed rather than extending nested manual string formatting. Preserve current numeric precision where the serializer permits; tests should compare parsed values and contracts, not irrelevant whitespace.

Alternatives rejected: emitting all-zero first rows keeps the misleading behavior; dropping newly appearing rows hides real processes; wrapping arrays in a version envelope breaks more consumers than necessary.

### 3. Fix Darwin priority at its actual source

Assign process priority from readable task information and thread priority from readable thread information. Preserve nice from its existing source. Convert to the existing storage type with checked/saturating behavior only as required by the native domain; never substitute nice on failure. Test a native priority that differs from nice and a denied task read. Add a macOS live self-process cross-check using the existing libproc helper, without making htop the oracle.

Alternative rejected: deriving priority from nice cannot represent the independently reported native scheduler priority.

### 4. Make navigation explicit without weakening action safety

Add sort and user picker screen state using the current screen-dispatch pattern. Sort selection is provisional until Enter, Escape cancels, and applying a new field uses its default direction. `I` toggles only direction, while `<`, `>`, and `.` retain their existing quick-cycling meanings. Move meter-mode cycling to unused `m`.

Enable crossterm mouse capture with a terminal-lifecycle guard. Record sortable header hit regions from the latest actual draw, including clipping, sidebars, and horizontal offset; do not derive clicks from stale static widths. Mouse header sorting enters the same command path as keyboard sorting. Handle it only on the process list and do not allow incidental clicks to retarget a pending signal operation. Restore mouse capture, raw mode, alternate screen, and cursor state on exit, errors, and unwinding where supported.

Keep tree structure generation in core. Store collapsed identities in application state keyed by row identity plus start time; apply folds during visible-order flattening, and update markers to show collapsed descendants. Prune exited identities against the full live set, not the filtered display set, so changing a filter does not erase a living node's state. Preserve cycle/orphan behavior and thread ownership.

`-` collapses and `+` expands a selected tree node. `*` collapses all expandable nodes if any is expanded, otherwise expands all. `[` slows and `]` accelerates sampling in either mode; list-mode `+`, `=`, and `-` retain speed aliases. Fold state survives samples and layout switches within the session, but is not written as a persistent list of PIDs. If folding hides selection, select its nearest visible ancestor; searches that select a folded descendant expand its ancestor path.

Route all reordering through identity-preserving selection restoration, not only new-sample adoption. Keep the existing target capture and start-time checks for signal, renice, and editor actions. Collapsing a subtree does not change the semantics of tagging a process and all of its real descendants.

Alternative rejected: storing collapse by PID or by visible row number fails under PID reuse, filtering, and resampling.

### 5. Use starter tabs only for unconfigured tab collections

Make the starter tabs CPU (`PERCENT_CPU` descending), Memory (`PERCENT_MEM` descending, with RSS visible), and All (PID ascending). Keep compact resource columns in panels layout and the detailed default columns for All where width permits. Each tab stores its own columns and sorting as it does today. First startup selects CPU.

Preserve explicit saved and imported tab lists, active indexes, columns, and sort choices. Pay particular attention to serde defaults and the htoprc importer's fallback path, which must not accidentally append the three starter tabs. A config with no explicit tabs receives starter defaults; an explicit invalid empty list retains current validation behavior. Keep atomic-save, invalid-config, symlink, and permission guarantees.

Alternative rejected: adding starter tabs to every saved configuration silently rewrites established workflows.

### 6. Give filters a persistent status surface

Separate the ephemeral input prompt from the active-condition summary. Reserve an adaptive row for active text/user conditions and matching/eligible counts, and keep it visible after confirmation. Eligible counts are computed after thread/container visibility but before text/user filtering and tree folding. This makes fold operations independent of filter-result counts.

Retain current literal, case-insensitive, pipe-separated command matching, history recall, and Escape-to-clear text behavior. The user picker lists distinct owners from the full current sample plus All users, supports name/UID search, and commits a UID only on confirmation. Keep a selected UID even if it currently matches zero rows. On empty results, clear selection/action eligibility and show a short no-match state without removing the active-condition summary.

On narrow widths, wrap the condition summary and recompute the table viewport rather than covering headers, rows, or the function bar. The full active expression remains reachable by reopening its prompt.

Alternative rejected: transient footer messages disappear and compete with action feedback, so they cannot be the only record of active filters.

### 7. Resolve layout from content budgets and retain raw command text

Add backward-compatible `show_system_panel` and `show_inspector` config preferences, defaulting true. Expose them in F2 Display and as unused `B` (system sidebar) and `V` (inspector) list-mode shortcuts. In classic htop layout, retain these panels preferences without changing the classic header. Distinguish user preference from temporary automatic hiding.

Compute actual fixed-column costs and give the command at least 20 terminal cells whenever possible. Hide/reduce the inspector before the system column when necessary; if even a full-width table cannot fit configured columns, retain horizontal scrolling. Use one resolved geometry for painting, viewport sizing, and mouse hit tests. Do not persist automatic visibility decisions.

For secondary screens, resolve the entire middle content band before splitting list sidebars. Info/help/setup/pickers/editors can consume full width; compact signal pickers can be centered over the full-width content area. Returning restores list geometry and selection. Eliminate empty reserved sidebar tracks.

Limit inspector command lines with a real bounded iterator and a budget that reserves title, separators, state, CPU, and RSS first. At short heights, reduce command rows before key facts. Mark truncation visibly without embedding new usage instructions in the main interface.

Store the original display command in the full-command screen instead of pre-wrapped 80-character lines. Reflow at current inner width with display-cell/grapheme-aware helpers, preserving source whitespace and safely escaping terminal controls. Keep a source-text anchor when resizing so the visible position remains near the same content. This preserves stored command text; it does not reconstruct original argv quoting that the sampler never retained. Reuse locked Unicode utilities already in the ratatui dependency graph with direct dependencies if needed.

Alternative rejected: simply enlarging the current modal does not fix the unbounded inspector or clipping after a later resize.

### 8. Keep the latest-sample slot and drive draws with explicit invalidation

Separate sample delivery, input delivery, render deadlines, and rendering. Retain the latest-snapshot slot to avoid cloning or buffering full row sets. Use a standard-library event channel: a terminal-input reader publishes crossterm events, and the sampler publishes a coalesced sample-ready wake after updating the slot. The UI thread owns application state and drawing.

The input reader uses a bounded poll wait so it can observe shutdown and be joined; do not leave a detached thread blocked indefinitely in terminal input. Coalesce sampler notifications around the latest-slot transition and test the swap/wake race. A dirty flag is set by sample adoption, relevant input, resize, and actual timed-content deadlines. Draw dirty state and then wait for an event or the nearest visible clock/status deadline. Do not advance histories on redraw. Use bounded frame coalescing only when needed during input bursts, not as an unconditional redraw clock.

Continue publishing delay/scan-option updates promptly. Preserve interruptible sampler sleeping and orderly thread shutdown. On errors, restore the terminal through a lifecycle guard before surfacing the failure. Keep event scheduling testable with fake events/clock/draw counters; real terminal timing is a separate acceptance check.

Alternatives rejected: rendering at the sample interval delays typing, while retaining 30 Hz draw calls wastes work even if ratatui emits few changed terminal cells. An async runtime is unnecessary for these two producer threads.

## Risks / Trade-offs

- [Nullable JSON values break consumers that require numbers] -> Mark the change as breaking, retain names/framing, document statuses and examples, and do not offer a compatibility mode that lies with zero.
- [Warm-up adds up to ten seconds of startup latency] -> Use the configured clamped delay consistently and document it; emit exactly the requested measured snapshots.
- [Validity coverage changes shared sorting/rendering on all platforms] -> Use independent fixture cases per backend and cross-surface tests; do not infer missingness from numerical zero or UID.
- [New defaults and key semantics surprise existing users] -> Preserve explicit configs/imports, publish migration notes, and update CLI/help/README translations together.
- [Fold/filter/layout work changes action targets] -> Retain and extend identity safety regressions, including reused PIDs, threads sharing a PID, and frozen signal pickers.
- [Unicode reflow silently alters command text] -> Use source-preserving wrapping with cell-width tests and control-character escaping; do not use whitespace splitting as a lossless representation.
- [Wake-up or shutdown races lose redraws or leave raw mode active] -> Deterministic slot/channel race tests, long-delay quit tests, PTY checks, and lifecycle restoration on normal/error paths.
- [Noisy live performance results hide regressions] -> Capture release baseline before implementation, repeat matched 30-second windows, record process counts/workload, and require deterministic idle draw-count reduction plus a median CPU improvement.

## Migration Plan

1. Capture a release-build baseline and preserve an isolated test configuration before changing implementation.
2. Land validity and batch/priority corrections with serializer tests, then command inspection/layout fixes, navigation/defaults/filtering, and finally refresh scheduling. Keep each group independently testable; do not release half-migrated consumers of the validity model.
3. Run workspace checks, config/import compatibility tests, deterministic render/event tests, and PTY scenarios. Verify Linux and macOS live behavior; run FreeBSD checks on an available runner and explicitly report any unavailable platform rather than claiming it passed.
4. Repeat performance measurements against the saved baseline with identical viewport, interval, thread settings, and workload. Use OS CPU accounting, not displayed htop CPU, and collect at least three 30-second measured windows after warm-up.
5. Publish nullable-metric, first-output-latency, and keybinding migration notes before release. Existing TOML files remain readable; added preferences are optional and saved normally.
6. Rollback reverts this implementation group together. Extra TOML preferences can be ignored by the prior reader, while JSON consumers must account for the released version's nullable-value contract. Preserve custom user configs and do not auto-delete new starter tabs on downgrade.
