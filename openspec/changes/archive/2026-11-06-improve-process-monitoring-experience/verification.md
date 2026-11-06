# Implementation Verification

## Scope and Status

All ten proposed improvements are implemented. The task list records implementation
and checks separately; this report does not claim a native FreeBSD execution or a
clean whole-repository formatter baseline.

No personal gander/htop configuration was used for live tests or recordings.
Process-changing tests target only their own child processes. No commit, release,
site publication, or OpenSpec archive was performed.

## Automated Checks

- macOS: `cargo test --workspace --quiet` passed: 481 tests, one ignored helper.
- Linux ARM64 in the existing Docker environment, Rust 1.89:
  `cargo test --workspace --locked --quiet` passed: 484 tests, one ignored helper.
- Native `cargo clippy --workspace --all-targets -- -D warnings` passed.
- FreeBSD `cargo check --workspace --target x86_64-unknown-freebsd --locked` and
  the equivalent `aarch64-unknown-freebsd` check passed using the installed rustup
  compiler/targets. This is compilation coverage, not native live verification.
- `git diff --check` passed. All added/modified Rust files pass rustfmt with
  `--edition 2021 --config skip_children=true --check`.
- `cargo fmt --all -- --check` still reports formatting in untouched files. The
  same failures were reproduced against an archive of original HEAD `6fb015b`.
  Remaining files include core affinity/tree, FreeBSD scan, Linux parse/fsops/
  delayacct, existing platform live tests, and TUI panels/theme. They were not
  reformatted as part of this behavior change.
- `packaging/check-site.sh` passed in a Bash-capable Linux container.
- OpenSpec strict validation is run after the final task update.

Core regression coverage includes explicit zero versus unavailable status,
unknown-last sorting in both directions, partial-read failure identity retention,
CPU counter regression/PID reuse/topology reset, native Darwin priority, and
thread memory validity. Bin tests cover warm-up/output counts, escaping and
nullable JSON, array streams, broken pipes, and first/idle/new-process behavior.
TUI tests cover render budgets, command reconstruction and Unicode cell widths,
resize anchors, picker commit/cancel, mouse hit regions including horizontal
scroll, stable selection, folding/PID reuse, filter visibility, and user pickers.
Runtime tests assert initial-plus-sample draw counts, no history updates from
painting, coalesced/latest snapshot wakes, and visible time-meter scheduling.

## Live Process Data

The focused `batch_contract` tests also passed with `--nocapture` on both hosts:

| Platform | First emitted busy CPU | Independent OS interval | Idle CPU |
|---|---:|---:|---:|
| macOS | 99.38% | 100.91% | available, numeric 0 |
| Linux | 100.00% | 101.44% | available, numeric 0 |

The reference calculation differences raw OS task counters over wall-clock time;
it does not use gander's derived percentage or htop's display. Live tolerances
allow sampling/granularity differences; exact scaling and normalized CPU are
covered by deterministic fixtures/render tests. Newly observed live processes
export `null`/`warming_up` in their first snapshot and an available measured CPU
in the next one. Native macOS priority is also checked against libproc for self.

A FreeBSD live runner was not available. Its live acceptance remains a platform
coverage limitation, explicitly not a claimed passing result.

## Real Terminal Workflows

`tools/pty_smoke.py target/release/gander` passed at 160x32, 120x24, 100x24, and
80x24 with a development-only `pyte` environment. Each workflow exercises fresh
CPU/Memory/All tabs, text filtering, F6 cancel, inversion, actual SGR mouse header
clicks, tree collapse/refresh/expand, full-command End/resize, owner selection,
explicit no-match recovery, B/V and L, and signal-picker cancellation.

Normal exits restored termios, disabled mouse capture, and left the alternate
screen. A deliberately closed output pipe verified that initialization failure
restores raw mode. Quitting at a 10000 ms sampling interval completed within one
second rather than waiting for the next sample. Ctrl-C from setup returned 130,
restored the terminal, and did not write configuration. The harness stopped and
reaped its own helper process families.

Three screenshots and both GIFs were regenerated from the VHS tapes and inspected
for correct flat/tree/classic views and legible, non-overlapping content. Recording
uses copied temporary config/state, interactive literal filtering, and a narrow
application allowlist to avoid capturing development-session command lines.
VHS was installed as a development tool; Homebrew also updated its dependencies,
including ffmpeg. No VHS/pyte dependency was added to gander's runtime.

## Matched Performance Measurements

Raw results are in `baseline.json` and `after.json`, including binary hashes,
explicit config, platform, window data, and medians. The preserved original
release binary/config remain under `/tmp/gander-monitoring-baseline-6fb015b/`.

Both measurements used macOS 15.7.1 ARM64, 160x32, panels, CPU-descending full
process scope, the same explicit five columns, hidden user threads, one-second
sampling, a three-second warm-up, and three consecutive 30-second windows.
Input probes run outside CPU measurement windows. Desktop background processes
were not frozen: counts at report time were 463 before and 515 after. These are
local comparative results, not a universal performance guarantee.

| Median per window | Original 6fb015b | Changed | Interpretation |
|---|---:|---:|---|
| OS-accounted CPU | 2.2412% | 0.8685% | 61.25% reduction |
| Terminal output bytes | 118505 | 56567 | 52.27% reduction |
| Search-input echo | 34.4029 ms | 1.4353 ms | No latency regression |
| RSS | 8.95 MiB | 11.14 MiB | Increased in this run; not a memory optimization claim |

Original CPU windows were 2.2412%, 3.1066%, and 1.7927%; changed windows were
0.9299%, 0.8685%, and 0.6555%. RSS varies with desktop memory pressure and newly
allocated/retained pages; the reported increase is retained rather than hidden.
Deterministic runtime coverage separately proves eleven draws for an initial
frame plus ten adopted samples, with no redraws caused by otherwise idle polls.

The final table and `after.json` identify the final release binary, including the
Ctrl-C correction. The earlier series is retained in `after-before-exit-fix.json`
for traceability rather than selecting whichever run had the best numbers.

## Improvement Coverage

| Agreed point | Delivery and acceptance |
|---|---|
| 1. First batch CPU | Unreported warm-up, counted outputs, independent OS live checks and stream tests |
| 2. Missing metrics | Per-field statuses, nullable exports, placeholders, unknown-last sorting and backend fixtures |
| 3. Darwin priority | Native process/thread source, denied-read unit case and live self check |
| 4. Long commands | Four-row summary budget, essential facts, source-preserving full-width reflow and PTY End/resize |
| 5. Sorting | F6 picker, I inversion, m meter mode, direction indicator and real mouse clicks |
| 6. Tree operations | Identity-stable folds/all-tree action, speed separation, search expansion and hidden-descendant tagging |
| 7. First-run defaults | CPU/Memory/All, preserved explicit/imported tabs, config round trips and refreshed media |
| 8. Filtering | Persistent conditions/counts, no-match state, searchable owner picker and live recovery |
| 9. Layout | Independent sidebar preferences, width budgeting, full-width secondary screens and viewport tests |
| 10. Redraw overhead | Coalesced event-driven invalidation, terminal lifecycle checks and repeated performance measurements |

Compatibility changes and examples are documented in `docs/METRICS.md`,
`docs/KEYS.md`, `docs/RELEASE-NOTES.md`, the README translations, and the four JSON
guide translations. Historical tag-derived `CHANGELOG.md` entries were preserved.
