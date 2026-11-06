# Tasks

All tasks below are implementation or verification work; none has been completed by creating this proposal. Follow group order except where work has no dependency on later groups. Each group includes its own regression coverage and documentation.

## 1. Capture a repeatable baseline

- [x] 1.1 Preserve a release build of the pre-change revision and an isolated explicit config for 160x32, one-second sampling, full process scope, and fixed thread/column/sort settings; verify the saved binary/config can reproduce the baseline without reading or changing personal configuration.
- [x] 1.2 Capture at least three post-warm-up 30-second baseline windows of OS-accounted CPU time, RSS, terminal bytes, and interaction latency; deliver commands, revision, workload/process count, raw results, and medians in verification notes.
- [x] 1.3 Add the repeatable PTY measurement harness and its usage documentation without introducing runtime monitoring dependencies; verify it restores the terminal, cleans up its own helper processes, and records equivalent htop settings when htop is included.

## 2. Introduce truthful resource availability (improvement 2)

- [x] 2.1 Add compact field-specific metric availability to core rows, with explicit production initialization and available fixture construction; verify tests distinguish measured zero, warming-up, permission-denied, unsupported, and failed-read states without changing valid CPU arithmetic.
- [x] 2.2 Propagate availability through Linux, Darwin, and FreeBSD process/thread reads, preserving partial successes and identity/reset rules; verify backend fixtures or injected read failures cover denied access, counter regression, first intervals, and inherited thread memory.
- [x] 2.3 Update core sorting, TUI fields/inspector, normalized CPU, and value-bar maxima to honor validity; verify unknowns sort last in both directions, known zero remains numeric, and unavailable metrics render as `-` with no bar.
- [x] 2.4 Document the shared validity/status meanings and the Linux baseline-output contract in relevant design/portability notes; verify examples agree with the new tests and do not claim host CPU meters or valid percentage scaling changed.

## 3. Correct batch warm-up and JSON export (improvement 1)

- [x] 3.1 Add one unreported baseline and one clamped interval before emitted batch samples, retaining one sampler; verify deterministic scan/wait tests for `--json`, text `-n`, zero iterations, clamp limits, newly appearing identities, and PID reuse.
- [x] 3.2 Add bin-local serde DTOs and the scoped serializer dependency, keeping arrays and field names while exporting nullable metrics plus `availability`; verify parser-based tests cover all six affected JSON keys, measured zero, reasons, escaped commands, nested Linux memory values, and repeated arrays.
- [x] 3.3 Preserve filter/sort-before-limit and broken-pipe success behavior; verify regression tests show a measurable busy process precedes idle and unavailable rows in CPU top output and that warm-up does not add an output snapshot.
- [x] 3.4 Update CLI batch help, the JSON guide, README batch examples/translations, and release notes for first-output latency and nullable metrics; verify documented commands parse successfully and example JSON is valid.

## 4. Use native Darwin scheduling priority (improvement 3)

- [x] 4.1 Read process/task and thread priority independently of nice and preserve missing-priority status; verify deterministic tests with native priority 31 and nice zero plus a denied read.
- [x] 4.2 Add a macOS live self-process cross-check against the existing libproc task helper and verify JSON/TUI priority consistency without using htop as the oracle; explicitly report platform gating when not running on macOS.
- [x] 4.3 Document native priority versus nice and unavailable values in the applicable portability/column guidance; verify the description matches both process and thread tests.

## 5. Repair inspection and responsive geometry (improvements 4 and 9)

- [x] 5.1 Enforce a maximum four-row inspector command summary with a reserved essential-fact budget and truncation indicator; verify render tests for long commands, short heights, and unavailable state/CPU/RSS facts.
- [x] 5.2 Store source command text in its info view and reflow by current inner cell width with safe escaping and source-preserving whitespace; verify reconstruction/reachability tests for long paths, repeated spaces, wide/combining characters, scrolling, and resize anchors.
- [x] 5.3 Add independent `show_system_panel`/`show_inspector` preferences, F2 Display controls, and `B`/`V` shortcuts; verify config round trips, old-config defaults, temporary auto-hide restoration, and unchanged classic-layout preference retention.
- [x] 5.4 Resolve command-width budgets from actual columns and reclaim sidebar tracks on secondary screens; verify render tests at 160x32, 120x24, 100x24, 80x24, 64x20, and tiny terminals show no overlap, usable horizontal navigation, and full-width details/setup/pickers.
- [x] 5.5 Update layout, settings, help, key docs, and affected README translations for sidebar behavior and command inspection; verify documented shortcuts with focused PTY scenarios and confirm returning from details restores selection.

## 6. Complete sorting controls (improvement 5)

- [x] 6.1 Add an F6 sort picker with explicit commit/cancel, direction indicators, `I` inversion, retained quick cycling, and `m` meter-mode cycling; verify key-routing and per-tab persistence tests for both layouts.
- [x] 6.2 Add mouse header sorting using freshly rendered/clipped hit regions and share its sort-command path with keyboard actions; verify clicks after sidebar changes, resize, horizontal scroll, and repeated clicks choose or invert the displayed field correctly.
- [x] 6.3 Guard terminal mouse capture/restoration and prevent mouse events from retargeting pending actions; verify error-path terminal restoration and existing frozen-signal-target/PID-reuse safety tests.
- [x] 6.4 Update CLI/help, `docs/KEYS.md`, related design notes, README translations, and release notes for F6, `I`, and `m`; verify the obsolete claim that sort inversion is unavailable is removed.

## 7. Add identity-stable tree folding (improvement 6)

- [x] 7.1 Add fold state keyed by live task identity/start time, folded flattening, and collapsed markers while retaining cycle/orphan/thread ownership handling; verify tests for new children, filtering round trips, exited identities, and PID reuse.
- [x] 7.2 Implement tree `+/-`, all-tree `*`, and global `[/]` speed controls while retaining list speed aliases; verify fold keys never change tree sampling delay and all speed paths respect the 100 ms to 10000 ms clamp.
- [x] 7.3 Preserve selection across every reorder, move hidden selections to visible ancestors, expand paths for search hits, and retain real-subtree tagging semantics; verify action-safety tests including threads sharing a PID and frozen signal targets.
- [x] 7.4 Update key/help/tree docs and README translations for folding and speed migration; verify a PTY scenario collapses a process family, resamples, expands it, and retains the correct selected process.

## 8. Ship useful starter tabs safely (improvement 7)

- [x] 8.1 Add CPU, Memory, and All starter tabs with the specified sort orders and initial CPU selection; verify fresh-config and render tests show resource hotspots with compact columns and RSS on the memory view.
- [x] 8.2 Preserve explicit saved/imported tabs, active indexes, columns, and sort directions without injecting starter tabs; verify serde default cases, htoprc fallbacks, invalid-empty-list behavior, and existing atomic-save/symlink/permission tests.
- [x] 8.3 Update first-run/tab documentation and relevant screenshots only where behavior changed; verify all documented tab defaults match serialized fresh configuration and that existing-config behavior is described accurately.

## 9. Make active filters visible and owners selectable (improvement 8)

- [x] 9.1 Add persistent text/user-condition summary and matching/eligible counts independent of fold visibility; verify confirmation, Escape clearing, OR/case-insensitive matching, thread/container toggles, narrow wrapping, and history-recall regressions.
- [x] 9.2 Add searchable owner selection with provisional cursor/commit/cancel and All users, plus explicit no-match states and safe empty action targets; verify disappeared-owner, zero-match, cancel, and combined text/user-filter scenarios.
- [x] 9.3 Update filter/user-picker help, key docs, and README examples/translations; verify PTY scenarios leave the effective conditions and counts visible after confirmation and allow recovery from a no-match view.

## 10. Replace unconditional redraws (improvement 10)

- [x] 10.1 Separate event delivery from rendering using the latest-snapshot slot, coalesced sample wakes, a stoppable terminal-event producer, and dirty/deadline scheduling; verify deterministic tests for wake races, burst coalescing, visible clock deadlines, and input/sample adoption within 100 ms.
- [x] 10.2 Verify one initial draw plus at most one per adopted sample in an idle fake ten-second run, with no redraws from poll time alone and no history advancement on repaint; add these assertions to the runtime regression suite.
- [x] 10.3 Preserve prompt speed/scan-option updates, quick long-delay shutdown, joined worker threads, and terminal restoration on failure; verify existing sampler tests plus PTY normal/error exits with a 10000 ms delay.
- [x] 10.4 Repeat the matched baseline protocol for at least three 30-second windows, documenting CPU/RSS/terminal-byte medians and interaction latency; verify median CPU cost improves over the saved gander baseline without latency regression and update runtime/performance guidance with the measured results.

## 11. End-to-end acceptance

- [x] 11.1 Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`; record exact results and investigate any regression attributable to this change.
- [x] 11.2 Run controlled busy/idle/new-process live checks on Linux and macOS for ordinary/normalized CPU, first JSON output, unreadable metrics, and native Darwin priority; record independent system references and explicitly identify any unavailable FreeBSD runner or remaining platform gap.
- [x] 11.3 Execute the combined PTY workflow across both layouts and desktop/narrow viewports: default tabs, sort picker/inversion/mouse, tree folding, filter/owner selection, long-command inspection, sidebar toggles, resize, and quit; verify identity-safe actions only against test-owned processes and no personal config changes.
- [x] 11.4 Review proposal-to-spec-to-task coverage for all ten improvements and finalize compatibility/release notes; verify every listed acceptance criterion has a passing test or a recorded live result, with platform limitations clearly stated before marking work complete.
