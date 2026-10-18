//! The `F2` setup screen.
//!
//! htop has five pages: Header, Columns, Display, Colors, Tabs. This implements
//! the three that change what the process table shows (Columns, Display, Colors)
//! plus an About page; Header editing and screen tabs remain on the roadmap.
//!
//! Editing is done on a *draft* copy of the config. `Esc` discards it, `F10`/`S`
//! applies it. That mirrors htop, where leaving the setup screen is what commits
//! the change, and it means a half-finished edit cannot corrupt the live view.

use gander_config::Config;
use gander_core::row::Field;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::theme::Theme;

/// Which page of the setup screen is showing.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SetupPage {
    Columns,
    Display,
    Colors,
    About,
}

impl SetupPage {
    pub const ALL: [SetupPage; 4] = [
        SetupPage::Columns,
        SetupPage::Display,
        SetupPage::Colors,
        SetupPage::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            SetupPage::Columns => "Columns",
            SetupPage::Display => "Display",
            SetupPage::Colors => "Colors",
            SetupPage::About => "About",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    fn next(self) -> Self {
        let i = (self.index() + 1) % Self::ALL.len();
        Self::ALL[i]
    }

    fn prev(self) -> Self {
        let i = (self.index() + Self::ALL.len() - 1) % Self::ALL.len();
        Self::ALL[i]
    }
}

/// What the setup screen changed, so the caller knows what to redo.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Applied {
    pub columns: bool,
    pub display: bool,
}

/// State of the setup screen while it is open.
pub struct Setup {
    pub page: SetupPage,
    /// Cursor row within the current page's list.
    pub cursor: usize,
    /// The config being edited. Not live until applied.
    pub draft: Config,
    /// Order of the available fields, with the enabled ones first.
    fields: Vec<(Field, bool)>,
}

impl Setup {
    pub fn new(config: &Config) -> Self {
        let enabled: Vec<Field> = config.active_tab().fields();
        let mut fields: Vec<(Field, bool)> = enabled.iter().map(|f| (*f, true)).collect();
        for field in Field::all() {
            if !enabled.contains(&field) {
                fields.push((field, false));
            }
        }

        Self {
            page: SetupPage::Columns,
            cursor: 0,
            draft: config.clone(),
            fields,
        }
    }

    /// Number of rows on the current page, for cursor clamping.
    fn row_count(&self) -> usize {
        match self.page {
            SetupPage::Columns => self.fields.len(),
            SetupPage::Display => 4,
            SetupPage::Colors => Theme::available().len(),
            SetupPage::About => 0,
        }
    }

    pub fn cursor_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn cursor_down(&mut self) {
        let max = self.row_count().saturating_sub(1);
        self.cursor = (self.cursor + 1).min(max);
    }

    pub fn next_page(&mut self) {
        self.page = self.page.next();
        self.cursor = 0;
    }

    pub fn prev_page(&mut self) {
        self.page = self.page.prev();
        self.cursor = 0;
    }

    /// Space/Enter: toggle the row under the cursor.
    pub fn activate(&mut self) -> Applied {
        match self.page {
            SetupPage::Columns => {
                if let Some((_, on)) = self.fields.get_mut(self.cursor) {
                    *on = !*on;
                    self.sync_columns();
                }
                Applied {
                    columns: true,
                    display: false,
                }
            }
            SetupPage::Display => {
                match self.cursor {
                    0 => {
                        self.draft.tree_mode = !self.draft.tree_mode;
                    }
                    1 => {
                        self.draft.show_kernel_threads = !self.draft.show_kernel_threads;
                    }
                    2 => {
                        self.draft.highlight_base_name = !self.draft.highlight_base_name;
                    }
                    _ => {
                        self.draft.highlight_deleted_exe = !self.draft.highlight_deleted_exe;
                    }
                }
                Applied {
                    columns: false,
                    display: true,
                }
            }
            SetupPage::Colors => {
                let names = Theme::available();
                if let Some(name) = names.get(self.cursor) {
                    self.draft.color_scheme = (*name).to_string();
                }
                Applied {
                    columns: false,
                    display: true,
                }
            }
            SetupPage::About => Applied::default(),
        }
    }

    /// Move the selected column earlier (`Up` with a modifier, or `[`).
    pub fn move_column(&mut self, up: bool) {
        if self.page != SetupPage::Columns || self.cursor >= self.fields.len() {
            return;
        }
        let target = if up {
            self.cursor.saturating_sub(1)
        } else {
            (self.cursor + 1).min(self.fields.len() - 1)
        };
        if target == self.cursor {
            return;
        }
        // Only meaningful among the enabled ones.
        if !self.fields[self.cursor].1 || !self.fields[target].1 {
            return;
        }
        self.fields.swap(self.cursor, target);
        self.cursor = target;
        self.sync_columns();
    }

    /// Write the enabled fields, in order, back into the draft config.
    fn sync_columns(&mut self) {
        let columns: Vec<String> = self
            .fields
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.name().to_string())
            .collect();

        let idx = self
            .draft
            .active_tab
            .min(self.draft.tabs.len().saturating_sub(1));
        if let Some(tab) = self.draft.tabs.get_mut(idx) {
            tab.columns = columns;
        }
    }

    /// The enabled columns, for the caller to rebuild its view.
    pub fn enabled_fields(&self) -> Vec<Field> {
        self.fields
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| *f)
            .collect()
    }

    pub fn draw(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let title = format!("Setup — {}", self.page.title());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme.dialog_border)
            .title(crate::app::span_title(&title, theme));
        let inner = block.inner(area);
        Widget::render(block, area, buf);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        match self.page {
            SetupPage::Columns => self.draw_columns(inner, buf, theme),
            SetupPage::Display => self.draw_display(inner, buf, theme),
            SetupPage::Colors => self.draw_colors(inner, buf, theme),
            SetupPage::About => self.draw_about(inner, buf, theme),
        }

        self.draw_tabs(inner, buf, theme);
    }

    fn draw_columns(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let visible = area.height.saturating_sub(2) as usize;
        let first = self.cursor.saturating_sub(visible.saturating_sub(1));
        let lines: Vec<Line> = self
            .fields
            .iter()
            .enumerate()
            .skip(first)
            .take(visible.max(1))
            .map(|(i, (field, on))| {
                let mark = if *on { "[x]" } else { "[ ]" };
                let text = format!("{mark} {:<24} {}", field.title().trim(), field.name());
                if i == self.cursor {
                    Line::styled(text, theme.panel_selected)
                } else if *on {
                    Line::from(text)
                } else {
                    Line::styled(text, theme.row_kernel_thread)
                }
            })
            .collect();

        let para = Paragraph::new(lines).style(theme.row_normal);
        Widget::render(para, area, buf);
    }

    fn draw_display(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let items = [
            ("Tree view", self.draft.tree_mode),
            ("Show kernel threads", self.draft.show_kernel_threads),
            (
                "Highlight program base name",
                self.draft.highlight_base_name,
            ),
            (
                "Highlight replaced/deleted executables",
                self.draft.highlight_deleted_exe,
            ),
        ];
        let lines: Vec<Line> = items
            .iter()
            .enumerate()
            .map(|(i, (label, on))| {
                let mark = if *on { "[x]" } else { "[ ]" };
                let text = format!("{mark} {label}");
                if i == self.cursor {
                    Line::styled(text, theme.panel_selected)
                } else {
                    Line::from(text)
                }
            })
            .collect();
        Widget::render(Paragraph::new(lines).style(theme.row_normal), area, buf);
    }

    fn draw_colors(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let lines: Vec<Line> = Theme::available()
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let selected = *name == self.draft.color_scheme;
                let mark = if selected { "(*)" } else { "( )" };
                let text = format!("{mark} {name}");
                if i == self.cursor {
                    Line::styled(text, theme.panel_selected)
                } else if selected {
                    Line::styled(text, theme.meter_value)
                } else {
                    Line::from(text)
                }
            })
            .collect();
        Widget::render(Paragraph::new(lines).style(theme.row_normal), area, buf);
    }

    fn draw_about(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let text = vec![
            Line::from(format!("gander {}", env!("CARGO_PKG_VERSION"))),
            Line::from(""),
            Line::from("A process viewer in Rust, modelled on htop's feature set."),
            Line::from(""),
            Line::from(format!("Backend: {}", backend_name())),
            Line::from(""),
            Line::from(vec![
                Span::styled("Tab/Shift-Tab", theme.meter_value),
                Span::raw("  switch pages"),
            ]),
            Line::from(vec![
                Span::styled("Space", theme.meter_value),
                Span::raw("        toggle the selected item"),
            ]),
            Line::from(vec![
                Span::styled("[ ]", theme.meter_value),
                Span::raw("          move a column up/down"),
            ]),
            Line::from(vec![
                Span::styled("Esc", theme.meter_value),
                Span::raw("          discard changes"),
            ]),
            Line::from(vec![
                Span::styled("S", theme.meter_value),
                Span::raw("            apply and close"),
            ]),
        ];
        Widget::render(Paragraph::new(text).style(theme.row_normal), area, buf);
    }

    fn draw_tabs(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        if area.height == 0 {
            return;
        }
        let mut x = area.x;
        let y = area.y + area.height - 1;
        for page in SetupPage::ALL {
            let label = format!(" {} ", page.title());
            let style = if page == self.page {
                theme.header_value
            } else {
                theme.header
            };
            crate::meters::put_str(buf, x, y, &label, style, label.len() as u16);
            x += label.len() as u16;
        }
    }
}

fn backend_name() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "linux (/proc)"
    }
    #[cfg(target_os = "macos")]
    {
        "darwin (libproc + sysctl + Mach)"
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        "none (fixtures only)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_cycle_in_both_directions() {
        let mut s = Setup::new(&Config::default());
        assert_eq!(s.page, SetupPage::Columns);
        s.next_page();
        assert_eq!(s.page, SetupPage::Display);
        s.prev_page();
        assert_eq!(s.page, SetupPage::Columns);
        // Wrapping.
        for _ in 0..SetupPage::ALL.len() {
            s.prev_page();
        }
        assert_eq!(s.page, SetupPage::Columns);
    }

    #[test]
    fn toggling_a_column_updates_the_draft() {
        let mut s = Setup::new(&Config::default());
        let before = s.draft.active_tab().fields().len();

        // Find a disabled field and enable it.
        let idx = s
            .fields
            .iter()
            .position(|(_, on)| !*on)
            .expect("some are off");
        s.cursor = idx;
        s.activate();

        assert_eq!(s.draft.active_tab().fields().len(), before + 1);
        assert_eq!(s.enabled_fields().len(), before + 1);
    }

    #[test]
    fn enabled_columns_keep_their_order() {
        let mut s = Setup::new(&Config::default());
        let first = s.enabled_fields();
        // Move the second enabled column above the first.
        s.cursor = 1;
        s.move_column(true);
        let after = s.enabled_fields();
        assert_eq!(after[0], first[1]);
        assert_eq!(after[1], first[0]);
    }

    #[test]
    fn display_toggles_flip_the_draft() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Display;
        let before = s.draft.tree_mode;
        s.cursor = 0;
        s.activate();
        assert_eq!(s.draft.tree_mode, !before);
    }

    #[test]
    fn colors_page_selects_a_scheme() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Colors;
        s.cursor = 0;
        s.activate();
        assert_eq!(s.draft.color_scheme, Theme::available()[0]);
    }

    #[test]
    fn cursor_stays_in_range() {
        let mut s = Setup::new(&Config::default());
        for _ in 0..1000 {
            s.cursor_down();
        }
        assert!(s.cursor < s.row_count());
        s.cursor_up();
        assert!(s.cursor < s.row_count());
    }

    #[test]
    fn drawing_each_page_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        for page in SetupPage::ALL {
            let s = Setup {
                page,
                ..Setup::new(&Config::default())
            };
            let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
            term.draw(|f| s.draw(f.area(), f.buffer_mut(), &Theme::default()))
                .unwrap();
        }
    }
}
