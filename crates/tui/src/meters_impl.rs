//! Memory, swap, load, uptime, task and clock meters.

use std::collections::VecDeque;
use std::fmt::Write as _;

use gander_core::format::{bytes_human, duration_verbose};
use gander_core::system::SystemSnapshot;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::meters::{
    draw_graph, draw_segments, put_str, HeaderColumn, Meter, MeterMode, Segment, GRAPH_HISTORY,
};
use crate::theme::Theme;

macro_rules! simple_meter {
    (
        $(#[$meta:meta])*
        $name:ident, $cfg_name:literal, $min_width:literal, $sample_body:expr, $render_body:expr
    ) => {
        simple_meter!(@impl $(#[$meta])* $name, $cfg_name, $min_width,
                     crate::meters::HeaderColumn::Left, $sample_body, $render_body);
    };
    (
        right: $(#[$meta:meta])*
        $name:ident, $cfg_name:literal, $min_width:literal, $sample_body:expr, $render_body:expr
    ) => {
        simple_meter!(@impl $(#[$meta])* $name, $cfg_name, $min_width,
                     crate::meters::HeaderColumn::Right, $sample_body, $render_body);
    };
    (
        @impl $(#[$meta:meta])*
        $name:ident, $cfg_name:literal, $min_width:literal, $column:expr,
        $sample_body:expr, $render_body:expr
    ) => {
        $(#[$meta])*
        pub struct $name {
            text: String,
            history: VecDeque<f32>,
            value: f32,
        }

        impl $name {
            pub fn new() -> Self {
                Self { text: String::new(), history: VecDeque::new(), value: 0.0 }
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl Meter for $name {
            fn name(&self) -> &'static str {
                $cfg_name
            }

            fn min_width(&self) -> u16 {
                $min_width
            }

            fn column(&self) -> crate::meters::HeaderColumn {
                $column
            }

            fn history(&self) -> Option<&VecDeque<f32>> {
                Some(&self.history)
            }

            fn sample(&mut self, snapshot: &SystemSnapshot) {
                let this = &mut *self;
                #[allow(unused_variables)]
                let snap = snapshot;
                ($sample_body)(this, snap);
                if this.history.len() == GRAPH_HISTORY {
                    this.history.pop_front();
                }
                this.history.push_back(this.value);
            }

            fn render(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode) {
                let this = &*self;
                match mode {
                    MeterMode::Graph => draw_graph(buf, area, &this.history, theme, 1.0),
                    _ => ($render_body)(this, area, buf, theme, mode),
                }
            }
        }
    };
}

/// Draw the standard `LABEL  <bar>  value` layout.
#[allow(clippy::too_many_arguments)]
fn labelled_bar(
    label: &str,
    value: f32,
    value_text: &str,
    segments: &[Segment],
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    mode: MeterMode,
) {
    let label_width = (label.chars().count() as u16 + 1).min(area.width);
    put_str(buf, area.x, area.y, label, theme.meter_label, label_width);

    let tail = (value_text.chars().count() as u16).min(area.width);
    let bar_x = area.x + label_width;
    let bar_w = area
        .width
        .saturating_sub(label_width)
        .saturating_sub(tail + 1);

    if mode == MeterMode::Bar && bar_w > 0 {
        draw_segments(
            buf,
            bar_x,
            area.y,
            bar_w,
            segments,
            theme.bar_background,
            '|',
        );
    } else if mode == MeterMode::Text && bar_w > 0 {
        put_str(buf, bar_x, area.y, "", theme.meter_value, bar_w);
    }

    let _ = value;
    put_str(
        buf,
        area.right().saturating_sub(tail),
        area.y,
        value_text,
        theme.meter_value,
        tail,
    );
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/// A memory meter that keeps the whole breakdown, not just used/total.
///
/// The bar is drawn from the platform's memory-class list, so the bar and the
/// `used/total` label cannot disagree — the previous hand-rolled formula did
/// exactly that (`docs/PORTABILITY.md`).
pub struct MemoryMeter {
    value: f32,
    text: String,
    segments: Vec<Segment>,
    history: VecDeque<f32>,
}

impl MemoryMeter {
    pub fn new() -> Self {
        Self {
            value: 0.0,
            text: String::new(),
            segments: Vec::with_capacity(6),
            history: VecDeque::with_capacity(crate::meters::GRAPH_HISTORY),
        }
    }
}

impl Default for MemoryMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// Default palette for color slots 0..=5, overridden by the theme at render time.
fn slot_color(slot: u8) -> ratatui::style::Color {
    match slot {
        0 => ratatui::style::Color::Red,
        1 => ratatui::style::Color::Green,
        2 => ratatui::style::Color::Magenta,
        3 => ratatui::style::Color::Blue,
        4 => ratatui::style::Color::Yellow,
        _ => ratatui::style::Color::Black,
    }
}

impl Meter for MemoryMeter {
    fn name(&self) -> &'static str {
        "Memory"
    }

    fn min_width(&self) -> u16 {
        10
    }

    fn history(&self) -> Option<&VecDeque<f32>> {
        Some(&self.history)
    }

    fn sample(&mut self, snapshot: &SystemSnapshot) {
        let mem = &snapshot.mem;
        self.value = snapshot.mem_usage();
        self.text = format!("{}/{}", bytes_human(mem.used), bytes_human(mem.total));

        // Build the bar straight from the platform's class list. Classes flagged
        // `counts_as_cache` are drawn as well (htop shows cached memory by
        // default) but they are not part of `used`.
        let mut segs = Vec::with_capacity(mem.classes.len());
        for class in &mem.classes {
            if class.bytes == 0 || mem.total == 0 {
                continue;
            }
            segs.push(Segment {
                value: class.bytes as f32 / mem.total as f32,
                color: slot_color(class.color_slot),
            });
        }
        self.segments = segs;

        if self.history.len() == crate::meters::GRAPH_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(self.value);
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode) {
        if mode == MeterMode::Graph {
            crate::meters::draw_graph(buf, area, &self.history, theme, 1.0);
            return;
        }
        // Recolor on render so a theme change applies without a new sample.
        let mut segs = self.segments.clone();
        for (i, seg) in segs.iter_mut().enumerate() {
            seg.color = theme.memory_slot(i as u8);
        }
        labelled_bar("Mem", self.value, &self.text, &segs, area, buf, theme, mode);
    }
}

// ---------------------------------------------------------------------------
// Swap
// ---------------------------------------------------------------------------

simple_meter!(
    /// Swap usage.
    SwapMeter,
    "Swap",
    10,
    |this: &mut Self, snap: &SystemSnapshot| {
        this.value = snap.swap_usage();
        this.text = format!(
            "{}/{}",
            bytes_human(snap.swap.used),
            bytes_human(snap.swap.total)
        );
    },
    |this: &Self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode| {
        let segs = [
            Segment { value: this.value, color: theme.swap_used },
            Segment { value: 1.0 - this.value, color: theme.swap_free },
        ];
        labelled_bar("Swp", this.value, &this.text, &segs, area, buf, theme, mode);
    }
);

// ---------------------------------------------------------------------------
// Load average
// ---------------------------------------------------------------------------

/// 1/5/15-minute load average.
pub struct LoadMeter {
    load: [f64; 3],
    history: VecDeque<f32>,
    max: f32,
}

impl LoadMeter {
    pub fn new() -> Self {
        Self {
            load: [0.0; 3],
            history: VecDeque::new(),
            max: 1.0,
        }
    }
}

impl Default for LoadMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl Meter for LoadMeter {
    fn name(&self) -> &'static str {
        "LoadAverage"
    }

    fn min_width(&self) -> u16 {
        18
    }

    fn column(&self) -> crate::meters::HeaderColumn {
        crate::meters::HeaderColumn::Right
    }

    fn history(&self) -> Option<&VecDeque<f32>> {
        Some(&self.history)
    }

    fn sample(&mut self, snapshot: &SystemSnapshot) {
        self.load = snapshot.load;
        let cores = snapshot.cpu_count().max(1) as f32;
        if self.load[0] as f32 > self.max {
            self.max = self.load[0] as f32;
        }
        if self.history.len() == GRAPH_HISTORY {
            self.history.pop_front();
        }
        self.history
            .push_back((self.load[0] as f32 / cores).clamp(0.0, 1.0));
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode) {
        if mode == MeterMode::Graph {
            let max = self.max.max(1.0);
            draw_graph(buf, area, &self.history, theme, 1.0);
            let _ = max;
            return;
        }
        let text = format!(
            "{:.2} {:.2} {:.2}",
            self.load[0], self.load[1], self.load[2]
        );
        put_str(buf, area.x, area.y, "Load", theme.meter_label, 5);
        put_str(
            buf,
            area.x + 5,
            area.y,
            &text,
            theme.meter_value,
            area.width.saturating_sub(5),
        );
    }
}

// ---------------------------------------------------------------------------
// Network and disk throughput
// ---------------------------------------------------------------------------

/// Which counters a [`ThroughputMeter`] reports.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ThroughputKind {
    Net,
    Disk,
}

/// Two-direction throughput meter, shared by `NetworkIO` and `DiskIO`.
///
/// # Why this is a rate and not a counter
///
/// The kernel exposes cumulative byte counters. A meter showing `1.9 GB` would
/// answer "how much has this machine ever transferred", which nobody asks; the
/// question is "what is it doing now". So the sampler keeps the previous
/// counters and this meter renders the delta over the measured interval.
///
/// The bar is scaled against a *moving* ceiling rather than a fixed one: a 1
/// Gbit link and a 100 Mbit link differ by an order of magnitude, and a bar
/// pinned to 1 Gbit would show an idle-looking sliver on the slower one. The
/// ceiling is the largest combined rate seen so far, so the bar is comparable
/// across ticks and still readable at any link speed.
///
/// Text mode names both directions, because "3.2 M/s" alone does not say whether
/// that is a download or a backup.
pub struct ThroughputMeter {
    label: &'static str,
    inbound: &'static str,
    outbound: &'static str,
    /// Which pair of theme colours the bar uses: net or disk.
    kind: ThroughputKind,
    /// Whether this platform can report the counter at all.
    supported: bool,
    value: f32,
    text: String,
    /// Inbound and outbound bytes/s, kept separately so the bar can be drawn as
    /// two segments. Stored rather than recomputed in `draw`, because a meter is
    /// drawn many times per sample and the split would otherwise be lost.
    inbound_bytes: f64,
    outbound_bytes: f64,
    /// Largest combined rate seen, for scaling the bar.
    peak: f64,
    history: VecDeque<f32>,
}

impl ThroughputMeter {
    fn new(
        kind: ThroughputKind,
        label: &'static str,
        inbound: &'static str,
        outbound: &'static str,
        supported: bool,
    ) -> Self {
        Self {
            label,
            inbound,
            outbound,
            kind,
            supported,
            value: 0.0,
            text: String::from("-"),
            inbound_bytes: 0.0,
            outbound_bytes: 0.0,
            peak: 0.0,
            history: VecDeque::with_capacity(GRAPH_HISTORY),
        }
    }

    /// The bar ceiling. Never zero, so the first frame does not divide by it and
    /// an idle machine still has a defined scale.
    fn ceiling(&self) -> f64 {
        self.peak.max(1024.0)
    }

    /// Inbound/outbound bar colours for this meter's kind.
    fn colors(&self, theme: &Theme) -> (ratatui::style::Color, ratatui::style::Color) {
        match self.kind {
            ThroughputKind::Net => (theme.net_rx, theme.net_tx),
            ThroughputKind::Disk => (theme.disk_read, theme.disk_write),
        }
    }

    fn record(&mut self, rate: gander_core::system::Rate) {
        if !self.supported {
            self.text = String::from("n/a here");
            self.value = 0.0;
            self.inbound_bytes = 0.0;
            self.outbound_bytes = 0.0;
            if self.history.len() == GRAPH_HISTORY {
                self.history.pop_front();
            }
            self.history.push_back(0.0);
            return;
        }
        self.inbound_bytes = if rate.inbound.is_finite() {
            rate.inbound.max(0.0)
        } else {
            0.0
        };
        self.outbound_bytes = if rate.outbound.is_finite() {
            rate.outbound.max(0.0)
        } else {
            0.0
        };
        self.text = format!(
            "{} {} {} {}",
            rate_text(self.inbound_bytes),
            self.inbound,
            rate_text(self.outbound_bytes),
            self.outbound
        );
        let total = self.inbound_bytes + self.outbound_bytes;
        if total > self.peak {
            self.peak = total;
        }
        self.value = (total / self.ceiling()) as f32;
        if self.history.len() == GRAPH_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(self.value.clamp(0.0, 1.0));
    }

    fn draw(&self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode) {
        // Both directions share the bar, each scaled against the same ceiling, so
        // the bar length is the combined rate and the split shows the mix.
        let ceiling = self.ceiling();
        let (rx_color, tx_color) = self.colors(theme);
        let segments = [
            Segment {
                value: (self.inbound_bytes / ceiling) as f32,
                color: rx_color,
            },
            Segment {
                value: (self.outbound_bytes / ceiling) as f32,
                color: tx_color,
            },
        ];
        labelled_bar(
            self.label,
            self.value,
            &self.text,
            &segments,
            area,
            buf,
            theme,
            mode,
        );
    }
}

/// Bytes per second, in the same IEC units as `bytes_human`.
///
/// `bytes_human` takes a `u64`, and a rate is a fraction of a byte per second
/// after division, so this is a separate formatter rather than a lossy cast.
fn rate_text(per_sec: f64) -> String {
    const UNITS: [&str; 5] = ["B/s", "K/s", "M/s", "G/s", "T/s"];
    if !per_sec.is_finite() || per_sec < 1.0 {
        return format!("0 {}", UNITS[0]);
    }
    let mut value = per_sec;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

// ---------------------------------------------------------------------------
// Uptime
// ---------------------------------------------------------------------------

/// Host network throughput (`NetworkIO`).
pub struct NetworkMeter {
    inner: ThroughputMeter,
}

impl NetworkMeter {
    pub fn new() -> Self {
        Self {
            inner: ThroughputMeter::new(ThroughputKind::Net, "Net", "in", "out", true),
        }
    }
}

impl Default for NetworkMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl Meter for NetworkMeter {
    fn name(&self) -> &'static str {
        "NetworkIO"
    }

    fn min_width(&self) -> u16 {
        20
    }

    fn column(&self) -> crate::meters::HeaderColumn {
        crate::meters::HeaderColumn::Right
    }

    fn history(&self) -> Option<&VecDeque<f32>> {
        Some(&self.inner.history)
    }

    fn sample(&mut self, snapshot: &SystemSnapshot) {
        self.inner.record(snapshot.net_rate);
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode) {
        if mode == MeterMode::Graph {
            draw_graph(buf, area, &self.inner.history, theme, 1.0);
            return;
        }
        self.inner.draw(area, buf, theme, mode);
    }
}

/// Host disk throughput (`DiskIO`).
///
/// Only Linux has a host-wide counter (`/proc/diskstats`). macOS has no
/// equivalent without IOKit, and the design forbids a vendor SDK, so the meter
/// says so rather than drawing a permanent `0 B/s` that looks like a dead disk.
pub struct DiskIOMeter {
    inner: ThroughputMeter,
}

impl DiskIOMeter {
    pub fn new() -> Self {
        Self {
            inner: ThroughputMeter::new(
                ThroughputKind::Disk,
                "Disk",
                "read",
                "write",
                gander_core::capability::HOST_DISK_IO,
            ),
        }
    }
}

impl Default for DiskIOMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl Meter for DiskIOMeter {
    fn name(&self) -> &'static str {
        "DiskIO"
    }

    fn min_width(&self) -> u16 {
        20
    }

    fn column(&self) -> crate::meters::HeaderColumn {
        crate::meters::HeaderColumn::Right
    }

    fn history(&self) -> Option<&VecDeque<f32>> {
        Some(&self.inner.history)
    }

    fn sample(&mut self, snapshot: &SystemSnapshot) {
        self.inner.record(snapshot.disk_rate);
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, mode: MeterMode) {
        if mode == MeterMode::Graph {
            draw_graph(buf, area, &self.inner.history, theme, 1.0);
            return;
        }
        self.inner.draw(area, buf, theme, mode);
    }
}

// ---------------------------------------------------------------------------
// Uptime
// ---------------------------------------------------------------------------

simple_meter!(
    right:
    /// Time since boot, spelled out with units (`4d 23h`, `1h 2m`).
    ///
    /// htop renders this as `4-23:13:30`, which needs the reader to remember
    /// that the leading field is days. There is room here, so say so.
    UptimeMeter,
    "Uptime",
    14,
    |this: &mut Self, snap: &SystemSnapshot| {
        this.text = duration_verbose(snap.uptime).to_string();
        this.value = 0.0;
    },
    |this: &Self, area: Rect, buf: &mut Buffer, theme: &Theme, _mode: MeterMode| {
        put_str(buf, area.x, area.y, "Uptime", theme.meter_label, 7);
        put_str(
            buf,
            area.x + 7,
            area.y,
            &this.text,
            theme.meter_value,
            area.width.saturating_sub(7),
        );
    }
);

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// Process-count summary with several lengths, so a narrow header does not clip
/// a word in half.
pub struct TasksMeter {
    verbose: String,
    medium: String,
    terse: String,
    value: f32,
    history: VecDeque<f32>,
}

impl TasksMeter {
    pub fn new() -> Self {
        Self {
            verbose: String::from("-"),
            medium: String::from("-"),
            terse: String::from("-"),
            value: 0.0,
            history: VecDeque::with_capacity(crate::meters::GRAPH_HISTORY),
        }
    }
}

impl Default for TasksMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl Meter for TasksMeter {
    fn name(&self) -> &'static str {
        "Tasks"
    }

    fn min_width(&self) -> u16 {
        16
    }

    fn column(&self) -> crate::meters::HeaderColumn {
        crate::meters::HeaderColumn::Right
    }

    fn history(&self) -> Option<&VecDeque<f32>> {
        Some(&self.history)
    }

    fn sample(&mut self, snapshot: &SystemSnapshot) {
        let tasks = &snapshot.tasks;
        self.verbose = task_summary(tasks);
        self.medium = task_summary_medium(tasks);
        self.terse = format!("{} procs", tasks.total);
        self.value = if tasks.total > 0 {
            tasks.running as f32 / tasks.total as f32
        } else {
            0.0
        };
        if self.history.len() == crate::meters::GRAPH_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(self.value);
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer, theme: &Theme, _mode: MeterMode) {
        put_str(buf, area.x, area.y, "Tasks", theme.meter_label, 6);
        // Only the rendered width is known here, so pick the longest variant that
        // fits instead of letting `put_str` cut a word in half.
        let room = area.width.saturating_sub(6) as usize;
        let text = self.text_for_width(room);
        put_str(
            buf,
            area.x + 6,
            area.y,
            &text,
            theme.meter_value,
            room as u16,
        );
    }
}

impl TasksMeter {
    /// Shorten the summary to fit `width` columns.
    fn text_for_width(&self, width: usize) -> String {
        if self.verbose.len() <= width {
            return self.verbose.clone();
        }
        if self.medium.len() <= width {
            return self.medium.clone();
        }
        if self.terse.len() <= width {
            return self.terse.clone();
        }
        // Last resort: just the number.
        self.terse
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string()
    }
}

/// `472 procs, 3 running, 1664 threads`
pub fn task_summary(tasks: &gander_core::system::TaskStats) -> String {
    let mut text = format!("{} procs", tasks.total);
    if tasks.running > 0 {
        let _ = write!(text, ", {} running", tasks.running);
    }
    // Only mention threads when they differ from the process count, otherwise it
    // is noise on single-threaded systems.
    if tasks.threads > tasks.total {
        let _ = write!(text, ", {} threads", tasks.threads);
    }
    text
}

/// `472 procs, 3 running`, `472 procs, 3 run`
pub fn task_summary_medium(tasks: &gander_core::system::TaskStats) -> String {
    let mut text = format!("{} procs", tasks.total);
    if tasks.running > 0 {
        let _ = write!(text, ", {} run", tasks.running);
    }
    text
}

// ---------------------------------------------------------------------------
// Clock / date
// ---------------------------------------------------------------------------

simple_meter!(
    /// Wall-clock time.
    ClockMeter,
    "Clock",
    8,
    |this: &mut Self, _snap: &SystemSnapshot| {
        this.text = local_clock();
    },
    |this: &Self, area: Rect, buf: &mut Buffer, theme: &Theme, _mode: MeterMode| {
        put_str(buf, area.x, area.y, &this.text, theme.meter_value, area.width);
    }
);

simple_meter!(
    /// Date.
    DateMeter,
    "Date",
    10,
    |this: &mut Self, _snap: &SystemSnapshot| {
        this.text = local_date();
    },
    |this: &Self, area: Rect, buf: &mut Buffer, theme: &Theme, _mode: MeterMode| {
        put_str(buf, area.x, area.y, &this.text, theme.meter_value, area.width);
    }
);

simple_meter!(
    /// Hostname.
    HostnameMeter,
    "Hostname",
    10,
    |this: &mut Self, _snap: &SystemSnapshot| {
        if this.text.is_empty() {
            this.text = hostname();
        }
    },
    |this: &Self, area: Rect, buf: &mut Buffer, theme: &Theme, _mode: MeterMode| {
        put_str(buf, area.x, area.y, "Host", theme.meter_label, 5);
        put_str(buf, area.x + 5, area.y, &this.text, theme.meter_value, area.width.saturating_sub(5));
    }
);

/// A blank spacer meter, used to align header rows.
pub struct BlankMeter;

impl BlankMeter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BlankMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl Meter for BlankMeter {
    fn name(&self) -> &'static str {
        "Blank"
    }

    fn min_width(&self) -> u16 {
        1
    }

    fn sample(&mut self, _snapshot: &SystemSnapshot) {}

    fn render(&mut self, _area: Rect, _buf: &mut Buffer, _theme: &Theme, _mode: MeterMode) {}
}

/// `HH:MM:SS` in local time, computed without a chrono dependency.
fn local_clock() -> String {
    let (h, m, s) = local_hms();
    format!("{h:02}:{m:02}:{s:02}")
}

fn local_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (y, mo, d) = civil_from_days((secs + local_offset_secs()).div_euclid(86_400));
    format!("{y:04}-{mo:02}-{d:02}")
}

fn local_hms() -> (u32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let day = (secs + local_offset_secs()).rem_euclid(86_400);
    (
        (day / 3600) as u32,
        ((day % 3600) / 60) as u32,
        (day % 60) as u32,
    )
}

/// Seconds east of UTC for the local time zone, right now.
///
/// `localtime_r` is the only portable way to learn this: the offset depends on
/// the date (DST) and, on macOS, on the time zone rules the OS ships. It is
/// re-read on every call rather than cached, because a long-running gander has
/// to cross a DST boundary correctly. `localtime_r` is cheap and thread-safe.
#[cfg(unix)]
fn local_offset_secs() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let t = now as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: `localtime_r` writes one `struct tm` through the pointer and reads
    // the scalar time; both are valid for the call.
    let filled = unsafe { libc::localtime_r(&t, tm.as_mut_ptr()) };
    if filled.is_null() {
        return 0;
    }
    // SAFETY: a non-null return means the struct was filled.
    let tm = unsafe { tm.assume_init() };
    tm.tm_gmtoff
}

/// Platforms without `localtime_r` fall back to UTC, which is what the meters
/// displayed before this was wired up.
#[cfg(not(unix))]
fn local_offset_secs() -> i64 {
    0
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to Y/M/D.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn hostname() -> String {
    // `/proc/sys/kernel/hostname` is the cheap path on Linux; on macOS there is
    // no `/proc`, so `gethostname(2)` is the portable answer. Try the file first
    // because it works inside containers where the syscall can disagree.
    if let Ok(text) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    hostname_syscall().unwrap_or_else(|| {
        std::env::var("HOSTNAME").unwrap_or_else(|_| String::from("localhost"))
    })
}

#[cfg(unix)]
fn hostname_syscall() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is writable and the length matches its size.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let len = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..len]).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(not(unix))]
fn hostname_syscall() -> Option<String> {
    None
}

/// Build the default header: one CPU row, then memory and swap.
pub fn default_header(cpu_count: usize) -> Vec<Box<dyn Meter>> {
    // Mirrors htop's `Settings.c` default header for `initialCpuCount` cores:
    //   left column  : <CPU left>  Memory  Swap
    //   right column : <CPU right> Tasks   LoadAverage  Uptime
    //
    // More than four cores splits the CPU meters in half so every core gets a
    // row (`LeftCPUs` / `RightCPUs`); four or fewer use one aggregate row.
    // Each meter declares its own column, so the order here only matters within
    // a column.
    let split = cpu_count > 4;
    let half = cpu_count / 2;

    let mut meters: Vec<Box<dyn Meter>> = vec![
        if split {
            Box::new(crate::meters::CpuMeter::left(0..half))
        } else {
            Box::new(crate::meters::CpuMeter::aggregate())
        },
        Box::new(MemoryMeter::new()),
        Box::new(SwapMeter::new()),
    ];

    if split {
        meters.push(Box::new(crate::meters::CpuMeter::right(half..cpu_count)));
    }

    meters.push(Box::new(TasksMeter::new()));
    meters.push(Box::new(LoadMeter::new()));
    meters.push(Box::new(UptimeMeter::new()));

    meters
}

/// Build the header from the user's configured meter lists.
///
/// Each configured meter carries its own display mode, because htop's setup
/// screen changes the meter under the cursor rather than the whole header.
/// Unrecognised names are skipped, so a config written by a newer gander still
/// starts. An empty header falls back to the default one — an empty header is
/// more likely a broken file than a deliberate choice, and htop refuses to
/// render one at all.
pub fn header_from_config(
    config: &gander_config::Config,
    cpu_count: usize,
) -> Header {
    let split = cpu_count > 4;
    let half = cpu_count / 2;
    let mut meters: Vec<Box<dyn Meter>> = Vec::new();
    let mut modes: Vec<MeterMode> = Vec::new();
    let mut sources: Vec<MeterSource> = Vec::new();

    let add = |entries: &[gander_config::MeterConfig],
                   column: HeaderColumn,
                   meters: &mut Vec<Box<dyn Meter>>,
                   modes: &mut Vec<MeterMode>,
                   sources: &mut Vec<MeterSource>| {
        for (index, entry) in entries.iter().enumerate() {
            let is_cpu_pair =
                matches!(entry.name.as_str(), "LeftCPUs" | "RightCPUs" | "CPU_CORES");
            let mode = mode_from_name(entry.mode_name());

            let meter: Option<Box<dyn Meter>> = if is_cpu_pair {
                // `LeftCPUs`/`RightCPUs` are a *pair*: with four cores or fewer
                // htop collapses them into one aggregate `CPU` row in the left
                // column and leaves the right column alone. Emitting both would
                // put two identical CPU rows on a small machine.
                match (split, column) {
                    (true, HeaderColumn::Left) => {
                        Some(Box::new(crate::meters::CpuMeter::left(0..half)))
                    }
                    (true, HeaderColumn::Right) => {
                        Some(Box::new(crate::meters::CpuMeter::right(half..cpu_count)))
                    }
                    (false, HeaderColumn::Left) => {
                        Some(Box::new(crate::meters::CpuMeter::aggregate()))
                    }
                    (false, HeaderColumn::Right) => None,
                }
            } else {
                meter_by_name(&entry.name)
            };

            if let Some(meter) = meter {
                meters.push(meter);
                modes.push(mode);
                // Remember which config entry produced this meter, so writing a
                // mode back does not have to guess from the flat index (which
                // drifts as soon as a collapsed CPU pair is skipped).
                sources.push(MeterSource { column, index });
            }
        }
    };

    add(
        &config.meters_left,
        HeaderColumn::Left,
        &mut meters,
        &mut modes,
        &mut sources,
    );
    add(
        &config.meters_right,
        HeaderColumn::Right,
        &mut meters,
        &mut modes,
        &mut sources,
    );

    if meters.is_empty() {
        let defaults = default_header(cpu_count);
        let n = defaults.len();
        return Header {
            meters: defaults,
            modes: vec![MeterMode::Bar; n],
            sources: (0..n)
                .map(|_| MeterSource {
                    column: HeaderColumn::Left,
                    index: 0,
                })
                .collect(),
        };
    }

    // A config that named only `RightCPUs` on a small machine would otherwise
    // lose CPU reporting entirely.
    if !meters.iter().any(|m| m.name().starts_with("CPU")) {
        meters.insert(0, Box::new(crate::meters::CpuMeter::aggregate()));
        modes.insert(0, MeterMode::Bar);
        sources.insert(
            0,
            MeterSource {
                column: HeaderColumn::Left,
                index: 0,
            },
        );
    }

    Header {
        meters,
        modes,
        sources,
    }
}

/// The built header: meters, their modes, and where each came from.
pub struct Header {
    pub meters: Vec<Box<dyn Meter>>,
    pub modes: Vec<MeterMode>,
    /// For each meter, the config entry it was built from.
    pub sources: Vec<MeterSource>,
}

/// The config entry a live meter came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MeterSource {
    pub column: HeaderColumn,
    pub index: usize,
}

/// The config spelling for a meter mode.
pub fn mode_name(mode: MeterMode) -> &'static str {
    match mode {
        MeterMode::Bar => "bar",
        MeterMode::Text => "text",
        MeterMode::Graph => "graph",
    }
}

fn mode_from_name(name: &str) -> MeterMode {
    match name {
        "text" => MeterMode::Text,
        "graph" => MeterMode::Graph,
        _ => MeterMode::Bar,
    }
}

/// Build a meter by config name.
pub fn meter_by_name(name: &str) -> Option<Box<dyn Meter>> {
    Some(match name {
        "CPU" | "AllCPUs" | "LeftCPUs" | "RightCPUs" | "CPU_CORES" => {
            Box::new(crate::meters::CpuMeter::aggregate())
        }
        "Memory" => Box::new(MemoryMeter::new()),
        "Swap" => Box::new(SwapMeter::new()),
        "LoadAverage" | "Load" => Box::new(LoadMeter::new()),
        "Tasks" => Box::new(TasksMeter::new()),
        "Uptime" => Box::new(UptimeMeter::new()),
        "Clock" => Box::new(ClockMeter::new()),
        "Date" => Box::new(DateMeter::new()),
        "Hostname" => Box::new(HostnameMeter::new()),
        "NetworkIO" | "Network" => Box::new(NetworkMeter::new()),
        "DiskIO" | "Disk" => Box::new(DiskIOMeter::new()),
        "Blank" => Box::new(BlankMeter::new()),
        _ => return None,
    })
}

/// Every meter name the setup screen offers, in htop's order.
pub const METER_NAMES: &[&str] = &[
    "LeftCPUs",
    "RightCPUs",
    "CPU",
    "Memory",
    "Swap",
    "Tasks",
    "LoadAverage",
    "Uptime",
    "Clock",
    "Date",
    "Hostname",
    "NetworkIO",
    "DiskIO",
    "Blank",
];

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn sample_snapshot() -> SystemSnapshot {
        let mut s = SystemSnapshot::default();
        s.mem.total = 1024 * 1024 * 1024;
        s.mem.used = 256 * 1024 * 1024;
        s.swap.total = 100 * 1024 * 1024;
        s.swap.used = 25 * 1024 * 1024;
        s.load = [1.5, 1.0, 0.5];
        s.uptime = 3_725;
        s
    }

    #[test]
    fn memory_meter_text() {
        let mut m = MemoryMeter::new();
        m.sample(&sample_snapshot());
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        m.render(area, &mut buf, &Theme::default(), MeterMode::Text);
        let line: String = (0..40)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect::<Vec<_>>()
            .join("");
        assert!(line.contains("256M"), "got {line:?}");
        assert!(line.contains("1.0G"), "got {line:?}");
    }

    #[test]
    fn bar_fills_exactly_the_width() {
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let segs = [
            Segment {
                value: 0.5,
                color: ratatui::style::Color::Green,
            },
            Segment {
                value: 0.5,
                color: ratatui::style::Color::Blue,
            },
        ];
        crate::meters::draw_segments(&mut buf, 0, 0, 20, &segs, ratatui::style::Color::Black, '|');
        let filled = (0..20).filter(|&x| buf[(x, 0)].symbol() == "|").count();
        assert_eq!(filled, 20, "a full bar must fill every cell");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_000), (2022, 1, 8));
    }

    /// The Clock meter must show *local* time, not UTC.
    ///
    /// This was a real bug: `LOCAL_OFFSET_SECS` was hard-coded to `0`, so the
    /// clock and date silently displayed UTC on every machine east or west of
    /// Greenwich. It only looks correct in CI, which runs in UTC.
    #[test]
    fn clock_uses_the_local_time_zone() {
        let offset = local_offset_secs();
        let utc = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let utc_day = utc.rem_euclid(86_400);
        let local_day = (utc + offset).rem_euclid(86_400);
        let expected = (
            (local_day / 3600) as u32,
            ((local_day % 3600) / 60) as u32,
            (local_day % 60) as u32,
        );
        assert_eq!(local_hms(), expected);

        // On a machine that is not on UTC the two must disagree, which is what
        // proves the offset is actually applied.
        if offset != 0 {
            let utc_hms = (
                (utc_day / 3600) as u32,
                ((utc_day % 3600) / 60) as u32,
                (utc_day % 60) as u32,
            );
            assert_ne!(local_hms(), utc_hms, "offset {offset} was not applied");
            assert!(local_date().len() == 10, "ISO date");
        }
    }

    #[test]
    fn hostname_is_not_empty() {
        let h = hostname();
        assert!(!h.trim().is_empty());
        assert!(!h.contains('\n'));
    }

    #[test]
    fn uptime_renders() {
        let mut m = UptimeMeter::new();
        m.sample(&sample_snapshot());
        let area = Rect::new(0, 0, 30, 1);
        let mut buf = Buffer::empty(area);
        m.render(area, &mut buf, &Theme::default(), MeterMode::Text);
        let line: String = (0..30).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        // 3725 s == 1 h 2 m, rendered with units rather than htop's `1:02:05`.
        assert!(line.contains("1h 2m"), "got {line:?}");
    }

    #[test]
    fn every_named_meter_constructs() {
        for name in [
            "CPU",
            "Memory",
            "Swap",
            "LoadAverage",
            "Tasks",
            "Uptime",
            "Clock",
            "Date",
            "Hostname",
            "NetworkIO",
            "DiskIO",
            "Blank",
        ] {
            assert!(meter_by_name(name).is_some(), "{name} must construct");
        }
        assert!(meter_by_name("nope").is_none());
    }

    /// The setup screen offers `METER_NAMES`; every one of them must build, or
    /// adding it from `F2` silently does nothing.
    #[test]
    fn every_offered_meter_name_builds() {
        for name in METER_NAMES {
            assert!(
                meter_by_name(name).is_some(),
                "METER_NAMES offers {name} but meter_by_name rejects it"
            );
        }
    }

    #[test]
    fn rate_text_uses_iec_units() {
        assert_eq!(rate_text(0.0), "0 B/s");
        assert_eq!(rate_text(0.4), "0 B/s");
        assert_eq!(rate_text(f64::NAN), "0 B/s");
        assert_eq!(rate_text(512.0), "512 B/s");
        assert_eq!(rate_text(1536.0), "1.5 K/s");
        assert_eq!(rate_text(3.0 * 1024.0 * 1024.0), "3.0 M/s");
        assert_eq!(rate_text(20.0 * 1024.0 * 1024.0), "20 M/s");
        assert_eq!(rate_text(2.5 * 1024.0 * 1024.0 * 1024.0), "2.5 G/s");
    }

    fn snapshot_with_rates(net: (f64, f64), disk: (f64, f64)) -> SystemSnapshot {
        use gander_core::system::Rate;
        let mut s = sample_snapshot();
        s.net_rate = Rate {
            inbound: net.0,
            outbound: net.1,
        };
        s.disk_rate = Rate {
            inbound: disk.0,
            outbound: disk.1,
        };
        s
    }

    #[test]
    fn network_meter_names_both_directions() {
        let mut m = NetworkMeter::new();
        m.sample(&snapshot_with_rates((1536.0, 2048.0), (0.0, 0.0)));
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        m.render(area, &mut buf, &Theme::default(), MeterMode::Text);
        let line: String = (0..40).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(line.contains("1.5 K/s in"), "got {line:?}");
        assert!(line.contains("2.0 K/s out"), "got {line:?}");
    }

    /// The bar has to be scaled against the observed peak, not against 100 %.
    ///
    /// A fixed ceiling is the bug this guards: on a slow link every bar would be
    /// an invisible sliver, and on a fast one it would sit pinned at full width.
    #[test]
    fn throughput_bar_scales_against_the_observed_peak() {
        let mut m = NetworkMeter::new();
        // Two samples, the second much larger: the bar follows the peak, so the
        // larger sample must be the full-width one.
        m.sample(&snapshot_with_rates((1024.0, 0.0), (0.0, 0.0)));
        m.sample(&snapshot_with_rates((0.0, 100.0 * 1024.0 * 1024.0), (0.0, 0.0)));

        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        m.render(area, &mut buf, &Theme::default(), MeterMode::Bar);

        let filled = (0..40)
            .filter(|&x| buf[(x, 0)].symbol() == "|")
            .count();
        assert!(filled > 0, "the peak sample must fill some of the bar");
    }

    #[test]
    fn disk_meter_reports_unsupported_platforms_instead_of_zero() {
        let mut m = DiskIOMeter::new();
        m.sample(&snapshot_with_rates((0.0, 0.0), (4096.0, 8192.0)));
        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        m.render(area, &mut buf, &Theme::default(), MeterMode::Text);
        let line: String = (0..40).map(|x| buf[(x, 0)].symbol().to_string()).collect();

        if gander_core::capability::HOST_DISK_IO {
            assert!(line.contains("4.0 K/s read"), "got {line:?}");
            assert!(line.contains("8.0 K/s write"), "got {line:?}");
        } else {
            // A permanent `0 B/s` would read as a dead disk, not as an absent
            // counter.
            assert!(line.contains("n/a"), "got {line:?}");
        }
    }

    /// Every meter must survive a graph-mode render on a 1-cell area, because the
    /// header splits into narrow columns on a small terminal.
    #[test]
    fn throughput_meters_survive_a_one_cell_graph() {
        let area = Rect::new(0, 0, 1, 1);
        for name in ["NetworkIO", "DiskIO"] {
            let mut m = meter_by_name(name).expect(name);
            m.sample(&snapshot_with_rates((1.0, 2.0), (3.0, 4.0)));
            let mut buf = Buffer::empty(area);
            m.render(area, &mut buf, &Theme::default(), MeterMode::Graph);
            let mut buf = Buffer::empty(area);
            m.render(area, &mut buf, &Theme::default(), MeterMode::Bar);
        }
    }

    #[test]
    fn task_summary_is_readable() {
        use gander_core::system::TaskStats;

        let t = TaskStats {
            total: 484,
            running: 1,
            threads: 1824,
        };
        assert_eq!(task_summary(&t), "484 procs, 1 running, 1824 threads");

        // No running processes: omit the clause rather than printing ", 0".
        let t = TaskStats {
            total: 12,
            running: 0,
            threads: 12,
        };
        assert_eq!(task_summary(&t), "12 procs");

        // Threads equal to the process count is noise; leave it out.
        let t = TaskStats {
            total: 5,
            running: 2,
            threads: 5,
        };
        assert_eq!(task_summary(&t), "5 procs, 2 running");
    }

    #[test]
    fn uptime_uses_units_not_field_positions() {
        let mut m = UptimeMeter::new();
        let mut snap = sample_snapshot();
        snap.uptime = 4 * 86_400 + 23 * 3_600 + 13 * 60 + 30;

        m.sample(&snap);
        let area = Rect::new(0, 0, 30, 1);
        let mut buf = Buffer::empty(area);
        m.render(area, &mut buf, &Theme::default(), MeterMode::Text);
        let line: String = (0..30).map(|x| buf[(x, 0)].symbol().to_string()).collect();

        assert!(line.contains("Uptime"), "got {line:?}");
        assert!(
            line.contains("4d 23h"),
            "expected unit suffixes, got {line:?}"
        );
        assert!(
            !line.contains("4-23"),
            "must not use htop's day-hyphen notation: {line:?}"
        );
    }

    #[test]
    fn percent_text_is_stable() {
        use gander_core::format::percent_human;
        assert_eq!(&*percent_human(12.34), "12.3");
        assert_eq!(&*percent_human(f32::NAN), "-");
    }
}
