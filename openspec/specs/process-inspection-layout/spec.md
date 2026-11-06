# process-inspection-layout Specification

## Purpose

Preserve readable process commands and essential facts across terminal sizes without wasting secondary-screen space or silently clipping inspected content.

## Requirements

### Requirement: Command summaries do not displace essential facts

The inspector command summary SHALL consume at most four rows and SHALL indicate omitted content. It SHALL reserve space for state, CPU, and resident-memory facts before allocating summary rows. When the terminal is too short for four summary rows plus those facts, the summary SHALL shrink first. Unavailable facts SHALL retain their explicit placeholders.

#### Scenario: A command exceeds one thousand characters
- **WHEN** the selected process has a command that wraps beyond four rows
- **THEN** the inspector shows no more than four command rows, indicates truncation, and still displays state, CPU, and resident memory when height permits

#### Scenario: Short inspector
- **WHEN** only enough height for essential facts and a one-line summary remains
- **THEN** the summary is reduced before essential facts are removed

### Requirement: Full command inspection preserves content

The full-command view SHALL wrap using its current inner display width, reflow after resizing, and allow vertical navigation through all command content. It SHALL NOT discard command characters or whitespace at line boundaries, except for safe visible escaping of terminal-control characters. Display-cell width, rather than byte count, SHALL govern wrapping.

#### Scenario: Command in a narrow view
- **WHEN** a command longer than 80 characters is opened in a 56-cell content area
- **THEN** every display line fits and all command content is reachable by scrolling without hidden line suffixes

#### Scenario: Resize while inspecting
- **WHEN** the terminal becomes narrower with the command view open
- **THEN** content reflows to the new width without losing characters or making the current scroll position invalid

#### Scenario: Non-ASCII arguments and repeated spaces
- **WHEN** the command contains wide characters, combining marks, or repeated spaces
- **THEN** wrapping respects terminal-cell width and preserves the inspected content

### Requirement: Sidebar preferences are independent and persistent

Users SHALL be able to independently toggle the system sidebar and process inspector through display settings and direct keyboard commands. Preferences SHALL persist on clean exit. Automatic hiding due to insufficient space SHALL NOT overwrite the user's preferences; preferred panels SHALL return when space is available. Existing configs without these preferences SHALL load with both panels preferred.

#### Scenario: Hide only the inspector
- **WHEN** the user disables the inspector
- **THEN** its width returns to the process table while the preferred system sidebar remains visible

#### Scenario: Resize and restore
- **WHEN** a narrow terminal temporarily hides a preferred sidebar and then grows wider
- **THEN** the sidebar reappears without requiring a settings change

### Requirement: Process commands receive a usable width budget

The layout SHALL prioritize a command column of at least 20 display cells when the terminal and configured fixed-width columns permit it. It SHALL remove optional inspector width before system-sidebar width when necessary. If the terminal cannot satisfy the budget even without sidebars, the table SHALL retain horizontal navigation rather than overlap or silently drop columns.

#### Scenario: Default columns at 100 cells
- **WHEN** both preferred sidebars leave fewer than 20 command cells
- **THEN** the inspector is reduced or hidden to restore the command budget

#### Scenario: Extremely narrow terminal
- **WHEN** the full table cannot fit after sidebars are removed
- **THEN** headers and rows remain aligned, horizontal navigation remains available, and the function bar does not overlap table content

### Requirement: Secondary screens reclaim sidebar space

Help, setup, information, sort/user pickers, affinity, I/O priority, and signal selection SHALL use the available content region without reserving blank inspector or system-sidebar tracks. Returning to the list SHALL restore its layout and selected identity. Signal targets SHALL remain frozen while the screen geometry changes.

#### Scenario: Open command details in panels layout
- **WHEN** the user opens the full-command view from a three-column list
- **THEN** the view can use the complete content width rather than the former center-column width

#### Scenario: Return from setup
- **WHEN** the user closes setup
- **THEN** the list returns with the selected process and preferred sidebars intact
