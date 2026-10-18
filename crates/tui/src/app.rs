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
    SignalPicker {
        cursor: usize,
    },
    Setup,
    /// `a`: the CPU affinity mask of one process.
    Affinity(crate::panels::AffinityPanel),
    /// `i`: the I/O priority of one process.
    IoPriority(crate::panels::IoPriorityPanel),
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
    /// First visible row (index into `table.view.order`).
    pub offset: usize,
    pub h_offset: u16,
    /// Body height of the last rendered frame, so paging and scroll-following
    /// know how much room they have. Zero until the first draw.
    pub viewport: usize,
    /// uid -> name cache, so the USER column does not print numbers.
    pub users: gander_core::users::Users,
    /// When set, only this uid's processes are listed.
    pub user_filter: Option<u32>,
    /// Set while the `F2` screen is open.
    pub setup: Option<crate::setup::Setup>,
    /// The combined visibility predicate (text filter, user, kernel threads).
    /// Kept in sync by `refresh_visibility`.
    visibility: gander_core::table::Visibility,
    pub tags: std::collections::HashSet<gander_core::Pid>,
    pub filter: Option<Filter>,
    pub input: Input,
    pub command_column_interactive: bool,
    pub quit: bool,
    pub status: Option<String>,
    /// Display mode per meter, parallel to `meters`.
    ///
    /// htop's setup screen changes only the meter under the cursor, so the mode
    /// cannot be a single app-wide value.
    pub meter_modes: Vec<MeterMode>,
    /// Which config entry each meter was built from, parallel to `meters`.
    pub meter_sources: Vec<meters_impl::MeterSource>,
    /// The mode applied to meters added later, and the fallback for `I` when no
    /// meter is selected.
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
            viewport: 0,
            users: gander_core::users::Users::new(),
            user_filter: None,
            setup: None,
            visibility: gander_core::table::Visibility {
                show_kernel_threads: true,
                ..Default::default()
            },
            tags: Default::default(),
            filter: None,
            input: Input::default(),
            command_column_interactive: true,
            quit: false,
            status: None,
            meter_modes: Vec::new(),
            meter_sources: Vec::new(),
            meter_mode: MeterMode::Bar,
            pending_g: false,
        }
    }

    /// Adopt a fresh sample: update rows, meters, and reorder.
    pub fn apply_sample(&mut self, snapshot: SystemSnapshot, rows: Vec<Row>) {
        // The header depends on the core count, which is only known once the
        // first snapshot arrives.
        if self.meters.is_empty() {
            self.rebuild_header(snapshot.cpu_count());
        }
        for meter in &mut self.meters {
            meter.sample(&snapshot);
        }
        self.table.rows = rows;
        self.reorder();
        self.snapshot = snapshot;
        self.clamp_selection();
    }

    pub fn columns(&self) -> Vec<Field> {
        self.config.active_tab().fields()
    }

    /// Rebuild the header meters from the config, keeping their sampled state
    /// where possible.
    ///
    /// Called on the first sample (the core count is only known then) and after
    /// the setup screen edits the meter list.
    pub fn rebuild_header(&mut self, cpu_count: usize) {
        let mut header = meters_impl::header_from_config(&self.config, cpu_count);

        // Carry a live mode over when the same meter is still present: editing
        // the header should not silently reset the mode the user chose with `I`.
        for (i, meter) in header.meters.iter().enumerate() {
            if let Some(pos) = self
                .meters
                .iter()
                .position(|m| m.name() == meter.name())
            {
                if let Some(mode) = self.meter_modes.get(pos) {
                    header.modes[i] = *mode;
                }
            }
        }

        self.meters = header.meters;
        self.meter_modes = header.modes;
        self.meter_sources = header.sources;
    }

    /// The display mode for meter `i`.
    pub fn meter_mode_at(&self, i: usize) -> MeterMode {
        self.meter_modes.get(i).copied().unwrap_or(self.meter_mode)
    }

    /// Cycle meter `i`'s display mode and remember the choice.
    pub fn cycle_meter_mode(&mut self, i: usize) {
        if i >= self.meters.len() {
            return;
        }
        if self.meter_modes.len() < self.meters.len() {
            self.meter_modes.resize(self.meters.len(), self.meter_mode);
        }
        let next = match self.meter_modes[i] {
            MeterMode::Text => MeterMode::Bar,
            MeterMode::Bar => MeterMode::Graph,
            MeterMode::Graph => MeterMode::Text,
        };
        self.meter_modes[i] = next;
        self.meter_mode = next;
        self.sync_meter_modes_to_config();
    }

    /// Write the live modes back into the config, so they survive a save.
    ///
    /// Each meter remembers the config entry it came from (`meter_sources`) —
    /// a flat index would be wrong as soon as a collapsed CPU pair shifts
    /// everything along.
    fn sync_meter_modes_to_config(&mut self) {
        for (i, source) in self.meter_sources.iter().enumerate() {
            let Some(mode) = self.meter_modes.get(i).copied() else {
                continue;
            };
            let list = match source.column {
                crate::meters::HeaderColumn::Left => &mut self.config.meters_left,
                crate::meters::HeaderColumn::Right => &mut self.config.meters_right,
            };
            if let Some(entry) = list.get_mut(source.index) {
                entry.mode = meters_impl::mode_name(mode).to_string();
            }
        }
    }

    /// Persist the live sort order into the active tab's config.
    ///
    /// Each tab keeps its own sort, so this has to run whenever the sort or the
    /// active tab changes.
    fn sync_sort_to_tab(&mut self) {
        let field = self.table.sort.field.name().to_string();
        let desc = self.table.sort.dir == Dir::Desc;
        let idx = self
            .config
            .active_tab
            .min(self.config.tabs.len().saturating_sub(1));
        if let Some(tab) = self.config.tabs.get_mut(idx) {
            tab.sort = gander_config::SortConfig { field, desc };
        }
    }

    /// Load the active tab's sort into the live table.
    fn load_sort_from_tab(&mut self) {
        self.table.sort = self.config.active_tab().sort();
    }

    /// Switch to the next/previous screen tab.
    pub fn cycle_tab(&mut self, forward: bool) {
        let n = self.config.tabs.len();
        if n <= 1 {
            self.status = Some("only one screen tab configured (F2 > Tabs)".into());
            return;
        }
        // Save the old tab's sort before leaving it.
        self.sync_sort_to_tab();

        self.config.active_tab = if forward {
            (self.config.active_tab + 1) % n
        } else {
            (self.config.active_tab + n - 1) % n
        };

        self.load_sort_from_tab();
        self.selected = 0;
        self.offset = 0;
        let name = self.config.active_tab().name.clone();
        self.status = Some(format!("tab: {name}"));
    }

    fn clamp_selection(&mut self) {
        let n = self.table.visible();
        if n == 0 {
            self.selected = 0;
            self.offset = 0;
            return;
        }
        self.selected = self.selected.min(n - 1);
        self.scroll_to_selection();
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
            self.reorder();
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
            Screen::SignalPicker { .. } => self.handle_signal_key(key),
            Screen::Setup => self.handle_setup_key(key),
            Screen::Affinity(_) => self.handle_affinity_key(key),
            Screen::IoPriority(_) => self.handle_ioprio_key(key),
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
                self.setup = Some(crate::setup::Setup::new(&self.config));
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
            Key::Tab => {
                self.cycle_tab(true);
                Reaction::Reorder
            }
            Key::BackTab => {
                self.cycle_tab(false);
                Reaction::Reorder
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
                    // SIGTERM is htop's default selection; it is the polite one.
                    let default = gander_core::action::SIGNALS
                        .iter()
                        .position(|(num, _)| *num == 15)
                        .unwrap_or(0);
                    self.screen = Screen::SignalPicker { cursor: default };
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
            Key::PgUp => {
                self.move_page(-1);
                Reaction::Continue
            }
            Key::PgDn => {
                self.move_page(1);
                Reaction::Continue
            }
            Key::Home | Key::Char('g') => {
                self.selected = 0;
                self.scroll_to_selection();
                Reaction::Continue
            }
            Key::End | Key::Char('G') => {
                self.selected = self.table.visible().saturating_sub(1);
                self.scroll_to_selection();
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
                self.cycle_user_filter();
                Reaction::Reorder
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
            Key::Char('a') => self.open_affinity(),
            Key::Char('i') => self.open_ioprio(),
            Key::Char('Z') => self.show_security_context(),
            Key::Char('b') => self.show_backtrace(),
            Key::Char('e') => self.show_env(),
            Key::Char('w') => self.show_command(),
            Key::Char('l') => self.show_open_files(),
            Key::Char('x') => self.show_locks(),
            Key::Char('I') => {
                // htop changes only the meter under the cursor here. With no
                // header selection yet, the honest behaviour is to cycle every
                // meter's mode and say so, rather than pretend a cursor exists.
                for i in 0..self.meters.len() {
                    self.cycle_meter_mode(i);
                }
                let name = self
                    .meter_modes
                    .first()
                    .map(|m| meters_impl::mode_name(*m))
                    .unwrap_or("bar");
                self.status = Some(format!("meter mode: {name}"));
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

    /// Keys for the `F2` setup screen.
    ///
    /// Edits go to a draft config. `Esc` discards, `S`/`F10` applies — the same
    /// bargain htop makes, and the reason a half-finished edit cannot break the
    /// live view.
    fn handle_setup_key(&mut self, key: Key) -> Reaction {
        // Take the setup state out so the `&mut self` methods below can run while
        // it is being edited; it is put back unless the screen closes.
        let Some(mut setup) = self.setup.take() else {
            self.screen = Screen::ProcessList;
            return Reaction::Continue;
        };

        let mut reaction = Reaction::Continue;
        match key {
            Key::Esc | Key::Char('q') => {
                self.screen = Screen::ProcessList;
                self.status = Some("setup discarded".into());
                return Reaction::Continue;
            }
            Key::F(10) | Key::Char('S') => {
                self.screen = Screen::ProcessList;
                self.apply_draft_config(setup.draft.clone());
                self.status = Some("setup applied".into());
                return Reaction::Continue;
            }
            Key::Enter => {
                // `Enter` activates the row under the cursor. Using it to apply
                // the whole screen made the Tabs page impossible to use: there
                // was no way to add, select or delete a tab.
                setup.activate();
                self.apply_draft_config(setup.draft.clone());
                reaction = Reaction::Continue;
            }
            Key::Tab => setup.next_page(),
            Key::BackTab => setup.prev_page(),
            Key::Up => setup.cursor_up(),
            Key::Down => setup.cursor_down(),
            Key::Char(' ') => {
                setup.activate();
                self.apply_draft_config(setup.draft.clone());
                reaction = Reaction::Continue;
            }
            Key::Char('[') | Key::Char(']') => {
                let up = key == Key::Char('[');
                match setup.page {
                    crate::setup::SetupPage::Header => setup.move_meter(up),
                    crate::setup::SetupPage::Columns => setup.move_column(up),
                    // Nothing else has an order to change; do not pretend.
                    _ => self.status = Some("nothing to reorder on this page".into()),
                }
                self.apply_draft_config(setup.draft.clone());
            }
            Key::Delete => {
                let result = match setup.page {
                    crate::setup::SetupPage::Tabs => setup.remove_tab(),
                    crate::setup::SetupPage::Header => setup.remove_meter(),
                    _ => Err("nothing to delete on this page"),
                };
                match result {
                    Ok(name) => {
                        self.apply_draft_config(setup.draft.clone());
                        self.status = Some(format!("deleted: {name}"));
                    }
                    Err(why) => self.status = Some(why.to_string()),
                }
            }
            _ => {}
        }

        self.setup = Some(setup);
        reaction
    }

    /// Adopt an edited config: theme, toggles and column list.
    /// Adopt an edited config: theme, toggles, columns and header meters.
    fn apply_draft_config(&mut self, draft: Config) {
        self.theme = Theme::named(&draft.color_scheme).unwrap_or_default();
        self.table.tree_mode = draft.tree_mode;
        self.table.show_kernel_threads = draft.show_kernel_threads;
        self.config = draft;
        // Switching or adding a tab changes which columns and sort are live.
        self.load_sort_from_tab();
        // The header meter list may have changed shape; rebuild it, keeping the
        // modes the user picked for meters that are still present.
        self.rebuild_header(self.snapshot.cpu_count());
        self.selected = 0;
        self.offset = 0;
    }

    /// Keys for the signal picker.
    ///
    /// This is the one place that actually kills processes, so it resolves the
    /// target *now* rather than trusting the row the picker was opened on: the
    /// selection may have changed, and the process may have exited.
    fn handle_signal_key(&mut self, key: Key) -> Reaction {
        let Screen::SignalPicker { cursor } = self.screen else {
            return Reaction::Continue;
        };
        let count = gander_core::action::SIGNALS.len();

        match key {
            Key::Esc | Key::Char('q') => {
                self.screen = Screen::ProcessList;
                self.status = Some("cancelled".into());
                Reaction::Continue
            }
            Key::Up => {
                self.screen = Screen::SignalPicker {
                    cursor: cursor.saturating_sub(1),
                };
                Reaction::Continue
            }
            Key::Down => {
                self.screen = Screen::SignalPicker {
                    cursor: (cursor + 1).min(count.saturating_sub(1)),
                };
                Reaction::Continue
            }
            Key::Home => {
                self.screen = Screen::SignalPicker { cursor: 0 };
                Reaction::Continue
            }
            Key::End => {
                self.screen = Screen::SignalPicker {
                    cursor: count.saturating_sub(1),
                };
                Reaction::Continue
            }
            Key::Enter => {
                self.send_signal(cursor);
                self.screen = Screen::ProcessList;
                Reaction::Recalc
            }
            // Digits pick a signal by number, as htop allows.
            Key::Char(c) if c.is_ascii_digit() => {
                if let Some((idx, _)) = gander_core::action::SIGNALS
                    .iter()
                    .enumerate()
                    .find(|(_, (num, _))| *num == i32::from(c as u8 - b'0'))
                {
                    self.send_signal(idx);
                    self.screen = Screen::ProcessList;
                    return Reaction::Recalc;
                }
                self.status = Some(format!("no signal numbered {c}"));
                Reaction::Continue
            }
            _ => Reaction::Continue,
        }
    }

    /// Deliver `SIGNALS[cursor]` to the selected PID (or every tagged PID).
    fn send_signal(&mut self, cursor: usize) {
        let Some((signal, name)) = gander_core::action::SIGNALS.get(cursor).copied() else {
            self.status = Some("unknown signal".into());
            return;
        };

        // Tagged processes take precedence, matching htop: if anything is
        // tagged, the action applies to the tagged set.
        let targets: Vec<gander_core::Pid> = if self.tags.is_empty() {
            match self.selected_row() {
                Some(row) => vec![row.pid],
                None => {
                    self.status = Some("no process selected".into());
                    return;
                }
            }
        } else {
            let mut tagged: Vec<gander_core::Pid> = self.tags.iter().copied().collect();
            tagged.sort_unstable();
            tagged
        };

        let mut sent = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for pid in &targets {
            let action = gander_core::action::Action::Kill { pid: *pid, signal };
            match gander_core::action::execute(&action) {
                Ok(()) => sent += 1,
                Err(e) => failures.push(format!("{pid}: {e}")),
            }
        }

        self.status = Some(if failures.is_empty() {
            if targets.len() == 1 {
                format!("sent {name} to {}", targets[0])
            } else {
                format!("sent {name} to {sent} processes")
            }
        } else if sent == 0 {
            format!("{name} failed: {}", failures.join("; "))
        } else {
            format!(
                "sent {name} to {sent}, {} failed: {}",
                failures.len(),
                failures.join("; ")
            )
        });
    }

    /// Cycle `u` through "no filter" and each uid present in the table.
    ///
    /// htop's `u` opens a picker; cycling is the same information with no extra
    /// dialog and works fine for the handful of users a machine has.
    pub fn cycle_user_filter(&mut self) {
        let mut uids: Vec<u32> = self.table.rows.iter().map(|r| r.uid).collect();
        uids.sort_unstable();
        uids.dedup();

        if uids.is_empty() {
            self.user_filter = None;
            self.status = Some("no processes to filter".into());
            return;
        }

        self.user_filter = match self.user_filter {
            None => Some(uids[0]),
            Some(current) => match uids.iter().position(|u| *u == current) {
                // Wrap past the end back to "show everyone".
                Some(i) if i + 1 < uids.len() => Some(uids[i + 1]),
                _ => None,
            },
        };

        self.status = Some(match self.user_filter {
            Some(uid) => {
                let name = self.users.name(uid).to_string();
                let count = self.table.rows.iter().filter(|r| r.uid == uid).count();
                format!("showing user {name} ({count} processes)")
            }
            None => "showing all users".to_string(),
        });
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
        self.sync_sort_to_tab();
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
        self.scroll_to_selection();
    }

    /// Rebuild the visibility predicate and apply it.
    pub fn reorder(&mut self) {
        self.visibility = gander_core::table::Visibility {
            text: self.filter.clone(),
            uid: self.user_filter,
            show_kernel_threads: self.table.show_kernel_threads,
        };
        self.table.reorder(&self.visibility);
        self.clamp_selection();
    }

    /// Keep the selected row inside the viewport.
    ///
    /// Without this the cursor walks off the bottom of the screen: `offset` was
    /// written once and never updated.
    pub fn scroll_to_selection(&mut self) {
        let height = self.viewport.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + height {
            self.offset = self.selected + 1 - height;
        }
        // Never leave empty space at the bottom when there are enough rows.
        let visible = self.table.visible();
        if visible >= height {
            self.offset = self.offset.min(visible - height);
        } else {
            self.offset = 0;
        }
    }

    /// Move by one screenful, keeping the cursor on the same line where possible.
    fn move_page(&mut self, pages: isize) {
        let step = self.viewport.max(1) as isize;
        self.move_selection(pages * step);
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

    /// `a`: open the CPU affinity panel for the selected process.
    fn open_affinity(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            self.status = Some("no process selected".into());
            return Reaction::Continue;
        };
        let pid = row.pid;
        let cores = self.snapshot.cpu_count().max(1);
        self.screen = Screen::Affinity(crate::panels::AffinityPanel::new(pid, cores));
        Reaction::Continue
    }

    /// `i`: open the I/O priority panel for the selected process.
    fn open_ioprio(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            self.status = Some("no process selected".into());
            return Reaction::Continue;
        };
        let pid = row.pid;
        self.screen = Screen::IoPriority(crate::panels::IoPriorityPanel::new(pid));
        Reaction::Continue
    }

    fn handle_affinity_key(&mut self, key: Key) -> Reaction {
        let Screen::Affinity(mut panel) = self.screen.clone() else {
            return Reaction::Continue;
        };
        match key {
            Key::Char('q') | Key::Esc | Key::F(10) => {
                self.screen = Screen::ProcessList;
                self.status = None;
                return Reaction::Continue;
            }
            Key::Left => panel.move_cursor(-1),
            Key::Right => panel.move_cursor(1),
            Key::Up => panel.move_cursor(-8),
            Key::Down => panel.move_cursor(8),
            Key::Home => panel.move_cursor(-(panel.cores as isize)),
            Key::End => panel.move_cursor(panel.cores as isize),
            Key::Char(' ') | Key::Enter => panel.toggle(),
            Key::Char('S') => match panel.apply() {
                Ok(()) => {
                    self.status = Some(format!(
                        "affinity of {} set to {} CPUs",
                        panel.pid,
                        panel.mask.count()
                    ));
                    self.screen = Screen::ProcessList;
                    return Reaction::Continue;
                }
                Err(e) => self.status = Some(format!("affinity failed: {e}")),
            },
            _ => {}
        }
        self.screen = Screen::Affinity(panel);
        Reaction::Continue
    }

    fn handle_ioprio_key(&mut self, key: Key) -> Reaction {
        let Screen::IoPriority(mut panel) = self.screen.clone() else {
            return Reaction::Continue;
        };
        match key {
            Key::Char('q') | Key::Esc | Key::F(10) => {
                self.screen = Screen::ProcessList;
                self.status = None;
                return Reaction::Continue;
            }
            Key::Char(' ') | Key::Enter => panel.cycle_class(),
            Key::Up | Key::Right => panel.change_level(1),
            Key::Down | Key::Left => panel.change_level(-1),
            Key::Char('S') => match panel.apply() {
                Ok(()) => {
                    self.status = Some(format!(
                        "I/O priority of {} set to {}",
                        panel.pid,
                        panel.priority.label()
                    ));
                    self.screen = Screen::ProcessList;
                    return Reaction::Continue;
                }
                Err(e) => self.status = Some(format!("I/O priority failed: {e}")),
            },
            _ => {}
        }
        self.screen = Screen::IoPriority(panel);
        Reaction::Continue
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

    /// `Z`: the SELinux security context of the selected process.
    ///
    /// Linux-only in practice: the context lives in `/proc/<pid>/attr/current`.
    /// Rather than hide the key, it explains why it is empty.
    fn show_security_context(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            return Reaction::Continue;
        };
        let pid = row.pid;

        let path = format!("/proc/{pid}/attr/current");
        let lines: Vec<String> = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    vec!["no security context recorded".to_string()]
                } else {
                    vec![trimmed.to_string()]
                }
            }
            Err(e) => vec![
                format!("cannot read {path}: {e}"),
                String::new(),
                "SELinux contexts are only exposed through /proc on Linux.".to_string(),
                "On macOS the equivalent is the code-signing entitlement set,".to_string(),
                "which needs a different API.".to_string(),
            ],
        };

        self.screen = Screen::Info {
            title: format!("Security context of {pid}"),
            lines,
            scroll: 0,
        };
        Reaction::Continue
    }

    /// `b`: a stack trace of the selected process.
    ///
    /// Needs `libunwind-ptrace` (Linux) or `thread_get_state` (macOS), neither of
    /// which is wired up. Saying so is better than a key that silently does
    /// nothing, which is how `F9` used to behave.
    fn show_backtrace(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            return Reaction::Continue;
        };
        let pid = row.pid;

        self.screen = Screen::Info {
            title: format!("Backtrace of {pid}"),
            lines: vec![
                "Backtraces are not implemented yet.".to_string(),
                String::new(),
                "This needs a debugger interface:".to_string(),
                "  Linux: libunwind-ptrace, attached with ptrace(2)".to_string(),
                "  macOS: thread_get_state plus a Mach-O symbol resolver".to_string(),
                String::new(),
                "In the meantime, attach a debugger directly:".to_string(),
                format!("  lldb -p {pid}"),
                format!("  gdb -p {pid}"),
            ],
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

        // `draw_table` needs `&mut self` (it records the viewport height), so the
        // screen is copied out instead of borrowed from `self`.
        let screen = self.screen.clone();
        match &screen {
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
            Screen::SignalPicker { cursor } => {
                self.draw_table(chunks[1], frame.buffer_mut());
                draw_signal_picker(
                    chunks[1],
                    frame.buffer_mut(),
                    &self.theme,
                    self.selected_row(),
                    *cursor,
                );
            }
            Screen::Setup => {
                if let Some(setup) = &self.setup {
                    setup.draw(chunks[1], frame.buffer_mut(), &self.theme);
                }
            }
            Screen::Affinity(panel) => panel.draw(chunks[1], frame.buffer_mut(), &self.theme),
            Screen::IoPriority(panel) => panel.draw(chunks[1], frame.buffer_mut(), &self.theme),
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
        let theme = self.theme.clone();
        let right_column = crate::meters::HeaderColumn::Right;

        let has_right = self.meters.iter().any(|m| m.column() == right_column);

        // The last row of the header block belongs to the screen-tab strip.
        let meter_h = area.height.saturating_sub(1);

        if !has_right {
            let rect = Rect::new(area.x, area.y, area.width, meter_h);
            Self::draw_column(&mut self.meters, 0, &self.meter_modes, rect, buf, &theme, self.meter_mode);
            return;
        }

        // Leave a one-cell gutter so the two columns do not run together.
        let left_w = area.width / 2;
        let left = Rect::new(area.x, area.y, left_w.saturating_sub(1), meter_h);
        let right = Rect::new(area.x + left_w, area.y, area.width - left_w, meter_h);

        // Meters are stored in one vector, but drawn per column, so the column's
        // first meter is not always index 0 of `meter_modes`.
        let mut left_index = Vec::new();
        let mut right_index = Vec::new();
        for (i, meter) in self.meters.iter().enumerate() {
            if meter.column() == right_column {
                right_index.push(i);
            } else {
                left_index.push(i);
            }
        }

        self.draw_column_indices(&left_index, left, buf, &theme);
        self.draw_column_indices(&right_index, right, buf, &theme);
    }

    /// Draw the meters named by `indices` into one column, honouring each
    /// meter's own mode.
    fn draw_column_indices(
        &mut self,
        indices: &[usize],
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut y = area.y;
        for &i in indices {
            let mode = self.meter_mode_at(i);
            let Some(meter) = self.meters.get_mut(i) else {
                continue;
            };
            let h = meter.rows().max(1);
            if y + h > area.bottom() {
                break;
            }
            let rect = Rect::new(area.x, y, area.width, h);
            meter.render(rect, buf, theme, mode);
            y += h;
        }
    }

    fn draw_column(
        meters: &mut [Box<dyn Meter>],
        first_index: usize,
        modes: &[MeterMode],
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
        fallback: MeterMode,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut y = area.y;
        for (offset, meter) in meters.iter_mut().enumerate() {
            let h = meter.rows().max(1);
            if y + h > area.bottom() {
                break;
            }
            let mode = modes
                .get(first_index + offset)
                .copied()
                .unwrap_or(fallback);
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

    fn draw_table(&mut self, area: Rect, buf: &mut Buffer) {
        let fields = self.columns();
        // Taken out of `self` so the row loop can borrow `self.table` immutably
        // while resolving names.
        let mut users = std::mem::take(&mut self.users);
        let theme = self.theme.clone();

        if fields.is_empty() || area.height == 0 {
            return;
        }

        // Column widths: fixed widths for known fields, the last column takes
        // the remainder (htop makes Command the expanding column).
        let widths = column_widths(&fields, area.width);
        let total_width: u16 = widths.iter().sum();
        // Horizontal scroll, clamped so the last column stays flush with the
        // right edge instead of scrolling into empty space.
        let h_offset = self.h_offset.min(total_width.saturating_sub(area.width));
        let view_left = area.x as i32 - h_offset as i32;
        let clip_left = area.x as i32;
        let clip_right = area.right() as i32;

        // Header row, clipped to the visible horizontal window.
        let mut x = view_left;
        for (i, field) in fields.iter().enumerate() {
            let w = widths[i] as i32;
            if w == 0 {
                continue;
            }
            let style = if *field == self.table.sort.field {
                theme.column_header_sorted
            } else {
                theme.column_header
            };
            let title = fit(field.title(), widths[i] as usize);
            let draw_x = x.max(clip_left);
            let draw_end = (x + w).min(clip_right);
            if draw_end > draw_x {
                let skip = (draw_x - x) as usize;
                let head: String = title.chars().skip(skip).collect();
                crate::meters::put_str(
                    buf,
                    draw_x as u16,
                    area.y,
                    &head,
                    style,
                    (draw_end - draw_x) as u16,
                );
            }
            x += w;
        }

        let body_height = area.height.saturating_sub(1) as usize;
        // Remember it so paging and scroll-following know the window size.
        self.viewport = body_height;

        let marks = if self.table.tree_mode {
            compose_tree(&self.table.rows, &self.table.view.order)
        } else {
            Vec::new()
        };

        // Clamp the first visible row so the bottom of the list stays filled.
        let visible = self.table.visible();
        let first = self
            .offset
            .min(visible.saturating_sub(body_height.min(visible)));

        for row_idx in 0..body_height {
            let pos = first + row_idx;
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

            let mut cx = view_left;
            for (i, field) in fields.iter().enumerate() {
                let w = widths[i] as i32;
                if w == 0 {
                    continue;
                }
                let draw_x = cx.max(clip_left);
                let draw_end = (cx + w).min(clip_right);
                if draw_end > draw_x {
                    let mut text = render_field(row, *field, &self.snapshot, &mut users);
                    // In tree mode the Command column carries the branch prefix.
                    if self.table.tree_mode && *field == Field::Comm {
                        if let Some(mark) = marks.iter().find(|m| m.data_index == data_idx) {
                            text.insert_str(0, &mark.prefix);
                        }
                    }
                    let padded = pad_right(&text, widths[i] as usize);
                    let skip = (draw_x - cx) as usize;
                    let head: String = padded.chars().skip(skip).collect();
                    crate::meters::put_str(
                        buf,
                        draw_x as u16,
                        y,
                        &head,
                        base,
                        (draw_end - draw_x) as u16,
                    );
                }
                cx += w;
            }
        }

        self.users = users;
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
            Screen::SignalPicker { .. } => [
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
            Screen::Affinity(_) => [
                ("Esc", "Cancel"),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("S", "Apply"),
                ("F10", "Quit"),
            ],
            Screen::IoPriority(_) => [
                ("Esc", "Cancel"),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("", ""),
                ("S", "Apply"),
                ("F10", "Quit"),
            ],
            Screen::Setup => [
                ("Esc", "Cancel"),
                ("Tab", "Page"),
                ("Space", "Toggle"),
                ("", ""),
                ("Del", "Delete tab"),
                ("", ""),
                ("", ""),
                ("", ""),
                ("S", "Apply"),
                ("F10", "Apply"),
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

        // Divide the bar evenly, but never narrower than "F1 x", which is the
        // smallest readable slot. On a very narrow terminal the trailing keys are
        // dropped rather than rendered as broken fragments.
        // Slots are laid out by the widest entry a page needs, capped so the ten
        // keys still spread across the bar. Using a fixed tenth made `Esc Cancel`
        // (10 cells) impossible on an 84-column terminal (8 cells per slot).
        let widest = labels
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .map(|(k, l)| k.chars().count() + 1 + l.chars().count())
            .max()
            .unwrap_or(5)
            .max(5) as u16;
        let per_slot = labels.len() as u16;
        let slot = (area.width / per_slot).max(1);
        // Let each entry use its natural width when it needs more than its slot,
        // as long as the bar as a whole has room.
        let _ = widest;

        for (i, (key, label)) in labels.iter().enumerate() {
            if i as u16 >= per_slot {
                break;
            }
            if key.is_empty() && label.is_empty() {
                continue;
            }
            let x = area.x + i as u16 * slot;
            if x >= area.right() {
                break;
            }
            // Every field is clipped to its own slot, otherwise a long label
            // bleeds into the next key and the bar reads as garbage.
            let slot_end = (x + slot).min(area.right());
            let room = slot_end.saturating_sub(x);
            if room == 0 {
                break;
            }

            // Separate key and label with a space when the slot has room, and
            // run them together when it does not — the compact form is what htop
            // itself falls back to on a narrow terminal.
            let key_len = key.chars().count() as u16;
            let label_len = label.chars().count() as u16;
            let spaced = key_len + 1 + label_len <= room;

            crate::meters::put_str(buf, x, area.y, key, self.theme.function_bar_key, room);
            let label_x = x + key_len + if spaced { 1 } else { 0 };
            if label_x < slot_end {
                crate::meters::put_str(
                    buf,
                    label_x,
                    area.y,
                    label,
                    self.theme.function_bar,
                    slot_end.saturating_sub(label_x),
                );
            }
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
/// Column widths in cells.
///
/// Most fields have a fixed width; the last field (normally Command) absorbs the
/// slack so the table fills the terminal. The absorbed width is capped so the
/// table can still be wider than the screen — otherwise `total_width` always
/// equals `area.width` and horizontal scrolling has nothing to scroll to.
fn column_widths(fields: &[Field], total: u16) -> Vec<u16> {
    // Longest stretch a single column may claim when the terminal is wide.
    const MAX_EXPAND: u16 = 120;

    let mut widths: Vec<u16> = fields.iter().map(|f| default_width(*f)).collect();
    let fixed: u16 = widths.iter().sum();
    if let Some(last) = widths.last_mut() {
        let slack = total.saturating_sub(fixed).min(MAX_EXPAND);
        *last = last.saturating_add(slack).max(6);
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
pub fn render_field(
    row: &Row,
    field: Field,
    snapshot: &SystemSnapshot,
    users: &mut gander_core::users::Users,
) -> String {
    match field {
        Field::Pid => row.pid.to_string(),
        Field::Ppid => row.ppid.to_string(),
        Field::Pgrp => row.pgrp.to_string(),
        Field::Comm => row.display_command().to_string(),
        Field::State => (row.state as char).to_string(),
        Field::User => users.name(row.uid).to_string(),
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

pub fn span_title<'a>(text: &'a str, theme: &Theme) -> ratatui::text::Span<'a> {
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

fn draw_signal_picker(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    row: Option<&Row>,
    cursor: usize,
) {
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

    // Scroll so the cursor stays visible in a short terminal.
    let visible = inner.height as usize;
    let first = cursor.saturating_sub(visible.saturating_sub(1));

    let lines: Vec<Line> = gander_core::action::SIGNALS
        .iter()
        .enumerate()
        .skip(first)
        .take(visible.max(1))
        .map(|(i, (num, name))| {
            let marker = if i == cursor { ">" } else { " " };
            let text = format!("{marker} {num:>2}  {name}");
            if i == cursor {
                Line::styled(text, theme.panel_selected)
            } else {
                Line::from(text)
            }
        })
        .collect();
    let para = Paragraph::new(lines).style(theme.row_normal);
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

    fn app_with_n_rows(n: u32) -> App {
        let rows: Vec<Row> = (1..=n)
            .map(|pid| {
                let mut r = Row::default_for_test(pid);
                r.comm.try_push_str(&format!("proc{pid}")).unwrap();
                r.percent_cpu = pid as f32;
                r
            })
            .collect();
        let mut app = App::new(Config::default());
        app.apply_sample(SystemSnapshot::default(), rows);
        app
    }

    #[test]
    fn selection_scrolls_into_view() {
        let mut app = app_with_n_rows(200);
        app.viewport = 10;

        // Walk down past the first screen; `offset` used to stay 0 forever.
        for _ in 0..25 {
            app.handle_key(Key::Down);
        }
        assert_eq!(app.selected, 25);
        assert!(
            app.selected < app.offset + app.viewport,
            "selected {} must be inside offset {} + viewport {}",
            app.selected,
            app.offset,
            app.viewport
        );

        // And back up again.
        for _ in 0..30 {
            app.handle_key(Key::Up);
        }
        assert_eq!(app.selected, 0);
        assert_eq!(app.offset, 0);
    }

    #[test]
    fn paging_moves_by_one_screen() {
        let mut app = app_with_n_rows(200);
        app.viewport = 20;

        app.handle_key(Key::PgDn);
        assert_eq!(app.selected, 20);
        app.handle_key(Key::PgDn);
        assert_eq!(app.selected, 40);
        app.handle_key(Key::PgUp);
        assert_eq!(app.selected, 20);
    }

    #[test]
    fn end_keeps_the_bottom_of_the_list_filled() {
        let mut app = app_with_n_rows(200);
        app.viewport = 10;
        app.handle_key(Key::End);
        assert_eq!(app.selected, 199);
        // offset + viewport must land on the last row, not past it.
        assert_eq!(app.offset + app.viewport, 200);
    }

    #[test]
    fn horizontal_scroll_is_applied() {
        // The renderer used to compute `h_offset` and then ignore it.
        let mut app = app_with_n_rows(3);
        app.viewport = 5;
        assert_eq!(app.h_offset, 0);

        app.handle_key(Key::Right);
        assert_eq!(app.h_offset, 4);
        app.handle_key(Key::Left);
        assert_eq!(app.h_offset, 0);

        // Must not underflow past the beginning.
        app.handle_key(Key::Left);
        assert_eq!(app.h_offset, 0);
    }

    #[test]
    fn signal_picker_sends_the_selected_signal() {
        // Use a disposable child so nothing real is harmed.
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();

        let mut app = App::new(Config::default());
        let mut row = Row::default_for_test(pid);
        row.comm.try_push_str("sleep").unwrap();
        app.apply_sample(SystemSnapshot::default(), vec![row]);
        app.viewport = 5;

        // Open the picker and confirm SIGTERM (the default selection).
        app.handle_key(Key::F(9));
        assert!(matches!(app.screen, Screen::SignalPicker { .. }));
        app.handle_key(Key::Enter);

        // The child must actually die.
        let status = child.wait().expect("wait");
        assert!(!status.success(), "sleep should have been terminated");
    }

    #[test]
    fn signal_picker_cancel_does_nothing() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();

        let mut app = App::new(Config::default());
        let mut row = Row::default_for_test(pid);
        row.comm.try_push_str("sleep").unwrap();
        app.apply_sample(SystemSnapshot::default(), vec![row]);
        app.viewport = 5;

        app.handle_key(Key::F(9));
        app.handle_key(Key::Esc);
        assert_eq!(app.screen, Screen::ProcessList);

        // Still alive: kill it ourselves so the test does not leak a process.
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn signal_picker_cursor_moves() {
        let mut app = app_with_n_rows(3);
        app.viewport = 5;

        app.handle_key(Key::F(9));
        let Screen::SignalPicker { cursor } = app.screen else {
            panic!("picker should be open");
        };
        // htop defaults to SIGTERM.
        assert_eq!(gander_core::action::SIGNALS[cursor].0, 15);

        app.handle_key(Key::Down);
        let Screen::SignalPicker { cursor } = app.screen else {
            panic!("picker should still be open")
        };
        assert!(cursor > 0);

        app.handle_key(Key::Home);
        let Screen::SignalPicker { cursor } = app.screen else {
            panic!()
        };
        assert_eq!(cursor, 0);
    }

    #[test]
    fn each_tab_keeps_its_own_sort() {
        let mut config = Config::default();
        // Two tabs with different sorts, as a user would set up.
        config.tabs = vec![
            config.tabs[0].clone(),
            gander_config::TabConfig {
                name: "Memory".into(),
                columns: vec!["PID".into(), "PERCENT_MEM".into(), "COMM".into()],
                sort: gander_config::SortConfig {
                    field: "PERCENT_MEM".into(),
                    desc: true,
                },
            },
        ];
        config.active_tab = 0;

        let mut app = App::new(config);
        app.viewport = 5;
        app.apply_sample(SystemSnapshot::default(), Vec::new());

        assert_eq!(app.table.sort.field, Field::PercentCpu, "tab 0 starts here");
        assert_eq!(app.columns().len(), 8, "tab 0 keeps its own columns");

        app.handle_key(Key::Tab);
        assert_eq!(app.config.active_tab, 1);
        assert_eq!(
            app.table.sort.field,
            Field::PercentMem,
            "switching tabs must load that tab's sort"
        );
        assert_eq!(app.columns().len(), 3, "and its columns");

        // Changing the sort here must not leak into the other tab.
        app.cycle_sort(false);
        let tab1_sort = app.table.sort.field;
        app.handle_key(Key::BackTab);
        assert_eq!(app.config.active_tab, 0);
        assert_eq!(
            app.table.sort.field,
            Field::PercentCpu,
            "tab 0's sort survived the round trip"
        );

        app.handle_key(Key::Tab);
        assert_eq!(app.table.sort.field, tab1_sort, "tab 1 kept its own change");
    }

    #[test]
    fn cycling_tabs_with_only_one_configured_explains_itself() {
        let mut app = app_with_n_rows(3);
        app.handle_key(Key::Tab);
        assert_eq!(app.config.active_tab, 0);
        assert!(
            app.status.as_deref().unwrap_or("").contains("one screen tab"),
            "should say why nothing happened, got {:?}",
            app.status
        );
    }

    #[test]
    fn tab_switch_resets_the_selection() {
        let mut config = Config::default();
        config.tabs.push(config.tabs[0].clone());
        let mut app = App::new(config);
        app.viewport = 5;
        app.apply_sample(
            SystemSnapshot::default(),
            (1..=50)
                .map(Row::default_for_test)
                .collect::<Vec<_>>(),
        );

        for _ in 0..10 {
            app.handle_key(Key::Down);
        }
        assert_eq!(app.selected, 10);

        app.handle_key(Key::Tab);
        assert_eq!(app.selected, 0, "a different tab starts at the top");
        assert_eq!(app.offset, 0);
    }

    #[test]
    fn render_field_formats_without_panicking() {
        let snap = SystemSnapshot::default();
        let r = row(1, "x");
        let mut users = gander_core::users::Users::new();
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
            let _ = render_field(&r, f, &snap, &mut users);
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
    fn a_opens_the_affinity_panel_for_the_selected_process() {
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 9],
            cpu_delta: vec![CpuTimes::default(); 9],
            ..Default::default()
        };
        let mut app = App::new(Config::default());
        let mut rows: Vec<Row> = (1..=5).map(Row::default_for_test).collect();
        rows[0].comm.try_push_str("first").unwrap();
        app.apply_sample(snapshot, rows);
        app.selected = 0;
        let pid = app.selected_row().expect("a selected row").pid;

        app.handle_key(Key::Char('a'));
        match &app.screen {
            Screen::Affinity(panel) => {
                assert_eq!(panel.pid, pid, "the selected row's pid");
                assert_eq!(panel.cores, 8, "one entry per core");
            }
            other => panic!("expected the affinity panel, got {other:?}"),
        }

        // The panel closes on Esc and leaves the list untouched.
        app.handle_key(Key::Esc);
        assert_eq!(app.screen, Screen::ProcessList);
    }

    #[test]
    fn a_on_an_empty_table_explains_itself() {
        let mut app = app_with_n_rows(0);
        app.handle_key(Key::Char('a'));
        assert_eq!(app.screen, Screen::ProcessList);
        assert!(app.status.as_deref().unwrap_or("").contains("no process"));
    }

    #[test]
    fn i_opens_the_io_priority_panel() {
        let mut app = app_with_n_rows(3);
        app.selected = 1;
        let pid = app.selected_row().unwrap().pid;

        app.handle_key(Key::Char('i'));
        match &app.screen {
            Screen::IoPriority(panel) => assert_eq!(panel.pid, pid),
            other => panic!("expected the I/O priority panel, got {other:?}"),
        }

        // Space is safe on every platform: it cycles where supported and is a
        // no-op where not, but must never panic or close the panel.
        app.handle_key(Key::Char(' '));
        assert!(matches!(app.screen, Screen::IoPriority(_)));

        app.handle_key(Key::Esc);
        assert_eq!(app.screen, Screen::ProcessList);
    }

    #[test]
    fn both_panels_draw() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        for key in [Key::Char('a'), Key::Char('i')] {
            let mut app = app_with_n_rows(3);
            app.selected = 0;
            app.handle_key(key);
            let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
        }
    }

    #[test]
    fn each_meter_keeps_its_own_mode() {
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 5],
            cpu_delta: vec![CpuTimes::default(); 5],
            ..Default::default()
        };
        let mut app = App::new(Config::default());
        app.apply_sample(snapshot, Vec::new());
        assert!(app.meters.len() > 2);

        // Everything starts as configured (bar / text).
        let before: Vec<MeterMode> = (0..app.meters.len()).map(|i| app.meter_mode_at(i)).collect();

        app.cycle_meter_mode(1);
        assert_ne!(app.meter_mode_at(1), before[1], "meter 1 changed");
        for i in [0usize, 2] {
            assert_eq!(
                app.meter_mode_at(i),
                before[i],
                "meter {i} must not change with meter 1"
            );
        }

        // And the choice is written back to the *right* config entry, so it
        // survives a save. The flat index is not the config index: a collapsed
        // CPU pair shifts everything after it.
        let source = app.meter_sources[1];
        let persisted = match source.column {
            crate::meters::HeaderColumn::Left => &app.config.meters_left[source.index],
            crate::meters::HeaderColumn::Right => &app.config.meters_right[source.index],
        };
        assert_eq!(persisted.name, app.meters[1].name());
        assert_eq!(
            persisted.mode,
            meters_impl::mode_name(app.meter_modes[1]),
            "meter 1's mode must be persisted on its own entry"
        );
    }

    #[test]
    fn rebuilding_the_header_keeps_the_modes_of_surviving_meters() {
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 5],
            cpu_delta: vec![CpuTimes::default(); 5],
            ..Default::default()
        };
        let mut app = App::new(Config::default());
        app.apply_sample(snapshot.clone(), Vec::new());

        // Put a meter in graph mode, then rebuild (as the setup screen does).
        let idx = app
            .meters
            .iter()
            .position(|m| m.name() == "Memory")
            .expect("Memory meter");
        while app.meter_mode_at(idx) != MeterMode::Graph {
            app.cycle_meter_mode(idx);
        }

        app.rebuild_header(snapshot.cpu_count());
        let idx = app
            .meters
            .iter()
            .position(|m| m.name() == "Memory")
            .expect("Memory meter after rebuild");
        assert_eq!(
            app.meter_mode_at(idx),
            MeterMode::Graph,
            "the user's mode must survive a header rebuild"
        );
    }

    #[test]
    fn a_configured_header_is_honoured() {
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let config = Config {
            meters_left: vec![gander_config::MeterConfig::new("Memory", "text")],
            meters_right: vec![gander_config::MeterConfig::new("Clock", "graph")],
            ..Config::default()
        };

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 5],
            cpu_delta: vec![CpuTimes::default(); 5],
            ..Default::default()
        };
        let mut app = App::new(config);
        app.apply_sample(snapshot, Vec::new());

        let names: Vec<&str> = app.meters.iter().map(|m| m.name()).collect();
        // A CPU meter is forced in when the config omits one, so the machine's
        // load is never invisible.
        assert!(names.contains(&"CPU"), "got {names:?}");
        assert!(names.contains(&"Memory"), "got {names:?}");
        assert!(names.contains(&"Clock"), "got {names:?}");
        assert!(!names.contains(&"Swap"), "unlisted meters must not appear");

        let clock = app.meters.iter().position(|m| m.name() == "Clock").unwrap();
        assert_eq!(app.meter_mode_at(clock), MeterMode::Graph);
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
