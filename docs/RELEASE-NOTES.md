# Unreleased Migration Notes

This work is not a tagged release. Historical entries in `CHANGELOG.md` remain
verbatim tag messages.

## Data Contract

- Batch mode takes one unreported warm-up interval before its first output.
  `-n` continues to count emitted snapshots. Single and repeated JSON array
  framing is unchanged.
- Basic resource metrics can be JSON `null`, with reasons in `availability`;
  TUI/text show `-`. Scripts must distinguish unavailable data from genuine zero.
  See [METRICS.md](METRICS.md) for statuses and examples.
- macOS process and thread priority now uses native scheduling priority rather
  than repeating nice. Valid ordinary/normalized CPU units and host memory
  accounting are unchanged.

## Controls and Defaults

| Control | New behavior |
|---|---|
| F6 | Sort picker; Enter commits, Escape cancels. |
| I | Invert the current sort direction. |
| m | Cycle the selected classic-header meter's mode (formerly I). |
| Click header | Select its sort field; click again to reverse. |
| Tree + / - | Expand / collapse the selected subtree; no sampling-speed change. |
| Tree * | Collapse all expanded nodes, or expand all if none is expanded. |
| [ / ] | Slower / faster sampling in either mode. List + / - speed aliases remain. |
| u | Searchable owner picker, including All users. |
| B / V | Toggle system sidebar / inspector in panels layout. |

Fresh tab collections contain CPU (descending CPU), Memory (descending memory,
RSS visible), and All (ascending PID), initially opening CPU. Explicit saved or
imported tabs are not replaced. Selection follows the same live identity across
reordering; pending process actions retain their identity-checked target set.

## Inspection and Rendering

The inspector limits command summaries to four rows and reserves state/CPU/RSS
facts. Full-command inspection reflows at current terminal width and preserves
source whitespace and characters, escaping terminal controls visibly. Secondary
screens reclaim sidebar width. Sidebar preferences persist, but automatic hiding
on narrow terminals does not change those preferences.

Confirmed text/owner filters remain visible with matching/eligible counts, and a
no-match state is explicit. Tree folding does not change filter-result counts.

Drawing follows new data, input, resize, and visible clock/date changes instead of
a fixed frame rate. Ctrl-C interrupts any screen with exit status 130 and does not
save configuration; it no longer falls through to a modal's F10 apply/back action.
See [PERFORMANCE.md](PERFORMANCE.md) for the repeatable PTY
measurement protocol and the change's verification notes for measured results.
