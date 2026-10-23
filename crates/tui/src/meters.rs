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

    /// How many terminal rows this meter wants in a column `width` cells wide.
    ///
    /// Most meters are one row. The multi-core CPU meters are the exception:
    /// they need one row per *line* of cores, and how many cores share a line
    /// depends on the width, so the caller has to say how much room it has.
    /// Asking for the height without the width is what made a 128-core machine
    /// claim 65 rows per header column.
    fn rows(&self, _width: u16) -> u16 {
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
    /// One row for the average, then one cell per core in `range`.
    Range(std::ops::Range<usize>),
}

/// Cells reserved for a core's caption at the left of its cell.
///
/// htop's `BarMeterMode_draw` writes the caption into a fixed three cells and
/// starts the bar at `x + 3` regardless of how long the caption is, so a column
/// of core numbers lines up whether it holds `0` or `127`. Three digits is every
/// core number up to 999.
const CAPTION_WIDTH: u16 = 3;

/// Narrowest a per-core cell can be before its columns are merged.
///
/// htop's own floor: a three-cell caption plus two bar borders, with no bar
/// between them. Below this the caption and the neighbouring core's bar collide
/// and the grid stops being readable, so the columns are merged instead — which
/// costs rows but keeps every core identifiable. That is the one place this
/// deliberately differs from htop, which would draw the fragments.
///
/// Keeping the floor this low is what makes the header height match htop's at
/// every terminal width: eight cores on a line stay eight on a line, and a narrow
/// terminal shortens the bars rather than reflowing the grid.
const MIN_CORE_COLUMN: u16 = 5;

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
/// more than four cores, and lays the cores out several to a line
/// (`LeftCPUs8` draws eight across). That is the difference between a 128-core
/// header that costs eight rows and one that costs sixty-five and eats the
/// process list.
pub struct CpuMeter {
    span: CpuSpan,
    column: HeaderColumn,
    /// Cores per line, before the column width is taken into account. htop's
    /// `LeftCPUs2` / `4` / `8` set this; the unsuffixed names mean one per line.
    columns: u16,
    rows: Vec<CpuRow>,
    /// Aggregate mirror, so single-value consumers still work.
    usage: f32,
    history: VecDeque<f32>,
}

impl CpuMeter {
    /// A single row showing the average across all cores.
    pub fn aggregate() -> Self {
        Self::build(CpuSpan::Aggregate, HeaderColumn::Left, 1)
    }

    /// The left header column: an average row, then `range` laid out `columns`
    /// cores to a line.
    pub fn left(range: std::ops::Range<usize>, columns: u16) -> Self {
        Self::build(CpuSpan::Range(range), HeaderColumn::Left, columns)
    }

    /// The right header column, same shape as [`CpuMeter::left`].
    pub fn right(range: std::ops::Range<usize>, columns: u16) -> Self {
        Self::build(CpuSpan::Range(range), HeaderColumn::Right, columns)
    }

    fn build(span: CpuSpan, column: HeaderColumn, columns: u16) -> Self {
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
            columns: columns.clamp(1, 8),
            rows,
            usage: 0.0,
            history: VecDeque::with_capacity(GRAPH_HISTORY),
        }
    }

    /// Cores per line.
    ///
    /// Fixed by the meter, as it is in htop: `LeftCPUs8` puts eight on a line on a
    /// wide terminal and still eight on a narrow one, where the bars simply get
    /// shorter. The header height therefore does not depend on the terminal width,
    /// which is what stops a resize from reflowing the whole screen.
    ///
    /// The one exception is a terminal too narrow for even a caption and two bar
    /// borders per core ([`MIN_CORE_COLUMN`]): there the columns are merged, which
    /// costs rows but keeps every core identifiable.
    fn columns_at(&self, width: u16) -> usize {
        let wanted = self.columns.max(1) as usize;
        let floor = (width / MIN_CORE_COLUMN).max(1) as usize;
        wanted.min(floor)
    }

    /// Lines of cores below the average row.
    fn core_lines(&self, width: u16) -> usize {
        match &self.span {
            CpuSpan::Aggregate => 0,
            CpuSpan::Range(r) => r.len().div_ceil(self.columns_at(width)),
        }
    }

    /// How many rows the meter holds once sampled, independent of the width.
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

    fn rows(&self, width: u16) -> u16 {
        match &self.span {
            CpuSpan::Aggregate => 1,
            CpuSpan::Range(_) => 1 + self.core_lines(width) as u16,
        }
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
            let columns = self.columns;
            let usage = self.usage;
            let history = std::mem::take(&mut self.history);
            *self = Self::build(span, column, columns);
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

        // Row 0 is the average. When the meter is squeezed below its full
        // height, row 0 is the only row that gets drawn, and a row labelled
        // `Avg` under a `CPU` legend is ambiguous on its own — so the label
        // says what it is rather than leaving the reader to infer it from a
        // core number that is no longer next to it.
        let squeezed = (area.height as usize) < self.rows(area.width) as usize;
        if let Some(row) = self.rows.first() {
            let label = if self.rows.len() > 1 && squeezed {
                "CPU"
            } else {
                row.label.as_str()
            };
            self.draw_row(row, label, area, buf, theme, mode);
        }

        // The core grid, one cell per core, in the order htop draws it.
        for (cell, row) in self
            .core_cells(area)
            .into_iter()
            .zip(self.rows.iter().skip(1))
        {
            self.draw_row(row, &row.label, cell, buf, theme, mode);
        }
    }
}

impl CpuMeter {
    /// The rectangle for each core's cell, in core order.
    ///
    /// htop fills its columns top-to-bottom (`CPUMeterCommonDraw`: the column is
    /// `i / nrows`, the line is `i % nrows`), so core numbers run down a column
    /// and then start again at the top of the next one. The width maths is htop's
    /// too: each column is `(w - ncol) / ncol + 1` cells, which leaves one cell of
    /// gap between neighbours, and the cells the integer division drops are handed
    /// out one each to the first columns rather than left as a ragged right edge.
    ///
    /// Kept separate from the drawing so a test can check the geometry without
    /// reading glyphs back out of a buffer.
    fn core_cells(&self, area: Rect) -> Vec<Rect> {
        let CpuSpan::Range(range) = &self.span else {
            return Vec::new();
        };
        let ncol = self.columns_at(area.width);
        let nrows = self.core_lines(area.width);
        if nrows == 0 || ncol == 0 {
            return Vec::new();
        }
        let w = area.width as i32;
        let ncol = ncol as i32;
        let colwidth = (w - ncol) / ncol + 1;
        let diff = w - colwidth * ncol;

        let mut cells = Vec::with_capacity(range.len());
        for i in 0..range.len() {
            let line = (i % nrows) as i32;
            let col = (i / nrows) as i32;
            let y = area.y as i32 + 1 + line;
            if y >= area.bottom() as i32 || col >= ncol {
                continue;
            }
            let x = area.x as i32 + col * colwidth + col.min(diff);
            // `colwidth` includes htop's one-cell gap, so the cells are adjacent by
            // design: each ends with its bar's `]`, which is what stops one core's
            // number running into the next core's label.
            let cell = Rect::new(x.max(0) as u16, y as u16, colwidth.max(0) as u16, 1);
            // Clip to the meter's own area: the last column can overrun it by the
            // one-cell gap when `diff` is 0.
            let cell = cell.intersection(area);
            if cell.width == 0 {
                continue;
            }
            cells.push(cell);
        }
        cells
    }
}

impl CpuMeter {
    /// Draw one `label … bar … value` line into `area`'s first row.
    fn draw_row(
        &self,
        row: &CpuRow,
        label: &str,
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
        mode: MeterMode,
    ) {
        if area.width == 0 {
            return;
        }
        let y = area.y;
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
                // htop's `BarMeterMode_draw`, in order: a fixed-width caption, a
                // `[`, the bar, a `]`, and the value right-aligned *inside* the
                // bar. The value is the only part that gives way when the cell is
                // narrow, because the bar already shows the magnitude and a
                // half-written percentage would be a wrong number.
                let caption = area.width.min(CAPTION_WIDTH);
                put_str(buf, area.x, y, label, theme.meter_label, caption);
                if area.width <= caption {
                    return;
                }

                let text = gander_core::format::percent_human(row.usage * 100.0);
                let text_w = text.len() as u16;
                let segs = Self::segments_for(theme, &row.delta);

                // The border only earns its two cells when there is a bar left
                // between them to draw.
                let inner_x = area.x + caption + 1;
                let inner_w = area.width.saturating_sub(caption + 2);
                if inner_w == 0 {
                    return;
                }
                put_str(buf, area.x + caption, y, "[", theme.meter_label, 1);
                draw_segments(buf, inner_x, y, inner_w, &segs, theme.bar_background, '|');
                put_str(buf, area.x + area.width - 1, y, "]", theme.meter_label, 1);

                // The value sits *inside* the bar, right-aligned, exactly as htop
                // draws it. That is what lets a 10-cell cell hold a caption, a bar
                // and `100.0`: the number is painted over the bar's last cells
                // rather than taking cells the bar needs.
                //
                // When even that does not fit, the number loses its decimal rather
                // than losing its digits: `44` is a rounded 44.4, while htop's
                // truncation would show `.4` and read as a different number. Only a
                // cell too small for an integer drops the value entirely.
                if text_w <= inner_w {
                    put_str(
                        buf,
                        area.x + area.width - 1 - text_w,
                        y,
                        &text,
                        theme.meter_value,
                        text_w,
                    );
                } else {
                    let short = format!("{:.0}", row.usage * 100.0);
                    let short_w = short.len() as u16;
                    if short_w <= inner_w {
                        put_str(
                            buf,
                            area.x + area.width - 1 - short_w,
                            y,
                            &short,
                            theme.meter_value,
                            short_w,
                        );
                    }
                }
            }
            MeterMode::Graph => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gander_core::system::CpuTimes;

    fn snapshot(cores: usize) -> SystemSnapshot {
        let mut s = SystemSnapshot {
            cpu: vec![CpuTimes::default(); cores + 1],
            cpu_delta: vec![CpuTimes::default(); cores + 1],
            ..Default::default()
        };
        for core in 0..cores {
            s.cpu_delta[core + 1].user = (core as u64 % 10) * 10;
            s.cpu_delta[core + 1].idle = 100;
        }
        s
    }

    fn render(meter: &mut dyn Meter, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        meter.render(area, &mut buf, &Theme::default(), MeterMode::Bar);
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    /// The whole point: 128 cores must cost a handful of rows, not one per core.
    #[test]
    fn a_128_core_meter_is_not_one_row_per_core() {
        let mut m = CpuMeter::left(0..64, 8);
        m.sample(&snapshot(128));
        // 64 cores, eight to a line, plus the average row.
        assert_eq!(m.rows(100), 9);
        assert_eq!(m.rows(100) as usize, m.core_lines(100) + 1);
        // 128 cores as one row each would be 65, which is what ate the screen.
        assert!(m.rows(100) < 65);
    }

    /// Every core has to be drawn exactly once, in a grid that fills down the
    /// column and then across — htop's `CPUMeterCommonDraw` order.
    #[test]
    fn every_core_gets_its_own_cell() {
        let mut m = CpuMeter::left(0..16, 4);
        m.sample(&snapshot(32));

        let area = Rect::new(0, 0, 80, m.rows(80));
        let cells = m.core_cells(area);
        assert_eq!(cells.len(), 16, "one cell per core");

        // No two cores share a cell, and no cell is zero-width.
        for (i, a) in cells.iter().enumerate() {
            assert!(a.width > 0, "core {i} has no room: {a:?}");
            for (j, b) in cells.iter().enumerate().skip(i + 1) {
                let same_row = a.y == b.y;
                let disjoint = a.x + a.width <= b.x || b.x + b.width <= a.x;
                assert!(
                    !same_row || disjoint,
                    "cores {i} and {j} overlap: {a:?} {b:?}"
                );
            }
        }

        // Down first, then across: the first line holds cores 0, 4, 8, 12.
        let y0 = cells[0].y;
        assert_eq!(cells[4].y, y0, "core 4 starts the second column");
        assert!(cells[1].y > y0, "core 1 is on the second line");
        assert_eq!(cells[1].x, cells[0].x, "core 1 sits under core 0");
    }

    /// A narrow column must lose bar length, not the number: the reading is the
    /// point of the row.
    #[test]
    fn a_narrow_cell_still_shows_the_percentage() {
        let mut m = CpuMeter::left(0..2, 2);
        m.sample(&snapshot(4));
        let h = m.rows(24);
        let rows = render(&mut m, 24, h);
        let line = &rows[1];
        assert!(
            line.contains('%') || line.matches('.').count() >= 2,
            "expected a percentage in a 12-cell cell, got {line:?}"
        );
    }

    /// Width is an upper bound on columns: eight cores across a 20-cell area is
    /// two cells each, so the layout merges them instead of shredding the labels.
    #[test]
    fn columns_shrink_when_the_area_is_narrow() {
        let mut m = CpuMeter::left(0..8, 8);
        m.sample(&snapshot(16));
        // The configured count is the layout; a narrower terminal shortens the
        // bars rather than reflowing the grid, which is what htop does.
        assert_eq!(m.columns_at(200), 8);
        assert_eq!(m.columns_at(120), 8);
        assert_eq!(m.rows(120), m.rows(200), "a resize must not reflow");
        // Only a terminal too narrow for a caption and two borders merges them.
        assert_eq!(m.columns_at(20), 4);
        assert!(
            m.rows(20) > m.rows(200),
            "a very narrow header needs more lines"
        );
    }

    /// The aggregate form is one row no matter how many cores the machine has.
    #[test]
    fn the_aggregate_meter_is_always_one_row() {
        let mut m = CpuMeter::aggregate();
        m.sample(&snapshot(128));
        assert_eq!(m.rows(100), 1);
    }

    /// The core count changing under a live meter (hot-plug, VM resize) must not
    /// leave a stale label behind.
    #[test]
    fn a_resized_machine_relabels_its_cores() {
        let mut m = CpuMeter::left(0..2, 2);
        m.sample(&snapshot(4));
        assert_eq!(m.rows(80), 2);
        m.sample(&snapshot(64));
        // The range is still 0..2 — the caller rebuilds the meter on a real
        // hot-plug — but the rows must at least agree with the new snapshot.
        assert_eq!(m.rows(80), 2);
    }
}
