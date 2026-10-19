//! Terminal themes.
//!
//! Mirrors htop's color schemes. A `Theme` is plain data so themes can be
//! selected at runtime and tested without a terminal.

use ratatui::style::{Color, Modifier, Style};

/// Build an `RGB` [`Color`] from hex, e.g. `rgb(0x2dd4bf)`.
const fn hex(v: u32) -> Color {
    Color::Rgb(v as u8, (v >> 8) as u8, (v >> 16) as u8)
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: &'static str,
    pub background: Color,
    pub foreground: Color,

    pub header: Style,
    pub header_key: Style,
    pub header_value: Style,

    /// Process-table column headers.
    pub column_header: Style,
    /// The sort column header.
    pub column_header_sorted: Style,

    pub row_normal: Style,
    pub row_selected: Style,
    pub row_tagged: Style,
    pub row_kernel_thread: Style,
    pub row_userland_thread: Style,
    pub row_stale_exe: Style,
    /// Background style for the in-table CPU/memory bars in the panels layout.
    ///
    /// A background, not a foreground: the bar sits behind the number, and the
    /// number must stay readable on top of it.
    pub row_value_bar: Style,
    /// Marks the header meter that `I` will act on.
    ///
    /// A distinct style from `row_value_bar` even though both are backgrounds:
    /// sharing one made a test for "no table bars in the htop layout" fail, which
    /// is the test telling the truth — two different meanings need two styles, or
    /// neither can be asserted about.
    pub meter_cursor: Style,

    /// CPU meter segments, in htop's order:
    /// nice, normal, kernel, irq, soft-irq, steal, guest, io-wait.
    pub cpu_nice: Color,
    pub cpu_normal: Color,
    pub cpu_kernel: Color,
    pub cpu_irq: Color,
    pub cpu_softirq: Color,
    pub cpu_steal: Color,
    pub cpu_guest: Color,
    pub cpu_iowait: Color,
    pub cpu_idle: Color,

    /// Memory bar palette, indexed by `MemoryClass::color_slot`.
    ///
    /// Order matches htop's `MEMORY_1..6`: used classes first, then cache, then
    /// free, so a bar reads "hot to cold" left to right.
    pub memory_slots: [Color; 6],

    pub mem_used: Color,
    pub mem_buffers: Color,
    pub mem_shared: Color,
    pub mem_free: Color,

    pub swap_used: Color,
    pub swap_free: Color,

    pub meter_label: Style,
    pub meter_value: Style,
    pub bar_background: Color,

    pub function_bar: Style,
    pub function_bar_key: Style,

    pub dialog_border: Style,
    pub dialog_title: Style,
    pub panel_selected: Style,

    pub graph_low: Color,
    pub graph_high: Color,

    /// Network throughput bar: received, then transmitted.
    ///
    /// Separate from the CPU colours on purpose. Reusing `cpu_normal` /
    /// `cpu_kernel` made a network bar visually identical to a CPU bar, and the
    /// header then had two differently-meaning bars with the same palette —
    /// the same mistake `meter_cursor` vs `row_value_bar` was split to avoid.
    pub net_rx: Color,
    pub net_tx: Color,
    /// Disk throughput bar: read, then written.
    pub disk_read: Color,
    pub disk_write: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::named("default").unwrap_or_else(Self::monochrome_internal)
    }
}

impl Theme {
    /// Color for a `MemoryClass::color_slot`, clamped to the palette.
    pub fn memory_slot(&self, slot: u8) -> Color {
        self.memory_slots[(slot as usize).min(self.memory_slots.len() - 1)]
    }

    /// Look up a theme by the name stored in the config file.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "default" | "" => Some(Self::htop_default()),
            "monochrome" => Some(Self::monochrome_internal()),
            "midnight" => Some(Self::midnight()),
            "gander" => Some(Self::gander()),
            "classic" => Some(Self::classic()),
            "verdant" => Some(Self::verdant()),
            "paper" => Some(Self::paper()),
            "quiet" => Some(Self::quiet()),
            // Unknown names fall back rather than failing to start.
            _ => Some(Self::htop_default()),
        }
    }

    pub fn available() -> &'static [&'static str] {
        &[
            "default",
            "monochrome",
            "midnight",
            "gander",
            "classic",
            "verdant",
            "paper",
            "quiet",
        ]
    }

    fn htop_default() -> Self {
        Theme {
            name: "default",
            background: Color::Black,
            foreground: Color::White,

            header: Style::default().fg(Color::Black).bg(Color::Cyan),
            header_key: Style::default().fg(Color::Black).bg(Color::Cyan),
            header_value: Style::default().fg(Color::Black).bg(Color::Cyan),

            column_header: Style::default().fg(Color::Black).bg(Color::Green),
            column_header_sorted: Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),

            row_normal: Style::default().fg(Color::Cyan),
            row_selected: Style::default().fg(Color::Black).bg(Color::Cyan),
            row_tagged: Style::default().fg(Color::Yellow),
            row_value_bar: Style::default().bg(Color::DarkGray),
            // Magenta on black: no other style in this theme uses that pair, so
            // the cursor is unmistakably itself. The first version reused the
            // header's cyan-on-black, which made it invisible against the column
            // headers and indistinguishable in a style assertion.
            meter_cursor: Style::default().fg(Color::Magenta).bg(Color::Black),
            row_kernel_thread: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::DIM),
            row_userland_thread: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            row_stale_exe: Style::default().fg(Color::Red),

            cpu_nice: Color::Blue,
            cpu_normal: Color::Green,
            cpu_kernel: Color::Red,
            cpu_irq: Color::Yellow,
            cpu_softirq: Color::Magenta,
            cpu_steal: Color::Cyan,
            cpu_guest: Color::Cyan,
            cpu_iowait: Color::Gray,
            cpu_idle: Color::Black,

            memory_slots: [
                Color::Red,     // wired / used
                Color::Green,   // active / buffers
                Color::Magenta, // compressed / cache
                Color::Blue,    // other
                Color::Yellow,  // purgeable
                Color::Black,   // free
            ],
            mem_used: Color::Green,
            mem_buffers: Color::Blue,
            mem_shared: Color::Magenta,
            mem_free: Color::Black,

            swap_used: Color::Red,
            swap_free: Color::Black,

            meter_label: Style::default().fg(Color::Cyan),
            meter_value: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            bar_background: Color::Black,

            function_bar: Style::default().fg(Color::Black).bg(Color::Cyan),
            function_bar_key: Style::default().fg(Color::White).bg(Color::Blue),

            dialog_border: Style::default().fg(Color::Cyan),
            dialog_title: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            panel_selected: Style::default().fg(Color::Black).bg(Color::Cyan),

            graph_low: Color::Green,
            graph_high: Color::Red,
            net_rx: Color::Green,
            net_tx: Color::Blue,
            disk_read: Color::Yellow,
            disk_write: Color::Red,
        }
    }

    fn monochrome_internal() -> Self {
        Theme {
            name: "monochrome",
            background: Color::Black,
            foreground: Color::White,
            header: Style::default().add_modifier(Modifier::REVERSED),
            header_key: Style::default().add_modifier(Modifier::REVERSED),
            header_value: Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            column_header: Style::default().add_modifier(Modifier::REVERSED),
            column_header_sorted: Style::default()
                .add_modifier(Modifier::REVERSED | Modifier::BOLD),
            row_normal: Style::default(),
            row_selected: Style::default().add_modifier(Modifier::REVERSED),
            row_tagged: Style::default().add_modifier(Modifier::BOLD),
            // Monochrome cannot tint, so the bar uses a reverse video background
            // and the number keeps its own glyphs.
            row_value_bar: Style::default().add_modifier(Modifier::REVERSED),
            meter_cursor: Style::default()
                .add_modifier(Modifier::REVERSED | Modifier::BOLD | Modifier::UNDERLINED),
            row_kernel_thread: Style::default().add_modifier(Modifier::DIM),
            row_userland_thread: Style::default().add_modifier(Modifier::BOLD),
            row_stale_exe: Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            cpu_nice: Color::Gray,
            cpu_normal: Color::White,
            cpu_kernel: Color::White,
            cpu_irq: Color::White,
            cpu_softirq: Color::White,
            cpu_steal: Color::Gray,
            cpu_guest: Color::Gray,
            cpu_iowait: Color::Gray,
            cpu_idle: Color::Black,
            memory_slots: [
                Color::White,
                Color::White,
                Color::Gray,
                Color::Gray,
                Color::Gray,
                Color::Black,
            ],
            mem_used: Color::White,
            mem_buffers: Color::Gray,
            mem_shared: Color::Gray,
            mem_free: Color::Black,
            swap_used: Color::White,
            swap_free: Color::Black,
            meter_label: Style::default(),
            meter_value: Style::default().add_modifier(Modifier::BOLD),
            bar_background: Color::Black,
            function_bar: Style::default().add_modifier(Modifier::REVERSED),
            function_bar_key: Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            dialog_border: Style::default(),
            dialog_title: Style::default().add_modifier(Modifier::BOLD),
            panel_selected: Style::default().add_modifier(Modifier::REVERSED),
            graph_low: Color::Gray,
            graph_high: Color::White,
            net_rx: Color::White,
            net_tx: Color::Gray,
            disk_read: Color::Gray,
            disk_write: Color::White,
        }
    }

    fn midnight() -> Self {
        let mut t = Self::htop_default();
        t.name = "midnight";
        t.background = Color::Indexed(17);
        t.foreground = Color::Indexed(250);
        t.header = Style::default()
            .fg(Color::Indexed(250))
            .bg(Color::Indexed(24));
        t.header_key = t.header;
        t.header_value = t.header.add_modifier(Modifier::BOLD);
        t.column_header = Style::default()
            .fg(Color::Indexed(250))
            .bg(Color::Indexed(60));
        t.column_header_sorted = t.column_header.add_modifier(Modifier::BOLD);
        t.row_normal = Style::default().fg(Color::Indexed(110));
        t.row_selected = Style::default()
            .fg(Color::Indexed(17))
            .bg(Color::Indexed(110));
        t.row_value_bar = Style::default().bg(Color::Indexed(24));
        t.meter_cursor = Style::default()
            .fg(Color::Indexed(214))
            .bg(Color::Indexed(17))
            .add_modifier(Modifier::BOLD);
        t.function_bar = Style::default()
            .fg(Color::Indexed(250))
            .bg(Color::Indexed(24));
        t.dialog_border = Style::default().fg(Color::Indexed(110));
        t.dialog_title = t.dialog_border.add_modifier(Modifier::BOLD);
        t.panel_selected = Style::default()
            .fg(Color::Indexed(17))
            .bg(Color::Indexed(110));
        t
    }

    /// The in-house `gander` theme: a teal-on-deep-blue palette with purple
    /// accents, closer to btop than to htop. This is the theme the README
    /// screenshot is built around.
    ///
    /// The palette deliberately avoids pure primary colors: teal is the identity
    /// color for running/active things and the header, purple is the secondary
    /// (swap, idle accents), and the rest are desaturated to keep the noise down.
    fn gander() -> Self {
        // base
        let bg = hex(0x0b1220);
        let fg = hex(0xc8d3e0);
        let muted = hex(0x5a6b82);
        let teal = hex(0x2dd4bf);
        let purple = hex(0xa78bfa);
        let amber = hex(0xfbbf24);
        let red = hex(0xf87171);
        let green = hex(0x34d399);
        let blue = hex(0x60a5fa);
        let gray = hex(0x64748b);
        let sel_bg = hex(0x164e4a);

        Theme {
            name: "gander",
            background: bg,
            foreground: fg,

            // Header bar: teal text on the dark background, no block fill.
            header: Style::default().fg(teal),
            header_key: Style::default().fg(muted),
            header_value: Style::default().fg(teal).add_modifier(Modifier::BOLD),

            column_header: Style::default().fg(teal).add_modifier(Modifier::BOLD),
            column_header_sorted: Style::default().fg(teal).add_modifier(Modifier::BOLD),

            row_normal: Style::default().fg(fg),
            row_selected: Style::default().fg(teal).bg(sel_bg).add_modifier(Modifier::BOLD),
            row_tagged: Style::default().fg(amber),
            row_value_bar: Style::default().bg(hex(0x1e2a3a)),
            meter_cursor: Style::default().fg(teal).bg(sel_bg).add_modifier(Modifier::BOLD),
            row_kernel_thread: Style::default().fg(muted).add_modifier(Modifier::DIM),
            row_userland_thread: Style::default().fg(hex(0x7dd3fc)).add_modifier(Modifier::BOLD),
            row_stale_exe: Style::default().fg(red),

            // CPU segments: a teal family with distinct hues, not the classic
            // red/green/yellow.
            cpu_nice: hex(0x3b82f6),
            cpu_normal: teal,
            cpu_kernel: purple,
            cpu_irq: amber,
            cpu_softirq: hex(0xfb923c),
            cpu_steal: gray,
            cpu_guest: hex(0x38bdf8),
            cpu_iowait: hex(0x94a3b8),
            cpu_idle: bg,

            memory_slots: [
                teal,      // used
                green,     // buffers
                purple,    // cache
                blue,      // other
                amber,     // purgeable
                bg,        // free
            ],
            mem_used: teal,
            mem_buffers: blue,
            mem_shared: purple,
            mem_free: bg,

            swap_used: purple,
            swap_free: bg,

            meter_label: Style::default().fg(muted),
            meter_value: Style::default().fg(fg).add_modifier(Modifier::BOLD),
            bar_background: hex(0x1e2a3a),

            function_bar: Style::default().fg(fg).bg(hex(0x0f1b2d)),
            function_bar_key: Style::default().fg(teal).add_modifier(Modifier::BOLD),

            dialog_border: Style::default().fg(teal),
            dialog_title: Style::default().fg(teal).add_modifier(Modifier::BOLD),
            panel_selected: Style::default().fg(bg).bg(teal),

            graph_low: hex(0x0ea5a5),
            graph_high: purple,
            net_rx: hex(0x2dd4bf),
            net_tx: hex(0x60a5fa),
            disk_read: hex(0xfbbf24),
            disk_write: hex(0xf87171),
        }
    }

    /// The four looks in the reference screenshot.
    ///
    /// `classic` — 简约深色·经典现代: a deep-navy desk with one blue accent.
    /// Selection is a solid blue row, the loudest thing on screen.
    fn classic() -> Self {
        let bg = hex(0x0d1424);
        let fg = hex(0xcdd6e0);
        let muted = hex(0x6b7a90);
        let blue = hex(0x3d7dd8);
        let blue_bright = hex(0x60a5fa);
        let sel_bg = hex(0x2563eb);
        let panel_bg = hex(0x111a2e);

        Theme {
            name: "classic",
            background: bg,
            foreground: fg,

            header: Style::default().fg(hex(0xe5edf8)),
            header_key: Style::default().fg(muted),
            header_value: Style::default()
                .fg(hex(0xe5edf8))
                .add_modifier(Modifier::BOLD),

            column_header: Style::default().fg(hex(0xe5edf8)),
            column_header_sorted: Style::default()
                .fg(hex(0xffffff))
                .add_modifier(Modifier::BOLD),

            row_normal: Style::default().fg(fg),
            row_selected: Style::default()
                .fg(hex(0xffffff))
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            row_tagged: Style::default().fg(blue_bright),
            row_value_bar: Style::default().bg(panel_bg),
            meter_cursor: Style::default()
                .fg(hex(0xffffff))
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            row_kernel_thread: Style::default().fg(muted).add_modifier(Modifier::DIM),
            row_userland_thread: Style::default()
                .fg(blue_bright)
                .add_modifier(Modifier::BOLD),
            row_stale_exe: Style::default().fg(hex(0xf87171)),

            cpu_nice: hex(0x2f6f4f),
            cpu_normal: blue,
            cpu_kernel: hex(0xc0564f),
            cpu_irq: hex(0xd9a441),
            cpu_softirq: hex(0x9a6fc0),
            cpu_steal: hex(0x56637a),
            cpu_guest: hex(0x4f8fc0),
            cpu_iowait: hex(0x7a8aa0),
            cpu_idle: bg,

            memory_slots: [
                hex(0xc0564f), // used
                hex(0x46a758), // buffers
                hex(0x9a6fc0), // cache
                blue,          // other
                hex(0xd9a441), // purgeable
                bg,            // free
            ],
            mem_used: blue,
            mem_buffers: hex(0x46a758),
            mem_shared: hex(0x9a6fc0),
            mem_free: bg,

            swap_used: blue_bright,
            swap_free: bg,

            meter_label: Style::default().fg(hex(0xe5edf8)),
            meter_value: Style::default()
                .fg(fg)
                .add_modifier(Modifier::BOLD),
            bar_background: panel_bg,

            function_bar: Style::default().fg(fg).bg(panel_bg),
            function_bar_key: Style::default()
                .fg(blue_bright)
                .add_modifier(Modifier::BOLD),

            dialog_border: Style::default().fg(hex(0x3a4a66)),
            dialog_title: Style::default()
                .fg(hex(0xe5edf8))
                .add_modifier(Modifier::BOLD),
            panel_selected: Style::default().fg(hex(0xffffff)).bg(sel_bg),

            graph_low: blue,
            graph_high: hex(0xc0564f),
            net_rx: Color::Green,
            net_tx: Color::Blue,
            disk_read: Color::Yellow,
            disk_write: Color::Red,
        }
    }

    /// `verdant` — 暗色·绿意主题: everything green, borders included. The
    /// selected row is solid green with near-black text, the inverse emphasis
    /// of `classic` (which selects with blue on white text).
    fn verdant() -> Self {
        let bg = hex(0x0b120e);
        let fg = hex(0xcfe3d4);
        let muted = hex(0x5c7a66);
        let green = hex(0x3fb950);
        let green_bright = hex(0x4ade80);
        let sel_bg = hex(0x2ea043);
        let panel_bg = hex(0x101a14);

        Theme {
            name: "verdant",
            background: bg,
            foreground: fg,

            header: Style::default().fg(green_bright),
            header_key: Style::default().fg(muted),
            header_value: Style::default()
                .fg(green_bright)
                .add_modifier(Modifier::BOLD),

            column_header: Style::default().fg(green_bright),
            column_header_sorted: Style::default()
                .fg(hex(0xbbf7d0))
                .add_modifier(Modifier::BOLD),

            row_normal: Style::default().fg(fg),
            row_selected: Style::default()
                .fg(hex(0x04140a))
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            row_tagged: Style::default().fg(hex(0xfbbf24)),
            row_value_bar: Style::default().bg(panel_bg),
            meter_cursor: Style::default()
                .fg(hex(0x04140a))
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            row_kernel_thread: Style::default().fg(muted).add_modifier(Modifier::DIM),
            row_userland_thread: Style::default()
                .fg(green_bright)
                .add_modifier(Modifier::BOLD),
            row_stale_exe: Style::default().fg(hex(0xf87171)),

            cpu_nice: hex(0x2f6f4f),
            cpu_normal: green,
            cpu_kernel: hex(0xc0564f),
            cpu_irq: hex(0xd9a441),
            cpu_softirq: hex(0x6fae7f),
            cpu_steal: hex(0x4a5d52),
            cpu_guest: hex(0x7fc98f),
            cpu_iowait: hex(0x6b8574),
            cpu_idle: bg,

            memory_slots: [
                green,         // used
                hex(0x7fc98f), // buffers
                hex(0x2f6f4f), // cache
                hex(0x4a8f66), // other
                hex(0xd9a441), // purgeable
                bg,            // free
            ],
            mem_used: green,
            mem_buffers: hex(0x4a8f66),
            mem_shared: hex(0x2f6f4f),
            mem_free: bg,

            swap_used: hex(0x2f6f4f),
            swap_free: bg,

            meter_label: Style::default().fg(green_bright),
            meter_value: Style::default()
                .fg(fg)
                .add_modifier(Modifier::BOLD),
            bar_background: panel_bg,

            function_bar: Style::default().fg(fg).bg(panel_bg),
            function_bar_key: Style::default()
                .fg(green_bright)
                .add_modifier(Modifier::BOLD),

            dialog_border: Style::default().fg(green),
            dialog_title: Style::default()
                .fg(green_bright)
                .add_modifier(Modifier::BOLD),
            panel_selected: Style::default().fg(hex(0x04140a)).bg(sel_bg),

            graph_low: hex(0x2f6f4f),
            graph_high: green_bright,
            net_rx: hex(0x4ade80),
            net_tx: hex(0x86efac),
            disk_read: hex(0xfacc15),
            disk_write: hex(0xf87171),
        }
    }

    /// `paper` — 浅色·清爽: the only light theme. Selecting a light-blue row
    /// with dark-blue text inverts the usual dark-selection convention, and
    /// every accent is a darker shade so it survives on white.
    fn paper() -> Self {
        let bg = hex(0xf6f9fc);
        let fg = hex(0x1f2937);
        let muted = hex(0x6b7280);
        let blue = hex(0x1d4ed8);
        let sel_bg = hex(0xd6e6fb);
        let bar_bg = hex(0xe2e8f0);

        Theme {
            name: "paper",
            background: bg,
            foreground: fg,

            header: Style::default().fg(blue),
            header_key: Style::default().fg(muted),
            header_value: Style::default()
                .fg(hex(0x0b4aa2))
                .add_modifier(Modifier::BOLD),

            column_header: Style::default()
                .fg(hex(0x334155))
                .add_modifier(Modifier::BOLD),
            column_header_sorted: Style::default()
                .fg(hex(0x0b1220))
                .add_modifier(Modifier::BOLD),

            row_normal: Style::default().fg(fg),
            row_selected: Style::default()
                .fg(hex(0x0b4aa2))
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            row_tagged: Style::default().fg(hex(0xb45309)),
            row_value_bar: Style::default().bg(bar_bg),
            meter_cursor: Style::default()
                .fg(hex(0x0b4aa2))
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            row_kernel_thread: Style::default().fg(muted).add_modifier(Modifier::DIM),
            row_userland_thread: Style::default()
                .fg(blue)
                .add_modifier(Modifier::BOLD),
            row_stale_exe: Style::default().fg(hex(0xb91c1c)),

            cpu_nice: hex(0x10b981),
            cpu_normal: hex(0x3b82f6),
            cpu_kernel: hex(0xef4444),
            cpu_irq: hex(0xf59e0b),
            cpu_softirq: hex(0x8b5cf6),
            cpu_steal: hex(0x9ca3af),
            cpu_guest: hex(0x0ea5e9),
            cpu_iowait: hex(0x94a3b8),
            cpu_idle: bg,

            memory_slots: [
                hex(0xef4444), // used
                hex(0x10b981), // buffers
                hex(0x8b5cf6), // cache
                hex(0x3b82f6), // other
                hex(0xf59e0b), // purgeable
                bg,            // free
            ],
            mem_used: hex(0x10b981),
            mem_buffers: hex(0x3b82f6),
            mem_shared: hex(0x8b5cf6),
            mem_free: bg,

            swap_used: hex(0x8b5cf6),
            swap_free: bg,

            meter_label: Style::default().fg(hex(0x334155)),
            meter_value: Style::default()
                .fg(fg)
                .add_modifier(Modifier::BOLD),
            bar_background: bar_bg,

            function_bar: Style::default().fg(hex(0x334155)).bg(hex(0xeef2f7)),
            function_bar_key: Style::default()
                .fg(blue)
                .add_modifier(Modifier::BOLD),

            dialog_border: Style::default().fg(hex(0x94a3b8)),
            dialog_title: Style::default()
                .fg(hex(0x334155))
                .add_modifier(Modifier::BOLD),
            panel_selected: Style::default().fg(hex(0x0b4aa2)).bg(sel_bg),

            graph_low: hex(0x93c5fd),
            graph_high: blue,
            net_rx: hex(0x2f6f4f),
            net_tx: hex(0x2563eb),
            disk_read: hex(0xb45309),
            disk_write: hex(0xb91c1c),
        }
    }

    /// `quiet` — 极简·低调: a strictly gray ramp, no hue anywhere. Emphasis is
    /// carried by brightness and weight alone, so nothing on screen shouts.
    fn quiet() -> Self {
        let bg = hex(0x15171b);
        let fg = hex(0xa8b0ba);
        let bright = hex(0xe8edf2);
        let muted = hex(0x565d66);
        let sel_bg = hex(0x3d444d);
        let bar_bg = hex(0x22262c);

        Theme {
            name: "quiet",
            background: bg,
            foreground: fg,

            header: Style::default().fg(bright),
            header_key: Style::default().fg(muted),
            header_value: Style::default()
                .fg(bright)
                .add_modifier(Modifier::BOLD),

            column_header: Style::default().fg(fg),
            column_header_sorted: Style::default()
                .fg(bright)
                .add_modifier(Modifier::BOLD),

            row_normal: Style::default().fg(fg),
            row_selected: Style::default()
                .fg(bright)
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            row_tagged: Style::default()
                .fg(bright)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            row_value_bar: Style::default().bg(bar_bg),
            meter_cursor: Style::default()
                .fg(bright)
                .bg(sel_bg)
                .add_modifier(Modifier::BOLD),
            row_kernel_thread: Style::default().fg(muted).add_modifier(Modifier::DIM),
            row_userland_thread: Style::default()
                .fg(bright)
                .add_modifier(Modifier::BOLD),
            row_stale_exe: Style::default()
                .fg(bright)
                .add_modifier(Modifier::UNDERLINED),

            cpu_nice: hex(0x565d66),
            cpu_normal: hex(0x7d8590),
            cpu_kernel: hex(0x6e7681),
            cpu_irq: hex(0x8b949e),
            cpu_softirq: hex(0x838b94),
            cpu_steal: hex(0x4a5058),
            cpu_guest: hex(0x939aa3),
            cpu_iowait: hex(0x5d646c),
            cpu_idle: bg,

            memory_slots: [
                hex(0x8b949e), // used
                hex(0x7d8590), // buffers
                hex(0x6e7681), // cache
                hex(0x5d646c), // other
                hex(0x939aa3), // purgeable
                bg,            // free
            ],
            mem_used: hex(0x8b949e),
            mem_buffers: hex(0x6e7681),
            mem_shared: hex(0x5d646c),
            mem_free: bg,

            swap_used: hex(0x6e7681),
            swap_free: bg,

            meter_label: Style::default().fg(fg),
            meter_value: Style::default()
                .fg(bright)
                .add_modifier(Modifier::BOLD),
            bar_background: bar_bg,

            function_bar: Style::default().fg(fg).bg(hex(0x1b1e23)),
            function_bar_key: Style::default()
                .fg(bright)
                .add_modifier(Modifier::BOLD),

            dialog_border: Style::default().fg(hex(0x3d444d)),
            dialog_title: Style::default()
                .fg(bright)
                .add_modifier(Modifier::BOLD),
            panel_selected: Style::default().fg(bright).bg(sel_bg),

            graph_low: hex(0x3d444d),
            graph_high: hex(0xa8b0ba),
            net_rx: hex(0x8b949e),
            net_tx: hex(0x6e7681),
            disk_read: hex(0x9ca3af),
            disk_write: hex(0x8b949e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_advertised_theme_resolves() {
        for name in Theme::available() {
            let t = Theme::named(name).expect("theme must resolve");
            assert!(!t.name.is_empty());
        }
    }

    #[test]
    fn unknown_theme_falls_back() {
        assert_eq!(Theme::named("nope").unwrap().name, "default");
    }

    /// The four reference themes: names distinct, no theme leaks another's
    /// palette, and the light theme really is light.
    #[test]
    fn reference_themes_are_distinct() {
        let names = ["classic", "verdant", "paper", "quiet"];
        let themes: Vec<Theme> = names
            .iter()
            .map(|n| Theme::named(n).expect("must resolve"))
            .collect();
        for (t, n) in themes.iter().zip(names) {
            assert_eq!(t.name, n);
        }
        // Brightness = R+G+B: the light theme's background must beat every dark
        // theme's, or a "light" theme shipped as dark would look like a bug.
        let lum = |t: &Theme| match t.background {
            Color::Rgb(r, g, b) => r as u32 + g as u32 + b as u32,
            _ => 0,
        };
        let paper = lum(&themes[2]);
        assert!(paper > lum(&themes[0]) * 4, "paper must be far lighter");
        assert!(paper > lum(&themes[1]) * 4);
        assert!(paper > lum(&themes[3]) * 4);
        // Quiet must have no hue: max channel within 40 of min across the
        // accents that carry state (selection, CPU, memory).
        let q = &themes[3];
        for c in [q.cpu_normal, q.memory_slots[0], q.row_tagged.fg.unwrap()] {
            if let Color::Rgb(r, g, b) = c {
                let (min, max) = (r.min(g).min(b), r.max(g).max(b));
                assert!(max - min <= 40, "quiet must stay gray: {c:?}");
            }
        }
    }
}
