# Proposal

## Why

Hands-on comparison of gander 0.6.7 (`6fb015b`) and htop 3.4.1 exposed misleading unavailable/initial metrics and broken long-command inspection, alongside gaps in sorting, tree navigation, filtering, layout, and idle rendering. Address all ten agreed improvements so users can trust the measurements and complete everyday process investigations without losing context.

## What Changes

1. Warm up batch sampling before the first emitted sample; never publish an unmeasured CPU interval as measured zero.
2. **BREAKING**: Represent unavailable process metrics as `null` in existing JSON fields and `-` in text/TUI, with additive per-field availability reasons. Keep measured zero numeric and existing JSON array framing and field names.
3. Read macOS scheduling priority from task information independently of nice.
4. Bound inspector command summaries to four rows while reserving key facts; make the full-command view reflow without clipping command content.
5. Add an F6 sort picker, explicit direction indicators, keyboard sort inversion, and mouse header sorting. **BREAKING**: Restore `I` to invert sorting; move meter-mode cycling to `m`.
6. Add identity-stable subtree collapse/expand and all-tree actions. **BREAKING**: In tree mode, `+/-` expand/collapse rather than changing sampling speed; provide `[/]` speed controls in both modes.
7. Ship CPU, Memory, and All default tabs, opening CPU descending on new configurations while preserving selection identity and existing/custom/imported tabs.
8. Keep active filters and matching/total counts visible, provide a genuine no-match state, and replace UID cycling with a searchable user picker.
9. Add independent persisted sidebar visibility controls, protect command-column width, and give detail/setup/picker screens the space released by hidden sidebars.
10. Replace unconditional approximately 30 Hz drawing with invalidation-driven rendering; verify redraw counts, responsiveness, CPU cost, and terminal output under repeatable conditions.

## Capabilities

### New Capabilities

- `batch-sampling`: Warm-up, emitted-sample counts, interval semantics, and compatible batch framing.
- `process-metric-validity`: Per-field availability, truthful missing-value presentation/sorting, and native macOS priority.
- `process-navigation`: Sorting, tree folding, stable selection, useful default tabs, and visible text/user filters.
- `process-inspection-layout`: Bounded summaries, lossless command inspection, responsive sidebars, and full-width secondary screens.
- `tui-refresh-scheduling`: Invalidation-driven rendering, sampler wake-ups, bounded input latency, and overhead verification.

### Modified Capabilities

- `linux-cpu-percent`: Retain ordinary/normalized CPU arithmetic and safe numeric baselines internally, but mark invalid intervals unavailable at output boundaries; permit nullable metrics and additive availability metadata while preserving field names and JSON array framing.

## Impact

- Core: `row.rs`, `table.rs`, `tree.rs`, and Linux/macOS/FreeBSD samplers; explicit metric validity accompanies existing raw scalar storage.
- CLI: `args.rs` and `batch.rs`; one initial sampling interval of startup latency, updated JSON value contract, no extra emitted warm-up sample.
- TUI/config: `app.rs`, `layout.rs`, `marks.rs`, `runtime.rs`, `setup.rs`, config defaults/persistence and htoprc import; restore terminal mouse state on every exit path.
- Tests: deterministic sampler/serializer/render/event-loop regressions, identity/action safety tests, config/import compatibility, PTY interaction tests, and repeatable live checks on supported platforms.
- Documentation: key reference, layout and JSON guides, relevant design notes, README translations, and release notes describing nullable metrics and changed keys.
- No daemon, privilege escalation, new monitoring backend, per-process I/O feature, or tracing implementation. Prefer existing dependencies; htop is an interaction reference, not the numerical authority on every platform.
