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

/// Write `text` at `(x, y)`, clipped to `max_width`.
pub fn put_str(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style, max_width: u16) {
    let mut x = x;
    let limit = x.saturating_add(max_width);
    for ch in text.chars() {
        if x >= limit || x >= buf.area.right() {
            break;
        }
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(ch);
            cell.set_style(style);
        }
        x += 1;
    }
}

// ---------------------------------------------------------------------------
// CPU meter
// ---------------------------------------------------------------------------

/// Aggregate or per-core CPU usage.
pub struct CpuMeter {
    /// `None` means the aggregate across all cores.
    core: Option<usize>,
    label: String,
    history: VecDeque<f32>,
    usage: f32,
    delta: gander_core::system::CpuTimes,
}

impl CpuMeter {
    pub fn aggregate() -> Self {
        Self::new(None, "CPU".to_string())
    }

    pub fn core(index: usize) -> Self {
        Self::new(Some(index), format!("{index}"))
    }

    fn new(core: Option<usize>, label: String) -> Self {
        Self {
            core,
            label,
            history: VecDeque::with_capacity(GRAPH_HISTORY),
            usage: 0.0,
            delta: gander_core::system::CpuTimes::default(),
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
        match self.core {
            None => "CPU",
            Some(_) => "CPU_CORE",
        }
    }

    fn min_width(&self) -> u16 {
        6
    }

    fn history(&self) -> Option<&VecDeque<f32>> {
        Some(&self.history)
    }

    fn sample(&mut self, snapshot: &SystemSnapshot) {
        let idx = self.core.map_or(0, |c| c + 1);
        self.delta = match snapshot.cpu_delta.get(idx) {
            Some(d) => *d,
            None => {
                self.usage = 0.0;
                return;
            }
        };
        let total = self.delta.total();
        self.usage = if total == 0 {
            0.0
        } else {
            self.delta.busy() as f32 / total as f32
        };
        if self.history.len() == GRAPH_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(self.usage);
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode) {
        match mode {
            MeterMode::Text => {
                let text = gander_core::format::percent_human(self.usage * 100.0);
                put_str(
                    buf,
                    area.x,
                    area.y,
                    &self.label,
                    theme.meter_label,
                    area.width,
                );
                put_str(
                    buf,
                    area.x + area.width.saturating_sub(text.len() as u16),
                    area.y,
                    &text,
                    theme.meter_value,
                    area.width,
                );
            }
            MeterMode::Bar => {
                let label_width = (self.label.len() as u16 + 1).min(area.width);
                put_str(
                    buf,
                    area.x,
                    area.y,
                    &self.label,
                    theme.meter_label,
                    label_width,
                );
                let bar_x = area.x + label_width;
                let bar_w = area.width.saturating_sub(label_width + 5);
                let segments = Self::segments_for(theme, &self.delta);
                draw_segments(
                    buf,
                    bar_x,
                    area.y,
                    bar_w,
                    &segments,
                    theme.bar_background,
                    '|',
                );

                let text = gander_core::format::percent_human(self.usage * 100.0);
                put_str(
                    buf,
                    area.right().saturating_sub(text.len() as u16),
                    area.y,
                    &text,
                    theme.meter_value,
                    5,
                );
            }
            MeterMode::Graph => {
                draw_graph(buf, area, &self.history, theme, 1.0);
            }
        }
    }
}
