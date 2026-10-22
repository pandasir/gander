//! Meters: the header widgets that show host-wide statistics.
//!
//! `trait Meter` replaces htop's `Meter` vtable. Rendering is immediate-mode:
//! each meter writes directly into the frame's `Buffer` for its `Rect`.

use std::collections::VecDeque;

use gander_core::system::SystemSnapshot;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::theme::Theme;

/// Which header column a meter is drawn in.
///
/// htop keeps an explicit two-column layout (`hColumns[0]` / `hColumns[1]`).
/// Declaring the column on the meter avoids inferring the split point from a
/// meter's name.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum HeaderColumn {
    Left,
    Right,
}

/// How a meter presents its value, matching htop's `MeterMode`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MeterMode {
    Text,
    Bar,
    Graph,
}

/// History depth for graph mode. htop keeps one sample per column.
pub const GRAPH_HISTORY: usize = 256;

/// A host-statistics widget.
pub trait Meter {
    /// Stable name used by the config file.
    fn name(&self) -> &'static str;

    /// Take a sample from the latest snapshot.
    fn sample(&mut self, snapshot: &SystemSnapshot);

    /// Minimum width in cells.
    fn min_width(&self) -> u16 {
        8
    }

    /// How many terminal rows this meter wants.
    ///
    /// Most meters are one row. htop's multi-core CPU meters stack one row per
    /// core, which is why a fixed-height header clips them.
    fn rows(&self) -> u16 {
        1
    }

    /// Which header column this meter belongs to.
    ///
    /// htop keeps an explicit two-column layout (`hColumns[0]` / `hColumns[1]`).
    /// Declaring the column on the meter avoids inferring the split from a
    /// meter's name, which is what produced a duplicated CPU block earlier.
    fn column(&self) -> HeaderColumn {
        HeaderColumn::Left
    }

    /// Seconds of history this meter keeps, if it keeps any.
    fn history(&self) -> Option<&VecDeque<f32>> {
        None
    }

    /// Draw into `area`.
    fn render(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode);
}

// ---------------------------------------------------------------------------
// Drawing primitives
// ---------------------------------------------------------------------------

/// A segment of a bar: a value in `0.0..=1.0` and the color to draw it with.
#[derive(Copy, Clone, Debug)]
pub struct Segment {
    pub value: f32,
    pub color: ratatui::style::Color,
}

/// Fill `width` cells of a row with stacked segments.
///
/// Fractional segments are rounded with a running remainder so the bar fills
/// exactly and does not drift (`docs/DESIGN.md`: the CPU bar length must equal
/// `width * percentage`).
pub fn draw_segments(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    segments: &[Segment],
    background: ratatui::style::Color,
    ch: char,
) {
    if width == 0 {
        return;
    }
    let mut x = x;
    let mut remaining = width as f32;
    let mut accumulated = 0.0_f32;

    for seg in segments {
        if x >= buf.area.right() {
            return;
        }
        let want = (seg.value.clamp(0.0, 1.0) * width as f32) + accumulated;
        let cells = want.floor();
        accumulated = want - cells;
        let mut cells = cells as u16;
        if cells > remaining as u16 {
            cells = remaining as u16;
        }
        for _ in 0..cells {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(ch);
                cell.set_style(Style::default().fg(seg.color));
            }
            x += 1;
        }
        remaining -= cells as f32;
        if remaining <= 0.0 {
            return;
        }
    }

    // Anything left is idle space.
    while x < buf.area.right() && remaining > 0.0 {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(' ');
            cell.set_style(Style::default().bg(background));
        }
        x += 1;
        remaining -= 1.0;
    }
}

/// The braille-style ramp used for graph mode.
const GRAPH_RAMP: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Draw a sparkline of `values` (0.0..=1.0) into one row.
pub fn draw_graph(buf: &mut Buffer, area: Rect, values: &VecDeque<f32>, theme: &Theme, max: f32) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let width = area.width as usize;
    let start = values.len().saturating_sub(width);
    for (i, value) in values.iter().skip(start).enumerate() {
        let x = area.x + i as u16;
        if x >= area.right() {
            break;
        }
        let norm = if max > 0.0 {
            (value / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let idx =
            ((norm * (GRAPH_RAMP.len() - 1) as f32).round() as usize).min(GRAPH_RAMP.len() - 1);
        let color = if norm > 0.75 {
            theme.graph_high
        } else {
            theme.graph_low
        };
        if let Some(cell) = buf.cell_mut((x, area.y)) {
            cell.set_char(GRAPH_RAMP[idx]);
            cell.set_style(Style::default().fg(color));
        }
    }
}

/// Replace characters that must never reach the terminal.
///
/// Process names and command lines are arbitrary bytes: a process can name itself
/// with a newline, an escape sequence, or a control code. Passing those to the
/// backend either panics (`ratatui` rejects control characters) or, worse, drives
/// the terminal itself.
///
/// Tabs are the one exception worth keeping readable, so they become a space.
fn sanitize(ch: char) -> char {
    match ch {
        '\t' => ' ',
        c if c.is_control() => '?',
        c => c,
    }
}

/// Write `text` at `(x, y)`, clipped to `max_width`.
pub fn put_str(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style, max_width: u16) {
    let limit = x.saturating_add(max_width);
    for (dx, ch) in (0u16..).zip(text.chars()) {
        let x = x + dx;
        if x >= limit || x >= buf.area.right() {
            break;
        }
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(sanitize(ch));
            cell.set_style(style);
        }
    }
}

// ---------------------------------------------------------------------------
// CPU meter
// ---------------------------------------------------------------------------

/// Which cores a `CpuMeter` shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CpuSpan {
    /// One row: the aggregate across every core.
    Aggregate,
    /// One row for the average, then one row per core in `range`.
    Range(std::ops::Range<usize>),
}

/// One rendered row of a CPU meter.
struct CpuRow {
    label: String,
    usage: f32,
    delta: gander_core::system::CpuTimes,
    history: VecDeque<f32>,
}

impl CpuRow {
    fn new(label: String) -> Self {
        Self {
            label,
            usage: 0.0,
            delta: gander_core::system::CpuTimes::default(),
            history: VecDeque::with_capacity(GRAPH_HISTORY),
        }
    }
}

/// Aggregate or per-core CPU usage.
///
/// htop's header puts `LeftCPUs` and `RightCPUs` side by side on machines with
/// more than four cores, so one meter can occupy several stacked rows.
pub struct CpuMeter {
    span: CpuSpan,
    column: HeaderColumn,
    rows: Vec<CpuRow>,
    /// Aggregate mirror, so single-value consumers still work.
    usage: f32,
    history: VecDeque<f32>,
}

impl CpuMeter {
    /// A single row showing the average across all cores.
    pub fn aggregate() -> Self {
        Self::build(CpuSpan::Aggregate, HeaderColumn::Left)
    }

    /// The left half of a split header: average, then cores in `range`.
    pub fn left(range: std::ops::Range<usize>) -> Self {
        Self::build(CpuSpan::Range(range), HeaderColumn::Left)
    }

    /// The right half of a split header.
    pub fn right(range: std::ops::Range<usize>) -> Self {
        Self::build(CpuSpan::Range(range), HeaderColumn::Right)
    }

    fn build(span: CpuSpan, column: HeaderColumn) -> Self {
        let mut rows = Vec::new();
        match &span {
            CpuSpan::Aggregate => rows.push(CpuRow::new("CPU".to_string())),
            CpuSpan::Range(range) => {
                rows.push(CpuRow::new("Avg".to_string()));
                for core in range.clone() {
                    rows.push(CpuRow::new(core.to_string()));
                }
            }
        }
        Self {
            span,
            column,
            rows,
            usage: 0.0,
            history: VecDeque::with_capacity(GRAPH_HISTORY),
        }
    }

    /// Total rows this meter needs for `cpu_count` cores.
    fn expected_rows(&self) -> usize {
        match &self.span {
            CpuSpan::Aggregate => 1,
            CpuSpan::Range(r) => r.len() + 1,
        }
    }

    /// Split the tick delta into htop's bar segments.
    ///
    /// The order and colors must match the legend in `htop.1` METERS:
    /// nice, normal, kernel, irq, soft-irq, steal, guest, io-wait.
    fn segments_for(theme: &Theme, delta: &gander_core::system::CpuTimes) -> Vec<Segment> {
        let total = delta.total().max(1) as f32;
        vec![
            Segment {
                value: delta.nice as f32 / total,
                color: theme.cpu_nice,
            },
            Segment {
                value: delta.user as f32 / total,
                color: theme.cpu_normal,
            },
            Segment {
                value: delta.system as f32 / total,
                color: theme.cpu_kernel,
            },
            Segment {
                value: delta.irq as f32 / total,
                color: theme.cpu_irq,
            },
            Segment {
                value: delta.softirq as f32 / total,
                color: theme.cpu_softirq,
            },
            Segment {
                value: delta.steal as f32 / total,
                color: theme.cpu_steal,
            },
            Segment {
                value: (delta.guest + delta.guest_nice) as f32 / total,
                color: theme.cpu_guest,
            },
            Segment {
                value: delta.iowait as f32 / total,
                color: theme.cpu_iowait,
            },
        ]
    }
}

impl Meter for CpuMeter {
    fn name(&self) -> &'static str {
        match self.span {
            CpuSpan::Aggregate => "CPU",
            CpuSpan::Range(_) => "CPU_CORES",
        }
    }

    fn min_width(&self) -> u16 {
        6
    }

    fn rows(&self) -> u16 {
        self.rows.len() as u16
    }

    fn column(&self) -> HeaderColumn {
        self.column
    }

    fn history(&self) -> Option<&VecDeque<f32>> {
        Some(&self.history)
    }

    fn sample(&mut self, snapshot: &SystemSnapshot) {
        // Rebuild only when the core count changed (SMT toggle, VM resize).
        if self.rows.len() != self.expected_rows() {
            let span = self.span.clone();
            let column = self.column;
            let usage = self.usage;
            let history = std::mem::take(&mut self.history);
            *self = Self::build(span, column);
            self.usage = usage;
            self.history = history;
        }

        for (i, row) in self.rows.iter_mut().enumerate() {
            // Row 0 is the aggregate at delta index 0. With a range, row i>0 is
            // core `range.start + i - 1`, stored at delta index `core + 1`.
            let idx = match &self.span {
                CpuSpan::Aggregate => 0,
                CpuSpan::Range(_) if i == 0 => 0,
                CpuSpan::Range(r) => r.start + i,
            };
            row.delta = snapshot.cpu_delta.get(idx).copied().unwrap_or_default();
            let total = row.delta.total();
            row.usage = if total == 0 {
                0.0
            } else {
                row.delta.busy() as f32 / total as f32
            };
            if row.history.len() == GRAPH_HISTORY {
                row.history.pop_front();
            }
            row.history.push_back(row.usage);
        }

        if let Some(first) = self.rows.first() {
            self.usage = first.usage;
            if self.history.len() == GRAPH_HISTORY {
                self.history.pop_front();
            }
            self.history.push_back(self.usage);
        }
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode) {
        if area.width == 0 {
            return;
        }

        if mode == MeterMode::Graph {
            // Only one series fits; use the aggregate.
            let empty = VecDeque::new();
            let hist = self.rows.first().map(|r| &r.history).unwrap_or(&empty);
            draw_graph(buf, area, hist, theme, 1.0);
            return;
        }

        for (i, row) in self.rows.iter().enumerate() {
            let y = area.y + i as u16;
            if y >= area.bottom() {
                break;
            }
            // Row 0 is the average. When the meter is squeezed below its full
            // height, row 0 is the only row that gets drawn, and a row labelled
            // `Avg` under a `CPU` legend is ambiguous on its own — so the label
            // says what it is rather than leaving the reader to infer it from a
            // core number that is no longer next to it.
            let label = if i == 0 && self.rows.len() > 1 && area.height < self.rows.len() as u16 {
                "CPU"
            } else {
                row.label.as_str()
            };
            match mode {
                MeterMode::Text => {
                    let text = gander_core::format::percent_human(row.usage * 100.0);
                    put_str(buf, area.x, y, label, theme.meter_label, area.width);
                    put_str(
                        buf,
                        area.right().saturating_sub(text.len() as u16),
                        y,
                        &text,
                        theme.meter_value,
                        area.width,
                    );
                }
                MeterMode::Bar => {
                    let label_width = (label.len() as u16 + 1).min(area.width);
                    put_str(buf, area.x, y, label, theme.meter_label, label_width);
                    let bar_x = area.x + label_width;
                    let bar_w = area.width.saturating_sub(label_width + 6);
                    let segs = Self::segments_for(theme, &row.delta);
                    draw_segments(buf, bar_x, y, bar_w, &segs, theme.bar_background, '|');

                    let text = gander_core::format::percent_human(row.usage * 100.0);
                    put_str(
                        buf,
                        area.right().saturating_sub(text.len() as u16),
                        y,
                        &text,
                        theme.meter_value,
                        5,
                    );
                }
                MeterMode::Graph => {}
            }
        }
    }
}
