//! Terminal themes.
//!
//! Mirrors htop's color schemes. A `Theme` is plain data so themes can be
//! selected at runtime and tested without a terminal.

use ratatui::style::{Color, Modifier, Style};

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
            // Unknown names fall back rather than failing to start.
            _ => Some(Self::htop_default()),
        }
    }

    pub fn available() -> &'static [&'static str] {
        &["default", "monochrome", "midnight"]
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
}
