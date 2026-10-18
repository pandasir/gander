//! Application state, key handling, and view rendering.
//!
//! This replaces htop's `ScreenManager` + `Panel` focus stack with a single
//! `App` value and a `match` on `Screen`. See `docs/DESIGN.md` §6.

use gander_config::Config;
use gander_core::row::{Field, Row};
use gander_core::system::SystemSnapshot;
use gander_core::table::{self, Dir, Filter, ProcTable, Sort};
use gander_core::{format, tree};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Widget};
use ratatui::Frame;

use crate::marks::compose_tree;
use crate::meters::{Meter, MeterMode};
use crate::meters_impl;
use crate::theme::Theme;

/// Which screen has focus. htop's `Panel` stack becomes this enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    ProcessList,
    Help,
    Info {
        title: String,
        lines: Vec<String>,
        scroll: usize,
    },
    SignalPicker,
    Setup,
}

/// What the runtime should do after an update.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Reaction {
    Continue,
    /// Recompute sort/filter/tree.
    Reorder,
    /// Take a new sample immediately.
    Recalc,
    Quit,
}

/// Where the cursor sits during text input.
#[derive(Clone, Debug, Default)]
pub struct Input {
    pub active: bool,
    pub buffer: String,
    pub kind: InputKind,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum InputKind {
    #[default]
    None,
    Search,
    Filter,
}

pub struct App {
    pub screen: Screen,
    pub table: ProcTable,
    pub snapshot: SystemSnapshot,
    pub theme: Theme,
    pub config: Config,
    pub meters: Vec<Box<dyn Meter>>,
    pub selected: usize,
    pub offset: usize,
    pub h_offset: u16,
    pub tags: std::collections::HashSet<gander_core::Pid>,
    pub filter: Option<Filter>,
    pub input: Input,
    pub command_column_interactive: bool,
    pub quit: bool,
    pub status: Option<String>,
    pub meter_mode: MeterMode,
    pub pending_g: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        let theme = Theme::named(&config.color_scheme).unwrap_or_default();
        let mut table = ProcTable {
            sort: config.active_tab().sort(),
            tree_mode: config.tree_mode,
            show_kernel_threads: config.show_kernel_threads,
            ..Default::default()
        };
        table.sort = Sort {
            dir: Dir::Desc,
            ..table.sort
        };

        Self {
            screen: Screen::ProcessList,
            table,
            snapshot: SystemSnapshot::default(),
            theme,
            config,
            // Rebuilt on the first sample, once the core count is known.
            meters: Vec::new(),
            selected: 0,
            offset: 0,
            h_offset: 0,
            tags: Default::default(),
            filter: None,
            input: Input::default(),
            command_column_interactive: true,
            quit: false,
            status: None,
            meter_mode: MeterMode::Bar,
            pending_g: false,
        }
    }

    /// Adopt a fresh sample: update rows, meters, and reorder.
    pub fn apply_sample(&mut self, snapshot: SystemSnapshot, rows: Vec<Row>) {
        // The header depends on the core count, which is only known once the
        // first snapshot arrives.
        if self.meters.is_empty() {
            self.meters = meters_impl::default_header(snapshot.cpu_count());
        }
        for meter in &mut self.meters {
            meter.sample(&snapshot);
        }
        self.table.rows = rows;
        self.table.reorder(self.filter.as_ref());
        self.snapshot = snapshot;
        self.clamp_selection();
    }

    pub fn columns(&self) -> Vec<Field> {
        self.config.active_tab().fields()
    }

    fn clamp_selection(&mut self) {
        let n = self.table.visible();
        if n == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self.selected.min(n - 1);
    }

    fn selected_row(&self) -> Option<&Row> {
        self.table.row_at(self.selected)
    }

    /// Handle one key press. Returns what the runtime should do next.
    ///
    /// A `Reorder` reaction is applied here rather than left to the caller, so
    /// the state is always consistent with what the next frame will render.
    pub fn handle_key(&mut self, key: Key) -> Reaction {
        let reaction = self.dispatch(key);
        if reaction == Reaction::Reorder {
            self.table.reorder(self.filter.as_ref());
            self.clamp_selection();
        }
        reaction
    }

    fn dispatch(&mut self, key: Key) -> Reaction {
        if self.input.active {
            return self.handle_input_key(key);
        }

        match self.screen {
            Screen::Help => {
                self.screen = Screen::ProcessList;
                Reaction::Continue
            }
            Screen::Info { .. } => self.handle_info_key(key),
            Screen::SignalPicker => self.handle_signal_key(key),
            Screen::Setup => {
                if key == Key::Char('q') || key == Key::Esc || key == Key::F(10) {
                    self.screen = Screen::ProcessList;
                }
                Reaction::Continue
            }
            Screen::ProcessList => self.handle_list_key(key),
        }
    }

    fn handle_list_key(&mut self, key: Key) -> Reaction {
        let half = 1; // the runtime does not know the viewport; page keys handled below
        let _ = half;
        match key {
            Key::Char('q') | Key::F(10) => {
                self.quit = true;
                Reaction::Quit
            }
            Key::F(1) | Key::Char('h') | Key::Char('?') => {
                self.screen = Screen::Help;
                Reaction::Continue
            }
            Key::F(2) => {
                self.screen = Screen::Setup;
                Reaction::Continue
            }
            Key::F(3) | Key::Char('/') => {
                self.start_input(InputKind::Search);
                Reaction::Continue
            }
            Key::F(4) | Key::Char('\\') => {
                self.start_input(InputKind::Filter);
                Reaction::Continue
            }
            Key::F(5) | Key::Char('t') => {
                self.toggle_tree();
                Reaction::Reorder
            }
            Key::F(6) | Key::Char('>') | Key::Char('<') => {
                self.cycle_sort(key == Key::Char('<'));
                Reaction::Reorder
            }
            Key::F(7) => {
                self.renice_selected(-1);
                Reaction::Continue
            }
            Key::F(8) => {
                self.renice_selected(1);
                Reaction::Continue
            }
            Key::F(9) | Key::Char('k') => {
                if self.selected_row().is_some() {
                    self.screen = Screen::SignalPicker;
                }
                Reaction::Continue
            }
            Key::Up => {
                self.move_selection(-1);
                Reaction::Continue
            }
            Key::Down => {
                self.move_selection(1);
                Reaction::Continue
            }
            Key::Home | Key::Char('g') => {
                self.selected = 0;
                Reaction::Continue
            }
            Key::End | Key::Char('G') => {
                self.selected = self.table.visible().saturating_sub(1);
                Reaction::Continue
            }
            Key::Left => {
                self.h_offset = self.h_offset.saturating_sub(4);
                Reaction::Continue
            }
            Key::Right => {
                self.h_offset = self.h_offset.saturating_add(4);
                Reaction::Continue
            }
            Key::Char(' ') => {
                self.toggle_tag();
                Reaction::Continue
            }
            Key::Char('c') => {
                self.tag_with_children();
                Reaction::Continue
            }
            Key::Char('U') => {
                self.tags.clear();
                Reaction::Continue
            }
            Key::Char('u') => {
                let mut users: Vec<u32> = self.table.rows.iter().map(|r| r.uid).collect();
                users.sort_unstable();
                users.dedup();
                let _ = users;
                self.status = Some("user filter not implemented yet".into());
                Reaction::Continue
            }
            Key::Char('K') => {
                self.table.show_kernel_threads = !self.table.show_kernel_threads;
                Reaction::Reorder
            }
            Key::Char('H') => {
                self.table.show_kernel_threads = false;
                Reaction::Reorder
            }
            Key::Char('+') | Key::Char('=') => {
                self.config.delay_ms = self.config.delay_ms.saturating_sub(100).max(100);
                Reaction::Recalc
            }
            Key::Char('-') => {
                self.config.delay_ms = (self.config.delay_ms + 100).min(10_000);
                Reaction::Recalc
            }
            Key::Char('e') => self.show_env(),
            Key::Char('w') => self.show_command(),
            Key::Char('l') => self.show_open_files(),
            Key::Char('x') => self.show_locks(),
            Key::Char('I') => {
                self.meter_mode = match self.meter_mode {
                    MeterMode::Text => MeterMode::Bar,
                    MeterMode::Bar => MeterMode::Graph,
                    MeterMode::Graph => MeterMode::Text,
                };
                Reaction::Continue
            }
            Key::Esc => {
                if self.filter.is_some() {
                    self.filter = None;
                    Reaction::Reorder
                } else {
                    Reaction::Continue
                }
            }
            Key::Char('.') => {
                self.cycle_sort(false);
                Reaction::Reorder
            }
            _ => Reaction::Continue,
        }
    }

    fn handle_input_key(&mut self, key: Key) -> Reaction {
        match key {
            Key::Esc => {
                self.input = Input::default();
                if self.input.kind == InputKind::Filter {
                    self.filter = None;
                    return Reaction::Reorder;
                }
                Reaction::Continue
            }
            Key::Enter => {
                let kind = self.input.kind;
                let text = self.input.buffer.clone();
                self.input = Input::default();
                if kind == InputKind::Filter {
                    self.filter = Filter::parse(&text);
                    Reaction::Reorder
                } else {
                    self.jump_to_match(&text)
                }
            }
            Key::Backspace => {
                self.input.buffer.pop();
                if self.input.kind == InputKind::Filter {
                    self.filter = Filter::parse(&self.input.buffer);
                    return Reaction::Reorder;
                }
                Reaction::Continue
            }
            Key::Char(c) => {
                if !c.is_ascii_control() {
                    self.input.buffer.push(c);
                }
                if self.input.kind == InputKind::Filter {
                    self.filter = Filter::parse(&self.input.buffer);
                    return Reaction::Reorder;
                }
                Reaction::Continue
            }
            _ => Reaction::Continue,
        }
    }

    fn start_input(&mut self, kind: InputKind) {
        // Editing the filter starts from the current filter so it can be refined.
        let initial = match kind {
            InputKind::Filter => self.input.buffer.clone(),
            InputKind::Search => String::new(),
            InputKind::None => String::new(),
        };
        self.input = Input {
            active: true,
            buffer: initial,
            kind,
        };
    }

    fn jump_to_match(&mut self, needle: &str) -> Reaction {
        if let Some(pos) = table::find_next(
            &self.table.rows,
            &self.table.view.order,
            self.selected,
            needle,
        ) {
            self.selected = pos;
            self.status = None;
        } else {
            self.status = Some(format!("pattern not found: {needle}"));
        }
        Reaction::Continue
    }

    fn handle_info_key(&mut self, key: Key) -> Reaction {
        let Screen::Info { lines, scroll, .. } = &mut self.screen else {
            return Reaction::Continue;
        };
        match key {
            Key::Char('q') | Key::Esc | Key::F(10) => self.screen = Screen::ProcessList,
            Key::Up => *scroll = scroll.saturating_sub(1),
            Key::Down => *scroll = (*scroll + 1).min(lines.len().saturating_sub(1)),
            Key::Home => *scroll = 0,
            Key::End => *scroll = lines.len().saturating_sub(1),
            _ => {}
        }
        if self.screen == Screen::ProcessList {
            self.status = None;
        }
        Reaction::Continue
    }

    fn handle_signal_key(&mut self, key: Key) -> Reaction {
        match key {
            Key::Esc | Key::Char('q') => {
                self.screen = Screen::ProcessList;
                Reaction::Continue
            }
            Key::Char(c) if c.is_ascii_digit() => {
                let mut digits = String::new();
                digits.push(c);
                self.status = Some(format!("signal {digits} not confirmed"));
                Reaction::Continue
            }
            Key::Enter => {
                self.screen = Screen::ProcessList;
                Reaction::Continue
            }
            _ => Reaction::Continue,
        }
    }

    pub fn toggle_tree(&mut self) {
        self.table.tree_mode = !self.table.tree_mode;
        self.config.tree_mode = self.table.tree_mode;
    }

    pub fn cycle_sort(&mut self, backwards: bool) {
        let fields: Vec<Field> = self.columns();
        if fields.is_empty() {
            return;
        }
        let cur = fields
            .iter()
            .position(|f| *f == self.table.sort.field)
            .unwrap_or(0);
        let next = if backwards {
            (cur + fields.len() - 1) % fields.len()
        } else {
            (cur + 1) % fields.len()
        };
        self.table.sort.field = fields[next];
        self.table.sort.dir = if self.table.sort.field.default_sort_desc() {
            Dir::Desc
        } else {
            Dir::Asc
        };
        self.status = Some(format!("sort: {}", self.table.sort.field.title().trim()));
    }

    fn move_selection(&mut self, delta: isize) {
        let n = self.table.visible();
        if n == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, n as isize - 1) as usize;
    }

    fn toggle_tag(&mut self) {
        if let Some(row) = self.selected_row() {
            let pid = row.pid;
            if !self.tags.remove(&pid) {
                self.tags.insert(pid);
            }
        }
    }

    fn tag_with_children(&mut self) {
        if let Some(row) = self.selected_row() {
            let pid = row.pid;
            for p in tree::subtree(&self.table.rows, pid) {
                self.tags.insert(p);
            }
        }
    }

    fn renice_selected(&mut self, delta: i8) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let action = gander_core::action::Action::Priority {
            pid: row.pid,
            delta,
        };
        match gander_core::action::execute(&action) {
            Ok(()) => self.status = Some(format!("reniced {}", row.pid)),
            Err(e) => self.status = Some(format!("renice failed: {e}")),
        }
    }

    fn show_env(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            return Reaction::Continue;
        };
        let pid = row.pid;
        let lines = read_limited(&format!("/proc/{pid}/environ"), |bytes| {
            bytes
                .split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect()
        })
        .unwrap_or_else(|| vec!["cannot read environment".to_string()]);
        self.screen = Screen::Info {
            title: format!("Environment of {pid}"),
            lines,
            scroll: 0,
        };
        Reaction::Continue
    }

    fn show_command(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            return Reaction::Continue;
        };
        let cmd = row.display_command().to_string();
        let lines: Vec<String> = wrap(&cmd, 80).into_iter().collect();
        self.screen = Screen::Info {
            title: format!("Command line of {}", row.pid),
            lines: if lines.is_empty() { vec![cmd] } else { lines },
            scroll: 0,
        };
        Reaction::Continue
    }

    fn show_open_files(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            return Reaction::Continue;
        };
        let pid = row.pid;
        let list = std::fs::read_dir(format!("/proc/{pid}/fd"))
            .map(|dir| {
                dir.filter_map(|e| e.ok())
                    .map(|e| {
                        let fd = e.file_name().to_string_lossy().into_owned();
                        let target = std::fs::read_link(e.path())
                            .map(|t| t.to_string_lossy().into_owned())
                            .unwrap_or_else(|_| "?".into());
                        format!("{fd:>4}  {target}")
                    })
                    .collect()
            })
            .unwrap_or_else(|_| vec!["cannot read file descriptors".to_string()]);
        self.screen = Screen::Info {
            title: format!("Open files of {pid}"),
            lines: list,
            scroll: 0,
        };
        Reaction::Continue
    }

    fn show_locks(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            return Reaction::Continue;
        };
        let pid = row.pid;
        let lines: Vec<String> = read_limited("/proc/locks", |bytes| {
            let text = String::from_utf8_lossy(bytes);
            let pid_marker = format!(" {pid} ");
            text.lines()
                .filter(|l| l.contains(&pid_marker))
                .map(|l| l.to_string())
                .collect()
        })
        .unwrap_or_default();
        self.screen = Screen::Info {
            title: format!("File locks of {pid}"),
            lines: if lines.is_empty() {
                vec!["no locks".to_string()]
            } else {
                lines
            },
            scroll: 0,
        };
        Reaction::Continue
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// Exposed for tests and the render harness.
    pub fn header_height_debug(&self) -> u16 {
        self.header_height()
    }

    /// Height the header needs: the taller column, plus the screen-tab row.
    ///
    /// A fixed height clipped the per-core CPU meters as soon as the machine had
    /// more than four cores.
    fn header_height(&self) -> u16 {
        let height_of = |column: crate::meters::HeaderColumn| -> u16 {
            self.meters
                .iter()
                .filter(|m| m.column() == column)
                .map(|m| m.rows().max(1))
                .sum()
        };
        height_of(crate::meters::HeaderColumn::Left)
            .max(height_of(crate::meters::HeaderColumn::Right))
            .max(1)
            + 1
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        // The header grows with the core count: 8 cores means two five-row CPU
        // meters, so a fixed four-row header would clip them.
        let header_h = self
            .header_height()
            .min(area.height.saturating_sub(2).max(1));
        let chunks = Layout::vertical([
            Constraint::Length(header_h),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        self.draw_header(chunks[0], frame.buffer_mut());
        self.draw_tabs(chunks[0], frame.buffer_mut());

        match &self.screen {
            Screen::ProcessList => self.draw_table(chunks[1], frame.buffer_mut()),
            Screen::Help => draw_help(chunks[1], frame.buffer_mut(), &self.theme),
            Screen::Info {
                title,
                lines,
                scroll,
            } => draw_info(
                chunks[1],
                frame.buffer_mut(),
                &self.theme,
                title,
                lines,
                *scroll,
            ),
            Screen::SignalPicker => {
                self.draw_table(chunks[1], frame.buffer_mut());
                draw_signal_picker(
                    chunks[1],
                    frame.buffer_mut(),
                    &self.theme,
                    self.selected_row(),
                );
            }
            Screen::Setup => draw_setup(chunks[1], frame.buffer_mut(), &self.theme),
        }

        self.draw_function_bar(chunks[2], frame.buffer_mut());
    }

    /// Draw the header meters in htop's two-column layout.
    ///
    /// Meters declare their column; within a column they are stacked in vector
    /// order. `LeftCPUs`/`RightCPUs` occupy one row per core.
    fn draw_header(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let mode = self.meter_mode;
        let theme = self.theme.clone();

        let has_right = self
            .meters
            .iter()
            .any(|m| m.column() == crate::meters::HeaderColumn::Right);

        // The last row of the header block belongs to the screen-tab strip.
        let meter_h = area.height.saturating_sub(1);

        if !has_right {
            let rect = Rect::new(area.x, area.y, area.width, meter_h);
            Self::draw_column(&mut self.meters, rect, buf, &theme, mode);
            return;
        }

        // Leave a one-cell gutter so the two columns do not run together.
        let left_w = area.width / 2;
        let left = Rect::new(area.x, area.y, left_w.saturating_sub(1), meter_h);
        let right = Rect::new(area.x + left_w, area.y, area.width - left_w, meter_h);

        let mut left_meters = Vec::new();
        let mut right_meters = Vec::new();
        for meter in std::mem::take(&mut self.meters) {
            if meter.column() == crate::meters::HeaderColumn::Right {
                right_meters.push(meter);
            } else {
                left_meters.push(meter);
            }
        }

        Self::draw_column(&mut left_meters, left, buf, &theme, mode);
        Self::draw_column(&mut right_meters, right, buf, &theme, mode);

        // Put the meters back so sampling continues to work.
        self.meters = left_meters;
        self.meters.extend(right_meters);
    }

    fn draw_column(
        meters: &mut [Box<dyn Meter>],
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
        mode: MeterMode,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut y = area.y;
        for meter in meters.iter_mut() {
            let h = meter.rows().max(1);
            if y + h > area.bottom() {
                break;
            }
            let rect = Rect::new(area.x, y, area.width, h);
            meter.render(rect, buf, theme, mode);
            y += h;
        }
    }

    fn draw_tabs(&self, area: Rect, buf: &mut Buffer) {
        let names: Vec<Line> = self
            .config
            .tabs
            .iter()
            .map(|t| Line::from(t.name.clone()))
            .collect();
        let tabs = Tabs::new(names)
            .select(
                self.config
                    .active_tab
                    .min(self.config.tabs.len().saturating_sub(1)),
            )
            .style(self.theme.header)
            .highlight_style(self.theme.header_value);
        let rect = Rect::new(
            area.x,
            area.y + area.height.saturating_sub(1),
            area.width,
            1,
        );
        if rect.height > 0 && area.height > 1 {
            ratatui::widgets::Widget::render(tabs, rect, buf);
        }
    }

    fn draw_table(&self, area: Rect, buf: &mut Buffer) {
        let fields = self.columns();
        let theme = &self.theme;

        if fields.is_empty() || area.height == 0 {
            return;
        }

        // Column widths: fixed widths for known fields, the last column takes
        // the remainder (htop makes Command the expanding column).
        let widths = column_widths(&fields, area.width);
        let total_width: u16 = widths.iter().sum();
        let _h_offset = self.h_offset.min(total_width.saturating_sub(area.width));

        // Header row.
        let mut x = area.x;
        for (i, field) in fields.iter().enumerate() {
            let w = widths[i];
            if w == 0 {
                continue;
            }
            let style = if *field == self.table.sort.field {
                theme.column_header_sorted
            } else {
                theme.column_header
            };
            let title = fit(field.title(), w as usize);
            crate::meters::put_str(buf, x, area.y, &title, style, w);
            x += w;
        }

        let body_height = area.height.saturating_sub(1) as usize;
        let visible = self.table.visible();
        if self.offset + body_height > visible && visible > body_height {
            // handled by the runtime's scroll clamp; keep rendering valid
        }

        let marks = if self.table.tree_mode {
            compose_tree(&self.table.rows, &self.table.view.order)
        } else {
            Vec::new()
        };

        for row_idx in 0..body_height {
            let pos = self.offset + row_idx;
            let Some(&data_idx) = self.table.view.order.get(pos) else {
                break;
            };
            let row = &self.table.rows[data_idx];
            let y = area.y + 1 + row_idx as u16;

            let base = if pos == self.selected {
                theme.row_selected
            } else if self.tags.contains(&row.pid) {
                theme.row_tagged
            } else if row.is_kernel_thread() {
                theme.row_kernel_thread
            } else {
                theme.row_normal
            };
            let base = if row.flags.contains(gander_core::row::RowFlags::STALE_EXE)
                && pos != self.selected
            {
                theme.row_stale_exe
            } else {
                base
            };

            let mut cx = area.x;
            for (i, field) in fields.iter().enumerate() {
                let w = widths[i];
                if w == 0 {
                    continue;
                }
                if cx + w > area.x && cx < area.right() {
                    let mut text = render_field(row, *field, &self.snapshot);
                    // In tree mode the Command column carries the branch prefix.
                    if self.table.tree_mode && *field == Field::Comm {
                        if let Some(mark) = marks.iter().find(|m| m.data_index == data_idx) {
                            text.insert_str(0, &mark.prefix);
                        }
                    }
                    let padded = pad_right(&text, w as usize);
                    crate::meters::put_str(buf, cx, y, &padded, base, w);
                }
                cx += w;
            }
        }
    }

    fn draw_function_bar(&self, area: Rect, buf: &mut Buffer) {
        let labels = match self.screen {
            Screen::ProcessList => [
                ("F1", "Help"),
                ("F2", "Setup"),
                ("F3", "Search"),
                ("F4", "Filter"),
                ("F5", "Tree"),
                ("F6", "SortBy"),
                ("F7", "Nice-"),
                ("F8", "Nice+"),
                ("F9", "Kill"),
                ("F10", "Quit"),
            ],
            Screen::Help => [
                ("F1", "Done"),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("F10", "Quit"),
            ],
            Screen::Info { .. } => [
                ("Esc", "Back"),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("F10", "Quit"),
            ],
            Screen::SignalPicker => [
                ("Esc", "Cancel"),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("Enter", "Send"),
                ("F10", "Quit"),
            ],
            Screen::Setup => [
                ("Esc", "Done"),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("F10", "Quit"),
            ],
        };

        if area.height == 0 {
            return;
        }
        // Paint the background first.
        for x in area.x..area.right() {
            if let Some(cell) = buf.cell_mut((x, area.y)) {
                cell.set_char(' ');
                cell.set_style(self.theme.function_bar);
            }
        }

        let slot = area.width / 10;
        for (i, (key, label)) in labels.iter().enumerate() {
            let x = area.x + i as u16 * slot;
            if key.is_empty() && label.is_empty() {
                continue;
            }
            crate::meters::put_str(buf, x, area.y, key, self.theme.function_bar_key, slot);
            crate::meters::put_str(
                buf,
                x + key.len() as u16,
                area.y,
                label,
                self.theme.function_bar,
                slot.saturating_sub(key.len() as u16),
            );
        }
    }
}

/// The key set the app understands. Input decoding lives in the runtime.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PgUp,
    PgDn,
    Tab,
    BackTab,
    Enter,
    Esc,
    Backspace,
    Delete,
    F(u8),
}

// ---------------------------------------------------------------------------
// Field rendering
// ---------------------------------------------------------------------------

/// Fixed column widths, with the last field expanding.
fn column_widths(fields: &[Field], total: u16) -> Vec<u16> {
    let mut widths: Vec<u16> = fields.iter().map(|f| default_width(*f)).collect();
    let fixed: u16 = widths.iter().sum();
    if let Some(last) = widths.last_mut() {
        let expand = total.saturating_sub(fixed);
        *last = last.saturating_add(expand).max(6);
    }
    widths
}

fn default_width(field: Field) -> u16 {
    match field {
        Field::Pid => 7,
        Field::Ppid | Field::Pgrp => 7,
        Field::Comm => 16,
        Field::State => 2,
        Field::User => 10,
        Field::Priority | Field::Nice => 4,
        Field::PercentCpu | Field::PercentNormCpu | Field::PercentMem => 6,
        Field::Time => 9,
        Field::Elapsed => 10,
        Field::VirtMem | Field::ResMem => 6,
        Field::Nlwp => 5,
        Field::Processor => 4,
        Field::MinFlt | Field::MajFlt => 8,
        _ => 8,
    }
}

/// Render one field to text. Formatting happens here, never during a scan.
pub fn render_field(row: &Row, field: Field, snapshot: &SystemSnapshot) -> String {
    match field {
        Field::Pid => row.pid.to_string(),
        Field::Ppid => row.ppid.to_string(),
        Field::Pgrp => row.pgrp.to_string(),
        Field::Comm => row.display_command().to_string(),
        Field::State => (row.state as char).to_string(),
        Field::User => row.uid.to_string(),
        Field::Priority => row.priority.to_string(),
        Field::Nice => row.nice.to_string(),
        Field::PercentCpu => format::percent_human(row.percent_cpu).to_string(),
        Field::PercentNormCpu => {
            let cores = snapshot.cpu_count().max(1) as f32;
            format::percent_human(row.percent_cpu / cores).to_string()
        }
        Field::PercentMem => format::percent_human(row.percent_mem).to_string(),
        Field::Time => format::cpu_time_human(row.time_ticks, snapshot.clk_tck).to_string(),
        Field::Elapsed => format::duration_human(row.elapsed).to_string(),
        Field::VirtMem => format::bytes_human(row.virt).to_string(),
        Field::ResMem => format::bytes_human(row.res).to_string(),
        Field::Nlwp => row.nlwp.to_string(),
        Field::Processor => row.processor.to_string(),
        Field::MinFlt => row.min_flt.to_string(),
        Field::MajFlt => row.maj_flt.to_string(),
        _ => "-".to_string(),
    }
}

fn fit(text: &str, width: usize) -> String {
    let mut out: String = text.chars().take(width).collect();
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn pad_right(text: &str, width: usize) -> String {
    let mut out: String = text.chars().take(width).collect();
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn read_limited<T>(path: &str, f: impl FnOnce(&[u8]) -> T) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    // Guard against pathological files (environ can be large but bounded).
    if bytes.len() > 4 * 1024 * 1024 {
        return None;
    }
    Some(f(&bytes))
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for ch in text.chars() {
        line.push(ch);
        if line.chars().count() >= width.max(1) {
            out.push(std::mem::take(&mut line));
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

// ---------------------------------------------------------------------------
// Overlays
// ---------------------------------------------------------------------------

fn draw_help(area: Rect, buf: &mut Buffer, theme: &Theme) {
    let lines = [
        "gander — keys",
        "",
        "  F1 h ?   help            F2       setup",
        "  F3 /     search          F4 \\     filter",
        "  F5 t     tree view       F6 < >   sort column",
        "  F7 F8    nice -/+        F9 k     kill",
        "  Space    tag process     c        tag children",
        "  U        untag all       I        meter mode",
        "  K        toggle kernel threads",
        "  e        environment     w        command line",
        "  l        open files      x        file locks",
        "  + -      faster/slower   F10 q    quit",
        "",
        "Filter terms are fixed strings, case-insensitive, separated by |.",
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.dialog_border)
        .title(span_title("Help", theme));
    let inner = block.inner(area);
    Widget::render(block, area, buf);
    let para = Paragraph::new(lines.join("\n")).style(theme.row_normal);
    ratatui::widgets::Widget::render(para, inner, buf);
}

fn span_title<'a>(text: &'a str, theme: &Theme) -> ratatui::text::Span<'a> {
    ratatui::text::Span::styled(text, theme.dialog_title)
}

fn draw_info(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    title: &str,
    lines: &[String],
    scroll: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.dialog_border)
        .title(span_title(title, theme));
    let inner = block.inner(area);
    ratatui::widgets::Widget::render(block, area, buf);

    let visible = inner.height as usize;
    let text: Vec<Line> = lines
        .iter()
        .skip(scroll)
        .take(visible)
        .map(|l| Line::from(l.clone()))
        .collect();
    let para = Paragraph::new(text).style(theme.row_normal);
    ratatui::widgets::Widget::render(para, inner, buf);
}

fn draw_signal_picker(area: Rect, buf: &mut Buffer, theme: &Theme, row: Option<&Row>) {
    let w = 40.min(area.width);
    let h = (gander_core::action::SIGNALS.len() as u16 + 4).min(area.height);
    let rect = Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    Widget::render(Clear, rect, buf);
    let title = match row {
        Some(r) => format!("Send signal to {} ({})", r.pid, r.comm.as_str()),
        None => "Send signal".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.dialog_border)
        .title(span_title(&title, theme));
    let inner = block.inner(rect);
    ratatui::widgets::Widget::render(block, rect, buf);

    let lines: Vec<Line> = gander_core::action::SIGNALS
        .iter()
        .enumerate()
        .map(|(i, (num, name))| Line::from(format!("{:>2}  {num:>2}  {name}", i + 1)))
        .collect();
    let para = Paragraph::new(lines).style(theme.row_normal);
    ratatui::widgets::Widget::render(para, inner, buf);
}

fn draw_setup(area: Rect, buf: &mut Buffer, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.dialog_border)
        .title(span_title("Setup", theme));
    let inner = block.inner(area);
    ratatui::widgets::Widget::render(block, area, buf);
    let text = "Setup screen is not implemented yet.\n\nPress Esc to return.\n\nPlanned pages: Header, Columns, Display, Colors, Tabs.";
    let para = Paragraph::new(text).style(theme.row_normal);
    ratatui::widgets::Widget::render(para, inner, buf);
}

#[allow(dead_code)]
fn _style_helper(theme: &Theme) -> Style {
    theme.row_normal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u32, cmd: &str) -> Row {
        let mut r = Row::default_for_test(pid);
        r.comm.try_push_str(cmd).unwrap();
        r
    }

    fn app_with(rows: Vec<Row>) -> App {
        let mut app = App::new(Config::default());
        app.apply_sample(SystemSnapshot::default(), rows);
        app
    }

    #[test]
    fn sort_cycle_advances_through_columns() {
        let mut app = app_with(vec![row(1, "a"), row(2, "b")]);
        let first = app.table.sort.field;
        app.cycle_sort(false);
        assert_ne!(app.table.sort.field, first);
        // cycling through every column returns to the start
        for _ in 0..app.columns().len() - 1 {
            app.cycle_sort(false);
        }
        assert_eq!(app.table.sort.field, first);
    }

    #[test]
    fn selection_is_clamped_to_visible_rows() {
        let mut app = app_with(vec![row(1, "a"), row(2, "b")]);
        app.handle_key(Key::Down);
        app.handle_key(Key::Down);
        app.handle_key(Key::Down);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn tagging_toggles() {
        let mut app = app_with(vec![row(7, "a")]);
        app.handle_key(Key::Char(' '));
        assert!(app.tags.contains(&7));
        app.handle_key(Key::Char(' '));
        assert!(!app.tags.contains(&7));
    }

    #[test]
    fn filter_is_applied_while_typing() {
        let mut app = app_with(vec![row(1, "alpha"), row(2, "beta")]);
        app.handle_key(Key::Char('\\'));
        assert!(app.input.active);
        app.handle_key(Key::Char('b'));
        assert_eq!(app.table.visible(), 1);
    }

    #[test]
    fn quit_sets_flag() {
        let mut app = app_with(vec![]);
        assert_eq!(app.handle_key(Key::Char('q')), Reaction::Quit);
        assert!(app.quit);
    }

    #[test]
    fn esc_leaves_help() {
        let mut app = app_with(vec![]);
        app.handle_key(Key::F(1));
        assert_eq!(app.screen, Screen::Help);
        app.handle_key(Key::Esc);
        assert_eq!(app.screen, Screen::ProcessList);
    }

    #[test]
    fn speed_keys_respect_bounds() {
        let mut app = app_with(vec![]);
        app.config.delay_ms = 100;
        app.handle_key(Key::Char('+'));
        assert_eq!(app.config.delay_ms, 100);
        app.config.delay_ms = 10_000;
        app.handle_key(Key::Char('-'));
        assert_eq!(app.config.delay_ms, 10_000);
    }

    #[test]
    fn header_shows_one_row_per_core() {
        // Regression guard: the header was a fixed four rows, so an 8-core
        // machine only ever showed the aggregate and the rest were clipped.
        use gander_core::system::{CpuTimes, SystemSnapshot};

        for cores in [2usize, 4, 8, 16] {
            let snapshot = SystemSnapshot {
                cpu: vec![CpuTimes::default(); cores + 1],
                cpu_delta: vec![CpuTimes::default(); cores + 1],
                ..Default::default()
            };

            let mut app = App::new(Config::default());
            app.apply_sample(snapshot, Vec::new());

            let cpu_rows: u16 = app
                .meters
                .iter()
                .filter(|m| m.name().starts_with("CPU"))
                .map(|m| m.rows())
                .sum();

            // More than four cores: two meters, each one average row plus half
            // the cores, so every core appears exactly once.
            // Four or fewer: a single aggregate row.
            let expected = if cores > 4 {
                let half = cores / 2;
                (half + 1) + (cores - half + 1)
            } else {
                1
            };
            assert_eq!(
                cpu_rows as usize, expected,
                "cores={cores} should render {expected} CPU rows"
            );
        }
    }

    #[test]
    fn header_splits_into_two_columns_above_four_cores() {
        use crate::meters::HeaderColumn;
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 9],
            cpu_delta: vec![CpuTimes::default(); 9],
            ..Default::default()
        };

        let mut app = App::new(Config::default());
        app.apply_sample(snapshot, Vec::new());

        let left = app
            .meters
            .iter()
            .filter(|m| m.column() == HeaderColumn::Left)
            .count();
        let right = app
            .meters
            .iter()
            .filter(|m| m.column() == HeaderColumn::Right)
            .count();
        assert!(left > 0 && right > 0, "expected two non-empty columns");
    }

    #[test]
    fn small_machines_use_a_single_cpu_meter() {
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let snapshot = SystemSnapshot {
            // 4 cores plus the aggregate entry.
            cpu: vec![CpuTimes::default(); 5],
            cpu_delta: vec![CpuTimes::default(); 5],
            ..Default::default()
        };

        let mut app = App::new(Config::default());
        app.apply_sample(snapshot, Vec::new());

        let cpu_meters: Vec<_> = app
            .meters
            .iter()
            .filter(|m| m.name().starts_with("CPU"))
            .collect();
        assert_eq!(cpu_meters.len(), 1);
        assert_eq!(cpu_meters[0].rows(), 1, "4 cores or fewer show one row");
    }

    #[test]
    fn header_height_grows_with_cores() {
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let height_for = |cores: usize| {
            let snapshot = SystemSnapshot {
                cpu: vec![CpuTimes::default(); cores + 1],
                cpu_delta: vec![CpuTimes::default(); cores + 1],
                ..Default::default()
            };
            let mut app = App::new(Config::default());
            app.apply_sample(snapshot, Vec::new());
            app.header_height()
        };

        // Four or fewer cores: one CPU row on the left, three text meters on the
        // right, plus the tab row => 4.
        assert_eq!(height_for(2), 4);
        assert_eq!(height_for(4), 4);
        // 8 cores: five CPU rows per column, plus the tab row.
        assert_eq!(height_for(8), 9);
        assert!(height_for(8) > height_for(4), "8 cores must not be clipped");
        assert!(height_for(16) > height_for(8));
    }

    #[test]
    fn render_field_formats_without_panicking() {
        let snap = SystemSnapshot::default();
        let r = row(1, "x");
        for f in [
            Field::Pid,
            Field::Comm,
            Field::PercentCpu,
            Field::PercentMem,
            Field::Time,
            Field::Elapsed,
            Field::VirtMem,
            Field::ResMem,
            Field::User,
            Field::State,
        ] {
            let _ = render_field(&r, f, &snap);
        }
    }

    #[test]
    fn draw_does_not_panic_on_empty_state() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut app = app_with(Vec::new());
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn draw_with_a_live_sample() {
        use gander_core::PlatformSampler as _;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut sampler = gander_core::Sampler::default();
        let (snapshot, rows) = sampler.sample();
        assert!(!rows.is_empty(), "a live sample must contain processes");

        let mut app = App::new(Config::default());
        app.apply_sample(snapshot, rows);

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = buffer
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(rendered.contains("PID"), "header must be drawn");
        assert!(rendered.contains("F10"), "function bar must be drawn");
    }

    #[test]
    fn draw_does_not_panic_with_rows() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut app = app_with(vec![row(1, "init"), row(2, "bash")]);
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
    }
}
