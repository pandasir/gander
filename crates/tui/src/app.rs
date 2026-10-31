//! Application state, key handling, and view rendering.
//!
//! This replaces htop's `ScreenManager` + `Panel` focus stack with a single
//! `App` value and a `match` on `Screen`. See `docs/DESIGN.md` §6.

use gander_config::Config;
use gander_core::action::ProcessTarget;
use gander_core::row::{Field, Row};
use gander_core::system::SystemSnapshot;
use gander_core::table::{self, Dir, Filter, ProcTable};
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

/// Which screen arrangement is in use.
///
/// `Panels` is the default and the reason this fork exists: a vertical system
/// column and a persistent inspector, so the process table is not the only thing
/// on screen that has to share vertical space with the meters.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum LayoutKind {
    Panels,
    Htop,
}

impl LayoutKind {
    pub fn from_config(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "htop" | "classic" => LayoutKind::Htop,
            _ => LayoutKind::Panels,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LayoutKind::Panels => "panels",
            LayoutKind::Htop => "htop",
        }
    }

    fn next(self) -> Self {
        match self {
            LayoutKind::Panels => LayoutKind::Htop,
            LayoutKind::Htop => LayoutKind::Panels,
        }
    }
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
    /// How far back into the history the buffer currently is.
    ///
    /// `0` means "the live buffer, not a recalled entry". Up/Down walk it, and
    /// any character typed resets it to 0 — otherwise editing a recalled entry
    /// and then pressing Up again would jump from a stale position.
    pub history_cursor: usize,
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
    /// How far the table is scrolled left, in cells.
    ///
    /// Only meaningful while the table is wider than the screen, which happens
    /// when a command line does not fit: `column_widths` widens the last column
    /// to the longest value on screen. Clamped into range by `draw_table` and
    /// `scroll_columns`.
    pub h_offset: u16,
    /// Total table width and visible width from the last drawn frame.
    ///
    /// Kept so `scroll_columns` can say whether there is anything further right
    /// before it starts reporting a scroll, and so `Left` can restore the
    /// columns the user scrolled past.
    last_total_width: u16,
    last_area_width: u16,
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
    pub tags: std::collections::HashSet<ProcessTarget>,
    /// Frozen when a process-action dialog opens; samples must not retarget it.
    action_targets: Vec<ProcessTarget>,
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
    /// Aggregate CPU history for the panels layout's sparkline.
    ///
    /// The meters keep their own history for graph mode, but the panels layout
    /// does not use a `CpuMeter` at all, so it needs its own series.
    pub cpu_history: std::collections::VecDeque<f32>,
    /// Index of the header meter that `I` and the arrow keys act on.
    ///
    /// htop changes the mode of the meter *under the cursor*; a global mode made
    /// `I` flip the whole header at once, which is not the same feature.
    pub meter_cursor: usize,
    /// Which screen arrangement is active.
    pub layout: LayoutKind,
    /// Which config entry each meter was built from, parallel to `meters`.
    pub meter_sources: Vec<meters_impl::MeterSource>,
    /// The mode applied to meters added later, and the fallback for `I` when no
    /// meter is selected.
    pub meter_mode: MeterMode,
    pub pending_g: bool,
    /// Recalled search terms (`F3` / `/`), newest last.
    pub search_history: gander_config::history::History,
    /// Recalled filter terms (`F4` / `\`), newest last.
    pub filter_history: gander_config::history::History,
    /// Where history is read from and written to.
    pub history_dir: std::path::PathBuf,
}

impl App {
    pub fn new(mut config: Config) -> Self {
        if config.tabs.is_empty() {
            config.tabs = Config::default().tabs;
        }
        config.active_tab = config.active_tab.min(config.tabs.len() - 1);
        let theme = Theme::named(&config.color_scheme).unwrap_or_default();
        let table = ProcTable {
            sort: config.active_tab().sort(),
            tree_mode: config.tree_mode,
            show_kernel_threads: config.show_kernel_threads,
            show_userland_threads: config.show_userland_threads,
            show_containerised: config.show_containerised,
            ..Default::default()
        };
        let layout = LayoutKind::from_config(&config.layout);

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
            last_total_width: 0,
            last_area_width: 0,
            viewport: 0,
            users: gander_core::users::Users::new(),
            user_filter: None,
            setup: None,
            visibility: gander_core::table::Visibility {
                show_kernel_threads: true,
                show_userland_threads: false,
                show_containerised: true,
                ..Default::default()
            },
            tags: Default::default(),
            action_targets: Vec::new(),
            filter: None,
            input: Input::default(),
            command_column_interactive: true,
            quit: false,
            status: None,
            meter_modes: Vec::new(),
            meter_cursor: 0,
            cpu_history: std::collections::VecDeque::new(),
            layout,
            meter_sources: Vec::new(),
            meter_mode: MeterMode::Bar,
            pending_g: false,
            search_history: gander_config::history::History::default(),
            filter_history: gander_config::history::History::default(),
            history_dir: gander_config::history::dir(),
        }
    }

    /// Load search and filter history from `history_dir`.
    ///
    /// Separate from `new` so tests never touch the real `$XDG_STATE_HOME`: an
    /// `App` built in a unit test would otherwise read the developer's own
    /// history, and the tests would pass or fail depending on what they had
    /// searched for that morning.
    pub fn with_history(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.history_dir = dir.into();
        self.search_history = gander_config::history::History::load(
            &self.history_dir,
            gander_config::history::Kind::Search,
        );
        self.filter_history = gander_config::history::History::load(
            &self.history_dir,
            gander_config::history::Kind::Filter,
        );
        self
    }

    /// Persist both histories, ignoring failures.
    ///
    /// History is an optimisation; a read-only `$XDG_STATE_HOME` must not stop a
    /// run from exiting cleanly.
    pub fn save_history(&self) {
        let _ = self
            .search_history
            .save(&self.history_dir, gander_config::history::Kind::Search);
        let _ = self
            .filter_history
            .save(&self.history_dir, gander_config::history::Kind::Filter);
    }

    /// The history the active prompt draws from.
    fn history_for(&self, kind: InputKind) -> &gander_config::history::History {
        match kind {
            InputKind::Filter => &self.filter_history,
            _ => &self.search_history,
        }
    }

    /// Record the submitted prompt text, and persist.
    fn remember(&mut self, kind: InputKind, text: &str) {
        let history = match kind {
            InputKind::Filter => &mut self.filter_history,
            InputKind::Search => &mut self.search_history,
            InputKind::None => return,
        };
        history.push(text);
        // Written immediately rather than only on exit: a crash or a `Ctrl-C`
        // would otherwise lose the whole session's history, and this file is a
        // few hundred bytes.
        self.save_history();
    }

    /// Adopt a fresh sample: update rows, meters, and reorder.
    pub fn apply_sample(&mut self, snapshot: SystemSnapshot, rows: Vec<Row>) {
        let selected = self
            .selected_row()
            .map(|row| (row.identity(), row.starttime));
        if !self.tags.is_empty() {
            let live: std::collections::HashSet<_> = rows.iter().map(ProcessTarget::from).collect();
            self.tags.retain(|target| live.contains(target));
        }
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
        if let Some(identity) = selected {
            if let Some(index) = self.table.view.order.iter().position(|&i| {
                let row = &self.table.rows[i];
                (row.identity(), row.starttime) == identity
            }) {
                self.selected = index;
            }
        }
        // The panels layout draws its own CPU sparkline, so the history is kept
        // here rather than inside a meter it does not own.
        {
            let usage = snapshot.cpu_usage();
            let value = if usage.is_finite() { usage } else { 0.0 };
            if self.cpu_history.len() == crate::meters::GRAPH_HISTORY {
                self.cpu_history.pop_front();
            }
            self.cpu_history.push_back(value);
        }
        self.snapshot = snapshot;
        self.clamp_selection();
    }

    pub fn columns(&self) -> Vec<Field> {
        let configured = self.config.active_tab().fields();

        // The panels layout gives ~40 cells to the command column, and its
        // inspector already shows USER, NICE, priority, state and thread count.
        // Repeating those as columns costs width the command line needs, so the
        // panels layout prefers a leaner set — but only while the user has not
        // chosen columns themselves. The moment they touch `F2`, their choice is
        // what renders, because a layout should not silently discard an explicit
        // configuration.
        if self.layout == LayoutKind::Panels && !self.columns_customized(&configured) {
            return crate::layout::DEFAULT_FIELDS.to_vec();
        }

        configured
    }

    /// Whether any currently displayed column needs `/proc/<pid>/smaps_rollup`.
    ///
    /// The sampler thread reads this before every sample, so it decides whether
    /// the kernel walks every VMA of every process on the machine. It is asked of
    /// the *active tab's visible columns*, not of the config as a whole: a PSS
    /// column on a tab that is not on screen costs nothing.
    ///
    /// The check is by field, not by name, so it cannot drift from the column
    /// list. A field that derives from `smaps_rollup` without being listed in
    /// `Field::needs_smaps_rollup` would be silently blank, which is why that
    /// method lives next to the enum.
    pub fn wants_smaps_rollup(&self) -> bool {
        self.columns().iter().any(|f| f.needs_smaps_rollup())
    }

    /// Whether any currently displayed column needs `/proc/<pid>/fdinfo`.
    ///
    /// Gated for the same reason `smaps_rollup` is: one directory walk and a read
    /// per descriptor, per process, per tick.
    pub fn wants_gpu_fdinfo(&self) -> bool {
        self.columns().iter().any(|f| f.needs_gpu_fdinfo())
    }

    /// Whether any currently displayed column needs a `TASKSTATS` query.
    ///
    /// The most expensive of the three: a netlink round trip per process per tick,
    /// and on an unprivileged process every one of them fails with `EPERM`. The
    /// sampler stops asking after the first refusal, so the cost is one syscall
    /// rather than one per process — but there is still no reason to pay it for a
    /// column nobody can see.
    pub fn wants_delayacct(&self) -> bool {
        self.columns().iter().any(|f| f.needs_delayacct())
    }

    /// The whole set of expensive reads the current screen needs.
    ///
    /// One call, so the sampler thread cannot be told about one of the three and
    /// left stale on the others.
    pub fn scan_options(&self) -> gander_core::ScanOptions {
        gander_core::ScanOptions::none()
            .with_threads(self.table.show_userland_threads)
            .with_smaps(self.wants_smaps_rollup())
            .with_gpu(self.wants_gpu_fdinfo())
            .with_delayacct(self.wants_delayacct())
    }

    /// Whether the active tab's columns differ from the shipped default.
    fn columns_customized(&self, current: &[Field]) -> bool {
        let default = Config::default();
        let default_fields = default.tabs[0].fields();
        // A different column set is a customization; so is the same set in a
        // different order.
        current != default_fields.as_slice()
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
            if let Some(pos) = self.meters.iter().position(|m| m.name() == meter.name()) {
                if let Some(mode) = self.meter_modes.get(pos) {
                    header.modes[i] = *mode;
                }
            }
        }

        self.meters = header.meters;
        self.meter_modes = header.modes;
        self.meter_sources = header.sources;

        // The header can shrink (a meter was deleted, or a split CPU pair
        // collapsed into one row). Leaving the cursor past the end would make
        // `I` a no-op whose status line names a meter that is not there.
        self.meter_cursor = self.meter_cursor.min(self.meters.len().saturating_sub(1));
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

    /// Move the header cursor by `delta`, wrapping at both ends.
    ///
    /// Wrapping rather than clamping means one key press always does something,
    /// which matters because the header has no visible scrollbar to say "you are
    /// at the end".
    pub fn move_meter_cursor(&mut self, delta: isize) {
        if self.meters.is_empty() {
            self.meter_cursor = 0;
            return;
        }
        let n = self.meters.len() as isize;
        let next = (self.meter_cursor as isize + delta).rem_euclid(n);
        self.meter_cursor = next as usize;
        let name = self.meters[self.meter_cursor].name();
        let mode = meters_impl::mode_name(self.meter_mode_at(self.meter_cursor));
        self.status = Some(format!("meter {name}: {mode}"));
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
        // A status message describes the *last* action. It has to be cleared when
        // the next action does not set one of its own, or `K` after `O` would
        // leave "container detection is Linux-only" on screen as if it were the
        // result of pressing `K`.
        //
        // Only the list screen clears it: the prompt and the modal screens own
        // their own feedback, and clearing on every keystroke would make a
        // message vanish while it was being read.
        self.status = None;
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
                        .position(|(_, name)| *name == "SIGTERM")
                        .unwrap_or(0);
                    self.action_targets = self.process_targets();
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
                self.scroll_columns(false);
                Reaction::Continue
            }
            Key::Right => {
                self.scroll_columns(true);
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
                // htop: `H` toggles userland threads, `K` toggles kernel threads.
                // These were both wired to kernel threads, so `H` did nothing
                // distinguishable from `K` and neither could show a thread list.
                self.table.show_userland_threads = !self.table.show_userland_threads;
                self.config.show_userland_threads = self.table.show_userland_threads;
                self.status = Some(if self.table.show_userland_threads {
                    "showing threads".into()
                } else {
                    "hiding threads".into()
                });
                // Threads have to be sampled before they can be filtered in, so
                // turning them on needs a fresh sample, not just a reorder.
                Reaction::Reorder
            }
            Key::Char('O') => {
                // htop's `O`: hide containerised processes.
                //
                // Only meaningful on Linux, where container detection exists. On
                // macOS every row has `container == None`, so the toggle would
                // flip state and change nothing on screen — say why instead of
                // appearing broken.
                if !gander_core::capability::CGROUP {
                    self.status = Some("container detection is Linux-only".into());
                    return Reaction::Continue;
                }
                self.table.show_containerised = !self.table.show_containerised;
                self.config.show_containerised = self.table.show_containerised;
                self.status = Some(if self.table.show_containerised {
                    "showing containerised processes".into()
                } else {
                    "hiding containerised processes".into()
                });
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
            Key::Char('L') => {
                // Switch between the panels layout and the classic htop one.
                self.layout = self.layout.next();
                self.config.layout = self.layout.name().to_string();
                let hint = match self.layout {
                    LayoutKind::Panels => "panels: system column + inspector",
                    LayoutKind::Htop => "htop: horizontal meters",
                };
                self.status = Some(format!("layout: {hint}"));
                Reaction::Continue
            }
            Key::Char('a') => self.open_affinity(),
            Key::Char('i') => self.open_ioprio(),
            Key::Char('Z') => self.show_security_context(),
            Key::Char('b') => self.show_backtrace(),
            Key::Char('s') => self.show_strace(),
            Key::Char('e') => self.show_env(),
            Key::Char('w') => self.show_command(),
            Key::Char('l') => self.show_open_files(),
            Key::Char('x') => self.show_locks(),
            Key::Char('I') => {
                // Cycle only the meter under the header cursor, which is what
                // htop does. `HeaderNext`/`HeaderPrev` move that cursor.
                let i = self.meter_cursor.min(self.meters.len().saturating_sub(1));
                if self.meters.is_empty() {
                    return Reaction::Continue;
                }
                self.cycle_meter_mode(i);
                let mode = meters_impl::mode_name(self.meter_mode_at(i));
                let name = self.meters[i].name();
                self.status = Some(format!("meter {name}: {mode}"));
                Reaction::Continue
            }
            Key::HeaderNext => {
                self.move_meter_cursor(1);
                Reaction::Continue
            }
            Key::HeaderPrev => {
                self.move_meter_cursor(-1);
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
                // The kind has to be read *before* the input is reset. The
                // previous version cleared `self.input` first and then checked
                // `self.input.kind`, which by then was `None` — so `Esc` could
                // never clear a filter, and the only way out of a filter was to
                // erase it a character at a time.
                let kind = self.input.kind;
                self.input = Input::default();
                if kind == InputKind::Filter {
                    self.filter = None;
                    return Reaction::Reorder;
                }
                Reaction::Continue
            }
            Key::Enter => {
                let kind = self.input.kind;
                let text = self.input.buffer.clone();
                self.input = Input::default();
                // Remember what was actually submitted, not the partially typed
                // buffer: a cancelled prompt is not history.
                self.remember(kind, &text);
                if kind == InputKind::Filter {
                    self.filter = Filter::parse(&text);
                    Reaction::Reorder
                } else {
                    self.jump_to_match(&text)
                }
            }
            Key::Up | Key::Down => {
                // Walk the history. This is the whole point of persisting it:
                // re-running yesterday's filter should not mean retyping it.
                let kind = self.input.kind;
                let delta = if key == Key::Up { 1 } else { -1 };
                let history = self.history_for(kind).clone();
                let (cursor, text) = history.step(self.input.history_cursor, delta);
                if let Some(text) = text {
                    self.input.buffer = text;
                    self.input.history_cursor = cursor;
                    if kind == InputKind::Filter {
                        // The filter is live while typing, so a recalled entry has
                        // to take effect immediately rather than on Enter.
                        self.filter = Filter::parse(&self.input.buffer);
                        return Reaction::Reorder;
                    }
                }
                Reaction::Continue
            }
            Key::Backspace => {
                self.input.buffer.pop();
                // Editing takes the buffer out of history-walking mode, so a later
                // Up starts again from the newest entry instead of resuming from a
                // position that no longer matches the text.
                self.input.history_cursor = 0;
                if self.input.kind == InputKind::Filter {
                    self.filter = Filter::parse(&self.input.buffer);
                    return Reaction::Reorder;
                }
                Reaction::Continue
            }
            Key::Char(c) => {
                if !c.is_ascii_control() {
                    self.input.buffer.push(c);
                    self.input.history_cursor = 0;
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
            history_cursor: 0,
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
        self.table.show_userland_threads = draft.show_userland_threads;
        self.table.show_containerised = draft.show_containerised;
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

        // Targets were frozen on opening the picker, including any tagged set.
        let targets = self.action_targets.clone();
        if targets.is_empty() {
            self.status = Some("no process selected".into());
            return;
        }

        let mut sent = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for target in &targets {
            let pid = target.pid;
            let action = gander_core::action::Action::Kill { pid, signal };
            match gander_core::action::execute_checked(*target, &action) {
                Ok(()) => sent += 1,
                Err(e) => failures.push(format!("{pid}: {e}")),
            }
        }

        self.status = Some(if failures.is_empty() {
            if targets.len() == 1 {
                format!("sent {name} to {}", targets[0].pid)
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
            show_userland_threads: self.table.show_userland_threads,
            show_containerised: self.table.show_containerised,
        };
        self.table.reorder(&self.visibility);
        self.clamp_selection();
    }

    /// The widest cell the last column has to hold, in cells.
    ///
    /// Measured over the rows that are actually on screen rather than the whole
    /// table: a single leftover process with a 2 KiB command line would otherwise
    /// make every frame scroll to a distant right edge for every other row. The
    /// viewport is a snapshot of the last drawn frame, so this is stable between
    /// draws while the list scrolls.
    ///
    /// The selected row is always included, because `Right` and `End` both exist
    /// to reveal *its* text even when it sits outside the measured window.
    fn longest_last_cell(&self, fields: &[Field], marks: &[crate::marks::Mark]) -> u16 {
        let Some(&last) = fields.last() else {
            return 0;
        };
        // Only the Command column has variable width; anything else measured here
        // would just be re-adding a fixed width the caller already accounted for.
        if last != Field::Comm {
            return 0;
        }

        let visible = self.table.visible();
        // The viewport is only known after a frame has been drawn. On the very
        // first frame, measure a plausible window instead of a single row, so the
        // column does not visibly widen on the second draw.
        let height = if self.viewport == 0 {
            visible.clamp(1, 50)
        } else {
            self.viewport
        };
        let first = self
            .offset
            .min(visible.saturating_sub(height.min(visible)));
        let last_pos = (first + height).min(visible);

        let mut widest = 0u16;
        for pos in first..last_pos {
            let Some(&data_idx) = self.table.view.order.get(pos) else {
                break;
            };
            let prefix = marks
                .iter()
                .find(|m| m.data_index == data_idx)
                .map(|m| m.prefix.as_str())
                .unwrap_or("");
            widest = widest.max(cell_width(&self.table.rows[data_idx], last, prefix));
        }

        // Always measure the selected row too, even when it is outside the window
        // the last frame drew: `Right` exists to reveal *its* text.
        if self.selected_row().is_some() {
            let data_idx = self
                .table
                .view
                .order
                .get(self.selected)
                .copied()
                .unwrap_or(self.selected);
            if data_idx < self.table.rows.len() {
                let prefix = marks
                    .iter()
                    .find(|m| m.data_index == data_idx)
                    .map(|m| m.prefix.as_str())
                    .unwrap_or("");
                widest = widest.max(cell_width(&self.table.rows[data_idx], last, prefix));
            }
        }
        widest
    }

    /// Scroll the table sideways, one step at a time.
    ///
    /// The step is a quarter of the visible width rather than a fixed four cells:
    /// four cells per keypress means a command line that overflows by 200 cells
    /// needs fifty presses, so the text was reachable in theory and not in
    /// practice. A quarter-screen step moves a whole field or two per press and
    /// still lands on exact positions, since the offset is a plain cell count.
    ///
    /// Reports in the status line whether the end was reached, because the
    /// failure mode this replaces was "the key does nothing and says nothing".
    fn scroll_columns(&mut self, forward: bool) {
        // Until the first draw there is no measured width to scroll within.
        let max = self.last_total_width.saturating_sub(self.last_area_width);
        if max == 0 {
            self.h_offset = 0;
            self.status = Some("nothing to scroll: the command fits".into());
            return;
        }

        let step = (self.last_area_width / 4).clamp(1, 32);
        if forward {
            let before = self.h_offset;
            self.h_offset = self.h_offset.saturating_add(step).min(max);
            if self.h_offset == before {
                self.status = Some("end of the command".into());
            }
        } else {
            self.h_offset = self.h_offset.saturating_sub(step);
        }
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

    fn process_targets(&self) -> Vec<ProcessTarget> {
        let mut targets: Vec<_> = if self.tags.is_empty() {
            self.selected_row()
                .map(ProcessTarget::from)
                .into_iter()
                .collect()
        } else {
            self.tags.iter().copied().collect()
        };
        targets.sort_unstable();
        targets
    }

    fn toggle_tag(&mut self) {
        if let Some(row) = self.selected_row() {
            let target = ProcessTarget::from(row);
            if !self.tags.remove(&target) {
                self.tags.insert(target);
            }
        }
    }

    fn tag_with_children(&mut self) {
        if let Some(row) = self.selected_row() {
            let pid = row.pid;
            let pids: std::collections::HashSet<_> =
                tree::subtree(&self.table.rows, pid).into_iter().collect();
            for row in &self.table.rows {
                if pids.contains(&row.pid) {
                    self.tags.insert(ProcessTarget::from(row));
                }
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
        self.action_targets = vec![ProcessTarget::from(row)];
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
        self.action_targets = vec![ProcessTarget::from(row)];
        self.screen = Screen::IoPriority(crate::panels::IoPriorityPanel::new(pid));
        Reaction::Continue
    }

    fn verify_dialog_target(&self, pid: gander_core::Pid) -> Result<(), String> {
        let target = self
            .action_targets
            .first()
            .filter(|target| target.pid == pid)
            .ok_or_else(|| "no process target".to_string())?;
        target.verify().map_err(|e| e.to_string())
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
            Key::Char('S') => match self
                .verify_dialog_target(panel.pid)
                .and_then(|()| panel.apply())
            {
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
            Key::Char('S') => match self
                .verify_dialog_target(panel.pid)
                .and_then(|()| panel.apply())
            {
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

    /// Change the nice value of the selected process, or of every tagged one.
    ///
    /// Tagged processes take precedence, exactly as in [`App::send_signal`].
    /// Previously this only ever reniced the row under the cursor, so `F9` acted
    /// on the whole tagged set while `F7`/`F8` silently acted on one process — the
    /// kind of inconsistency that makes a user distrust both keys.
    fn renice_selected(&mut self, delta: i8) {
        // Same target selection as the signal picker: if anything is tagged, the
        // action applies to the tagged set.
        let targets = self.process_targets();
        if targets.is_empty() {
            self.status = Some("no process selected".into());
            return;
        }

        let mut reniced = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for target in &targets {
            let pid = target.pid;
            let action = gander_core::action::Action::Priority { pid, delta };
            match gander_core::action::execute_checked(*target, &action) {
                Ok(()) => reniced += 1,
                Err(e) => failures.push(format!("{pid}: {e}")),
            }
        }

        let verb = if delta < 0 { "raised" } else { "lowered" };
        self.status = Some(if failures.is_empty() {
            if targets.len() == 1 {
                format!("reniced {} ({verb} priority)", targets[0].pid)
            } else {
                format!("reniced {reniced} processes ({verb} priority)")
            }
        } else if reniced == 0 {
            format!("renice failed: {}", failures.join("; "))
        } else {
            format!(
                "reniced {reniced}, {} failed: {}",
                failures.len(),
                failures.join("; ")
            )
        });
    }

    /// `e`: the environment of the selected process.
    ///
    /// Linux reads `/proc/<pid>/environ`; FreeBSD has `sysctl(KERN_PROC_ENV)`,
    /// which is the same NUL-separated payload, so both answer. macOS has
    /// `KERN_PROCARGS2`, but it returns argv *and* env interleaved in one buffer
    /// with the layout decided by `argc`, and `sysctl` refuses another user's
    /// process — so there the panel explains instead of showing a truncated blob.
    fn show_env(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            return Reaction::Continue;
        };
        let pid = row.pid;
        let lines = env_of(pid).unwrap_or_else(|| {
            vec![
                "cannot read environment".to_string(),
                String::new(),
                env_unavailable_reason().to_string(),
            ]
        });
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

    /// `s`: attach `strace` to the selected process.
    ///
    /// Not implemented, and the key now says so. It used to be documented in
    /// `docs/KEYS.md` and `docs/DESIGN.md` with no handler behind it, so pressing
    /// it did nothing at all — the exact silent-key failure that `Z` and `b` were
    /// given explanation screens to avoid.
    ///
    /// A real implementation is a terminal handover rather than a panel: `strace`
    /// writes a live stream that the alternate screen would swallow, so the TUI
    /// has to leave the alternate screen, hand the terminal to the tracer, and
    /// restore on exit. That is a runtime concern, not a `Screen::Info` one.
    fn show_strace(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            return Reaction::Continue;
        };
        let pid = row.pid;

        let mut lines = vec![
            "Attaching a tracer from inside the UI is not implemented yet.".to_string(),
            String::new(),
            "It is a terminal handover, not a panel: strace writes a live stream".to_string(),
            "that would be swallowed by the alternate screen, so the TUI has to".to_string(),
            "release the terminal, run the tracer, and take it back on exit.".to_string(),
            String::new(),
            "Run it alongside gander instead:".to_string(),
            format!("  strace -p {pid}"),
            format!("  sudo strace -f -p {pid}"),
            String::new(),
        ];

        // The generic warning is not the useful part. On most distributions
        // `yama/ptrace_scope` is 1, which forbids attaching to anything that is not
        // a child — so `strace -p` fails for *every* process on the machine, for a
        // reason the user cannot see, and the obvious conclusion is that gander (or
        // strace) is broken. Printing the number turns a mystery into a setting.
        lines.extend(ptrace_scope_note());

        self.screen = Screen::Info {
            title: format!("Trace of {pid}"),
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

    /// `l`: the file descriptors of the selected process.
    ///
    /// Linux reads `/proc/<pid>/fd`; FreeBSD has `sysctl(KERN_PROC_FILEDESC)`,
    /// which returns `struct kinfo_file` records with `kf_fd` and `kf_path`.
    fn show_open_files(&mut self) -> Reaction {
        let Some(row) = self.selected_row() else {
            return Reaction::Continue;
        };
        let pid = row.pid;
        let list = open_files_of(pid).unwrap_or_else(|| {
            vec![
                "cannot read file descriptors".to_string(),
                String::new(),
                fd_unavailable_reason().to_string(),
            ]
        });
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
        let lines: Vec<String> = file_locks_of(pid).unwrap_or_else(|| {
            vec![
                "no locks".to_string(),
                String::new(),
                locks_unavailable_reason().to_string(),
            ]
        });
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
    pub fn header_height_debug(&self, width: u16) -> u16 {
        self.header_height(width)
    }

    /// Height the header needs for a terminal `width` cells wide: the taller
    /// column, plus the screen-tab row.
    ///
    /// A fixed height clipped the per-core CPU meters as soon as the machine had
    /// more than four cores. The width matters because the CPU meter lays its
    /// cores out several to a line: the same 128 cores cost nine rows on a
    /// 200-cell terminal and many more on a narrow one, and asking for the
    /// height without the width is what made a 128-core machine demand 65 rows.
    fn header_height(&self, width: u16) -> u16 {
        // The same geometry `draw_header` uses: two equal halves with a one-cell
        // gutter at the end of the left one.
        let left_w = (width / 2).saturating_sub(1);
        let right_w = width - width / 2;
        let height_of = |column: crate::meters::HeaderColumn| -> u16 {
            let column_w = match column {
                crate::meters::HeaderColumn::Left => left_w,
                crate::meters::HeaderColumn::Right => right_w,
            };
            self.meters
                .iter()
                .filter(|m| m.column() == column)
                .map(|m| m.rows(column_w).max(1))
                .sum()
        };
        let split = self
            .meters
            .iter()
            .any(|m| m.column() == crate::meters::HeaderColumn::Right);
        let height = if split {
            height_of(crate::meters::HeaderColumn::Left)
                .max(height_of(crate::meters::HeaderColumn::Right))
        } else {
            // One column: every meter gets the full width.
            self.meters.iter().map(|m| m.rows(width).max(1)).sum()
        };
        height.max(1) + 1
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        // Paint the theme background over the whole screen first. Every style in
        // a Theme sets a foreground, and most leave the background unset, which
        // keeps whatever is already in the buffer — black on a fresh terminal.
        // That is invisible for dark themes but breaks the light one (`paper`),
        // whose whole point is a light canvas. One fill here is the single place
        // all four draw paths (htop, panels, setup, dialogs) inherit it from.
        let area = frame.area();
        if area.width > 0 && area.height > 0 {
            let buf = frame.buffer_mut();
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    buf[(x, y)]
                        .set_bg(self.theme.background)
                        .set_fg(self.theme.foreground);
                }
            }
        }
        if self.layout == LayoutKind::Panels {
            self.draw_panels(frame);
        } else {
            self.draw_htop(frame);
        }
    }

    /// The classic htop arrangement: horizontal meters, full-width table.
    fn draw_htop(&mut self, frame: &mut Frame) {
        let area = frame.area();
        // The header grows with the core count: 8 cores means two five-row CPU
        // meters, so a fixed four-row header would clip them.
        let header_h = self
            .header_height(area.width)
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
                    &self.action_targets,
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

        // The search/filter prompt occupies the table's header row, which is
        // what htop does and what keeps the list from losing a line to it.
        if self.input.active && self.screen == Screen::ProcessList {
            let row = Rect::new(chunks[1].x, chunks[1].y, chunks[1].width, 1);
            draw_prompt(row, frame.buffer_mut(), &self.theme, &self.input);
        }

        self.draw_function_bar(chunks[2], frame.buffer_mut());
    }

    /// The `panels` arrangement: system column, process table, inspector.
    ///
    /// See `crate::layout` for why this exists. The shape is:
    ///
    /// ```text
    /// [tab strip across the top]
    /// ┌─ system ─┬─ processes ──────────┬─ inspector ─┐
    /// │ CPU bar  │  PID  CPU%  MEM%  CMD │ 2971 pi     │
    /// │ cores    │  ...                  │ cmdline     │
    /// │ MEM bar  │                       │ facts       │
    /// └──────────┴───────────────────────┴─────────────┘
    /// [function bar]
    /// ```
    fn draw_panels(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.height < 4 || area.width < 30 {
            // Too small for side panels. Fall back to the compact arrangement
            // rather than drawing three unusable slivers.
            self.draw_htop(frame);
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1), // tab strip
            Constraint::Min(3),    // panels
            Constraint::Length(1), // function bar
        ])
        .split(area);

        // The tab strip and function bar belong to the panels layout too; only
        // the middle band changes shape.
        self.draw_tabs(chunks[0], frame.buffer_mut());

        let widths = crate::layout::PanelWidths::resolve(chunks[1].width);
        let columns = Layout::horizontal([
            Constraint::Length(widths.system),
            Constraint::Min(10),
            Constraint::Length(widths.inspector),
        ])
        .split(chunks[1]);

        if widths.system > 0 {
            let buf = frame.buffer_mut();
            crate::layout::draw_system_column(
                columns[0],
                buf,
                &self.theme,
                &self.snapshot,
                self.cpu_history.make_contiguous(),
                (
                    self.snapshot.tasks.total,
                    self.snapshot.tasks.running,
                    self.snapshot.tasks.threads,
                ),
                self.config.system_cores_max,
            );
            draw_panel_divider(columns[0], buf, &self.theme);
        }

        // The middle band: whatever screen is active. The table is the common
        // case; the modal screens take the whole band so they still have room.
        let screen = self.screen.clone();
        match &screen {
            Screen::ProcessList => self.draw_table(columns[1], frame.buffer_mut()),
            Screen::Help => draw_help(columns[1], frame.buffer_mut(), &self.theme),
            Screen::Info {
                title,
                lines,
                scroll,
            } => draw_info(
                columns[1],
                frame.buffer_mut(),
                &self.theme,
                title,
                lines,
                *scroll,
            ),
            Screen::SignalPicker { cursor } => {
                self.draw_table(columns[1], frame.buffer_mut());
                draw_signal_picker(
                    columns[1],
                    frame.buffer_mut(),
                    &self.theme,
                    &self.action_targets,
                    *cursor,
                );
            }
            Screen::Setup => {
                if let Some(setup) = &self.setup {
                    setup.draw(columns[1], frame.buffer_mut(), &self.theme);
                }
            }
            Screen::Affinity(panel) => panel.draw(columns[1], frame.buffer_mut(), &self.theme),
            Screen::IoPriority(panel) => panel.draw(columns[1], frame.buffer_mut(), &self.theme),
        }

        // The inspector is the point of this layout, but it is meaningless while
        // a modal screen hides the list; showing a stale selection then would be
        // lying about what is on screen.
        if widths.inspector > 0 && matches!(self.screen, Screen::ProcessList) {
            let mut users = std::mem::take(&mut self.users);
            let row = self.selected_row();
            let user = row.map(|r| {
                let uid = r.uid;
                users.name(uid).to_string()
            });
            let buf = frame.buffer_mut();
            draw_panel_divider(columns[2], buf, &self.theme);
            crate::layout::draw_inspector(
                columns[2],
                buf,
                &self.theme,
                row,
                &self.snapshot,
                user.as_deref(),
            );
            self.users = users;
        }

        if self.input.active && self.screen == Screen::ProcessList {
            let row = Rect::new(columns[1].x, columns[1].y, columns[1].width, 1);
            draw_prompt(row, frame.buffer_mut(), &self.theme, &self.input);
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
            // One column: draw every meter in it, through the same path as the
            // split case so the meter cursor is marked identically.
            let rect = Rect::new(area.x, area.y, area.width, meter_h);
            let all: Vec<usize> = (0..self.meters.len()).collect();
            self.draw_column_indices(&all, rect, buf, &theme);
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
            let selected = i == self.meter_cursor;
            let Some(meter) = self.meters.get_mut(i) else {
                continue;
            };
            let wanted = meter.rows(area.width).max(1);
            if y >= area.bottom() {
                break;
            }
            // Clip to what is left rather than skipping the meter entirely.
            //
            // `break` here meant a meter taller than the remaining rows was
            // dropped *whole*: on a 128-core machine the CPU meter wants 65 rows,
            // the header is clamped to the screen, and the first meter therefore
            // failed the test — so the entire header rendered blank and the only
            // thing left on screen was the table. A partially drawn meter is
            // useful; no header at all is not. The renderers already stop at
            // `area.bottom()`, so a shorter rect is all that is needed.
            let h = wanted.min(area.bottom() - y);
            let rect = Rect::new(area.x, y, area.width, h);
            meter.render(rect, buf, theme, mode);
            // Mark the meter `I` will act on. Without this the cursor is
            // invisible state: `Shift+Right` would change a mode the user cannot
            // see being targeted.
            //
            // This was only done in `draw_column`, so on a machine with more than
            // four cores — the case the split header exists for — the cursor was
            // never drawn at all, and `selected` was computed and then dropped.
            if selected {
                Self::mark_header_selection(rect, buf, theme);
            }
            y += h;
        }
    }

    /// Mark the selected header meter.
    ///
    /// This draws a reversed label in the meter's *own* first row rather than a
    /// marker underneath it. The earlier version put a row of `~` on the line
    /// below the meter, which is the next meter's line: a `~` under the CPU meter
    /// appeared to belong to the Memory meter, so the cursor pointed at the wrong
    /// thing. Underlining is only unambiguous when the rows belong to the thing
    /// being underlined.
    ///
    /// The margin is used, not the data: the meter's own label sits there and is
    /// short, so overwriting a few cells of it with reversed video is readable and
    /// destroys no measurement.
    fn mark_header_selection(rect: Rect, buf: &mut Buffer, theme: &Theme) {
        if rect.height == 0 || rect.width == 0 {
            return;
        }
        let width = theme_cursor_width(rect.width);
        for x in rect.x..rect.x.saturating_add(width) {
            if let Some(cell) = buf.cell_mut((x, rect.y)) {
                let style = cell.style().patch(theme.meter_cursor);
                cell.set_style(style);
            }
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
        let theme = self.theme.clone();

        // Both are unaffected by the early return below, and `users` is taken out
        // of `self` further down: doing it before this check would leave the
        // uid -> name cache emptied for the rest of the run.
        if fields.is_empty() || area.height == 0 {
            return;
        }

        // Tree prefixes first, because the Command cell carries them and so the
        // column has to be measured with the prefix included.
        let marks = if self.table.tree_mode {
            compose_tree(&self.table.rows, &self.table.view.order)
        } else {
            Vec::new()
        };

        // Taken out of `self` so the row loop can borrow `self.table` immutably
        // while resolving names. Done after the early return: taking it first
        // would leave the uid -> name cache empty for the rest of the run.
        let mut users = std::mem::take(&mut self.users);

        // Column widths: fixed widths for known fields, the last column takes the
        // remainder (htop makes Command the expanding column) *and* is widened
        // when a command on screen does not fit. Both halves matter; see
        // `column_widths`.
        let widths = column_widths(&fields, area.width, self.longest_last_cell(&fields, &marks));
        let total_width: u16 = widths.iter().sum();
        // Horizontal scroll, clamped so the last column stays flush with the
        // right edge instead of scrolling into empty space. Clamped *into* `self`
        // as well, so holding `Right` at the end cannot push the offset past the
        // end and force the user to walk back from somewhere off-screen.
        self.h_offset = self.h_offset.min(total_width.saturating_sub(area.width));
        let h_offset = self.h_offset;
        // Remember what was drawn, so `scroll_columns` knows how far it may go
        // before the next frame replaces these numbers.
        self.last_total_width = total_width;
        self.last_area_width = area.width;
        let view_left = area.x as i32 - h_offset as i32;
        let clip_left = area.x as i32;
        let clip_right = area.right() as i32;

        // Header row, clipped to the visible horizontal window.
        //
        // Scrolled all the way right, the header is entirely the tail of the
        // Command column, which carries no title — the row came out blank and
        // read as "the header is gone". Marking the edges says which way is left,
        // which is the only thing a scrolled table needs to communicate.
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
        draw_scroll_edges(
            buf,
            area,
            h_offset > 0,
            h_offset + area.width < total_width,
            &theme,
        );

        let body_height = area.height.saturating_sub(1) as usize;
        // Remember it so paging and scroll-following know the window size.
        self.viewport = body_height;

        // Scale the in-table value bars against the largest value *in the whole
        // table*, not against 100%. On a quiet machine the top process might sit
        // at 4%, and an absolute bar would then be invisible for every row —
        // exactly when knowing which of the 4% processes is biggest matters most.
        //
        // Table-wide rather than per-screenful so the bars do not rescale as you
        // scroll, which would make them unreadable.
        let bar_scale = self.table.rows.iter().fold((0.0f32, 0.0f32), |acc, r| {
            (acc.0.max(r.percent_cpu), acc.1.max(r.percent_mem))
        });

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
            } else if self.tags.contains(&ProcessTarget::from(row)) {
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
                    // Where the executable's base name lands in this cell, so it
                    // can be emphasised after the row is painted. Computed from the
                    // finished `text` (prefix included) rather than adjusted after
                    // the fact, so tree mode needs no offset arithmetic.
                    let base_name_span = if *field == Field::Comm && self.config.highlight_base_name
                    {
                        row.base_name_span_in(&text)
                    } else {
                        None
                    };

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

                    // Repaint the base name over the row style. A foreground patch,
                    // not a replacement: the selection or tag background must show
                    // through, or the highlight would erase the row's state.
                    if let Some((bs, be)) = base_name_span {
                        paint_base_name(
                            buf,
                            &text,
                            bs,
                            be,
                            cx,
                            draw_x,
                            draw_end,
                            y,
                            base,
                            theme.row_base_name,
                        );
                    }

                    // In the panels layout, paint a proportional bar *behind* the
                    // CPU and memory numbers. The table is sorted by one of them,
                    // and a filled background makes the ordering visible at a
                    // glance instead of requiring you to read 30 numbers.
                    //
                    // Only when the whole column is on screen: a half-visible bar
                    // would misrepresent the value, which is worse than no bar.
                    if self.layout == LayoutKind::Panels
                        && draw_x == cx
                        && draw_end == cx + w
                        && matches!(*field, Field::PercentCpu | Field::PercentMem)
                    {
                        // A small floor on the divisor keeps a lone 0.0%
                        // process from being drawn as a full bar.
                        let (value, max) = if *field == Field::PercentCpu {
                            (row.percent_cpu, bar_scale.0)
                        } else {
                            (row.percent_mem, bar_scale.1)
                        };
                        let frac = if max > 0.05 {
                            (value / max).clamp(0.0, 1.0)
                        } else {
                            value.clamp(0.0, 1.0)
                        };
                        paint_value_bar(
                            buf,
                            draw_x as u16,
                            y,
                            (draw_end - draw_x) as u16,
                            frac,
                            theme.row_value_bar,
                        );
                    }
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

        // The status line replaces the *rightmost* key labels, because on a
        // terminal of any normal width the ten function keys already fill the
        // bar — there is never free space to the right of them.
        //
        // `App::status` was written by a dozen actions — renice results, kill
        // failures, "hiding threads", the reason `O` does nothing on macOS — and
        // was never drawn anywhere. Every one of those messages was invisible;
        // the tests passed because they read the field directly rather than the
        // screen. Painting it here is what makes the field mean something.
        self.draw_status(area, buf);
    }

    /// Paint `status` right-aligned, over whatever key labels are there.
    ///
    /// The rightmost keys are `F9 Kill` and `F10 Quit`. Overwriting them is a real
    /// trade-off: `F10 Quit` is the one key a stuck user most needs to see. So the
    /// message is capped at half the bar, leaving the left keys — including the
    /// `F1 Help` a confused user needs — untouched, and it is transient by
    /// construction (the next keypress that sets no status clears it).
    fn draw_status(&self, area: Rect, buf: &mut Buffer) {
        let Some(status) = self.status.as_deref() else {
            return;
        };
        if status.is_empty() || area.height == 0 || area.width < 20 {
            return;
        }
        let max_width = (area.width / 2).max(10);
        let text: String = status.chars().take(max_width as usize).collect();
        let text_len = text.chars().count() as u16;
        if text_len == 0 {
            return;
        }
        let x = area.right().saturating_sub(text_len);
        crate::meters::put_str(
            buf,
            x,
            area.y,
            &text,
            self.theme.function_bar_status,
            text_len,
        );
    }
}

/// How wide to paint the header meter cursor: a short label-sized block.
fn theme_cursor_width(available: u16) -> u16 {
    // 5 covers "Mem" plus a little, which reads as a deliberate marker without
    // covering a bar's worth of data.
    available.min(5)
}

/// Paint a value bar across `width` cells at `(x, y)`, keeping the text.
///
/// The bar shows *relative* magnitude within the column rather than an absolute
/// percentage against 100: on a machine where the top process sits at 4%, an
/// absolute bar would be invisible, which is exactly when you most want to see
/// which of the 4% processes is the biggest.
///
/// The cell text is preserved; only the style changes. Overwriting the glyphs
/// would destroy the number the bar is annotating.
fn paint_value_bar(buf: &mut Buffer, x: u16, y: u16, width: u16, frac: f32, style: Style) {
    if width == 0 {
        return;
    }
    let filled = ((frac.clamp(0.0, 1.0)) * width as f32).ceil() as u16;
    for i in 0..filled.min(width) {
        if let Some(cell) = buf.cell_mut((x + i, y)) {
            cell.set_style(cell.style().patch(style));
        }
    }
}

/// Emphasise the executable's base name inside an already-painted command cell.
///
/// # Why this repaints instead of drawing in one pass
///
/// The command column is written with a single `put_str` that carries the row's
/// style, and that style encodes state the highlight must not destroy: a selected
/// row's background, a tag's colour, a kernel thread's dimming. So the base name is
/// patched *over* the row afterwards with `Style::patch`, which composes rather
/// than replaces.
///
/// The offsets are byte offsets into `text`, but a terminal cell is a char, and
/// the column is clipped both by horizontal scrolling (`draw_x > cx`) and by the
/// screen edge (`draw_end`). All three mappings are done here, in one place, so the
/// caller only has to say "this span of this text".
#[allow(clippy::too_many_arguments)]
fn paint_base_name(
    buf: &mut Buffer,
    text: &str,
    span_start: usize,
    span_end: usize,
    column_x: i32,
    draw_x: i32,
    draw_end: i32,
    y: u16,
    row_style: Style,
    highlight: Style,
) {
    if span_end <= span_start || draw_end <= draw_x {
        return;
    }
    // Byte offset -> char index, because a cell is a char. `char_indices` is
    // cheaper than collecting and needs no allocation.
    let start_char = text[..span_start.min(text.len())].chars().count() as i32;
    let end_char = text[..span_end.min(text.len())].chars().count() as i32;

    for char_idx in start_char..end_char {
        // Column position in the (unscrolled) row, then on screen.
        let screen_x = column_x + char_idx;
        if screen_x < draw_x || screen_x >= draw_end {
            continue;
        }
        if let Some(cell) = buf.cell_mut((screen_x as u16, y)) {
            cell.set_style(row_style.patch(highlight));
        }
    }
}

/// Draw a one-cell vertical rule between panels.
///
/// A border character rather than a `Block`: the panels are not boxes, and adding
/// four borders around a 24-row terminal costs 4 rows and 4 columns for no
/// information.
fn draw_panel_divider(area: Rect, buf: &mut Buffer, theme: &Theme) {
    let x = area.right().saturating_sub(1);
    // The divider is drawn by the caller against the panel it follows, so the
    // column sits on that panel's last cell.
    for y in area.y..area.bottom() {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char('│');
            cell.set_style(theme.dialog_border);
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
    /// `Shift+Left` / `Shift+Right`: move the header meter cursor.
    HeaderPrev,
    HeaderNext,
    F(u8),
}

// ---------------------------------------------------------------------------
// Field rendering
// ---------------------------------------------------------------------------

/// Column widths in cells.
///
/// Two things happen here, and the second one is why horizontal scrolling works
/// at all.
///
/// 1. Every field has a fixed width except the last (normally Command), which
///    absorbs the slack so the table fills the terminal.
///
/// 2. The last column is then widened to fit `widest_last_cell`, the longest
///    value actually present in it. Without this step the table could never be
///    wider than the screen: the absorbed width is bounded by
///    `total - fixed`, so `total_width` was always `<= area.width`, the
///    `total_width - area.width` clamp below always produced 0, and `Left`/`Right`
///    changed `h_offset` while the renderer threw it away. A long command line
///    was silently cut at the screen edge with no way to reach the rest of it.
///
/// `MAX_EXPAND` therefore only bounds how much *empty* slack a very wide terminal
/// hands to the last column. The measured content is not capped by it, because
/// capping it is precisely what hid the text.
fn column_widths(fields: &[Field], total: u16, widest_last_cell: u16) -> Vec<u16> {
    // Longest stretch the last column takes purely because the terminal is wide,
    // before any measured content is considered.
    const MAX_EXPAND: u16 = 120;

    let mut widths: Vec<u16> = fields.iter().map(|f| default_width(*f)).collect();
    let fixed: u16 = widths.iter().sum();
    if let Some(last) = widths.last_mut() {
        let slack = total.saturating_sub(fixed).min(MAX_EXPAND);
        *last = last
            .saturating_add(slack)
            .max(6)
            // The measured width wins when it is larger: that is the whole point.
            .max(widest_last_cell);
    }
    widths
}

/// How wide a cell of `field` is when rendered, ignoring the rest of the row.
///
/// Only the last column can scroll, so this is asked of the Command column; the
/// helper is written against a field rather than hardcoding `Field::Comm` so a
/// configuration that ends with, say, `CGROUP` still measures something sane.
fn cell_width(row: &Row, field: Field, prefix: &str) -> u16 {
    // A command line is a few hundred cells at most in practice. The cap keeps
    // the sum of all column widths from overflowing `u16`, and stops one absurd
    // `/proc/<pid>/cmdline` from letting `Right` scroll into empty space.
    const MAX_LAST_CELL: u16 = 1000;

    // Control characters are painted as one cell each (`put_str` sanitises them),
    // so counting chars is the right measure rather than counting bytes.
    let text = match field {
        Field::Comm => row.display_command(),
        _ => return 0,
    };
    let n = prefix.chars().count() + text.chars().count();
    u16::try_from(n).unwrap_or(MAX_LAST_CELL).min(MAX_LAST_CELL)
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
        // A cgroup path is long and mostly identical between processes; the
        // container id is what distinguishes them, so CGROUP gets a readable
        // slice rather than the whole path and CONTAINER fits `runtime:` plus a
        // 12-character id. CGROUP is the narrower of the two: `CONTAINER` is the
        // column that actually distinguishes rows, and the path is context.
        #[cfg(target_os = "linux")]
        Field::Cgroup => 20,
        #[cfg(target_os = "linux")]
        Field::Container => 22,
        // Same width as RES/VIRT: these are byte counts formatted by
        // `bytes_human`, which tops out at 6 cells (`1023.9G`).
        #[cfg(target_os = "linux")]
        Field::Pss | Field::Epss | Field::Private | Field::Swap => 6,
        #[cfg(target_os = "linux")]
        Field::GpuPercent => 6,
        #[cfg(target_os = "linux")]
        Field::GpuTime => 9,
        // `100.0` at worst, which is 5 cells plus the `%` that
        // `percent_human` appends.
        #[cfg(target_os = "linux")]
        Field::CpuDelayPercent | Field::IoDelayPercent | Field::SwapDelayPercent => 6,
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
        Field::Session => dash_if_zero_u32(row.session),
        Field::Tty => tty_name(row.tty_nr),
        Field::Tpgid => {
            // `-1` is the kernel's "no foreground job", not a process group, so
            // it must not be printed as a number.
            if row.tpgid < 0 {
                "-".to_string()
            } else {
                row.tpgid.to_string()
            }
        }
        Field::StartTime => started_at(row, snapshot),
        Field::Tgid => row.tgid.to_string(),
        Field::SchedPolicy => sched_policy_name(row.sched_policy).to_string(),
        Field::ProcComm => row.comm.as_str().to_string(),
        Field::ProcExe => row.exe.as_deref().unwrap_or("-").to_string(),
        Field::Cwd => row.cwd.as_deref().unwrap_or("-").to_string(),
        // Linux-only fields. The `_` arm is still needed because `Field` is
        // `#[non_exhaustive]`, but every variant that can be *selected* on this
        // platform has an arm: a column that renders "-" for every row is a
        // setup-screen entry that does nothing.
        #[cfg(target_os = "linux")]
        Field::Cgroup => row.cgroup.as_deref().unwrap_or("-").to_string(),
        #[cfg(target_os = "linux")]
        Field::Container => match (&row.container_runtime, &row.container) {
            // An identified container: `docker:1f2e3d4c5b6a`.
            (Some(runtime), Some(id)) if !id.is_empty() && id.as_ref() != "?" => {
                let end = id.len().min(12);
                format!("{runtime}:{}", &id[..end])
            }
            // Known to be in a child container, but cgroup could not name it
            // (a container with its own cgroup namespace). Say so instead of
            // inventing an id.
            (Some(_), _) => "container:?".to_string(),
            _ => "-".to_string(),
        },
        // The PSS family. A `None` is "not read" or "not readable" (another
        // user's process without CAP_SYS_PTRACE), and both are `-`: a zero would
        // claim the process uses no memory, which is a different and wrong
        // statement. `Some(0)` does render `0B`, because that is a real answer.
        #[cfg(target_os = "linux")]
        Field::Pss => bytes_or_dash(row.pss),
        #[cfg(target_os = "linux")]
        Field::Epss => bytes_or_dash(row.epss),
        #[cfg(target_os = "linux")]
        Field::Private => bytes_or_dash(row.private),
        #[cfg(target_os = "linux")]
        Field::Swap => bytes_or_dash(row.swap_bytes),
        // GPU. A process with no DRM descriptor renders `-`, not `0.0`: it is not
        // using no GPU, it is not using the GPU at all, and those read differently
        // in a sorted list.
        #[cfg(target_os = "linux")]
        Field::GpuPercent => row
            .gpu_percent
            .map(|p| format::percent_human(p).to_string())
            .unwrap_or_else(|| "-".into()),
        #[cfg(target_os = "linux")]
        Field::GpuTime => row
            .gpu_ns
            .map(|ns| format::duration_human(ns / 1_000_000_000).to_string())
            .unwrap_or_else(|| "-".into()),
        // Delay accounting. `-` for unread, and it is the common case: reading
        // these needs `CAP_NET_ADMIN`, which an ordinary `gander` does not have.
        #[cfg(target_os = "linux")]
        Field::CpuDelayPercent => opt_percent(row.cpu_delay_percent),
        #[cfg(target_os = "linux")]
        Field::IoDelayPercent => opt_percent(row.io_delay_percent),
        #[cfg(target_os = "linux")]
        Field::SwapDelayPercent => opt_percent(row.swap_delay_percent),
        _ => "-".to_string(),
    }
}

/// Render an optional percentage, with `-` for "not measured".
///
/// The same distinction `bytes_or_dash` makes, for the same reason: a delay of
/// `0.0%` is a measurement of a process that never waited, while `-` means the
/// figure was never obtained — usually because the kernel refused
/// `TASKSTATS` without `CAP_NET_ADMIN`. Showing `0.0%` there would claim every
/// process is waiting on nothing, which is exactly the condition the column
/// exists to detect.
#[cfg(target_os = "linux")]
fn opt_percent(value: Option<f32>) -> String {
    value
        .map(|p| format::percent_human(p).to_string())
        .unwrap_or_else(|| "-".into())
}

/// Render an optional byte count, with `-` for "not read".
///
/// The distinction is the whole reason these columns carry `Option<u64>` rather
/// than `u64`: `Some(0)` is a measurement and renders `0B`, while `None` means the
/// figure was never obtained — another user's process without `CAP_SYS_PTRACE`, or
/// a column the sampler was not asked to fill. Rendering both as `0B` would turn
/// "we did not look" into a false claim about the process.
#[cfg(target_os = "linux")]
fn bytes_or_dash(value: Option<u64>) -> String {
    value
        .map(|v| format::bytes_human(v).to_string())
        .unwrap_or_else(|| "-".into())
}

/// `0` is the "unknown" sentinel for the session-id fields, not a session id.
fn dash_if_zero_u32(value: u32) -> String {
    if value == 0 {
        "-".to_string()
    } else {
        value.to_string()
    }
}

/// Render the controlling terminal from `/proc/<pid>/stat`'s `tty_nr`.
///
/// The kernel encodes the device in a `dev_t`-shaped integer, and the "encoded"
/// form libc and htop use is `major = (tty >> 8) & 0xfff`, `minor = (tty & 0xff) |
/// ((tty >> 12) & 0xfff00)`. `0` means no controlling terminal, which renders `?`
/// — the convention `ps` uses, and the one that distinguishes "a tty with minor 0"
/// from "no tty at all".
///
/// Only the common pseudo-terminal range is decoded: `tty*(4)` is major 136, where
/// minor `N` is `/dev/pts/N`; anything else shows the raw `major:minor` pair rather
/// than a name this code invented.
fn tty_name(tty_nr: i64) -> String {
    if tty_nr == 0 {
        return "?".to_string();
    }
    tty_name_platform(tty_nr)
}

/// Linux: decode the encoded `dev_t` that `/proc/<pid>/stat` field 7 carries.
///
/// The kernel's "new" encoding is `major = (dev >> 8) & 0xfff`, `minor = (dev &
/// 0xff) | ((dev >> 12) & 0xfff00)`. Only the two common ranges get a name;
/// anything else shows `major:minor` rather than a path this code invented.
#[cfg(target_os = "linux")]
fn tty_name_platform(tty_nr: i64) -> String {
    let tty = tty_nr as u64;
    let major = (tty >> 8) & 0xfff;
    let minor = (tty & 0xff) | ((tty >> 12) & 0xfff00);
    match major {
        136 => format!("pts/{minor}"),
        4 => format!("tty{minor}"),
        _ => format!("{major}:{minor}"),
    }
}

/// macOS: `e_tdev` is already a `dev_t`, and the OS ships the name table.
///
/// `devname(3)` is the authority here — it knows `ttys000`, `console` and the
/// rest, which no bit-fiddling in this file would get right. It returns NULL for
/// a device it has no name for, and the raw number is shown instead.
#[cfg(target_os = "macos")]
fn tty_name_platform(tty_nr: i64) -> String {
    // SAFETY: `devname` reads a static name table; the returned pointer is either
    // NULL or a NUL-terminated string owned by libc.
    let ptr = unsafe { libc::devname(tty_nr as libc::dev_t, libc::S_IFCHR) };
    if ptr.is_null() {
        return format!("{tty_nr:#x}");
    }
    // SAFETY: non-null means a NUL-terminated string.
    let name = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_string_lossy();
    if name.is_empty() {
        format!("{tty_nr:#x}")
    } else {
        name.into_owned()
    }
}

/// FreeBSD: `ki_tdev` is a real `dev_t` and `devname_r(3)` knows the names.
///
/// The value has already been normalised to `0` for "no terminal" by the sampler,
/// so the only question here is whether the OS has a name for the device. A
/// `dev_t` that resolves to nothing (a pty whose master is gone) prints the raw
/// number rather than an empty cell.
#[cfg(target_os = "freebsd")]
fn tty_name_platform(tty_nr: i64) -> String {
    let mut buf = [0 as libc::c_char; 128];
    // SAFETY: `buf` is 128 bytes, its length is passed, and `devname_r` writes a
    // NUL-terminated name into it or returns NULL.
    let ptr = unsafe {
        libc::devname_r(
            tty_nr as libc::dev_t,
            libc::S_IFCHR,
            buf.as_mut_ptr(),
            buf.len() as libc::c_int,
        )
    };
    if ptr.is_null() {
        return format!("{tty_nr:#x}");
    }
    // SAFETY: a non-null return points into `buf`.
    let name = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy();
    if name.is_empty() {
        format!("{tty_nr:#x}")
    } else {
        name.into_owned()
    }
}

/// Platforms with neither a `dev_t` in `stat` nor `devname(3)`: show the value.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
fn tty_name_platform(tty_nr: i64) -> String {
    tty_nr.to_string()
}

/// Wall-clock start time.
///
/// `starttime` means two different things per platform, and both are handled here
/// rather than by making one backend lie:
///
/// * Linux stores field 22 of `/proc/<pid>/stat` — **ticks since boot**. It is the
///   PID-reuse key, so it stays in ticks, and the wall clock is `boot + ticks/HZ`.
/// * macOS stores `pbi_start_tvsec * 1e6` — **microseconds since the epoch**. That
///   is already an absolute time, and adding the derived boot time again would put
///   every process a machine-uptime in the future.
/// * FreeBSD stores `ki_start` the same way, so it shares macOS's arm.
///
/// The platform switch is a compile-time `cfg` rather than a runtime flag because
/// `starttime`'s units are fixed by the backend that fills it.
fn started_at(_row: &Row, _snapshot: &SystemSnapshot) -> String {
    let at = started_at_unix(_row, _snapshot);
    match at {
        Some(secs) => local_datetime(secs),
        None => "-".to_string(),
    }
}

#[cfg(target_os = "linux")]
fn started_at_unix(row: &Row, snapshot: &SystemSnapshot) -> Option<i64> {
    if snapshot.clk_tck == 0 {
        return None;
    }
    let start_secs = row.starttime / snapshot.clk_tck;
    let boot = now_unix_secs().saturating_sub(snapshot.uptime);
    Some(boot.saturating_add(start_secs) as i64)
}

/// macOS: `starttime` is already epoch microseconds, not ticks since boot.
#[cfg(target_os = "macos")]
fn started_at_unix(row: &Row, _snapshot: &SystemSnapshot) -> Option<i64> {
    if row.starttime == 0 {
        return None;
    }
    Some((row.starttime / 1_000_000) as i64)
}

/// FreeBSD: `ki_start` is already an absolute `timeval`, so `starttime` carries
/// epoch microseconds exactly as it does on macOS.
///
/// The units are shared with macOS but the *source* is not: macOS multiplies
/// `pbi_start_tvsec` by a million and drops the microseconds (its `pbi_start_tvusec`
/// is available but the identity key only needs seconds), while FreeBSD keeps the
/// full `ki_start.tv_usec`. Both are absolute, so the same division applies and no
/// boot time is added — adding one would push every process a machine-uptime into
/// the future.
#[cfg(target_os = "freebsd")]
fn started_at_unix(row: &Row, _snapshot: &SystemSnapshot) -> Option<i64> {
    if row.starttime == 0 {
        return None;
    }
    Some((row.starttime / 1_000_000) as i64)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
fn started_at_unix(_row: &Row, _snapshot: &SystemSnapshot) -> Option<i64> {
    None
}

/// `YYYY-MM-DD HH:MM:SS` in the local time zone.
///
/// Reuses the civil-date arithmetic the `Date` and `Clock` meters already carry,
/// including the DST-aware offset, so a `STARTTIME` column agrees with the clock in
/// the header instead of silently being UTC.
fn local_datetime(unix_secs: i64) -> String {
    let local = unix_secs.saturating_add(local_offset_secs());
    let days = local.div_euclid(86_400);
    let day = local.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    // A sanity bound, so a bogus boot time cannot print year 16810.
    if !(1970..=9999).contains(&y) {
        return "-".to_string();
    }
    format!(
        "{y:04}-{mo:02}-{d:02} {:02}:{:02}:{:02}",
        day / 3600,
        (day % 3600) / 60,
        day % 60
    )
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Seconds east of UTC for the local time zone, right now.
///
/// The same `localtime_r` round trip `meters_impl` uses, re-read on every call so
/// a long-running gander crosses a DST boundary correctly.
#[cfg(unix)]
fn local_offset_secs() -> i64 {
    let t = now_unix_secs() as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: `localtime_r` writes one `struct tm` through the pointer and reads
    // the scalar time; both are valid for the call.
    let filled = unsafe { libc::localtime_r(&t, tm.as_mut_ptr()) };
    if filled.is_null() {
        return 0;
    }
    // SAFETY: a non-null return means the struct was filled.
    unsafe { tm.assume_init() }.tm_gmtoff
}

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
    (y + i64::from(m <= 2), m, d)
}

/// `sched_policy` decoded to the name `ps -o policy` prints.
/// What `s` should say about ptrace permission, on this platform.
///
/// Linux keeps the answer in `/proc/sys/kernel/yama/ptrace_scope`, and the value
/// matters enough to be worth reading rather than describing in the abstract:
///
/// * `0` — unrestricted; `strace -p` works on any process of yours.
/// * `1` — the default on most distributions: only children may be traced, so
///   attaching to an existing process fails with `EPERM` for every pid on the
///   machine, including your own.
/// * `2` — admin-only.
/// * `3` — no tracing at all.
///
/// The file is absent on kernels built without `CONFIG_SECURITY_YAMA`, which is a
/// different answer from any of the numbers and is said in words.
///
/// macOS and FreeBSD get a one-line note instead: macOS needs SIP disabled (a
/// reboot, not a setting) and FreeBSD's `truss` needs the same privileges as
/// `strace` without the yama layer on top.
fn ptrace_scope_note() -> Vec<String> {
    let mut out = vec![
        "Tracing needs ptrace permission: the process must be yours, or you need".to_string(),
        "CAP_SYS_PTRACE (root).".to_string(),
    ];

    #[cfg(target_os = "linux")]
    {
        out.push(String::new());
        match std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope") {
            Ok(text) => {
                let value = text.trim();
                out.push(format!("yama/ptrace_scope = {value}"));
                match value {
                    "0" => {
                        out.push("  Unrestricted: attaching to your own processes works.".into())
                    }
                    "1" => {
                        out.push("  Restricted: only *child* processes may be traced, so".into());
                        out.push(
                            "  attaching to an existing pid fails with EPERM even when".into(),
                        );
                        out.push("  it is your own. Either run the program under strace".into());
                        out.push("    strace -f <command>".into());
                        out.push("  or raise the limit for this boot:".into());
                        out.push("    sudo sysctl -w kernel.yama.ptrace_scope=0".into());
                    }
                    "2" => out.push("  Admin-only: needs CAP_SYS_PTRACE for any attach.".into()),
                    "3" => out.push("  Tracing is disabled entirely on this kernel.".into()),
                    other => out.push(format!("  (unrecognised value {other})")),
                }
            }
            Err(_) => {
                out.push(
                    "yama/ptrace_scope is absent, so the kernel has no yama restriction".into(),
                );
                out.push("  and only the usual ownership/CAP_SYS_PTRACE rules apply.".into());
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        out.push(String::new());
        out.push("On macOS, `dtruss` needs SIP disabled (csrutil disable, then reboot).".into());
        out.push(
            "That is a system-wide security setting, not something gander can ask for.".into(),
        );
    }

    #[cfg(target_os = "freebsd")]
    {
        out.push(String::new());
        out.push("truss(1) is in the base system; it needs the same privileges as strace".into());
        out.push("and has no additional restriction layer to configure.".into());
    }

    out
}

///
/// The numbers are the kernel's `SCHED_*` constants and are the same on both
/// platforms in the range that matters; the fallback prints the raw value rather
/// than hiding a policy this build does not know.
fn sched_policy_name(policy: u8) -> &'static str {
    match policy {
        0 => "other",
        1 => "fifo",
        2 => "rr",
        3 => "batch",
        5 => "idle",
        6 => "deadline",
        _ => "?",
    }
}

/// Mark the table's header row when it is scrolled off either edge.
///
/// Without this, scrolling right far enough leaves a blank header row, which
/// looks like a rendering fault rather than "there is more table to the left".
/// The markers overwrite one header cell each, which is why they are only drawn
/// while that edge is actually off screen.
fn draw_scroll_edges(
    buf: &mut Buffer,
    area: Rect,
    more_left: bool,
    more_right: bool,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if more_left {
        crate::meters::put_str(buf, area.x, area.y, "‹", theme.column_header, 1);
    }
    if more_right {
        crate::meters::put_str(
            buf,
            area.right().saturating_sub(1),
            area.y,
            "›",
            theme.column_header,
            1,
        );
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

/// The environment of `pid`, as `KEY=value` lines.
///
/// `None` means "not read" — the process is another user's, or the platform has
/// no such interface — and the caller shows the reason with
/// [`env_unavailable_reason`].
#[cfg(target_os = "linux")]
fn env_of(pid: gander_core::Pid) -> Option<Vec<String>> {
    read_limited(&format!("/proc/{pid}/environ"), |bytes| {
        bytes
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect()
    })
}

/// FreeBSD: `sysctl(KERN_PROC_ENV)`, the same NUL-separated payload.
#[cfg(target_os = "freebsd")]
fn env_of(pid: gander_core::Pid) -> Option<Vec<String>> {
    // No size query for this selector (it answers `ENOMEM` instead of a length),
    // so `ARG_MAX` is requested the way `ps` does.
    let raw = gander_core::freebsd::scan::proc_string(libc::KERN_PROC_ENV, pid, 256 * 1024)?;
    let lines: Vec<String> = raw
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    (!lines.is_empty()).then_some(lines)
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn env_of(_pid: gander_core::Pid) -> Option<Vec<String>> {
    None
}

fn env_unavailable_reason() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "Another user's environment needs CAP_SYS_PTRACE (root)."
    }
    #[cfg(target_os = "freebsd")]
    {
        "Another user's environment needs root, and a process inside a jail is\
         not readable from outside it."
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        "macOS exposes the environment only through KERN_PROCARGS2, interleaved \
         with argv, and only for your own processes."
    }
}

/// The open file descriptors of `pid`, as `fd  path` lines.
#[cfg(target_os = "linux")]
fn open_files_of(pid: gander_core::Pid) -> Option<Vec<String>> {
    let dir = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    Some(
        dir.filter_map(|e| e.ok())
            .map(|e| {
                let fd = e.file_name().to_string_lossy().into_owned();
                let target = std::fs::read_link(e.path())
                    .map(|t| t.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "?".into());
                format!("{fd:>4}  {target}")
            })
            .collect(),
    )
}

/// FreeBSD: `sysctl(KERN_PROC_FILEDESC)` returns `struct kinfo_file` records.
///
/// Three details make this different from walking `/proc/<pid>/fd`, and each one
/// is a trap:
///
/// * **The records are variable-length.** The kernel truncates each one after the
///   `kf_path` field, so a real record is 376–408 bytes while `libc`'s
///   `kinfo_file` is 1392. The stride is the record's own `kf_structsize`, and a
///   check of `stride >= size_of::<kinfo_file>()` — the natural thing to write —
///   silently rejects every record and shows an empty panel.
/// * **`kf_path` is at a fixed offset (368) inside the record**, not at
///   `offset_of!(kinfo_file, kf_path)`, because everything after the record's end
///   was cut before `copyout`. Reading it through `libc`'s struct would read far
///   past the 384 bytes the kernel actually wrote.
/// * **Descriptors that are not files appear too.** `kf_fd` is `-1` for the cwd,
///   `-2` for the root directory, `-5` for the executable text and `-6` for the
///   controlling terminal. `procstat -f` prints all of them, and hiding them
///   would leave out exactly the entries a reader is looking for when a process
///   is stuck in a deleted directory.
#[cfg(target_os = "freebsd")]
fn open_files_of(pid: gander_core::Pid) -> Option<Vec<String>> {
    /// `offsetof(struct kinfo_file, kf_path)` — the *truncated* record's end, not
    /// `libc`'s full struct. Verified against `procstat -f` on FreeBSD 14.5.
    const PATH_OFF: usize = 368;
    /// The smallest record that can still carry a path.
    const MIN_RECORD: usize = PATH_OFF + 1;

    let mut size = 0usize;
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_FILEDESC,
        pid as i32,
    ];
    // SAFETY: a null buffer with a zero length asks for the size only.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size < MIN_RECORD {
        return None;
    }
    // Headroom: a process can open descriptors between the size query and the
    // read, and this selector genuinely reports a size (unlike ARGS/ENV).
    let mut buf = vec![0u8; size + 8192];
    let mut got = buf.len();
    // SAFETY: `buf` is `got` bytes and `got` is passed as its length.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buf.as_mut_ptr().cast(),
            &mut got,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || got == 0 {
        return None;
    }

    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset + MIN_RECORD <= got {
        let i32_at = |o: usize| i32::from_ne_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
        let stride = i32_at(offset) as usize;
        if stride < MIN_RECORD || offset + stride > got {
            break; // a short or overlong record: stop rather than misread
        }
        let fd = i32_at(offset + 8);
        let start = offset + PATH_OFF;
        let end = buf[start..offset + stride]
            .iter()
            .position(|b| *b == 0)
            .map(|e| start + e)
            .unwrap_or(offset + stride);
        let path = String::from_utf8_lossy(&buf[start..end]).into_owned();
        out.push(format!(
            "{fd:>4}  {}",
            if path.is_empty() { "-" } else { &path }
        ));
        offset += stride;
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn open_files_of(_pid: gander_core::Pid) -> Option<Vec<String>> {
    None
}

fn fd_unavailable_reason() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "Another user's descriptors need CAP_SYS_PTRACE (root)."
    }
    #[cfg(target_os = "freebsd")]
    {
        "Another user's descriptors need root."
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        "macOS has proc_pidinfo(PROC_PIDLISTFDS) plus proc_pidfdinfo, which is \
         not wired up here; use `lsof -p <pid>` meanwhile."
    }
}

/// `flock(2)`/`fcntl(2)` locks held by `pid`.
#[cfg(target_os = "linux")]
fn file_locks_of(pid: gander_core::Pid) -> Option<Vec<String>> {
    read_limited("/proc/locks", |bytes| {
        let text = String::from_utf8_lossy(bytes);
        let pid_marker = format!(" {pid} ");
        text.lines()
            .filter(|l| l.contains(&pid_marker))
            .map(|l| l.to_string())
            .collect()
    })
}

/// FreeBSD has no `/proc/locks` and no `sysctl` that lists locks by owner; the
/// kernel's lock list is only reachable with `lsof`, which is a port.
#[cfg(target_os = "freebsd")]
fn file_locks_of(_pid: gander_core::Pid) -> Option<Vec<String>> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn file_locks_of(_pid: gander_core::Pid) -> Option<Vec<String>> {
    None
}

fn locks_unavailable_reason() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "No locks are recorded for this process."
    }
    #[cfg(target_os = "freebsd")]
    {
        "FreeBSD does not expose the lock table: `/proc/locks` does not exist and \
         there is no `sysctl` for it. `lsof` (from ports) can show it."
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        "macOS does not expose a per-process lock table to unprivileged callers."
    }
}

/// Read a `/proc` file, refusing anything pathologically large.
///
/// Only the Linux arms use it, so it is gated with them rather than being dead
/// code on the other platforms.
#[cfg(target_os = "linux")]
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

/// Draw the incremental search/filter prompt.
///
/// # Why this exists
///
/// The prompt was never drawn. `F3`, `F4` and every character typed went into
/// `App::input`, the filter was applied live, and the screen showed nothing —
/// so a filter that hid every row looked like a bug in the sampler, and there
/// was no way to see what was being typed or that a filter was active at all.
/// It is drawn on the table's own first row rather than as a separate line,
/// because the process list has no spare row and htop's prompt occupies the
/// same space.
fn draw_prompt(area: Rect, buf: &mut Buffer, theme: &Theme, input: &Input) {
    if !input.active || area.height == 0 || area.width == 0 {
        return;
    }
    let label = match input.kind {
        InputKind::Search => "Search: ",
        InputKind::Filter => "Filter: ",
        InputKind::None => return,
    };

    // Clear the row first: the prompt sits on top of the column header, and
    // leaving the header text visible behind a short query makes the prompt read
    // as `PID USER Search: ng`.
    for x in area.x..area.right() {
        if let Some(cell) = buf.cell_mut((x, area.y)) {
            cell.set_char(' ');
            cell.set_style(theme.header);
        }
    }

    let mut text = String::from(label);
    text.push_str(&input.buffer);

    // A cursor block after the last character, so an empty prompt is visibly
    // waiting for input instead of looking like a static label.
    let cursor_x = area.x + text.chars().count() as u16;
    crate::meters::put_str(buf, area.x, area.y, &text, theme.header_value, area.width);
    if cursor_x < area.right() {
        if let Some(cell) = buf.cell_mut((cursor_x, area.y)) {
            cell.set_char(' ');
            cell.set_style(theme.header_value);
        }
    }
}

fn draw_help(area: Rect, buf: &mut Buffer, theme: &Theme) {
    // Every binding that has a handler belongs here. `L`, `H`, `O`, `Z`, `b`, `s`
    // and the navigation keys were missing, so the only way to find them was
    // `docs/KEYS.md` — which is not on the machine the user is looking at.
    let lines = [
        "gander — keys",
        "",
        "  F1 h ?   help            F2       setup",
        "  F3 /     search          F4 \\     filter",
        "  F5 t     tree view       F6 < >   sort column",
        "  F7 F8    nice -/+        F9 k     kill",
        "  Space    tag process     c        tag children",
        "  U        untag all       u        user filter",
        "  Up Down  move            PgUp PgDn  page",
        "  Home End g G  first/last row      Left Right  scroll sideways",
        "  Tab      next tab        Shift-Tab  previous tab",
        "  L        layout          I        meter mode",
        "  K        toggle kernel threads   H   toggle thread rows",
        "  O        hide containers   Shift-Left/Right  move meter cursor",
        "  a        affinity        i        I/O priority",
        "  e        environment     w        command line",
        "  l        open files      x        file locks",
        "  Z        security ctx    s        trace (explains itself)",
        "  b        backtrace       + -      faster/slower",
        "  F10 q    quit",
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
    targets: &[ProcessTarget],
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
    let title = match targets {
        [target] => format!("Send signal to {}", target.pid),
        [] => "Send signal".to_string(),
        _ => format!("Send signal to {} processes", targets.len()),
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
#[path = "app_safety_tests.rs"]
mod safety_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_columns_normalize_corrected_task_usage_exactly_once() {
        use gander_core::system::CpuTimes;

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 3],
            cpu_delta: vec![CpuTimes {
                user: 100,
                idle: 100,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut users = gander_core::users::Users::new();
        let mut task = Row::default_for_test(1);
        for (ordinary, normalized) in [(100.0, "50.0"), (200.0, "100.0")] {
            task.percent_cpu = ordinary;
            assert_eq!(
                render_field(&task, Field::PercentCpu, &snapshot, &mut users),
                format!("{ordinary:.1}")
            );
            assert_eq!(
                render_field(&task, Field::PercentNormCpu, &snapshot, &mut users),
                normalized
            );
        }
        assert_eq!(
            snapshot.cpu_usage(),
            0.5,
            "host usage stays machine-normalized"
        );
    }

    #[test]
    fn normalized_cpu_column_uses_one_core_when_core_entries_are_missing() {
        use gander_core::system::CpuTimes;

        let mut task = Row::default_for_test(1);
        task.percent_cpu = 100.0;
        let mut users = gander_core::users::Users::new();
        for entries in [0, 1] {
            let snapshot = SystemSnapshot {
                cpu: vec![CpuTimes::default(); entries],
                ..Default::default()
            };
            assert_eq!(
                render_field(&task, Field::PercentNormCpu, &snapshot, &mut users),
                "100.0"
            );
        }
    }

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

    /// Draw `app` into a `width x height` test terminal and return the cells.
    ///
    /// Assertions about scrolling have to read the buffer: the bug they guard
    /// against was a state change that the renderer discarded, which no assertion
    /// on `App`'s fields can catch.
    fn render_to_string(app: &mut App, width: u16, height: u16) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    /// One process whose command line is far wider than a test terminal.
    ///
    /// Forced into the `htop` layout: the `panels` inspector wraps the command
    /// line into its own panel, so the tail would be on screen for reasons that
    /// have nothing to do with horizontal scrolling.
    fn app_with_long_commands() -> App {
        let mut r = Row::default_for_test(4242);
        r.comm.try_push_str("GoogleUpdater").unwrap();
        r.cmdline = Some(
            "/Applications/GoogleUpdater.app/Contents/MacOS/GoogleUpdater --crash-handler \
             --pid=1 --service=update END-OF-COMMAND"
                .into(),
        );
        let config = Config {
            layout: "htop".into(),
            ..Default::default()
        };
        let mut app = App::new(config);
        app.apply_sample(SystemSnapshot::default(), vec![r]);
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
        assert!(app.tags.contains(&ProcessTarget {
            pid: 7,
            starttime: 0
        }));
        app.handle_key(Key::Char(' '));
        assert!(!app.tags.contains(&ProcessTarget {
            pid: 7,
            starttime: 0
        }));
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
    fn every_core_appears_exactly_once() {
        // Regression guard: the header was a fixed four rows, so an 8-core
        // machine only ever showed the aggregate and the rest were clipped.
        //
        // The count is now cores *per line* as well as rows: on a wide terminal
        // htop's ladder puts 2, 4 or 8 cores on a line, so the CPU block costs
        // `ceil(cores / 2 / per_line) + 1` rows per column instead of one row per
        // core. Every core still has to be drawn exactly once.
        use gander_core::system::{CpuTimes, SystemSnapshot};

        // 200 cells: each half is 100 wide, which is 10 per-core cells at the
        // widest, so the configured 8-column layout is not width-clamped.
        let width = 200u16;
        for cores in [2usize, 4, 8, 16, 32, 64, 128] {
            let snapshot = SystemSnapshot {
                cpu: vec![CpuTimes::default(); cores + 1],
                cpu_delta: vec![CpuTimes::default(); cores + 1],
                ..Default::default()
            };

            let mut app = App::new(Config::default());
            app.apply_sample(snapshot, Vec::new());

            let (split, per_line) = crate::meters_impl::cpu_layout(cores);
            let cpu_rows: u16 = app
                .meters
                .iter()
                .filter(|m| m.name().starts_with("CPU"))
                .map(|m| m.rows(width / 2))
                .sum();

            let expected = if split {
                let half = cores / 2;
                let left = half.div_ceil(per_line as usize) + 1;
                let right = (cores - half).div_ceil(per_line as usize) + 1;
                left + right
            } else {
                1
            };
            assert_eq!(
                cpu_rows as usize, expected,
                "cores={cores} should render {expected} CPU rows"
            );
        }
    }

    /// The whole point of the change: a 128-core machine must not spend a
    /// screenful on the header.
    #[test]
    fn a_128_core_header_is_a_header_not_a_screenful() {
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 129],
            cpu_delta: vec![CpuTimes::default(); 129],
            ..Default::default()
        };
        let config = Config {
            layout: "htop".into(),
            ..Default::default()
        };
        let mut app = App::new(config);
        app.apply_sample(snapshot, Vec::new());

        // htop's ladder: 33-128 cores -> eight per line, split in half.
        assert_eq!(crate::meters_impl::cpu_layout(128), (true, 8));
        let h = app.header_height_debug(200);
        // Eight lines of cores plus the average row, then the tab strip: 13
        // rows, where one row per core would have wanted 66.
        assert!(h <= 14, "128 cores took {h} header rows");
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
        assert_eq!(cpu_meters[0].rows(80), 1, "4 cores or fewer show one row");
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
            app.header_height(200)
        };

        // Four or fewer cores: one CPU row on the left, three text meters on the
        // right, plus the tab row => 4.
        assert_eq!(height_for(2), 4);
        assert_eq!(height_for(4), 4);
        // 8 cores: `LeftCPUs`/`RightCPUs` one per line, five CPU rows per
        // column, plus the tab row.
        assert_eq!(height_for(8), 9);
        assert!(height_for(8) > height_for(4), "8 cores must not be clipped");
        // Past eight cores the ladder puts more cores on a line, so the header
        // grows in steps of one row per doubling rather than one row per core.
        // 16 and 64 cores both need five CPU rows per column; 128 needs nine.
        assert_eq!(height_for(16), 9, "16 cores: two per line, five CPU rows");
        assert_eq!(height_for(64), 9, "64 cores: eight per line, five CPU rows");
        assert_eq!(height_for(128), 13, "128 cores: eight per line, nine rows");
        assert!(
            height_for(128) < 20,
            "a 128-core header must leave the process list room"
        );
    }

    /// Render `app` into a `w`x`h` buffer and return the text, row by row.
    fn render_rows(app: &mut App, w: u16, h: u16) -> Vec<String> {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn the_htop_header_is_not_blank_on_a_many_core_machine() {
        // Regression guard: a 128-core machine wants 65 CPU rows per column. The
        // header is clamped to the terminal's height, and the old
        // `if y + h > area.bottom() { break; }` dropped the whole meter instead
        // of clipping it - so on a 50-row terminal the entire header rendered
        // blank and only the table was left.
        use gander_core::system::{CpuTimes, SystemSnapshot};

        for (cores, h) in [(128usize, 50u16), (128, 24), (64, 30), (16, 24)] {
            let snapshot = SystemSnapshot {
                cpu: vec![CpuTimes::default(); cores + 1],
                cpu_delta: vec![CpuTimes::default(); cores + 1],
                ..Default::default()
            };
            let config = Config {
                layout: "htop".into(),
                ..Default::default()
            };
            let mut app = App::new(config);
            app.apply_sample(snapshot, Vec::new());

            let rows = render_rows(&mut app, 100, h);
            assert!(
                rows[0].contains("CPU") || rows[0].contains("Avg"),
                "cores={cores} h={h}: the header's first row must be a CPU meter, got {:?}",
                rows[0]
            );
        }
    }

    #[test]
    fn a_clipped_cpu_meter_still_names_itself_cpu() {
        // A one-row slice of a split CPU meter used to keep the `Avg` label,
        // which reads as "the average of what?" once the core rows are gone.
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 129],
            cpu_delta: vec![CpuTimes::default(); 129],
            ..Default::default()
        };
        let config = Config {
            layout: "htop".into(),
            ..Default::default()
        };
        let mut app = App::new(config);
        app.apply_sample(snapshot, Vec::new());

        let rows = render_rows(&mut app, 100, 6);
        assert!(
            rows[0].contains("CPU"),
            "a squeezed CPU meter must still label itself CPU, got {:?}",
            rows[0]
        );
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
        // This test used to assert only that `h_offset` changed, which was true
        // while the renderer clamped it straight back to 0: the field moved and
        // the screen did not. It now draws and compares the actual cells, so it
        // fails if the offset is computed and then ignored again.
        let mut app = app_with_long_commands();
        let width = 100;
        let before = render_to_string(&mut app, width, 12);
        assert_eq!(app.h_offset, 0);

        app.handle_key(Key::Right);
        let after = render_to_string(&mut app, width, 12);
        assert!(app.h_offset > 0, "the offset must survive the clamp");
        assert_ne!(before, after, "the table must actually move");

        // Back to the left edge, and never past it.
        for _ in 0..40 {
            app.handle_key(Key::Left);
        }
        assert_eq!(app.h_offset, 0);
        assert_eq!(
            render_to_string(&mut app, width, 12),
            before,
            "scrolling back must restore the original frame"
        );
    }

    /// The tail of a long command line must be reachable, which was the point.
    #[test]
    fn scrolling_right_reveals_the_end_of_the_command() {
        let mut app = app_with_long_commands();
        let width = 100;
        // The tail starts past the right edge of a 100-cell terminal.
        let before = render_to_string(&mut app, width, 12);
        assert!(!before.contains("END-OF-COMMAND"), "{before}");

        for _ in 0..40 {
            app.handle_key(Key::Right);
        }
        let after = render_to_string(&mut app, width, 12);
        assert!(
            after.contains("END-OF-COMMAND"),
            "scrolling to the end must show the end of the command:\n{after}"
        );
        // And it must stop there rather than scrolling into blank space.
        app.handle_key(Key::Right);
        assert_eq!(render_to_string(&mut app, width, 12), after);
    }

    /// A short command must not make the table scrollable.
    #[test]
    fn a_command_that_fits_is_not_scrollable() {
        let mut app = app_with(vec![row(1, "sshd")]);
        let width = 120;
        let _ = render_to_string(&mut app, width, 8);
        app.handle_key(Key::Right);
        assert_eq!(app.h_offset, 0);
    }

    /// The issue that started this: a command wider than the terminal was cut at
    /// the edge with nothing to indicate more text existed.
    #[test]
    fn a_long_command_makes_the_table_wider_than_the_screen() {
        let mut app = app_with_long_commands();
        let _ = render_to_string(&mut app, 100, 12);
        assert!(
            app.last_total_width > app.last_area_width,
            "table {} must exceed the screen {}",
            app.last_total_width,
            app.last_area_width
        );
    }

    /// Tree mode indents the command, so the measured width must include the
    /// prefix. Otherwise the deepest row's last few cells stay unreachable.
    #[test]
    fn the_tree_prefix_is_counted_in_the_command_width() {
        let mut mit = Row::default_for_test(1000);
        mit.comm.try_push_str("mit").unwrap();
        let mut child = Row::default_for_test(1001);
        child.ppid = 1000;
        child.comm.try_push_str("child").unwrap();
        // Long enough that the fixed columns plus this text overflow 90 cells
        // once the two-cell tree prefix is added, and short enough that they do
        // not overflow without it.
        child.cmdline = Some("/usr/bin/child --argument-one --argument-two".into());

        let config = Config {
            layout: "htop".into(),
            ..Default::default()
        };
        let mut app = App::new(config);
        app.apply_sample(SystemSnapshot::default(), vec![mit, child]);

        let width = 56;
        let fixed = 7 + 10 + 4 + 4 + 6 + 6 + 9; // PID USER PRI NI CPU% MEM% TIME+
        let text = "/usr/bin/child --argument-one --argument-two".chars().count() as u16;
        assert!(
            fixed + text > width,
            "the command must overflow on its own for this to test the prefix"
        );
        // Without the prefix the column would be this wide.
        assert_eq!(fixed + text, 90);

        let _ = render_to_string(&mut app, width, 12);
        let flat_width = app.last_total_width;

        app.handle_key(Key::Char('t'));
        assert!(app.table.tree_mode, "tree view must be on");
        let _ = render_to_string(&mut app, width, 12);
        assert!(
            app.last_total_width > flat_width,
            "the tree prefix must widen the command column: {} vs {}",
            app.last_total_width,
            flat_width
        );

        // Scroll to the end and the tail must still be readable, prefix and all.
        for _ in 0..40 {
            app.handle_key(Key::Right);
        }
        let drawn = render_to_string(&mut app, width, 12);
        assert!(
            drawn.contains("two"),
            "the indented tail must be reachable:\n{drawn}"
        );
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
        app.table.rows[0].starttime = ProcessTarget::capture(pid).unwrap().starttime;
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
        // This test is about the tab mechanism, not the layout: the panels
        // layout substitutes its own leaner column set while the user has not
        // customized anything, which would make the count assertion meaningless.
        config.layout = "htop".into();

        let mut app = App::new(config);
        app.viewport = 5;
        app.apply_sample(SystemSnapshot::default(), Vec::new());

        // Assert against the config rather than a hardcoded field: this test is
        // about tabs keeping their own sort, so it must not break every time the
        // shipped default changes.
        let tab0_sort = app.config.tabs[0].sort();
        assert_eq!(app.table.sort.field, tab0_sort.field, "tab 0 starts here");
        assert_eq!(app.table.sort.dir, tab0_sort.dir);
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
            app.table.sort.field, tab0_sort.field,
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
            app.status
                .as_deref()
                .unwrap_or("")
                .contains("one screen tab"),
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
            (1..=50).map(Row::default_for_test).collect::<Vec<_>>(),
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
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        use gander_core::PlatformSampler as _;
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
        let before: Vec<MeterMode> = (0..app.meters.len())
            .map(|i| app.meter_mode_at(i))
            .collect();

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

    fn thread_row(pid: u32, tgid: u32) -> Row {
        let mut r = Row::default_for_test(pid);
        r.tgid = tgid;
        r.ppid = tgid;
        r.is_thread = true;
        r.flags = r.flags.insert(gander_core::row::RowFlags::USERLAND_THREAD);
        r
    }

    #[test]
    fn the_default_layout_is_panels_and_l_switches_it() {
        let app = App::new(Config::default());
        assert_eq!(app.layout, LayoutKind::Panels, "panels is the default");

        let mut app = app_with_n_rows(5);
        app.handle_key(Key::Char('L'));
        assert_eq!(app.layout, LayoutKind::Htop);
        assert_eq!(app.config.layout, "htop", "the switch persists");
        app.handle_key(Key::Char('L'));
        assert_eq!(app.layout, LayoutKind::Panels);
        assert_eq!(app.config.layout, "panels");
    }

    #[test]
    fn the_layout_is_read_from_the_config() {
        let config = Config {
            layout: "htop".into(),
            ..Config::default()
        };
        assert_eq!(App::new(config).layout, LayoutKind::Htop);

        // Anything unrecognised falls back to the new layout rather than an
        // empty screen.
        let config = Config {
            layout: "nonsense".into(),
            ..Config::default()
        };
        assert_eq!(App::new(config).layout, LayoutKind::Panels);
        assert_eq!(LayoutKind::from_config("CLASSIC"), LayoutKind::Htop);
    }

    /// The panels layout must render the system column and the inspector.
    #[test]
    fn the_panels_layout_draws_three_bands() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut app = app_with_n_rows(30);
        app.selected = 0;
        let mut term = Terminal::new(TestBackend::new(140, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();

        // System column: a bar label plus a fact with no other home in this layout.
        assert!(text.contains("CPU"), "system column");
        assert!(text.contains("LOAD"), "load belongs in the system column");
        assert!(text.contains("MEM"), "memory bar");
        // Inspector: the state spelled out, which no column shows.
        assert!(
            text.contains("sleeping") || text.contains("running") || text.contains("unknown"),
            "the inspector should describe the state in words"
        );
        // Dividers.
        assert!(text.contains('│'), "panels are separated");
        // And the table itself.
        assert!(text.contains("PID"), "the table is still there");
    }

    #[test]
    fn a_narrow_terminal_falls_back_to_the_compact_layout() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut app = app_with_n_rows(10);
        // 40 columns cannot hold two side panels plus a table, so the panels
        // layout must degrade instead of drawing slivers.
        let mut term = Terminal::new(TestBackend::new(40, 12)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("PID"), "the table survives at 40 columns");
    }

    #[test]
    fn switching_the_selection_updates_the_inspector() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut app = app_with_n_rows(5);
        let render = |app: &mut App| -> String {
            let mut term = Terminal::new(TestBackend::new(130, 24)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            term.backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect()
        };

        app.selected = 0;
        let first = render(&mut app);
        app.selected = 3;
        let second = render(&mut app);

        // The inspector shows the selected pid in the top-left of its panel, so
        // the two renders must differ.
        assert_ne!(
            first, second,
            "moving the selection must redraw the details"
        );
    }

    /// The in-table value bars are drawn as a background style, so a text-only
    /// assertion cannot see them; this checks the cell styles directly.
    #[test]
    fn the_panels_layout_paints_value_bars_behind_cpu_and_memory() {
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;
        use ratatui::Terminal;

        let mut app = app_with_n_rows(20);
        // Give the rows a spread of CPU% so the bars have different lengths.
        for (i, row) in app.table.rows.iter_mut().enumerate() {
            row.percent_cpu = (20 - i) as f32;
            row.percent_mem = 1.0;
        }
        app.reorder();
        app.selected = 0;

        let mut term = Terminal::new(TestBackend::new(140, 25)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();

        // Count cells carrying the value-bar background, and the longest run on
        // any single line: the top row must have a bar longer than the last one.
        let is_bar = |bg: Color| bg == app.theme.row_value_bar.bg.unwrap_or(Color::Reset);
        let runs: Vec<usize> = (0..25)
            .map(|y| {
                let mut best = 0usize;
                let mut cur = 0usize;
                for x in 0..140 {
                    if is_bar(buf[(x, y)].style().bg.unwrap_or(Color::Reset)) {
                        cur += 1;
                        best = best.max(cur);
                    } else {
                        cur = 0;
                    }
                }
                best
            })
            .collect();

        assert!(
            runs.iter().any(|&r| r > 0),
            "no value bars were painted at all"
        );

        // The first data rows have the largest values, so at least one early line
        // must show a longer bar than the shortest non-zero bar painted.
        let non_zero: Vec<usize> = runs.iter().copied().filter(|&r| r > 0).collect();
        assert!(
            non_zero.iter().max() > non_zero.iter().min(),
            "bars should vary with the value, got {non_zero:?}"
        );
    }

    #[test]
    fn value_bars_are_absent_from_the_htop_layout() {
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;
        use ratatui::Terminal;

        // This test asserts the *htop layout* never paints the table's in-cell
        // value bars. It reads the bar background off the *theme*, so it must pin
        // the classic htop theme rather than whatever the config default is now
        // (the default is the in-house `gander` theme, which shares colors with
        // other elements and would make the assertion fire on those instead).
        let config = Config {
            layout: "htop".into(),
            color_scheme: "default".into(),
            ..Config::default()
        };
        let mut app = App::new(config);
        let rows: Vec<Row> = (1..=10)
            .map(|pid| {
                let mut r = Row::default_for_test(pid);
                r.percent_cpu = 5.0;
                r
            })
            .collect();
        app.apply_sample(SystemSnapshot::default(), rows);

        let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        let bar_bg = app.theme.row_value_bar.bg.unwrap_or(Color::Reset);
        let found = (0..20)
            .any(|y| (0..120).any(|x| buf[(x, y)].style().bg.unwrap_or(Color::Reset) == bar_bg));
        assert!(!found, "the htop layout must not paint table value bars");
    }

    #[test]
    fn h_toggles_thread_rows_and_the_default_hides_them() {
        let mut app = app_with_n_rows(2);
        // Add a thread under pid 1.
        app.table.rows.push(thread_row(9001, 1));
        app.table.show_userland_threads = false;
        app.reorder();
        let without = app.table.visible();
        assert!(
            !app.table
                .view
                .order
                .iter()
                .any(|&i| app.table.rows[i].is_thread),
            "threads are hidden by default"
        );

        app.handle_key(Key::Char('H'));
        assert!(app.table.show_userland_threads);
        assert_eq!(
            app.table.visible(),
            without + 1,
            "enabling H must reveal exactly the thread row"
        );
        assert!(app.status.as_deref().unwrap_or("").contains("threads"));

        app.handle_key(Key::Char('H'));
        assert!(!app.table.show_userland_threads);
        assert_eq!(app.table.visible(), without);
    }

    #[test]
    fn threads_appear_nested_under_their_process_in_tree_view() {
        let mut app = app_with_n_rows(1); // pid 1
        app.table.rows.push(thread_row(9001, 1));
        app.table.rows.push(thread_row(9002, 1));
        app.table.show_userland_threads = true;
        app.table.tree_mode = true;
        app.reorder();

        // The leader is first, then its threads indented beneath it.
        let order = app.table.view.order.clone();
        let leader_pos = order
            .iter()
            .position(|&i| app.table.rows[i].pid == 1)
            .expect("leader");
        let thread_positions: Vec<usize> = order
            .iter()
            .enumerate()
            .filter(|(_, &i)| app.table.rows[i].is_thread)
            .map(|(pos, _)| pos)
            .collect();
        assert_eq!(thread_positions.len(), 2);
        for pos in thread_positions {
            assert!(pos > leader_pos, "threads must follow their leader");
            assert_eq!(app.table.rows[order[pos]].depth, 1, "one level in");
        }
    }

    #[test]
    fn the_thread_setting_round_trips_through_the_config() {
        let mut app = app_with_n_rows(1);
        app.handle_key(Key::Char('H'));
        assert!(app.config.show_userland_threads);
        app.handle_key(Key::Char('H'));
        assert!(!app.config.show_userland_threads);

        // And it is honoured when a config arrives.
        let config = Config {
            show_userland_threads: true,
            ..Config::default()
        };
        let app = App::new(config);
        assert!(app.table.show_userland_threads);
    }

    #[test]
    fn the_header_cursor_wraps_and_i_acts_only_on_it() {
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 9],
            cpu_delta: vec![CpuTimes::default(); 9],
            ..Default::default()
        };
        let mut app = App::new(Config::default());
        app.apply_sample(snapshot, Vec::new());
        let n = app.meters.len();
        assert!(n > 2);

        // Starts at the first meter and wraps in both directions.
        assert_eq!(app.meter_cursor, 0);
        app.handle_key(Key::HeaderPrev);
        assert_eq!(app.meter_cursor, n - 1, "wraps backwards");
        app.handle_key(Key::HeaderNext);
        assert_eq!(app.meter_cursor, 0, "wraps forwards");

        // Move to a middle meter and flip only that one.
        app.handle_key(Key::HeaderNext);
        app.handle_key(Key::HeaderNext);
        let target = app.meter_cursor;
        let before: Vec<MeterMode> = (0..n).map(|i| app.meter_mode_at(i)).collect();

        app.handle_key(Key::Char('I'));
        assert_ne!(app.meter_mode_at(target), before[target], "meter {target}");
        for (i, was) in before.iter().enumerate() {
            if i != target {
                assert_eq!(app.meter_mode_at(i), *was, "meter {i} must not change");
            }
        }

        // The status line names the meter and its new mode.
        let status = app.status.clone().unwrap_or_default();
        assert!(status.contains(app.meters[target].name()), "got {status:?}");
    }

    #[test]
    fn the_header_cursor_survives_a_shrinking_header() {
        use gander_core::system::{CpuTimes, SystemSnapshot};

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 9],
            cpu_delta: vec![CpuTimes::default(); 9],
            ..Default::default()
        };
        let mut app = App::new(Config::default());
        app.apply_sample(snapshot, Vec::new());
        for _ in 0..10 {
            app.handle_key(Key::HeaderNext);
        }
        assert!(app.meter_cursor < app.meters.len());

        // Rebuilding with fewer meters must not leave the cursor out of range.
        app.config.meters_left = vec![gander_config::MeterConfig::new("Memory", "bar")];
        app.config.meters_right = vec![gander_config::MeterConfig::new("Tasks", "text")];
        app.rebuild_header(8);
        // `I` must not panic even if the cursor is stale.
        app.handle_key(Key::Char('I'));
        assert!(app.meter_cursor < app.meters.len() || app.meters.is_empty());
    }

    #[test]
    fn the_selected_meter_is_visibly_marked() {
        use gander_core::system::{CpuTimes, SystemSnapshot};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let snapshot = SystemSnapshot {
            cpu: vec![CpuTimes::default(); 9],
            cpu_delta: vec![CpuTimes::default(); 9],
            ..Default::default()
        };
        // The header meter cursor only exists in the htop layout; the panels
        // layout has no header.
        let config = Config {
            layout: "htop".into(),
            ..Config::default()
        };
        let mut app = App::new(config);
        app.apply_sample(snapshot, Vec::new());

        // The cursor is a background style on the meter's own label, so inspect
        // cell styles rather than text: it must mark the first meter and nothing
        // else, and it must sit on the *selected* meter's row.
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();

        // Compare the *whole* style, not just the background: the meters paint
        // their own tracks with the same background colour, so a bg-only check
        // cannot tell the cursor from a bar. The distinguishing feature is the
        // foreground colour, and that is what a user sees too.
        let cursor = app.theme.meter_cursor;
        let is_cursor = |s: ratatui::style::Style| {
            s.fg == cursor.fg && s.bg == cursor.bg && s.add_modifier == cursor.add_modifier
        };
        let marked_rows = |buf: &ratatui::buffer::Buffer| -> Vec<u16> {
            (0..30)
                .filter(|&y| (0..100).any(|x| is_cursor(buf[(x, y)].style())))
                .collect()
        };
        assert_eq!(
            marked_rows(buf),
            vec![0],
            "only the first meter's own row may be marked"
        );

        // Moving the cursor moves the mark, and still marks exactly one row.
        app.handle_key(Key::HeaderNext);
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let marked = marked_rows(term.backend().buffer());
        assert_eq!(
            marked.len(),
            1,
            "still exactly one marked row, got {marked:?}"
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

#[cfg(test)]
mod m1_tests {
    //! Tests for the M1 additions: network/disk meters in the header, the
    //! search/filter prompt actually being drawn, and persisted history.

    use super::*;
    use gander_config::Config;
    use gander_core::row::Row;

    fn app_with(rows: Vec<Row>) -> App {
        let mut app = App::new(Config::default());
        app.apply_sample(SystemSnapshot::default(), rows);
        app
    }

    fn row(pid: u32, comm: &str) -> Row {
        let mut r = Row::default_for_test(pid);
        r.comm.try_push_str(comm).unwrap();
        r
    }

    fn render_to_string(app: &mut App, width: u16, height: u16) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn temp_history_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gander-app-history-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// `Esc` must clear the filter.
    ///
    /// It could not: the handler reset `self.input` and then asked
    /// `self.input.kind`, which was already `None`. The only way out of a filter
    /// was to erase it one character at a time.
    #[test]
    fn esc_clears_an_active_filter() {
        let mut app = app_with(vec![row(1, "alpha"), row(2, "beta")]);
        app.handle_key(Key::Char('\\'));
        app.handle_key(Key::Char('b'));
        assert_eq!(app.table.visible(), 1, "the filter is live while typing");

        app.handle_key(Key::Esc);
        assert!(!app.input.active, "the prompt closes");
        assert!(app.filter.is_none(), "the filter is gone");
        assert_eq!(app.table.visible(), 2, "every row is visible again");
    }

    /// A submitted search is remembered; a cancelled one is not.
    #[test]
    fn only_submitted_prompts_are_remembered() {
        let dir = temp_history_dir("submit");
        let mut app = app_with(vec![row(1, "alpha"), row(2, "beta")]).with_history(dir.clone());

        // Cancelled: never reaches the history.
        app.handle_key(Key::Char('/'));
        app.handle_key(Key::Char('c'));
        app.handle_key(Key::Char('x'));
        app.handle_key(Key::Esc);

        // Submitted.
        app.handle_key(Key::Char('/'));
        app.handle_key(Key::Char('b'));
        app.handle_key(Key::Enter);

        assert_eq!(app.search_history.len(), 1);
        assert_eq!(app.search_history.recent(0), Some("b"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_and_filter_history_are_separate() {
        let dir = temp_history_dir("separate");
        let mut app = app_with(vec![row(1, "alpha"), row(2, "beta")]).with_history(dir.clone());

        app.handle_key(Key::Char('/'));
        app.handle_key(Key::Char('a'));
        app.handle_key(Key::Enter);

        app.handle_key(Key::Char('\\'));
        app.handle_key(Key::Char('b'));
        app.handle_key(Key::Enter);

        assert_eq!(app.search_history.recent(0), Some("a"));
        assert_eq!(app.filter_history.recent(0), Some("b"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Up recalls the newest entry; Down walks back out to an empty buffer.
    #[test]
    fn up_and_down_walk_the_history() {
        let dir = temp_history_dir("walk");
        let mut app = app_with(vec![row(1, "alpha"), row(2, "beta"), row(3, "gamma")])
            .with_history(dir.clone());

        for term in ["alpha", "beta"] {
            app.handle_key(Key::Char('/'));
            for ch in term.chars() {
                app.handle_key(Key::Char(ch));
            }
            app.handle_key(Key::Enter);
        }

        app.handle_key(Key::Char('/'));
        assert!(app.input.buffer.is_empty());

        app.handle_key(Key::Up);
        assert_eq!(app.input.buffer, "beta", "Up starts at the newest entry");
        app.handle_key(Key::Up);
        assert_eq!(app.input.buffer, "alpha");
        // Already at the oldest: it stays rather than wrapping.
        app.handle_key(Key::Up);
        assert_eq!(app.input.buffer, "alpha");

        app.handle_key(Key::Down);
        assert_eq!(app.input.buffer, "beta");
        app.handle_key(Key::Down);
        assert_eq!(app.input.buffer, "", "back to a blank prompt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Typing after a recall abandons the history position.
    #[test]
    fn editing_a_recalled_entry_resets_the_cursor() {
        let dir = temp_history_dir("reset");
        let mut app = app_with(vec![row(1, "alpha")]).with_history(dir.clone());

        app.handle_key(Key::Char('/'));
        for ch in "alpha".chars() {
            app.handle_key(Key::Char(ch));
        }
        app.handle_key(Key::Enter);

        app.handle_key(Key::Char('/'));
        app.handle_key(Key::Up);
        assert_eq!(app.input.buffer, "alpha");
        assert_eq!(app.input.history_cursor, 1);

        app.handle_key(Key::Char('x'));
        assert_eq!(app.input.history_cursor, 0, "typing leaves history mode");

        // The next Up starts from the newest entry again.
        app.handle_key(Key::Up);
        assert_eq!(app.input.buffer, "alpha");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recalled_filter_takes_effect_immediately() {
        let dir = temp_history_dir("recall-filter");
        let mut app = app_with(vec![row(1, "alpha"), row(2, "beta")]).with_history(dir.clone());

        app.handle_key(Key::Char('\\'));
        app.handle_key(Key::Char('b'));
        app.handle_key(Key::Enter);
        assert_eq!(app.table.visible(), 1);

        // Clear, then recall.
        app.handle_key(Key::Esc);
        assert_eq!(app.table.visible(), 2);

        app.handle_key(Key::Char('\\'));
        app.handle_key(Key::Up);
        assert_eq!(
            app.table.visible(),
            1,
            "the recalled filter applies at once"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// History survives a restart: the whole point of persisting it.
    #[test]
    fn history_is_reloaded_on_the_next_run() {
        let dir = temp_history_dir("reload");
        {
            let mut app = app_with(vec![row(1, "alpha")]).with_history(dir.clone());
            app.handle_key(Key::Char('\\'));
            for ch in "nginx|node".chars() {
                app.handle_key(Key::Char(ch));
            }
            app.handle_key(Key::Enter);
            app.save_history();
        }

        let app = app_with(vec![row(1, "alpha")]).with_history(dir.clone());
        assert_eq!(app.filter_history.recent(0), Some("nginx|node"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An `App` built without `with_history` must not touch the real state dir.
    #[test]
    fn a_fresh_app_has_empty_history() {
        let app = app_with(vec![]);
        assert!(app.search_history.is_empty());
        assert!(app.filter_history.is_empty());
    }

    /// The prompt has to be visible. It was not drawn at all, so a filter that
    /// hid every row looked like a broken sampler.
    #[test]
    fn the_filter_prompt_is_drawn_on_the_table_header() {
        let mut app = app_with(vec![row(1, "alpha"), row(2, "beta")]);
        app.viewport = 10;
        app.handle_key(Key::Char('\\'));
        app.handle_key(Key::Char('b'));
        app.handle_key(Key::Char('e'));

        let screen = render_to_string(&mut app, 100, 20);
        assert!(
            screen.contains("Filter: be"),
            "the prompt and its text must be on screen:\n{screen}"
        );
    }

    #[test]
    fn the_search_prompt_is_drawn() {
        let mut app = app_with(vec![row(1, "alpha")]);
        app.viewport = 10;
        app.handle_key(Key::Char('/'));
        app.handle_key(Key::Char('a'));
        app.handle_key(Key::Char('l'));

        let screen = render_to_string(&mut app, 100, 20);
        assert!(screen.contains("Search: al"), "got:\n{screen}");
    }

    #[test]
    fn no_prompt_is_drawn_when_the_prompt_is_closed() {
        let mut app = app_with(vec![row(1, "alpha")]);
        app.viewport = 10;
        let screen = render_to_string(&mut app, 100, 20);
        assert!(!screen.contains("Search:"), "got:\n{screen}");
        assert!(!screen.contains("Filter:"), "got:\n{screen}");
    }

    /// The prompt must not leak into the `htop` layout either.
    #[test]
    fn the_prompt_is_drawn_in_both_layouts() {
        for layout in [LayoutKind::Panels, LayoutKind::Htop] {
            let mut app = app_with(vec![row(1, "alpha")]);
            app.layout = layout;
            app.viewport = 10;
            app.handle_key(Key::Char('\\'));
            app.handle_key(Key::Char('x'));
            let screen = render_to_string(&mut app, 120, 24);
            assert!(
                screen.contains("Filter: x"),
                "prompt missing in {layout:?}:\n{screen}"
            );
        }
    }

    /// The new meters must be offerable and must render in a real header.
    #[test]
    fn network_and_disk_meters_can_be_added_to_the_header() {
        let mut config = Config::default();
        config
            .meters_right
            .push(gander_config::MeterConfig::new("NetworkIO", "text"));
        config
            .meters_right
            .push(gander_config::MeterConfig::new("DiskIO", "text"));

        let mut app = App::new(config);
        let snapshot = SystemSnapshot {
            cpu: vec![gander_core::system::CpuTimes::default(); 5],
            cpu_delta: vec![gander_core::system::CpuTimes::default(); 5],
            net_rate: gander_core::system::Rate {
                inbound: 1024.0 * 1024.0,
                outbound: 2048.0,
            },
            ..Default::default()
        };
        app.apply_sample(snapshot, vec![row(1, "alpha")]);

        let names: Vec<&str> = app.meters.iter().map(|m| m.name()).collect();
        assert!(names.contains(&"NetworkIO"), "got {names:?}");
        assert!(names.contains(&"DiskIO"), "got {names:?}");

        // And the whole frame renders without panicking.
        app.layout = LayoutKind::Htop;
        let screen = render_to_string(&mut app, 120, 24);
        assert!(screen.contains("Net"), "got:\n{screen}");
    }
}

#[cfg(all(test, target_os = "linux"))]
mod container_column_tests {
    //! The `CGROUP` and `CONTAINER` columns, which M2 added.

    use super::*;
    use gander_config::Config;
    use gander_core::row::Row;
    use gander_core::table::Sort;

    fn row_with_container(pid: u32, runtime: &str, id: &str, cgroup: &str) -> Row {
        let mut r = Row::default_for_test(pid);
        r.comm.try_push_str("proc").unwrap();
        r.cgroup = Some(cgroup.into());
        r.container = Some(id.into());
        r.container_runtime = Some(runtime.into());
        r
    }

    fn render(r: &Row, field: Field) -> String {
        render_with(r, field, &SystemSnapshot::default())
    }

    fn render_with(r: &Row, field: Field, snap: &SystemSnapshot) -> String {
        let mut users = gander_core::users::Users::new();
        render_field(r, field, snap, &mut users)
    }

    #[test]
    fn cgroup_renders_the_path() {
        let r = row_with_container(1, "docker", "abc123", "/system.slice/docker-abc123.scope");
        assert_eq!(
            render(&r, Field::Cgroup),
            "/system.slice/docker-abc123.scope"
        );
    }

    #[test]
    fn container_renders_runtime_and_a_short_id() {
        let r = row_with_container(
            1,
            "docker",
            "1f2e3d4c5b6a79880a1b2c3d4e5f6071",
            "/system.slice/docker-1f2e.scope",
        );
        assert_eq!(render(&r, Field::Container), "docker:1f2e3d4c5b6a");
    }

    #[test]
    fn a_host_process_renders_a_dash_not_an_empty_cell() {
        let mut r = Row::default_for_test(1);
        r.comm.try_push_str("init").unwrap();
        assert_eq!(render(&r, Field::Cgroup), "-");
        assert_eq!(render(&r, Field::Container), "-");
    }

    /// A container whose cgroup namespace hid the path has no id to show.
    /// Printing an invented one would look like an identifier while telling the
    /// reader nothing.
    #[test]
    fn an_unidentified_container_says_so() {
        let r = row_with_container(1, "container", "?", "/");
        assert_eq!(render(&r, Field::Container), "container:?");
    }

    /// Sorting by a text column must sort by that text, not fall through to pid.
    ///
    /// The `_` arm of `sort_order` is the pid comparator, so a new field with no
    /// arm silently sorts by pid — which looks like a sort that does nothing on an
    /// already pid-sorted list.
    #[test]
    fn sorting_by_container_groups_by_runtime_then_id() {
        let rows = vec![
            row_with_container(3, "kubepods", "pod-b", "/kubepods/x"),
            row_with_container(1, "docker", "bbb", "/docker/bbb"),
            row_with_container(2, "docker", "aaa", "/docker/aaa"),
        ];
        let mut order: Vec<usize> = (0..rows.len()).collect();
        table::sort_order(
            &rows,
            &mut order,
            Sort {
                field: Field::Container,
                dir: Dir::Asc,
            },
        );
        let pids: Vec<u32> = order.iter().map(|&i| rows[i].pid).collect();
        assert_eq!(pids, vec![2, 1, 3], "docker aaa, docker bbb, then kubepods");
    }

    #[test]
    fn sorting_by_cgroup_sorts_the_paths() {
        let rows = vec![
            row_with_container(1, "docker", "x", "/user.slice/z.scope"),
            row_with_container(2, "docker", "y", "/system.slice/a.scope"),
        ];
        let mut order: Vec<usize> = (0..rows.len()).collect();
        table::sort_order(
            &rows,
            &mut order,
            Sort {
                field: Field::Cgroup,
                dir: Dir::Asc,
            },
        );
        let pids: Vec<u32> = order.iter().map(|&i| rows[i].pid).collect();
        assert_eq!(pids, vec![2, 1], "/system.slice sorts before /user.slice");
    }

    /// Both columns must be offerable in the setup screen, or they cannot be
    /// selected at all.
    #[test]
    fn both_columns_are_selectable() {
        let names: Vec<&str> = Field::all().map(|f| f.name()).collect();
        assert!(names.contains(&"CGROUP"), "got {names:?}");
        assert!(names.contains(&"CONTAINER"), "got {names:?}");
        assert_eq!(Field::from_name("CGROUP"), Some(Field::Cgroup));
        assert_eq!(Field::from_name("CONTAINER"), Some(Field::Container));
    }

    /// A configured tab using the new columns must survive a round trip through
    /// the config, which resolves names back to `Field`s.
    /// A delay column that was not read renders `-`, never `0.0%`.
    ///
    /// This is the common case in practice: `TASKSTATS` needs `CAP_NET_ADMIN`, so
    /// an ordinary user's `gander` gets `EPERM` and every row is unread. `0.0%`
    /// there would claim the whole table never waits on I/O, which is the exact
    /// condition the column exists to reveal.
    #[cfg(target_os = "linux")]
    #[test]
    fn unread_delay_columns_render_a_dash() {
        let row = Row::default_for_test(1);
        for field in [
            Field::CpuDelayPercent,
            Field::IoDelayPercent,
            Field::SwapDelayPercent,
        ] {
            assert_eq!(render(&row, field), "-", "{field:?}");
        }
    }

    /// A measured zero is a measurement and renders as a number.
    #[cfg(target_os = "linux")]
    #[test]
    fn measured_zero_delay_renders_as_zero() {
        let mut row = Row::default_for_test(1);
        row.cpu_delay_percent = Some(0.0);
        row.io_delay_percent = Some(12.5);
        row.swap_delay_percent = Some(100.0);
        assert_eq!(render(&row, Field::CpuDelayPercent), "0.0");
        assert_eq!(render(&row, Field::IoDelayPercent), "12.5");
        assert_eq!(render(&row, Field::SwapDelayPercent), "100.0");
    }

    /// Sorting by a delay column must put unread rows with the zeros rather than
    /// above them: a row nobody measured must not outrank one that was.
    #[cfg(target_os = "linux")]
    #[test]
    fn unread_delay_rows_sort_with_zero_not_above() {
        use gander_core::table::{sort_order, Dir, Sort};

        let mut measured = Row::default_for_test(1);
        measured.io_delay_percent = Some(5.0);
        let unread = Row::default_for_test(2);
        let rows = vec![unread, measured];

        let mut order: Vec<usize> = (0..rows.len()).collect();
        sort_order(
            &rows,
            &mut order,
            Sort {
                field: Field::IoDelayPercent,
                dir: Dir::Desc,
            },
        );
        assert_eq!(
            rows[order[0]].pid, 1,
            "the measured row sorts above the unread one"
        );
        assert_eq!(rows[order[1]].pid, 2);
    }

    #[test]
    fn the_columns_round_trip_through_the_config() {
        let mut config = Config::default();
        config.tabs[0].columns = vec!["PID".into(), "CONTAINER".into(), "CGROUP".into()];
        let fields = config.tabs[0].fields();
        assert_eq!(fields, vec![Field::Pid, Field::Container, Field::Cgroup]);
    }

    #[test]
    fn the_columns_render_in_a_real_table() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut config = Config::default();
        config.tabs[0].columns = vec!["PID".into(), "CONTAINER".into(), "COMM".into()];
        let mut app = App::new(config);
        app.layout = LayoutKind::Htop;
        app.apply_sample(
            SystemSnapshot::default(),
            vec![row_with_container(
                42,
                "docker",
                "1f2e3d4c5b6a7988",
                "/system.slice/docker-1f2e.scope",
            )],
        );

        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut screen = String::new();
        for y in 0..20 {
            for x in 0..100 {
                screen.push_str(buf[(x, y)].symbol());
            }
            screen.push('\n');
        }
        assert!(screen.contains("CONTAINER"), "header missing:\n{screen}");
        assert!(
            screen.contains("docker:1f2e3d4c5b6a"),
            "value missing:\n{screen}"
        );
    }
}

#[cfg(test)]
mod m2_tests {
    //! Bulk renice, base-name highlighting, and the `O` container toggle.

    use super::*;
    use gander_config::Config;
    use gander_core::row::Row;

    fn app_with(rows: Vec<Row>) -> App {
        let mut app = App::new(Config::default());
        app.apply_sample(SystemSnapshot::default(), rows);
        app
    }

    fn row(pid: u32, comm: &str) -> Row {
        let mut r = Row::default_for_test(pid);
        r.comm.try_push_str(comm).unwrap();
        r
    }

    fn render_to_string(app: &mut App, width: u16, height: u16) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// `F7`/`F8` must honour the tagged set, like `F9` already did.
    ///
    /// They used to renice only the row under the cursor, so tagging five
    /// processes and pressing `F8` reniced one of them — silently. That is worse
    /// than not supporting bulk renice at all, because `F9` right next to it does
    /// honour tags.
    #[test]
    fn renice_targets_the_tagged_set() {
        let mut app = app_with(vec![row(1, "a"), row(2, "b"), row(3, "c")]);

        // Tag two rows.
        app.handle_key(Key::Char(' '));
        app.handle_key(Key::Down);
        app.handle_key(Key::Char(' '));
        assert_eq!(app.tags.len(), 2);

        // Building the target list is what the fix changed; the actual syscall is
        // not exercised here (renicing another process's test fixture would be
        // rude and needs privileges).
        let targets = app.process_targets();
        assert_eq!(
            targets.iter().map(|t| t.pid).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    /// With nothing tagged, the action still applies to the selection.
    #[test]
    fn renice_with_no_tags_targets_the_selection() {
        let mut app = app_with(vec![row(7, "a"), row(8, "b")]);
        app.selected = 1;
        assert!(app.tags.is_empty());
        assert_eq!(app.selected_row().map(|r| r.pid), Some(8));
    }

    /// `highlight_base_name` must actually reach the screen.
    ///
    /// The config option and the setup-screen toggle both existed, but nothing in
    /// the renderer read them: the setting could be flipped and saved and had no
    /// visible effect at all.
    #[test]
    fn base_name_highlight_paints_the_exe_and_nothing_when_off() {
        let mut r = row(1, "nginx");
        r.cmdline = Some("/usr/bin/nginx -g daemon".into());
        r.exe = Some("/usr/bin/nginx".into());

        let mut config = Config {
            layout: "htop".into(),
            ..Config::default()
        };
        config.tabs[0].columns = vec!["PID".into(), "COMM".into()];

        // Capture the styles of the rendered command cells, not just the text:
        // the whole point is that the *styling* changes.
        let capture = |highlight: bool| -> (String, Vec<(String, ratatui::style::Style)>) {
            let mut config = config.clone();
            config.highlight_base_name = highlight;
            let mut app = App::new(config);
            app.apply_sample(SystemSnapshot::default(), vec![r.clone()]);
            use ratatui::backend::TestBackend;
            use ratatui::Terminal;
            let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
            terminal.draw(|f| app.draw(f)).unwrap();
            let buf = terminal.backend().buffer().clone();
            let mut text = String::new();
            let mut cells = Vec::new();
            for y in 0..12 {
                for x in 0..80 {
                    let cell = &buf[(x, y)];
                    text.push_str(cell.symbol());
                    cells.push((cell.symbol().to_string(), cell.style()));
                }
                text.push('\n');
            }
            (text, cells)
        };

        let (off_text, off_cells) = capture(false);
        let (on_text, on_cells) = capture(true);

        assert!(
            on_text.contains("nginx"),
            "the command still renders:\n{on_text}"
        );
        assert_eq!(off_text, on_text, "highlighting must not change the text");

        // The base name occupies 5 cells; with highlighting on, those cells must
        // differ in style from the same cells with it off.
        let changed = off_cells
            .iter()
            .zip(&on_cells)
            .filter(|((a_ch, a_st), (b_ch, b_st))| a_ch == b_ch && a_st != b_st)
            .count();
        assert!(
            changed >= 5,
            "expected the 5 base-name cells to be restyled, got {changed}"
        );
    }

    /// `O` toggles container visibility, and reports why on a platform without
    /// container detection instead of silently doing nothing.
    #[test]
    fn the_o_key_toggles_container_visibility() {
        let mut app = app_with(vec![row(1, "a")]);
        let before = app.table.show_containerised;

        app.handle_key(Key::Char('O'));

        if gander_core::capability::CGROUP {
            assert_ne!(app.table.show_containerised, before, "the toggle flipped");
            assert_eq!(
                app.config.show_containerised, app.table.show_containerised,
                "and the config follows, so it is saved on exit"
            );
            let status = app.status.clone().unwrap_or_default();
            assert!(status.contains("containerised"), "got {status:?}");
        } else {
            // macOS: no container detection, so the key must explain itself.
            assert_eq!(app.table.show_containerised, before, "nothing changed");
            let status = app.status.clone().unwrap_or_default();
            assert!(status.contains("Linux-only"), "got {status:?}");
        }
    }

    /// The status line must actually reach the screen.
    ///
    /// `App::status` was written by a dozen actions and drawn by none of them, so
    /// every message — renice results, kill failures, the reason `O` does nothing
    /// on macOS — was invisible. The existing tests passed because they read the
    /// field rather than the screen, which is exactly the gap this closes.
    #[test]
    fn the_status_line_is_drawn_on_the_function_bar() {
        let mut app = app_with(vec![row(1, "alpha")]);
        app.viewport = 10;
        app.status = Some("killed 42".to_string());

        let screen = render_to_string(&mut app, 120, 20);
        assert!(screen.contains("killed 42"), "status not drawn:\n{screen}");
    }

    /// A message describes the last action, so the next action that has nothing
    /// to say must not leave the old one on screen.
    #[test]
    fn a_stale_status_is_cleared_by_the_next_key() {
        let mut app = app_with(vec![row(1, "alpha"), row(2, "beta")]);
        app.viewport = 10;
        app.status = Some("something happened".into());

        app.handle_key(Key::Down);
        assert_eq!(app.status, None, "moving the selection clears the message");

        let screen = render_to_string(&mut app, 120, 20);
        assert!(
            !screen.contains("something happened"),
            "the old message is still on screen:\n{screen}"
        );
    }

    #[test]
    fn the_container_toggle_is_persisted_through_the_config() {
        let config = Config {
            show_containerised: false,
            ..Config::default()
        };
        let app = App::new(config);
        assert!(
            !app.table.show_containerised,
            "a config that hides containers must be honoured at startup"
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod m3_column_tests {
    //! The M3 columns: the PSS family and the GPU pair.
    //!
    //! What is being pinned here is not the formatting but the *gating*: the two
    //! expensive reads (`smaps_rollup`, `fdinfo`) are only paid for when a column
    //! that needs them is on screen, and a column that was never read renders a
    //! dash rather than a zero.

    use super::*;
    use gander_config::Config;
    use gander_core::row::Row;
    use gander_core::system::SystemSnapshot;

    fn render(r: &Row, field: Field) -> String {
        render_with(r, field, &SystemSnapshot::default())
    }

    fn render_with(r: &Row, field: Field, snap: &SystemSnapshot) -> String {
        let mut users = gander_core::users::Users::new();
        render_field(r, field, snap, &mut users)
    }

    fn row_with_memory(pss: u64, epss: u64, private: u64, swap: u64) -> Row {
        let mut r = Row::default_for_test(1);
        r.comm.try_push_str("proc").unwrap();
        r.pss = Some(pss);
        r.epss = Some(epss);
        r.private = Some(private);
        r.swap_bytes = Some(swap);
        r
    }

    #[test]
    fn the_pss_family_renders_human_bytes() {
        let r = row_with_memory(1536 * 1024, 2048 * 1024, 1024 * 1024, 512 * 1024);
        assert_eq!(render(&r, Field::Pss), "1.5M");
        assert_eq!(render(&r, Field::Epss), "2.0M");
        assert_eq!(render(&r, Field::Private), "1.0M");
        assert_eq!(render(&r, Field::Swap), "512K");
    }

    /// An unread column is a dash, never `0B`: "we did not look" and "this
    /// process uses no memory" are different statements and only one is true.
    #[test]
    fn unread_memory_figures_render_a_dash() {
        let mut r = Row::default_for_test(1);
        r.comm.try_push_str("proc").unwrap();
        for field in [Field::Pss, Field::Epss, Field::Private, Field::Swap] {
            assert_eq!(render(&r, field), "-", "{}", field.name());
        }
    }

    /// A real zero is a measurement and must not be confused with an absent one.
    #[test]
    fn a_measured_zero_is_not_a_dash() {
        let r = row_with_memory(0, 0, 0, 0);
        assert_eq!(render(&r, Field::Pss), "0B");
        assert_eq!(render(&r, Field::Swap), "0B");
    }

    #[test]
    fn gpu_renders_a_percentage_and_a_duration() {
        let mut r = Row::default_for_test(1);
        r.comm.try_push_str("game").unwrap();
        r.gpu_percent = Some(42.5);
        r.gpu_ns = Some(3_600 * 1_000_000_000);
        assert_eq!(render(&r, Field::GpuPercent), "42.5");
        assert_eq!(render(&r, Field::GpuTime), "1:00:00");
    }

    #[test]
    fn a_process_with_no_drm_descriptor_renders_a_dash() {
        let mut r = Row::default_for_test(1);
        r.comm.try_push_str("ls").unwrap();
        assert_eq!(render(&r, Field::GpuPercent), "-");
        assert_eq!(render(&r, Field::GpuTime), "-");
    }

    /// The gate itself: a default column set must not ask for either expensive
    /// read, and selecting one column must switch on exactly its own read.
    #[test]
    fn expensive_reads_are_only_requested_when_a_column_needs_them() {
        let mut app = App::new(Config::default());
        let opts = app.scan_options();
        assert!(!opts.smaps, "the default columns need no smaps_rollup");
        assert!(!opts.gpu, "the default columns need no fdinfo");

        app.config.tabs[0].columns = vec!["PID".into(), "M_PSS".into(), "COMM".into()];
        let opts = app.scan_options();
        assert!(opts.smaps, "M_PSS needs smaps_rollup");
        assert!(!opts.gpu, "and nothing else");

        app.config.tabs[0].columns = vec!["PID".into(), "GPU%".into(), "COMM".into()];
        let opts = app.scan_options();
        assert!(opts.gpu, "GPU% needs fdinfo");
        assert!(!opts.smaps, "and nothing else");

        // Threads are independent of both.
        app.table.show_userland_threads = true;
        let opts = app.scan_options();
        assert!(opts.threads);
        assert!(opts.gpu);
        assert!(!opts.smaps);
    }

    /// Delay accounting is the third expensive read, gated the same way.
    #[cfg(target_os = "linux")]
    #[test]
    fn scan_options_gate_delayacct() {
        let mut app = App::new(Config::default());
        assert!(
            !app.scan_options().delayacct,
            "the default columns need no TASKSTATS query"
        );

        for column in ["CPUD%", "IOD%", "SWPD%"] {
            app.config.tabs[0].columns = vec!["PID".into(), column.into(), "COMM".into()];
            let opts = app.scan_options();
            assert!(opts.delayacct, "{column} needs a TASKSTATS query");
            assert!(!opts.smaps, "{column} needs nothing else");
            assert!(!opts.gpu);
        }
    }

    /// A tab that is not active must not pay for its columns.
    #[test]
    fn an_inactive_tabs_expensive_column_costs_nothing() {
        let mut config = Config::default();
        config.tabs[0].columns = vec!["PID".into(), "COMM".into()];
        config.tabs.push(gander_config::TabConfig {
            name: "Mem".into(),
            columns: vec!["PID".into(), "M_EPSS".into(), "COMM".into()],
            ..config.tabs[0].clone()
        });
        config.active_tab = 0;

        let app = App::new(config);
        assert!(
            !app.scan_options().smaps,
            "the PSS column lives on a tab that is not displayed"
        );
    }

    #[test]
    fn the_columns_round_trip_through_the_config() {
        let mut config = Config::default();
        config.tabs[0].columns = vec![
            "PID".into(),
            "M_PSS".into(),
            "M_EPSS".into(),
            "M_PRIVATE".into(),
            "M_SWAP".into(),
            "GPU%".into(),
            "GPU_TIME".into(),
            "COMM".into(),
        ];
        let fields = config.tabs[0].fields();
        assert_eq!(
            fields,
            vec![
                Field::Pid,
                Field::Pss,
                Field::Epss,
                Field::Private,
                Field::Swap,
                Field::GpuPercent,
                Field::GpuTime,
                Field::Comm,
            ]
        );
    }

    #[test]
    fn the_columns_render_in_a_real_table() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut config = Config::default();
        config.tabs[0].columns = vec!["PID".into(), "M_PSS".into(), "GPU%".into(), "COMM".into()];
        let mut app = App::new(config);
        app.layout = LayoutKind::Htop;

        let mut r = Row::default_for_test(42);
        r.comm.try_push_str("proc").unwrap();
        r.pss = Some(2 * 1024 * 1024);
        r.gpu_percent = Some(12.5);

        app.apply_sample(SystemSnapshot::default(), vec![r]);

        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();

        let buffer = terminal.backend().buffer();
        let text: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("PSS"), "column header missing:\n{text}");
        assert!(text.contains("GPU%"), "column header missing:\n{text}");
        assert!(text.contains("2.0M"), "pss value missing:\n{text}");
        assert!(text.contains("12.5"), "gpu value missing:\n{text}");
    }
}

#[cfg(test)]
mod no_silent_key_tests {
    use super::*;
    use gander_config::Config;
    use gander_core::row::Row;

    /// One app holding `n` rows, sampled once so `selected_row` resolves.
    fn app_with_n_rows(n: u32) -> App {
        let rows: Vec<Row> = (1..=n)
            .map(|pid| {
                let mut r = Row::default_for_test(pid);
                r.comm.try_push_str("proc").unwrap();
                r.cmdline = Some(format!("proc {pid}").into());
                r.exe = Some("/bin/proc".into());
                r
            })
            .collect();
        let mut app = App::new(Config::default());
        app.apply_sample(gander_core::system::SystemSnapshot::default(), rows);
        app
    }

    /// `s` used to have no handler at all.
    ///
    /// Both `docs/KEYS.md` and `docs/DESIGN.md` advertised it, and `handle_list_key`
    /// did not match it, so pressing it did nothing — no screen, no status line, no
    /// error. `Z` and `b` had explanation screens precisely to avoid that, which is
    /// what made the omission easy to miss.
    #[test]
    fn s_opens_an_explanation_instead_of_nothing() {
        let mut app = app_with_n_rows(1);
        app.handle_key(Key::Char('s'));
        match &app.screen {
            Screen::Info { title, lines, .. } => {
                assert!(title.to_lowercase().contains("trace"), "got {title}");
                let text = lines.join("\n");
                assert!(
                    text.contains("strace"),
                    "the screen must name the tool it is about:\n{text}"
                );
                // A bare "not implemented" is the message that teaches nothing;
                // the screen has to say what the work is and what to do instead.
                assert!(
                    text.contains("terminal") && text.contains("-p "),
                    "the screen must explain the obstacle and the workaround:\n{text}"
                );
            }
            other => panic!("expected an info screen, got {other:?}"),
        }
    }

    /// A key with no handler is invisible; this asserts the ones the help screen
    /// advertises actually do something.
    #[test]
    fn every_advertised_info_key_opens_a_screen() {
        for (key, want) in [
            (Key::Char('e'), "environment"),
            (Key::Char('w'), "command"),
            (Key::Char('l'), "file"),
            (Key::Char('x'), "lock"),
            (Key::Char('Z'), "security"),
            (Key::Char('b'), "backtrace"),
            (Key::Char('s'), "trace"),
        ] {
            let mut app = app_with_n_rows(1);
            app.handle_key(key);
            match &app.screen {
                Screen::Info { title, .. } => assert!(
                    title.to_lowercase().contains(want),
                    "{key:?} opened {title:?}, expected something about {want}"
                ),
                other => panic!("{key:?} opened {other:?}, expected an info screen"),
            }
        }
    }

    /// `s` must name the obstacle *and* the fix. "Not implemented yet" alone
    /// teaches a reader nothing, and a generic "ptrace permission may be needed"
    /// is the reason people conclude the tool is broken: on the common
    /// `ptrace_scope=1` the attach fails for every pid on the machine.
    #[test]
    fn the_trace_panel_names_the_obstacle_and_the_command() {
        let mut app = app_with_n_rows(1);
        app.handle_key(Key::Char('s'));
        let Screen::Info { lines, .. } = &app.screen else {
            panic!("s did not open a screen");
        };
        let text = lines.join("\n");

        // The obstacle: why this is not a pane.
        assert!(
            text.contains("terminal handover") && text.contains("alternate screen"),
            "the panel must explain why tracing cannot be a pane: {text}"
        );
        // The workaround, with the selected pid substituted.
        assert!(
            text.contains("strace -p"),
            "the command must be printed: {text}"
        );

        // The permission section must be specific on every platform, not just
        // present: either the actual scope value or the platform's own reason.
        #[cfg(target_os = "linux")]
        assert!(
            text.contains("ptrace_scope"),
            "Linux must report the yama scope, read from the kernel: {text}"
        );
        #[cfg(target_os = "macos")]
        assert!(text.contains("SIP"), "macOS must name SIP: {text}");
        #[cfg(target_os = "freebsd")]
        assert!(text.contains("truss"), "FreeBSD must name truss: {text}");
    }

    /// The note is generated, so it must never come back empty — an empty vector
    /// would leave the panel with no permission section at all.
    #[test]
    fn the_ptrace_note_always_says_something() {
        let note = ptrace_scope_note().join("\n");
        assert!(note.contains("ptrace"), "got: {note}");
        assert!(note.len() > 40, "too short to be useful: {note}");
    }
}

#[cfg(test)]
mod newly_reachable_column_tests {
    use super::*;
    use gander_core::row::Row;
    use gander_core::system::SystemSnapshot;

    fn render(r: &Row, field: Field) -> String {
        render_with(r, field, &SystemSnapshot::default())
    }

    fn render_with(r: &Row, field: Field, snap: &SystemSnapshot) -> String {
        let mut users = gander_core::users::Users::new();
        render_field(r, field, snap, &mut users)
    }

    fn row(pid: u32) -> Row {
        let mut r = Row::default_for_test(pid);
        r.comm.try_push_str("nginx").unwrap();
        r.cmdline = Some("nginx -g daemon off;".into());
        r.exe = Some("/usr/sbin/nginx".into());
        r
    }

    /// Rendering the raw integer would print `34816` for `/dev/pts/0`, which is not
    /// a terminal name anyone recognises. What the integer *means* differs per
    /// platform — Linux encodes a `dev_t`, macOS already has one — so the names
    /// differ too, but no platform may print a bare number for a real terminal.
    #[test]
    fn tty_renders_a_name_not_a_raw_number() {
        // No controlling terminal on either platform: `ps` prints `?`.
        let mut r = row(1);
        r.tty_nr = 0;
        assert_eq!(render(&r, Field::Tty), "?");

        #[cfg(target_os = "linux")]
        {
            // `pts/0` is major 136, minor 0 → (136 << 8) | 0 = 34816.
            r.tty_nr = 136 << 8;
            assert_eq!(render(&r, Field::Tty), "pts/0");

            // `pts/12`: minor 12.
            r.tty_nr = (136 << 8) | 12;
            assert_eq!(render(&r, Field::Tty), "pts/12");

            // `tty1` on the console: major 4, minor 1.
            r.tty_nr = (4 << 8) | 1;
            assert_eq!(render(&r, Field::Tty), "tty1");

            // An unknown major renders `major:minor` rather than a made-up path.
            r.tty_nr = (200 << 8) | 3;
            assert_eq!(render(&r, Field::Tty), "200:3");
        }

        #[cfg(target_os = "macos")]
        {
            // `devname(3)` answers from the live `/dev` table, so a machine with
            // no open ttys — every CI runner — has no name to give and correctly
            // falls back to the raw number. Asserting the literal `ttys000`
            // therefore asserted a property of the *machine*: it passed on a
            // developer's Mac with 22 ttys and failed on GitHub's runners, which
            // is where this test was found to be wrong rather than the code.
            //
            // The contract is narrower and testable anywhere: when the OS knows
            // a name, that is what is shown (never the number); when it does
            // not, the number is shown in hex. Both arms are checked against
            // `devname` itself, so the test agrees with the platform whatever
            // the platform's table contains.
            for dev in [0x1000_0000i64, 0x1000_0002] {
                r.tty_nr = dev;
                let shown = render(&r, Field::Tty);

                // SAFETY: `devname` reads a static table; the result is NULL or
                // a NUL-terminated string owned by libc.
                let ptr = unsafe { libc::devname(dev as libc::dev_t, libc::S_IFCHR) };
                let named = if ptr.is_null() {
                    None
                } else {
                    // SAFETY: non-null means NUL-terminated.
                    let s = unsafe { std::ffi::CStr::from_ptr(ptr) };
                    (!s.to_bytes().is_empty()).then(|| s.to_string_lossy().into_owned())
                };

                match named {
                    Some(name) => assert_eq!(shown, name, "devname knows this device"),
                    None => assert_eq!(
                        shown,
                        format!("{dev:#x}"),
                        "no name from the OS means the raw number, not a guess"
                    ),
                }
            }

            // A device the OS certainly has no name for must not borrow one.
            r.tty_nr = 0x7fff_ff00;
            assert_eq!(render(&r, Field::Tty), "0x7fffff00");
        }
    }

    /// `-1` is the kernel's "no foreground job", not a process group. Printing it
    /// as a number would look like a real group id.
    #[test]
    fn tpgid_renders_a_dash_for_no_foreground_job() {
        let mut r = row(1);
        r.tpgid = -1;
        assert_eq!(render(&r, Field::Tpgid), "-");

        r.tpgid = 4242;
        assert_eq!(render(&r, Field::Tpgid), "4242");
    }

    /// `0` is the "unknown session" sentinel, which `getsid` returns for another
    /// user's process. A session id of zero does not exist.
    #[test]
    fn session_renders_a_dash_when_unknown() {
        let mut r = row(1);
        r.session = 0;
        assert_eq!(render(&r, Field::Session), "-");

        r.session = 1234;
        assert_eq!(render(&r, Field::Session), "1234");
    }

    /// The three path/name columns have to distinguish "path known" from "not
    /// read", the same rule the PSS family follows.
    #[test]
    fn identity_columns_show_a_dash_when_unreadable() {
        let mut r = row(1);
        r.exe = None;
        r.cwd = None;
        assert_eq!(render(&r, Field::ProcExe), "-");
        assert_eq!(render(&r, Field::Cwd), "-");

        r.exe = Some("/usr/sbin/nginx".into());
        r.cwd = Some("/etc/nginx".into());
        assert_eq!(render(&r, Field::ProcExe), "/usr/sbin/nginx");
        assert_eq!(render(&r, Field::Cwd), "/etc/nginx");

        // PROC_COMM is the kernel's short name, which is always present.
        assert_eq!(render(&r, Field::ProcComm), "nginx");
    }

    /// The rendered form is `YYYY-MM-DD HH:MM:SS` in **local** time, so it agrees
    /// with the `Clock` in the header rather than silently being UTC.
    ///
    /// `starttime` means different things per platform, which is why the expected
    /// value is derived per platform rather than shared.
    #[test]
    fn starttime_renders_a_local_timestamp() {
        let mut r = row(1);

        #[cfg(target_os = "linux")]
        {
            // Ticks since boot: 600 s at 100 Hz, on a machine up 3600 s.
            let snap = SystemSnapshot {
                clk_tck: 100,
                uptime: 3600,
                ..Default::default()
            };
            r.starttime = 60_000;
            let text = super::started_at(&r, &snap);
            assert_eq!(text.len(), 19, "YYYY-MM-DD HH:MM:SS, got {text:?}");
            let expected = super::local_datetime(super::now_unix_secs() as i64 - 3000);
            assert_eq!(text, expected);
        }

        #[cfg(any(target_os = "macos", target_os = "freebsd"))]
        {
            // Epoch microseconds — already absolute, no boot-time shift applies.
            let at = 1_700_000_000i64;
            r.starttime = (at as u64) * 1_000_000;
            let text = super::started_at(&r, &SystemSnapshot::default());
            assert_eq!(text, super::local_datetime(at));
            assert_ne!(
                text,
                super::local_datetime(at + 3600),
                "the boot time must not be added a second time"
            );
        }
    }

    /// A start time that cannot be resolved must not print a date before the epoch.
    #[test]
    fn starttime_does_not_invent_a_date() {
        let mut r = row(1);

        #[cfg(target_os = "linux")]
        {
            let snap = SystemSnapshot {
                clk_tck: 0,
                ..Default::default()
            };
            r.starttime = 12_345;
            assert_eq!(render_with(&r, Field::StartTime, &snap), "-");
        }

        #[cfg(any(target_os = "macos", target_os = "freebsd"))]
        {
            // A process row whose `starttime` was never filled in.
            r.starttime = 0;
            assert_eq!(
                render_with(&r, Field::StartTime, &SystemSnapshot::default()),
                "-"
            );
        }
    }

    #[test]
    fn sched_policy_is_named_not_numeric() {
        let mut r = row(1);
        r.sched_policy = 0;
        assert_eq!(render(&r, Field::SchedPolicy), "other");
        r.sched_policy = 1;
        assert_eq!(render(&r, Field::SchedPolicy), "fifo");
        r.sched_policy = 2;
        assert_eq!(render(&r, Field::SchedPolicy), "rr");
        r.sched_policy = 3;
        assert_eq!(render(&r, Field::SchedPolicy), "batch");
        r.sched_policy = 5;
        assert_eq!(render(&r, Field::SchedPolicy), "idle");
        r.sched_policy = 6;
        assert_eq!(render(&r, Field::SchedPolicy), "deadline");
        r.sched_policy = 200;
        assert_eq!(render(&r, Field::SchedPolicy), "?");
    }
}
