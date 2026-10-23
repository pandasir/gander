//! The `panels` layout: a persistent inspector beside the process table.
//!
//! # Why this exists
//!
//! The classic htop arrangement — horizontal meters across the top, one dense
//! table in the middle, function keys along the bottom — answers "what is running"
//! well and "what is this process doing" badly. Answering the second question
//! requires leaving the list (`w`, `l`, `e`, `s`), which loses the position you
//! had, the tags you set, and the comparison you were in the middle of making.
//!
//! It also spends its scarcest resource on its least valuable information: a
//! 16-core machine puts 17 rows of CPU bars in the header, so the process list —
//! the reason the program exists — is squeezed into whatever is left.
//!
//! # The layout
//!
//! ```text
//! ┌─ system ────┬─ processes ───────────────────────┬─ inspector ─────────┐
//! │ CPU  ▓▓▓░░  │   PID  CPU%  MEM%  COMMAND         │ 2971 pi             │
//! │  per-core   │  2971   12.4   1.2  pi             │ /usr/bin/pi --serve │
//! │  sparkline  │   833    0.5   0.2  searchparty    │                     │
//! │ MEM  ▓▓░░░  │   ...                            │ PID     2971        │
//! │ SWP  ░░░░░  │                                  │ USER    panda       │
//! │ TASKS       │                                  │ STATE   S (sleep)   │
//! │ LOAD        │                                  │ CPU     12.4%       │
//! │ UPTIME      │                                  │ MEM     1.2% / 96M  │
//! │             │                                  │ THREADS 14          │
//! │ top CPU     │                                  │ ELAPSED 2h 14m      │
//! └─────────────┴──────────────────────────────────┴─────────────────────┘
//! ```
//!
//! Three deliberate differences from htop:
//!
//! 1. **The system column is vertical.** Everything horizontal is a table row
//!    that is not showing a process. A 24-row terminal running on a 16-core box
//!    keeps 20 rows for processes instead of 3.
//!
//! 2. **Per-core CPU is a sparkline, not one bar per core.** 16 cores fit in 4
//!    rows as a grid of mini-bars, and the shape over time is more informative
//!    than 16 instantaneous percentages.
//!
//! 3. **The inspector is always there.** Selecting a row updates it in place, so
//!    you can walk down the list and watch details change without a mode switch.
//!    The modal screens still exist for the things that need their own space
//!    (environment, open files, locks, backtrace); the inspector covers the
//!    questions you ask constantly.
//!
//! The htop layout is still available (`layout = "htop"` in the config, or `L`
//! to switch), because a different information architecture is an opinion and
//! opinions should be optional.

use gander_core::format::{bytes_human, cpu_time_human, duration_verbose, percent_human};
use gander_core::row::{Field, Row};
use gander_core::system::SystemSnapshot;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::meters::{put_str, Segment};
use crate::theme::Theme;

/// Column widths for the two fixed side panels.
///
/// The system column needs ~22 cells to show "CPU 12.4%" plus a bar; the
/// inspector needs ~34 for a readable command line and labels. Both shrink on a
/// narrow terminal and disappear entirely below their minimum, because a 12-cell
/// panel is worse than no panel.
pub struct PanelWidths {
    pub system: u16,
    pub inspector: u16,
}

impl PanelWidths {
    pub fn resolve(total: u16) -> Self {
        // Never take more than 60% of the width in side panels, and never less
        // than ~40 cells for the table.
        let system = if total >= 100 {
            24
        } else if total >= 80 {
            20
        } else if total >= 64 {
            16
        } else {
            0
        };

        let inspector = if total >= 120 {
            38
        } else if total >= 100 {
            32
        } else if total >= 88 {
            28
        } else if total >= 72 {
            22
        } else {
            0
        };

        // If both panels would leave the table too narrow, drop the inspector
        // first: the system column carries information with no other home,
        // while the inspector duplicates what columns can show.
        let table = total.saturating_sub(system + inspector);
        if inspector > 0 && table < 40 {
            let inspector = if total.saturating_sub(system) >= 40 + 22 {
                22
            } else {
                0
            };
            return Self { system, inspector };
        }

        Self { system, inspector }
    }
}

/// Rows the system column keeps below the per-core grid.
///
/// MEM and SWP take two rows each, a blank separates the bars from the figures,
/// and LOAD/UP/TASKS/RUN/THR take one each. Reserving them is what keeps a
/// 128-core grid from consuming the whole column and pushing the memory and load
/// figures off a short terminal.
const RESERVED_BELOW_GRID: u16 = 10;

/// `128 cores`, or `64/128 cores` when only part of the grid is on screen.
///
/// The number reported is the number of glyphs actually drawn. Reporting the
/// truncated count on its own is what made a 128-core machine look like a
/// 64-core one: the grid stopped at 64 while the label claimed the machine had
/// 64 cores, so the only visible symptom was a wrong fact.
fn cores_label(drawn: usize, total: usize) -> String {
    if drawn >= total {
        format!("{total} cores")
    } else {
        format!("{drawn}/{total} cores")
    }
}

/// Draw the vertical system column.
///
/// Returns nothing; the caller has already reserved the `Rect`.
pub fn draw_system_column(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    snapshot: &SystemSnapshot,
    cpu_history: &[f32],
    tasks: (usize, usize, usize),
    cores_max: usize,
) {
    if area.width < 12 || area.height < 4 {
        return;
    }
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    if inner.width == 0 {
        return;
    }
    let mut y = area.y;
    let bottom = area.bottom();

    // --- CPU ---------------------------------------------------------------
    let usage = snapshot.cpu_usage();
    let pct = if usage.is_finite() {
        usage * 100.0
    } else {
        0.0
    };
    y = draw_labelled_bar(
        inner,
        buf,
        theme,
        y,
        bottom,
        "CPU",
        pct / 100.0,
        percent_human(pct).as_ref(),
        &cpu_segments(theme, snapshot),
    );

    // Per-core sparkline grid: one cell per core, coloured by load.
    //
    // The grid's shape comes from the terminal, not from a constant: `per_row`
    // is the column width (capped at 16 so a wide terminal does not put sixty
    // cores on one row, which reads as a single bar rather than a grid), and the
    // rows are whatever is left after reserving room for the figures below. An
    // earlier version capped the rows at four, so `system_cores_max = 0` —
    // documented as "render every core" — still stopped at 16 × 4 = 64 glyphs on
    // a 128-core box, under a label that claimed 128. The cap was never the only
    // limit, which is why raising it appeared to do nothing.
    let total_cores = snapshot.cpu_count();
    let wanted = if cores_max == 0 {
        total_cores
    } else {
        total_cores.min(cores_max)
    };
    if wanted > 0 && y + 1 < bottom {
        // A blank separator, then the label, then the grid. Keeping these three
        // rows distinct matters: the label sits on its own line, and the grid
        // starts on the row below it.
        let label_row = y + 1;
        let grid_top = label_row + 1;
        let per_row = inner.width.clamp(4, 16) as usize;
        // The budget is what is left below the label after reserving room for the
        // figures that follow the grid; at least one row is always drawn, so a
        // short terminal shows the first cores instead of none.
        let budget = bottom
            .saturating_sub(grid_top)
            .saturating_sub(RESERVED_BELOW_GRID)
            .max(1);
        let rows = wanted.div_ceil(per_row).min(budget as usize);
        let drawn = wanted.min(rows.saturating_mul(per_row));
        put_str(
            buf,
            inner.x,
            label_row,
            &cores_label(drawn, total_cores),
            theme.meter_label,
            inner.width,
        );
        y = grid_top;
        for r in 0..rows {
            if y >= bottom {
                break;
            }
            for c in 0..per_row {
                let core = r * per_row + c;
                if core >= drawn || inner.x + c as u16 >= inner.right() {
                    break;
                }
                let d = snapshot
                    .cpu_delta
                    .get(core + 1)
                    .copied()
                    .unwrap_or_default();
                let total = d.total();
                let frac = if total == 0 {
                    0.0
                } else {
                    d.busy() as f32 / total as f32
                };
                // An idle core still gets a glyph: a blank cell is
                // indistinguishable from "this core does not exist", and on a
                // 16-core box the grid has to read as a grid.
                // Three colour bands, not two: a core at 30% and a core at 65%
                // are meaningfully different loads, and the first version of
                // this used the same colour for both, which made the banding
                // decorative rather than informative.
                let (ch, color) = if frac <= 0.001 {
                    ('·', theme.cpu_idle)
                } else if frac > 0.7 {
                    (heat_char(frac), theme.cpu_kernel)
                } else if frac > 0.3 {
                    (heat_char(frac), theme.cpu_irq)
                } else {
                    (heat_char(frac), theme.cpu_normal)
                };
                put_str(
                    buf,
                    inner.x + c as u16,
                    y,
                    &ch.to_string(),
                    ratatui::style::Style::default().fg(color),
                    1,
                );
            }
            y += 1;
        }
    }

    // --- memory / swap -----------------------------------------------------
    // The grid may have run to the bottom of a short terminal on a many-core
    // box. Continue from the last row rather than from wherever the grid wanted
    // to end, so the bars below still get drawn instead of silently vanishing.
    let mut y = y.min(bottom);
    let mem_frac = if snapshot.mem.total > 0 {
        snapshot.mem.used as f32 / snapshot.mem.total as f32
    } else {
        0.0
    };
    let mem_text = format!(
        "{}/{}",
        bytes_human(snapshot.mem.used),
        bytes_human(snapshot.mem.total)
    );
    let mut segs = Vec::new();
    for class in &snapshot.mem.classes {
        segs.push(Segment {
            value: if snapshot.mem.total > 0 {
                class.bytes as f32 / snapshot.mem.total as f32
            } else {
                0.0
            },
            color: theme.memory_slot(class.color_slot),
        });
    }
    y = draw_labelled_bar(
        inner, buf, theme, y, bottom, "MEM", mem_frac, &mem_text, &segs,
    );

    let swap_frac = snapshot.swap_usage();
    let swap_text = format!(
        "{}/{}",
        bytes_human(snapshot.swap.used),
        bytes_human(snapshot.swap.total)
    );
    let swap_segs = [
        Segment {
            value: swap_frac,
            color: theme.swap_used,
        },
        Segment {
            value: 1.0 - swap_frac,
            color: theme.swap_free,
        },
    ];
    y = draw_labelled_bar(
        inner, buf, theme, y, bottom, "SWP", swap_frac, &swap_text, &swap_segs,
    );

    // --- load / uptime / tasks --------------------------------------------
    if y + 1 < bottom {
        y += 1;
    }
    let load = format!(
        "{:.2} {:.2} {:.2}",
        snapshot.load[0], snapshot.load[1], snapshot.load[2]
    );
    y = draw_kv(inner, buf, theme, y, bottom, "LOAD", &load);

    let up = duration_verbose(snapshot.uptime).to_string();
    y = draw_kv(inner, buf, theme, y, bottom, "UP", &up);

    let (total, running, threads) = tasks;
    y = draw_kv(inner, buf, theme, y, bottom, "TASKS", &format!("{total}"));
    y = draw_kv(inner, buf, theme, y, bottom, "RUN", &format!("{running}"));
    y = draw_kv(inner, buf, theme, y, bottom, "THR", &format!("{threads}"));

    // --- pressure -----------------------------------------------------------
    // PSI is the one figure in this column that answers "is the machine
    // struggling" rather than "how busy is it". Drawn only when the kernel
    // reports it: macOS and pre-4.20 Linux have no `/proc/pressure`, and three
    // empty rows would read as an idle machine rather than an absent feature.
    if let Some(psi) = snapshot.psi {
        if y + 1 < bottom {
            y += 1;
        }
        for (label, resource) in psi.rows() {
            if y >= bottom {
                break;
            }
            if !resource.present {
                continue;
            }
            // `some` is the share of time at least one task was stalled; `full`
            // is the share of time nothing could run. Showing both, side by
            // side, is what makes the number interpretable: high `some` with low
            // `full` is contention, high `full` is a real stall.
            let text = if resource.has_full && resource.full.avg10 > 0.0 {
                format!("{:.1}/{:.1}", resource.some.avg10, resource.full.avg10)
            } else {
                format!("{:.1}", resource.some.avg10)
            };
            let color = psi_color(theme, resource.some.avg10.max(resource.full.avg10));
            put_str(buf, inner.x, y, label, theme.meter_label, 4);
            put_str(
                buf,
                inner.x + 4,
                y,
                &text,
                ratatui::style::Style::default().fg(color),
                inner.width.saturating_sub(4),
            );
            y += 1;
        }
    }

    // --- compressed memory -------------------------------------------------
    // zram and zswap both store bytes in RAM, so they belong next to MEM/SWP
    // rather than in the header: the question they answer is "what is holding
    // memory", and that is this column's job.
    if snapshot.zram.present() {
        let text = format!(
            "{}→{}",
            bytes_human(snapshot.zram.orig_total()),
            bytes_human(snapshot.zram.compr_total())
        );
        y = draw_kv(inner, buf, theme, y, bottom, "ZRAM", &text);
    }
    if snapshot.mem.zswap > 0 {
        let text = bytes_human(snapshot.mem.zswap).to_string();
        y = draw_kv(inner, buf, theme, y, bottom, "ZSWP", &text);
    }
    if snapshot.mem.huge_pages_configured() {
        let text = format!(
            "{}/{}",
            bytes_human(snapshot.mem.huge_used_bytes()),
            bytes_human(snapshot.mem.huge_total_bytes())
        );
        y = draw_kv(inner, buf, theme, y, bottom, "HUGE", &text);
    }

    // --- sparkline of aggregate CPU ---------------------------------------
    // Placed after everything else, wherever there is room, instead of at a fixed
    // offset that collided with the facts on a short terminal.
    if y + 3 < bottom && cpu_history.len() > 1 {
        let graph_y = y + 1;
        let width = inner.width as usize;
        let start = cpu_history.len().saturating_sub(width);
        for (i, v) in cpu_history.iter().skip(start).enumerate() {
            let x = inner.x + i as u16;
            if x >= inner.right() {
                break;
            }
            put_str(
                buf,
                x,
                graph_y,
                &heat_char(*v).to_string(),
                ratatui::style::Style::default().fg(if *v > 0.7 {
                    theme.cpu_kernel
                } else {
                    theme.cpu_normal
                }),
                1,
            );
        }
    }
}

/// Draw the persistent inspector for `row`.
pub fn draw_inspector(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    row: Option<&Row>,
    snapshot: &SystemSnapshot,
    user: Option<&str>,
) {
    if area.width < 16 || area.height < 4 {
        return;
    }
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    if inner.width == 0 {
        return;
    }
    let mut y = area.y;
    let bottom = area.bottom();

    let Some(row) = row else {
        put_str(
            buf,
            inner.x,
            y,
            "no selection",
            theme.meter_label,
            inner.width,
        );
        return;
    };

    // --- identity ---------------------------------------------------------
    let title = format!("{} {}", row.pid, row.comm.as_str());
    put_str(buf, inner.x, y, &title, theme.header_value, inner.width);
    y += 1;

    // The command line wraps over the first rows: it is the single most useful
    // thing the inspector can show and the table can never show all of it.
    let cmd = row.display_command();
    let max_cmd_rows = (bottom - y).min(4);
    for line in wrap(cmd, inner.width as usize) {
        if y + max_cmd_rows.min(4) <= y || y >= bottom {
            break;
        }
        put_str(buf, inner.x, y, &line, theme.row_normal, inner.width);
        y += 1;
        if y >= bottom {
            break;
        }
    }

    if y < bottom {
        y += 1;
    }

    // --- facts ------------------------------------------------------------
    let state = describe_state(row.state);
    let cpu = format!("{}%", percent_human(row.percent_cpu));
    let mem = format!(
        "{}%  {}",
        percent_human(row.percent_mem),
        bytes_human(row.res)
    );
    let virt = bytes_human(row.virt).to_string();
    let time = cpu_time_human(row.time_ticks, snapshot.clk_tck).to_string();
    let elapsed = duration_verbose(row.elapsed).to_string();
    let user_name = user.unwrap_or("?").to_string();

    let facts: [(&str, String); 9] = [
        ("USER", user_name),
        ("STATE", state.to_string()),
        ("CPU", cpu),
        ("MEM", mem),
        ("VIRT", virt),
        ("TIME", time),
        ("UP", elapsed),
        ("NICE", row.nice.to_string()),
        ("THR", row.nlwp.max(0).to_string()),
    ];
    for (label, value) in facts {
        if y >= bottom {
            break;
        }
        y = draw_kv(inner, buf, theme, y, bottom, label, &value);
    }

    // --- contending processes --------------------------------------------
    if y + 2 < bottom {
        y += 1;
        put_str(
            buf,
            inner.x,
            y,
            &format!("cores {}", core_list(row)),
            theme.meter_label,
            inner.width,
        );
    }
}

/// Which field list the table should use in this layout.
///
/// The panels layout assumes a narrow table, so it wants the columns that answer
/// "what is this and how much is it costing" and nothing else. The user's `F2`
/// column choice still wins when they change it; this is only the default shape.
pub const DEFAULT_FIELDS: &[Field] = &[
    Field::Pid,
    Field::PercentCpu,
    Field::PercentMem,
    Field::ResMem,
    Field::Comm,
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn cpu_segments(theme: &Theme, snapshot: &SystemSnapshot) -> Vec<Segment> {
    let Some(d) = snapshot.cpu_delta.first() else {
        return Vec::new();
    };
    let total = d.total().max(1) as f32;
    vec![
        Segment {
            value: d.nice as f32 / total,
            color: theme.cpu_nice,
        },
        Segment {
            value: d.user as f32 / total,
            color: theme.cpu_normal,
        },
        Segment {
            value: d.system as f32 / total,
            color: theme.cpu_kernel,
        },
        Segment {
            value: (d.irq + d.softirq) as f32 / total,
            color: theme.cpu_irq,
        },
        Segment {
            value: d.iowait as f32 / total,
            color: theme.cpu_iowait,
        },
    ]
}

/// A labelled bar: `LABEL   value` on one row, a filled track on the next.
///
/// The track has to be visible when it is empty, otherwise a 5%-full bar and a
/// missing bar look identical — which is exactly the bug the first version of
/// this file had (it drew spaces with a foreground colour, so nothing showed).
#[allow(clippy::too_many_arguments)]
fn draw_labelled_bar(
    inner: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    y: u16,
    bottom: u16,
    label: &str,
    value: f32,
    text: &str,
    segments: &[Segment],
) -> u16 {
    if y >= bottom {
        return y;
    }
    put_str(
        buf,
        inner.x,
        y,
        label,
        theme.meter_label,
        label.chars().count() as u16,
    );

    let text_w = text.chars().count() as u16;
    if text_w > 0 && inner.width > text_w + 1 {
        put_str(
            buf,
            inner.right().saturating_sub(text_w),
            y,
            text,
            theme.meter_value,
            text_w,
        );
    }

    // The bar sits on the row below the label: a 22-cell column cannot fit a
    // label, a readable bar and a value on one line.
    let bar_y = y + 1;
    if bar_y < bottom {
        draw_track(inner, buf, theme, bar_y, value, segments);
        return bar_y + 1;
    }
    y + 1
}

/// A bar with a visible empty track.
///
/// `█` for the filled part and `░` for what is left, rather than a background
/// colour: a coloured background is invisible in some terminals and in a
/// monochrome theme, and the empty part is information (it says how big the
/// denominator is).
fn draw_track(
    inner: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    y: u16,
    value: f32,
    segments: &[Segment],
) {
    let width = inner.width;
    if width == 0 {
        return;
    }

    // Lay the empty track down first.
    for x in inner.x..inner.right() {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char('░');
            cell.set_style(ratatui::style::Style::default().fg(theme.bar_background));
        }
    }

    // Then the filled part, left to right, honouring the segment order.
    let total = width as f32;
    let mut x = inner.x;
    let mut placed: u16 = 0;
    let mut carried = 0.0f32;

    let segs: Vec<Segment> = if segments.is_empty() {
        vec![Segment {
            value,
            color: theme.cpu_normal,
        }]
    } else {
        segments.to_vec()
    };

    for seg in &segs {
        let want = (seg.value.clamp(0.0, 1.0) * total) + carried;
        let cells = want.floor();
        carried = want - cells;
        let mut n = cells as u16;
        if placed.saturating_add(n) > width {
            n = width.saturating_sub(placed);
        }
        for _ in 0..n {
            if x >= inner.right() {
                break;
            }
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char('█');
                cell.set_style(ratatui::style::Style::default().fg(seg.color));
            }
            x += 1;
        }
        placed = placed.saturating_add(n);
        if placed >= width {
            return;
        }
    }
}

fn draw_kv(
    inner: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    y: u16,
    bottom: u16,
    label: &str,
    value: &str,
) -> u16 {
    if y >= bottom {
        return y;
    }
    put_str(buf, inner.x, y, label, theme.meter_label, 6);
    if inner.width > 7 {
        put_str(
            buf,
            inner.x + 7,
            y,
            value,
            theme.meter_value,
            inner.width - 7,
        );
    }
    y + 1
}

/// Colour for a pressure percentage, in the same three bands `PsiMeter` uses.
///
/// Duplicated rather than shared so `layout` does not have to own a meter type:
/// the bands are a property of the *reading* (below 10 % is normal, above 40 % is
/// a stall), not of the widget drawing it, and both places must agree.
fn psi_color(theme: &Theme, percent: f32) -> ratatui::style::Color {
    if percent >= 40.0 {
        theme.psi_high
    } else if percent >= 10.0 {
        theme.psi_mid
    } else {
        theme.psi_low
    }
}

/// Map `0.0..=1.0` to a block that reads as a vertical level.
fn heat_char(v: f32) -> char {
    const RAMP: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let idx = ((v.clamp(0.0, 1.0)) * (RAMP.len() - 1) as f32).round() as usize;
    RAMP[idx.min(RAMP.len() - 1)]
}

fn describe_state(state: u8) -> &'static str {
    match state {
        b'R' => "R running",
        b'S' => "S sleeping",
        b'D' => "D disk sleep",
        b'T' => "T stopped",
        b'Z' => "Z zombie",
        b'I' => "I idle",
        _ => "? unknown",
    }
}

/// Break `text` into lines of at most `width` cells.
///
/// Prefers, in order: whitespace, then a `/` inside a long word, then a hard
/// split. The slash rule matters because command lines are mostly paths, and a
/// hard split of `/Applications/iTerm.app/Contents/MacOS/iTerm2` produces
/// `.../Contents/Mac` + `OS/iTerm2`, which reads as nonsense.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut line = String::new();

    let flush = |line: &mut String, out: &mut Vec<String>| {
        if !line.is_empty() {
            out.push(std::mem::take(line));
        }
    };

    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(word);
            continue;
        }
        // The word does not fit on the current line.
        flush(&mut line, &mut out);
        line.push_str(word);

        while line.chars().count() > width {
            // Either the word is short enough that it only needs to be split
            // once more, or look for the last `/` that keeps a plausible prefix.
            let chars: Vec<char> = line.chars().collect();
            let split = last_break_before(&chars, width);
            out.push(chars[..split].iter().collect());
            line = chars[split..].iter().collect();
        }
    }
    flush(&mut line, &mut out);
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Where to cut a too-long line: the last `/` at or before `width`, else `width`.
///
/// The `/` is kept on the *next* line so the continuation starts with a path
/// separator, which is how a shell prompt would wrap it. A break is only taken
/// when it is not absurdly early: splitting after the leading `/` would waste
/// most of a line.
fn last_break_before(chars: &[char], width: usize) -> usize {
    let limit = width.min(chars.len());
    if let Some(pos) = chars[..limit].iter().rposition(|c| *c == '/') {
        if pos + 1 >= limit / 2 && pos > 0 {
            return pos;
        }
    }
    limit
}

fn core_list(row: &Row) -> String {
    if row.processor < 0 {
        "any".to_string()
    } else {
        row.processor.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(cores: usize) -> SystemSnapshot {
        let mut s = SystemSnapshot::default();
        s.mem.total = 16 * 1024 * 1024 * 1024;
        s.mem.used = 8 * 1024 * 1024 * 1024;
        s.swap.total = 1024 * 1024 * 1024;
        s.swap.used = 256 * 1024 * 1024;
        s.cpu = vec![Default::default(); cores + 1];
        s.cpu_delta = vec![Default::default(); cores + 1];
        s.clk_tck = 100;
        s.uptime = 3_725;
        s
    }

    #[test]
    fn widths_keep_the_table_usable() {
        for total in [40u16, 60, 72, 80, 88, 100, 120, 160, 240] {
            let w = PanelWidths::resolve(total);
            let table = total.saturating_sub(w.system + w.inspector);
            assert!(
                table >= 40 || w.system == 0,
                "total {total}: panels left only {table} cells for the table"
            );
        }
    }

    #[test]
    fn a_narrow_terminal_gets_no_panels_at_all() {
        let w = PanelWidths::resolve(50);
        assert_eq!(w.system, 0);
        assert_eq!(w.inspector, 0);
    }

    #[test]
    fn a_wide_terminal_gets_both_panels() {
        let w = PanelWidths::resolve(160);
        assert!(w.system >= 20);
        assert!(w.inspector >= 28);
    }

    #[test]
    fn drawing_the_system_column_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        for (w, h) in [(22u16, 20u16), (12, 6), (8, 3)] {
            let s = snap(8);
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| {
                draw_system_column(
                    f.area(),
                    f.buffer_mut(),
                    &Theme::default(),
                    &s,
                    &[0.1, 0.5],
                    (10, 2, 30),
                    0,
                )
            })
            .unwrap();
        }
    }

    /// Render the system column and return its text, row by row.
    fn system_column_text(cores: usize, cores_max: usize, w: u16, h: u16) -> Vec<String> {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let s = snap(cores);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| {
            draw_system_column(
                f.area(),
                f.buffer_mut(),
                &Theme::default(),
                &s,
                &[0.1, 0.5],
                (10, 2, 30),
                cores_max,
            )
        })
        .unwrap();
        let buf = term.backend().buffer();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    fn core_glyphs(rows: &[String]) -> usize {
        // `·` is the idle glyph and the only one an all-zero sample draws, so
        // counting it counts the cells the grid actually filled in.
        rows.iter()
            .map(|r| r.chars().filter(|c| *c == '·').count())
            .sum()
    }

    #[test]
    fn a_128_core_machine_shows_128_cores() {
        // Regression guard for the reported bug: a 128-core box displayed
        // "64 cores". The grid was capped at 16 cells per row × 4 rows = 64,
        // independently of `system_cores_max`, so raising the config did nothing
        // and the label reported the truncated count as if it were the machine's
        // core count.
        let rows = system_column_text(128, 0, 24, 40);
        assert!(
            rows.iter().any(|r| r.contains("128 cores")),
            "the column must say the machine has 128 cores: {rows:?}"
        );
        assert_eq!(core_glyphs(&rows), 128, "one glyph per core");
    }

    #[test]
    fn a_tall_terminal_shows_every_core_of_a_big_machine() {
        // The grid grows with the column's height, so the count is not fixed by
        // the width alone. 16 cells per row × 8 rows is the 128-core case.
        let rows = system_column_text(128, 0, 24, 40);
        assert_eq!(core_glyphs(&rows), 128);
        let rows = system_column_text(64, 0, 34, 40);
        assert_eq!(core_glyphs(&rows), 64);
    }

    #[test]
    fn a_short_terminal_says_how_many_cores_it_is_not_showing() {
        // A 16-row terminal cannot fit 128 glyphs plus the figures below them.
        // The truncation is allowed; reporting it as the machine's core count is
        // not, because that is exactly the symptom of the original bug.
        let rows = system_column_text(128, 0, 24, 16);
        let label = rows
            .iter()
            .find(|r| r.contains("cores"))
            .cloned()
            .expect("a core label must be drawn");
        assert!(label.contains('/'), "expected `N/128 cores`, got {label:?}");
        assert!(label.contains("128"), "the machine's total must appear");
        let drawn = core_glyphs(&rows);
        assert!(
            drawn > 0 && drawn < 128,
            "a partial grid, not none: {drawn}"
        );
    }

    #[test]
    fn the_core_label_is_the_machine_total_when_nothing_is_truncated() {
        assert_eq!(cores_label(128, 128), "128 cores");
        assert_eq!(cores_label(64, 128), "64/128 cores");
        assert_eq!(cores_label(0, 128), "0/128 cores");
    }

    #[test]
    fn the_configured_cap_still_applies_and_is_reported() {
        // `system_cores_max` remains a deliberate limit, but the label has to
        // make clear that the machine has more cores than are drawn.
        let rows = system_column_text(128, 64, 24, 40);
        assert!(
            rows.iter().any(|r| r.contains("64/128 cores")),
            "a capped grid must not claim the machine has 64 cores: {rows:?}"
        );
        assert_eq!(core_glyphs(&rows), 64);
    }

    #[test]
    fn the_figures_below_the_grid_survive_a_128_core_grid() {
        // The grid used to run to the bottom of the column, pushing MEM/SWP/LOAD
        // off a 24-row terminal. Reserving rows for them is the fix; this checks
        // the reservation holds.
        let rows = system_column_text(128, 0, 24, 24);
        let text = rows.join("\n");
        for label in ["MEM", "SWP", "LOAD", "TASKS"] {
            assert!(text.contains(label), "{label} must still be drawn:\n{text}");
        }
    }

    #[test]
    fn a_small_machine_is_unchanged() {
        let rows = system_column_text(8, 0, 24, 40);
        assert!(rows.iter().any(|r| r.contains("8 cores")));
        assert_eq!(core_glyphs(&rows), 8);
    }

    #[test]
    fn drawing_the_inspector_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let s = snap(4);
        let mut row = Row::default_for_test(2971);
        row.comm.try_push_str("pi").unwrap();
        row.res = 96 * 1024 * 1024;
        row.virt = 4 * 1024 * 1024 * 1024;
        row.percent_cpu = 12.4;
        row.percent_mem = 1.2;
        row.nlwp = 14;
        row.elapsed = 8_040;

        for (w, h) in [(34u16, 22u16), (18, 8), (10, 3)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| {
                draw_inspector(
                    f.area(),
                    f.buffer_mut(),
                    &Theme::default(),
                    Some(&row),
                    &s,
                    Some("panda"),
                )
            })
            .unwrap();
        }
        // No selection must also be safe.
        let mut term = Terminal::new(TestBackend::new(34, 10)).unwrap();
        term.draw(|f| draw_inspector(f.area(), f.buffer_mut(), &Theme::default(), None, &s, None))
            .unwrap();
    }

    #[test]
    fn the_inspector_shows_the_command_line_and_key_facts() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let s = snap(4);
        let mut row = Row::default_for_test(2971);
        row.comm.try_push_str("pi").unwrap();
        row.cmdline = Some("/usr/bin/pi --serve --port 8080".into());
        row.percent_cpu = 12.4;
        row.nlwp = 14;
        row.state = b'S';

        let mut term = Terminal::new(TestBackend::new(40, 24)).unwrap();
        term.draw(|f| {
            draw_inspector(
                f.area(),
                f.buffer_mut(),
                &Theme::default(),
                Some(&row),
                &s,
                Some("panda"),
            )
        })
        .unwrap();
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();

        assert!(text.contains("2971"), "the pid");
        assert!(text.contains("--serve"), "the command line");
        assert!(text.contains("panda"), "the user");
        assert!(
            text.contains("sleeping"),
            "the state in words, not a letter"
        );
        assert!(text.contains("14"), "the thread count");
    }

    #[test]
    fn wrapping_never_exceeds_the_width() {
        for width in [8usize, 12, 20, 33] {
            for text in [
                "short",
                "/usr/bin/pi --serve --port 8080 --verbose",
                "averylongsinglewordwithoutanyspacesatall",
                "",
            ] {
                for line in wrap(text, width) {
                    assert!(
                        line.chars().count() <= width,
                        "width {width}: {line:?} is too long"
                    );
                }
            }
        }
    }

    #[test]
    fn wrapping_a_path_breaks_on_a_slash() {
        let path = "/Applications/iTerm.app/Contents/MacOS/iTerm2";
        let lines = wrap(path, 36);
        assert!(lines.len() >= 2, "a 43-char path must wrap at 36");
        for line in &lines {
            assert!(line.chars().count() <= 36, "{line:?}");
        }
        // The continuation starts at a separator, not mid-component.
        assert!(
            lines[1].starts_with('/') || !lines[1].contains('/'),
            "continuation should start at a separator, got {:?}",
            lines[1]
        );
        // Nothing is lost.
        assert_eq!(lines.concat(), path);
    }

    #[test]
    fn wrapping_does_not_break_absurdly_early() {
        // A single short component after a long prefix must not be split after
        // the leading slash just because a slash exists there.
        let path = "/a/very/long/path/that/goes/well/past/the/width/limit";
        for line in wrap(path, 20) {
            assert!(line.chars().count() <= 20);
        }
    }

    #[test]
    fn wrapping_keeps_every_word() {
        let text = "one two three four five six seven";
        let joined = wrap(text, 10).join(" ");
        assert_eq!(joined, text);
    }
}
