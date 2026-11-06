# process-navigation Specification

## Purpose

Let users find resource hotspots, inspect process families, and refine visible processes with predictable sorting and filtering while retaining selection and action safety.

## Requirements

### Requirement: Sorting has an explicit selector and direction

F6 SHALL open a keyboard-navigable sort picker for the current platform's selectable table fields. Confirming SHALL apply the chosen field; cancelling SHALL preserve the old order. `I` SHALL reverse the current direction. The active field and direction SHALL remain visible and persist per tab. `<` and `>` SHALL retain previous/next field shortcuts; `m` SHALL cycle the selected header meter mode.

#### Scenario: Choose memory ordering
- **WHEN** the user opens F6, selects RSS, and confirms
- **THEN** the table is sorted by RSS in its default direction and the header identifies the field and direction

#### Scenario: Reverse without changing fields
- **WHEN** the user presses `I` while sorting by CPU descending
- **THEN** the table switches to CPU ascending without changing the active field

#### Scenario: Cancel the picker
- **WHEN** the user navigates to another field and presses Escape before confirming
- **THEN** the previous field and direction remain active

### Requirement: Mouse header sorting matches keyboard sorting

Clicking a visible sortable header SHALL choose that field in its default direction; clicking the active header SHALL reverse direction. Hit testing SHALL account for layout, terminal resizing, and horizontal scrolling. Mouse interaction SHALL NOT alter frozen process-action targets. Mouse capture SHALL be restored on normal and error exits.

#### Scenario: Click the current CPU header
- **WHEN** CPU descending is active and the user clicks the visible CPU header
- **THEN** CPU ascending becomes active, matching keyboard inversion

#### Scenario: Click after resize
- **WHEN** sidebar layout changes after terminal resizing and the user clicks a visible header
- **THEN** sorting applies to the displayed field at that position, not an old coordinate mapping

### Requirement: Tree nodes can be collapsed and expanded

In tree mode, `-` SHALL collapse the selected node and `+` SHALL expand it. `*` SHALL collapse all expandable nodes if any is expanded, and otherwise expand all. Folding SHALL preserve parent/child structure, indicate hidden descendants, and retain state across samples and layout switches using live task identity. Stale fold state SHALL NOT apply to a reused PID.

#### Scenario: Collapse a process family
- **WHEN** the user selects a process with descendants and presses `-`
- **THEN** the parent remains visible, its descendants are hidden, and its collapsed state is indicated

#### Scenario: Expand after a refresh
- **WHEN** a collapsed parent receives new children and the user later presses `+`
- **THEN** its current children become visible without losing the parent selection

#### Scenario: Collapse all then expand all
- **WHEN** expanded tree nodes exist and the user presses `*` twice
- **THEN** the first action collapses all expandable nodes and the second expands all

#### Scenario: Folded PID is reused
- **WHEN** a folded process exits and another process reuses its PID with a different start time
- **THEN** the replacement does not inherit its fold state

### Requirement: Sampling speed remains accessible in both list modes

`[` SHALL increase the sampling delay and `]` SHALL decrease it in list and tree modes, within the existing 100 ms to 10000 ms range. Outside tree mode, existing `+/-` speed aliases SHALL remain available. Inside tree mode, folding keys SHALL NOT change sampling delay.

#### Scenario: Collapse without slowing sampling
- **WHEN** the user presses `-` on a tree node
- **THEN** only fold state changes and the configured sampling interval remains unchanged

#### Scenario: Adjust speed while viewing a tree
- **WHEN** the user presses `]` in tree mode
- **THEN** sampling becomes faster within the permitted range without changing fold state

### Requirement: Fresh configurations prioritize resource investigation

A fresh configuration SHALL provide CPU, Memory, and All tabs: CPU descending, memory usage descending, and PID ascending respectively. CPU SHALL be initially active. Existing persisted and explicitly imported tabs, ordering, active tab, columns, and sort directions SHALL remain unchanged. Per-tab changes SHALL persist after a clean exit.

#### Scenario: First run
- **WHEN** no saved configuration exists
- **THEN** CPU, Memory, and All tabs are available and CPU descending is initially active

#### Scenario: Existing custom tabs
- **WHEN** a saved or imported configuration defines its own tabs and active sort
- **THEN** gander retains those choices rather than inserting or replacing them with starter tabs

### Requirement: Selection and process actions remain identity safe

Sorting, sampling, filtering, tab changes, layout changes, and folding SHALL retain the selected live task when it remains visible. Hiding a selected descendant by folding SHALL select its visible ancestor. If selection cannot remain visible, the replacement SHALL be deterministic. Existing tagged-target identity checks and signal-picker target freezing SHALL remain intact.

#### Scenario: CPU ranks change
- **WHEN** the selected process changes position after a new CPU sample
- **THEN** selection follows the same process and start time, not its former row number

#### Scenario: Collapse hides the selection
- **WHEN** a collapse-all action hides the selected descendant
- **THEN** selection moves to its nearest visible ancestor

#### Scenario: A signal picker is already open
- **WHEN** new samples change process ordering or remove a row
- **THEN** confirming the signal still uses the picker's original identity-checked target set

### Requirement: Effective filters remain visible

Applied text and user filters SHALL remain visible after input confirmation, together with matching and eligible row counts. The eligible total SHALL be counted before text/user filters but after thread/container visibility settings, and SHALL NOT change merely because tree nodes fold. No matches SHALL show a clear empty state rather than an unexplained blank table.

#### Scenario: Confirm a text filter
- **WHEN** the user confirms a command filter matching 3 of 50 eligible rows
- **THEN** the filter text remains visible with counts 3 and 50 while navigating the table

#### Scenario: No rows match
- **WHEN** active filters match no eligible row
- **THEN** an explicit no-match state is shown, the active conditions remain visible, and process actions have no targets

#### Scenario: Clear a text filter
- **WHEN** the user presses Escape on the process list with an active text filter
- **THEN** the text filter is cleared and the displayed conditions and counts update consistently

### Requirement: Users can select a process owner directly

`u` SHALL open a searchable user picker containing All users and distinct owners from the current sampled processes. Confirming SHALL apply the chosen UID; cancelling SHALL leave the previous filter unchanged. Owner selection SHALL remain stable across samples even when the matching row set becomes empty.

#### Scenario: Select an owner without cycling UIDs
- **WHEN** the user opens `u`, searches for an owner, and confirms
- **THEN** that owner's processes are shown and the active owner remains visibly identified

#### Scenario: Cancel owner selection
- **WHEN** the user changes the picker cursor and presses Escape
- **THEN** the previous owner filter remains in effect
