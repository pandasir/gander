//! The `F2` setup screen.
//!
//! htop has five pages: Header, Columns, Display, Colors, Tabs. All five are
//! implemented here.
//!
//! Editing is done on a *draft* copy of the config. `Esc` discards it, `S`/`F10`
//! applies it. That mirrors htop, where leaving the setup screen is what commits
//! the change, and it means a half-finished edit cannot corrupt the live view.

use gander_config::Config;
use gander_core::row::Field;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::meters::HeaderColumn;
use crate::theme::Theme;

/// One row of the setup screen's Header page.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HeaderRow {
    Meter {
        column: HeaderColumn,
        index: usize,
        name: String,
        mode: &'static str,
    },
    /// Not selectable in practice: marks where the right column starts.
    Separator(HeaderColumn),
    /// The "add a meter" row.
    Add,
}

/// Which page of the setup screen is showing.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SetupPage {
    Header,
    Columns,
    Tabs,
    Display,
    Colors,
    About,
}

impl SetupPage {
    pub const ALL: [SetupPage; 6] = [
        SetupPage::Header,
        SetupPage::Columns,
        SetupPage::Tabs,
        SetupPage::Display,
        SetupPage::Colors,
        SetupPage::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            SetupPage::Header => "Header",
            SetupPage::Columns => "Columns",
            SetupPage::Tabs => "Tabs",
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
        let mut this = Self {
            page: SetupPage::Columns,
            cursor: 0,
            draft: config.clone(),
            fields: Vec::new(),
        };
        this.reload_fields();
        this
    }

    /// Number of rows on the current page, for cursor clamping.
    fn row_count(&self) -> usize {
        match self.page {
            SetupPage::Columns => self.fields.len(),
            SetupPage::Tabs => self.draft.tabs.len() + 1, // tabs plus "New tab"
            // Left column meters, a separator, the right column meters, then
            // the "add" row.
            SetupPage::Header => self.header_rows().len(),
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
            SetupPage::Header => {
                self.activate_header_row();
                Applied {
                    columns: false,
                    display: true,
                }
            }
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
            SetupPage::Tabs => {
                if self.cursor < self.draft.tabs.len() {
                    // Selecting a tab makes it active, and the Columns page has to
                    // follow: it lists the *active* tab's columns, so leaving it
                    // showing the previous tab's would edit the wrong layout.
                    self.draft.active_tab = self.cursor;
                    self.reload_fields();
                } else {
                    self.add_tab();
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

    /// Rebuild the Columns page's field list from the active tab.
    ///
    /// The list is ordered: enabled fields first, in the tab's own order, then
    /// the disabled remainder in `Field::all()` order.
    fn reload_fields(&mut self) {
        let enabled: Vec<Field> = self.draft.active_tab().fields();
        let mut fields: Vec<(Field, bool)> = enabled.iter().map(|f| (*f, true)).collect();
        for field in Field::all() {
            if !enabled.contains(&field) {
                fields.push((field, false));
            }
        }
        self.fields = fields;
        self.cursor = 0;
    }

    // -----------------------------------------------------------------------
    // Header page
    // -----------------------------------------------------------------------

    /// One row of the Header page.
    ///
    /// The page is a flat list so the cursor is a single index: the left
    /// column's meters, then a separator telling the user the next block is the
    /// right column, then the right column's meters, then an "add" row.
    pub fn header_rows(&self) -> Vec<HeaderRow> {
        let mut rows = Vec::new();
        for (i, entry) in self.draft.meters_left.iter().enumerate() {
            rows.push(HeaderRow::Meter {
                column: HeaderColumn::Left,
                index: i,
                name: entry.name.clone(),
                mode: entry.mode_name(),
            });
        }
        rows.push(HeaderRow::Separator(HeaderColumn::Left));
        for (i, entry) in self.draft.meters_right.iter().enumerate() {
            rows.push(HeaderRow::Meter {
                column: HeaderColumn::Right,
                index: i,
                name: entry.name.clone(),
                mode: entry.mode_name(),
            });
        }
        rows.push(HeaderRow::Separator(HeaderColumn::Right));
        rows.push(HeaderRow::Add);
        rows
    }

    /// The row under the cursor, if it names a meter.
    fn header_meter_at_cursor(&self) -> Option<(HeaderColumn, usize)> {
        match self.header_rows().get(self.cursor) {
            Some(HeaderRow::Meter { column, index, .. }) => Some((*column, *index)),
            _ => None,
        }
    }

    /// Space/Enter on the Header page.
    ///
    /// On a meter row this cycles its display mode, which is what htop does;
    /// on the "add" row it appends a Clock meter to the right column, so a
    /// header can be extended without a separate picker dialog.
    fn activate_header_row(&mut self) {
        match self.header_rows().get(self.cursor).cloned() {
            Some(HeaderRow::Meter { column, index, .. }) => {
                let entry = match column {
                    HeaderColumn::Left => self.draft.meters_left.get_mut(index),
                    HeaderColumn::Right => self.draft.meters_right.get_mut(index),
                };
                if let Some(entry) = entry {
                    entry.mode = match entry.mode_name() {
                        "bar" => "text",
                        "text" => "graph",
                        _ => "bar",
                    }
                    .to_string();
                }
            }
            Some(HeaderRow::Separator(_)) => {}
            Some(HeaderRow::Add) | None => self.add_meter(),
        }
    }

    /// Append the first meter not already present to the right column.
    ///
    /// Cycling through `METER_NAMES` means repeated presses add Clock, then
    /// Date, then Hostname, which is the useful order and needs no dialog.
    fn add_meter(&mut self) {
        let existing: Vec<&str> = self
            .draft
            .meters_left
            .iter()
            .chain(self.draft.meters_right.iter())
            .map(|m| m.name.as_str())
            .collect();
        // Skip the CPU family when the header already has one: `LeftCPUs` and
        // `CPU` are alternatives, not companions, and adding a second CPU block
        // is never what the user meant.
        let has_cpu = existing.iter().any(|n| n.contains("CPU"));
        let name = crate::meters_impl::METER_NAMES
            .iter()
            .find(|n| {
                let is_cpu = n.contains("CPU");
                !existing.contains(*n) && (!is_cpu || !has_cpu)
            })
            .copied()
            .unwrap_or("Blank");
        self.draft
            .meters_right
            .push(gander_config::MeterConfig::new(name, "text"));

        // Put the cursor on the meter that was just added, so pressing Space
        // again cycles *its* mode and pressing the add row again adds the next
        // one. Derive the index from the row list rather than arithmetic on the
        // column lengths: the separators make the offset easy to get wrong.
        let added = self.draft.meters_right.len() - 1;
        if let Some(pos) = self.header_rows().iter().position(|r| {
            matches!(r, HeaderRow::Meter { column: HeaderColumn::Right, index, .. } if *index == added)
        }) {
            self.cursor = pos;
        }
    }

    /// Remove the meter under the cursor.
    ///
    /// Removing the last meter would leave a header with nothing in it; htop
    /// allows that and renders blank, but here the config loader would fall back
    /// to the default header, which is more surprising than refusing.
    pub fn remove_meter(&mut self) -> Result<String, &'static str> {
        if self.page != SetupPage::Header {
            return Err("not on the Header page");
        }
        let Some((column, index)) = self.header_meter_at_cursor() else {
            return Err("select a meter to delete");
        };
        let total = self.draft.meters_left.len() + self.draft.meters_right.len();
        if total <= 1 {
            return Err("the header cannot be empty");
        }

        let entry = match column {
            HeaderColumn::Left => self.draft.meters_left.remove(index),
            HeaderColumn::Right => self.draft.meters_right.remove(index),
        };

        // The row that was under the cursor is gone. Land on the meter that took
        // its place in the same column, or on the last meter of that column if it
        // was the final one. Leaving the cursor on a separator would make a
        // second Delete silently do nothing, which reads as a broken key.
        let list_len = match column {
            HeaderColumn::Left => self.draft.meters_left.len(),
            HeaderColumn::Right => self.draft.meters_right.len(),
        };
        let target = if list_len == 0 {
            None
        } else {
            Some(index.min(list_len - 1))
        };
        self.cursor = target
            .and_then(|index| {
                self.header_rows()
                    .iter()
                    .position(|r| matches!(r, HeaderRow::Meter { column: c, index: i, .. } if *c == column && *i == index))
            })
            // That column is now empty: fall back to the first selectable row, so
            // the cursor is never stranded on a separator.
            .or_else(|| {
                self.header_rows()
                    .iter()
                    .position(|r| matches!(r, HeaderRow::Meter { .. }))
            })
            .unwrap_or(0);

        Ok(entry.name)
    }

    /// Move the meter under the cursor within its column.
    pub fn move_meter(&mut self, up: bool) {
        if self.page != SetupPage::Header {
            return;
        }
        let Some((column, index)) = self.header_meter_at_cursor() else {
            return;
        };
        let list = match column {
            HeaderColumn::Left => &mut self.draft.meters_left,
            HeaderColumn::Right => &mut self.draft.meters_right,
        };
        let target = if up {
            index.saturating_sub(1)
        } else {
            (index + 1).min(list.len().saturating_sub(1))
        };
        if target != index {
            list.swap(index, target);
            self.cursor = self.cursor.saturating_sub(1).max(
                self.cursor
                    .saturating_sub(1)
                    .min(self.header_rows().len() - 1),
            );
            // Recompute from the row list so the cursor follows the meter.
            if let Some(pos) = self.header_rows().iter().position(|r| {
                matches!(r, HeaderRow::Meter { column: c, index: i, .. }
                    if *c == column && *i == target)
            }) {
                self.cursor = pos;
            }
        }
    }

    /// Add a tab that starts as a copy of the current one.
    ///
    /// htop's "New" does the same: a duplicate is far more useful than an empty
    /// tab, because the usual reason for a second screen is the same columns
    /// sorted differently.
    fn add_tab(&mut self) {
        let source = self
            .draft
            .tabs
            .get(self.draft.active_tab)
            .cloned()
            .unwrap_or_else(|| self.draft.tabs[0].clone());
        let name = format!("Tab {}", self.draft.tabs.len() + 1);
        self.draft.tabs.push(gander_config::TabConfig {
            name,
            ..source
        });
        self.draft.active_tab = self.draft.tabs.len() - 1;
        self.cursor = self.draft.active_tab;
    }

    /// Remove the selected tab. The last one cannot be removed.
    ///
    /// Returns `Ok` with the removed tab's name so the caller can report which
    /// one went away, or `Err` with the reason it could not.
    pub fn remove_tab(&mut self) -> Result<String, &'static str> {
        if self.page != SetupPage::Tabs {
            return Err("not on the Tabs page");
        }
        if self.cursor >= self.draft.tabs.len() {
            // The "New tab" row is not a tab.
            return Err("select a tab to delete, not 'New tab'");
        }
        if self.draft.tabs.len() <= 1 {
            return Err("the last tab cannot be deleted");
        }

        let removed = self.draft.tabs.remove(self.cursor);
        self.draft.active_tab = self
            .draft
            .active_tab
            .min(self.draft.tabs.len().saturating_sub(1));
        self.cursor = self.cursor.min(self.draft.tabs.len() - 1);
        // Deleting the active tab makes a different layout live.
        self.reload_fields();
        Ok(removed.name)
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
            SetupPage::Header => self.draw_header_page(inner, buf, theme),
            SetupPage::Columns => self.draw_columns(inner, buf, theme),
            SetupPage::Tabs => self.draw_tabs_page(inner, buf, theme),
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

    fn draw_header_page(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let rows = self.header_rows();
        let visible = area.height.saturating_sub(2).max(1) as usize;
        let first = self.cursor.saturating_sub(visible.saturating_sub(1));

        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .skip(first)
            .take(visible)
            .map(|(i, row)| {
                let selected = i == self.cursor;
                let text = match row {
                    HeaderRow::Meter {
                        column,
                        name,
                        mode,
                        ..
                    } => {
                        let side = match column {
                            HeaderColumn::Left => "L",
                            HeaderColumn::Right => "R",
                        };
                        format!("[{side}] {name:<14} {mode}")
                    }
                    HeaderRow::Separator(column) => match column {
                        HeaderColumn::Left => "-- right column --".to_string(),
                        HeaderColumn::Right => "-- end --".to_string(),
                    },
                    HeaderRow::Add => "[+] add meter".to_string(),
                };

                let dim = matches!(row, HeaderRow::Separator(_) | HeaderRow::Add);
                if selected {
                    Line::styled(text, theme.panel_selected)
                } else if dim {
                    Line::styled(text, theme.row_kernel_thread)
                } else {
                    Line::from(text)
                }
            })
            .collect();

        Widget::render(Paragraph::new(lines).style(theme.row_normal), area, buf);
    }

    fn draw_tabs_page(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let mut lines: Vec<Line> = self
            .draft
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let active = i == self.draft.active_tab;
                let marker = if active { "(*)" } else { "( )" };
                let text = format!(
                    "{marker} {:<12} {} columns, sort {}{}",
                    tab.name,
                    tab.columns.len(),
                    tab.sort.field,
                    if tab.sort.desc { " desc" } else { "" }
                );
                if i == self.cursor {
                    Line::styled(text, theme.panel_selected)
                } else if active {
                    Line::styled(text, theme.meter_value)
                } else {
                    Line::from(text)
                }
            })
            .collect();

        let new_row = self.draft.tabs.len();
        let text = "[+] New tab (copies the current one)";
        lines.push(if self.cursor == new_row {
            Line::styled(text, theme.panel_selected)
        } else {
            Line::styled(text, theme.row_kernel_thread)
        });

        lines.push(Line::from(""));
        lines.push(Line::styled(
            "Enter selects, Del removes the selected tab",
            theme.meter_label,
        ));

        Widget::render(
            Paragraph::new(lines).style(theme.row_normal),
            area,
            buf,
        );
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
        assert_eq!(s.page, SetupPage::Tabs);
        s.prev_page();
        assert_eq!(s.page, SetupPage::Columns);

        // Walking forward through every page returns to the start.
        for _ in 0..SetupPage::ALL.len() {
            s.next_page();
        }
        assert_eq!(s.page, SetupPage::Columns);

        // And backwards.
        for _ in 0..SetupPage::ALL.len() {
            s.prev_page();
        }
        assert_eq!(s.page, SetupPage::Columns);
    }

    #[test]
    fn new_tab_is_a_copy_of_the_current_one() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Tabs;
        s.cursor = s.draft.tabs.len(); // the "New tab" row
        s.activate();

        assert_eq!(s.draft.tabs.len(), 2);
        assert_eq!(s.draft.active_tab, 1, "the new tab becomes active");
        // A copy is more useful than an empty tab: same columns, different sort.
        assert_eq!(
            s.draft.tabs[0].columns.len(),
            s.draft.tabs[1].columns.len()
        );
    }

    #[test]
    fn selecting_a_tab_makes_it_active() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Tabs;
        s.cursor = s.draft.tabs.len();
        s.activate(); // add
        s.cursor = 0;
        s.activate(); // select the first
        assert_eq!(s.draft.active_tab, 0);
    }

    #[test]
    fn the_last_tab_cannot_be_removed() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Tabs;
        s.cursor = 0;
        assert_eq!(s.remove_tab(), Err("the last tab cannot be deleted"));
        assert_eq!(s.draft.tabs.len(), 1);
    }

    #[test]
    fn the_new_tab_row_is_not_a_tab() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Tabs;
        s.cursor = s.draft.tabs.len(); // "New tab"
        assert!(s.remove_tab().is_err());
        assert_eq!(s.draft.tabs.len(), 1);
    }

    #[test]
    fn selecting_a_tab_reloads_the_column_list() {
        // The Columns page lists the *active* tab's fields. Switching tabs on the
        // Tabs page without reloading left it editing the previous tab's layout.
        let mut config = Config::default();
        config.tabs.push(gander_config::TabConfig {
            name: "Second".into(),
            columns: vec!["PID".into(), "COMM".into()],
            sort: gander_config::SortConfig {
                field: "PID".into(),
                desc: false,
            },
        });

        let mut s = Setup::new(&config);
        assert_eq!(s.enabled_fields().len(), 8, "tab 0's columns");

        s.page = SetupPage::Tabs;
        s.cursor = 1;
        s.activate();

        assert_eq!(s.draft.active_tab, 1);
        assert_eq!(s.enabled_fields(), vec![Field::Pid, Field::Comm]);
    }

    #[test]
    fn removing_a_tab_keeps_the_index_valid() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Tabs;

        // `add_tab` moves the cursor onto the new tab, so ask for the next one
        // explicitly each time instead of pressing Enter twice.
        for _ in 0..2 {
            s.cursor = s.draft.tabs.len();
            s.activate();
        }
        assert_eq!(s.draft.tabs.len(), 3);
        assert_eq!(s.draft.active_tab, 2);

        s.cursor = 2;
        assert!(s.remove_tab().is_ok());
        assert_eq!(s.draft.tabs.len(), 2);
        assert!(s.draft.active_tab < 2, "active index must stay in range");
        assert!(s.cursor < s.draft.tabs.len(), "cursor must stay in range");
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

    // -----------------------------------------------------------------------
    // Header page
    // -----------------------------------------------------------------------

    #[test]
    fn header_page_lists_both_columns_and_an_add_row() {
        let s = Setup::new(&Config::default());
        let rows = s.header_rows();
        let left = rows
            .iter()
            .filter(|r| matches!(r, HeaderRow::Meter { column: HeaderColumn::Left, .. }))
            .count();
        let right = rows
            .iter()
            .filter(|r| matches!(r, HeaderRow::Meter { column: HeaderColumn::Right, .. }))
            .count();
        assert_eq!(left, s.draft.meters_left.len());
        assert_eq!(right, s.draft.meters_right.len());
        assert!(matches!(rows.last(), Some(HeaderRow::Add)));
    }

    #[test]
    fn header_page_row_count_matches_the_cursor_range() {
        let mut s = Setup {
            page: SetupPage::Header,
            ..Setup::new(&Config::default())
        };
        let n = s.row_count();
        for _ in 0..n + 10 {
            s.cursor_down();
        }
        assert!(s.cursor < n, "cursor {} out of {} rows", s.cursor, n);
    }

    #[test]
    fn activating_a_meter_cycles_its_mode() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Header;
        s.cursor = 0; // first left-column meter (LeftCPUs)
        assert_eq!(s.draft.meters_left[0].mode, "bar");

        s.activate();
        assert_eq!(s.draft.meters_left[0].mode, "text");
        s.activate();
        assert_eq!(s.draft.meters_left[0].mode, "graph");
        s.activate();
        assert_eq!(s.draft.meters_left[0].mode, "bar", "wraps");

        // And only that meter changed.
        assert_eq!(s.draft.meters_left[1].mode, "bar");
    }

    #[test]
    fn the_add_row_appends_a_meter_not_already_present() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Header;
        let before = s.draft.meters_right.len();
        s.cursor = s.row_count() - 1; // the Add row
        s.activate();

        assert_eq!(s.draft.meters_right.len(), before + 1);
        // Clock is the first name not already configured.
        assert_eq!(s.draft.meters_right.last().unwrap().name, "Clock");
        // The cursor follows the new meter so a second press adds the next one.
        assert!(matches!(
            s.header_rows().get(s.cursor),
            Some(HeaderRow::Meter { name, .. }) if name == "Clock"
        ));
    }

    #[test]
    fn deleting_a_meter_works_and_the_last_one_is_kept() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Header;
        s.cursor = 0;

        // `remove_meter` places the cursor on a selectable row, so repeated
        // Delete presses keep working until the header is down to one meter.
        while s.draft.meters_left.len() + s.draft.meters_right.len() > 1 {
            let before = s.draft.meters_left.len() + s.draft.meters_right.len();
            assert!(
                matches!(
                    s.header_rows().get(s.cursor),
                    Some(HeaderRow::Meter { .. })
                ),
                "cursor {} is not on a meter: {:?}",
                s.cursor,
                s.header_rows().get(s.cursor)
            );
            assert!(s.remove_meter().is_ok());
            assert_eq!(
                s.draft.meters_left.len() + s.draft.meters_right.len(),
                before - 1
            );
        }
        assert_eq!(s.remove_meter(), Err("the header cannot be empty"));
    }

    #[test]
    fn deleting_on_a_separator_row_says_why() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Header;
        // The separator sits right after the left column's meters.
        s.cursor = s.draft.meters_left.len();
        assert_eq!(s.remove_meter(), Err("select a meter to delete"));
    }

    #[test]
    fn moving_a_meter_reorders_within_its_column() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Header;
        let first = s.draft.meters_left[0].name.clone();
        let second = s.draft.meters_left[1].name.clone();
        assert_ne!(first, second);

        s.cursor = 0;
        s.move_meter(false); // move down
        assert_eq!(s.draft.meters_left[0].name, second);
        assert_eq!(s.draft.meters_left[1].name, first);

        // The cursor followed the meter instead of staying on the row index.
        match s.header_rows().get(s.cursor) {
            Some(HeaderRow::Meter {
                column: HeaderColumn::Left,
                index: 1,
                name,
                ..
            }) => assert_eq!(name, &first),
            other => panic!("cursor did not follow the meter: {other:?}"),
        }

        s.move_meter(true); // and back up
        assert_eq!(s.draft.meters_left[0].name, first);
    }

    #[test]
    fn moving_a_meter_cannot_cross_the_column_boundary() {
        let mut s = Setup::new(&Config::default());
        s.page = SetupPage::Header;
        let last_left = s.draft.meters_left.last().unwrap().name.clone();

        // The last left meter moved down must stay in the left column.
        s.cursor = s.draft.meters_left.len() - 1;
        s.move_meter(false);
        assert_eq!(s.draft.meters_left.last().unwrap().name, last_left);
        assert_eq!(s.draft.meters_right[0].name, "RightCPUs");
    }
}
