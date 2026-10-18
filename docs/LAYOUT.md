# The `panels` layout

Why gander's default screen does not look like htop, and what it does instead.

The htop arrangement is still available — `L` switches, `layout = "htop"` in the
config makes it permanent — because an information architecture is an opinion and
opinions should be optional. This document explains the opinion.

## What is wrong with the htop arrangement

Three structural problems, none of which is about colours or fonts:

**1. The meters are horizontal, and horizontal space is not scarce.**
On a 16-core machine htop's header is 17 rows of CPU bars above a 24-row
terminal's process list. The meters consume the one resource the process list
cannot get back. Vertical space is the scarce axis; horizontal space is what a
160-column terminal has spare.

**2. Answering "what is this process doing" requires a mode switch.**
`w`, `e`, `l`, `x`, `s` each replace the whole screen. You lose your scroll
position, your tags, the neighbouring rows you were comparing against, and the
sampling view. You then press `Esc` and try to find where you were. The questions
you ask constantly — what is the command line, how much memory exactly, how long
has it run, is it sleeping or running — cost a screen swap each time.

**3. Every core gets a full row.**
16 cores means 16 bars, each 25 cells long and 90% empty. The information is real
but it does not need 16 rows to convey.

## The layout

```text
   Main │ Memory │ Disk                                                  [tabs]
┌───────────────────┬───────────────────────────────────────┬──────────────────────┐
│ CPU          9.8% │  PID  CPU%  MEM%   RES  COMMAND       │ 2032 iTerm2          │
│ ██░░░░░░░░░░░░░░░░│ 2032  25.7   1.5  122M  /Applications… │ /Applications/iTerm… │
│                   │ 22472 17.0   0.1  6.0M  gander        │                      │
│ 8 cores           │ 15101 13.1   4.3  350M  pi            │ USER   panda         │
│ ▂▁▁▁▁             │  730   0.3   0.4   35M  Notification… │ STATE  S sleeping    │
│                   │ 22468  0.3   0.2   15M  Python        │ CPU    25.7%         │
│ MEM      3.8G/8.0G│ 1024   0.2   2.6  211M  magpie        │ MEM    1.5%  122M    │
│ ██████████████████│ 1510   0.2   0.4   30M  WebKit        │ VIRT   416G          │
│                   │ …                                     │ TIME   27:03.99      │
│ SWP    168M/1.0G  │                                       │ UP     1h 47m        │
│ ██████████████████│                                       │ NICE   0             │
│                   │                                       │ THR    10            │
│ LOAD  2.69 2.33   │                                       │                      │
│ UP    1h 48m      │                                       │                      │
│ TASKS 460         │                                       │                      │
│ RUN   1           │                                       │                      │
│ THR   1421        │                                       │                      │
│ ▁                 │                                       │                      │
└───────────────────┴───────────────────────────────────────┴──────────────────────┘
 F1 Help  F2 Setup  F3 Search  F4 Filter  F5 Tree  F6 SortBy  F7 Nice-  …  F10 Quit
```

### The system column

Everything in it is vertical, and nothing in it is a process. A 24-row terminal
on a 16-core box now keeps 22 rows for processes.

- **One CPU bar**, not one per core. The aggregate is what you look at first.
- **A per-core grid** underneath, one glyph per core, coloured by load. 16 cores
  fit in one or two rows; 64 cores cap at four. `·` means idle, which keeps the
  grid reading as a grid instead of a ragged line of blanks.
- **Memory and swap as stacked bars**, using the platform's own class breakdown,
  so "how much is cache versus anonymous" is visible without a keypress. The
  track is `░` rather than a background colour, so the denominator is visible in a
  monochrome theme and in terminals that drop background colours.
- **Load, uptime, and task counts** as label/value pairs, with threads separate
  from processes because they answer different questions.
- **An aggregate CPU sparkline** at the bottom, where it does not compete with
  anything.

### The process table

- Default columns are `PID`, `CPU%`, `MEM%`, `RES`, `COMMAND`.
- **`CPU%` and `MEM%` carry a bar painted behind the number.** The table is
  sorted by one of them; the bar makes the ordering readable at a glance instead
  of requiring you to compare thirty numbers.
- The bar is scaled against the **largest value in the whole table**, not against
  100%. On a quiet machine the top process might be at 4%; an absolute bar would
  be invisible for every row, which is exactly when "which of these is biggest"
  matters most. Table-wide rather than per-screen so the bars do not rescale as
  you scroll.
- **The bars are paint, not glyphs.** The cell background changes; the number
  stays on top of it, readable and selectable by the terminal's own copy.

### The inspector

The right-hand panel, always present while the list is showing:

- The pid and command name.
- **The full command line, wrapped over up to four rows.** This is the single
  most valuable thing the table can never show, and it is the reason `w` exists in
  htop.
- **`STATE` in words** (`S sleeping`), not a letter. A one-letter state column is
  a lookup table you have to memorise; the inspector has room to spell it out.
- Exact numbers the narrow columns round: `MEM 1.5% 122M`, `VIRT 416G`,
  `TIME`, elapsed time, nice, thread count.

Moving the selection updates it in place, so you can walk down the list and watch
the details change without leaving the list.

The modal screens stay for what genuinely needs its own space: environment,
open files, locks, backtrace, and the `a`/`i`/`F2` editors.

## Degrading

The layout is not allowed to produce an unusable screen on a small terminal:

| Terminal width | System column | Inspector |
|---|---|---|
| ≥ 120 | 24 | 38 |
| ≥ 100 | 24 | 32 |
| ≥ 88 | 20 | 28 |
| ≥ 80 | 20 | 22 |
| ≥ 72 | 16 | 22 |
| ≥ 64 | 16 | — |
| < 64 | — | — |

The inspector is dropped before the system column, because the system column
carries information with no other home while the inspector duplicates what
columns and the modal screens can show.

Below 30×4 for the whole terminal, or when the panels would leave under 40 cells
for the table, the screen falls back to the compact htop arrangement entirely.

## What was deliberately not done

- **No new dependencies.** Everything here is `put_str` and cell styling.
- **No glyphs outside the block range.** `█ ░ ▁▂▃▄▅▆▇ ·` are all single-width and
  present in every terminal font that handles htop today. No Nerd Font, no emoji.
- **No removal of the htop layout.** It is the reference implementation for
  everything upstream documents, and it is one keypress away.
- **No per-column bar for every numeric field.** Only the two the table is sorted
  by; bars everywhere would be noise, and the inspector has the exact numbers.
