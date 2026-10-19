//! The process table: scan, cache, derive, sort, filter.
//!
//! The central design point is the index indirection: sorting and filtering move
//! only `Vec<usize>`, never the `Row` storage. See `docs/DESIGN.md` §6.

use std::collections::HashMap;

use crate::row::{Row, RowFlags};
use crate::system::SystemSnapshot;
use crate::tree;
use crate::Pid;

/// Data that never changes for a live process, keyed by `(pid, starttime)`.
///
/// Caching it is the single largest syscall reduction available;
/// `docs/DESIGN.md` §3.2.
#[derive(Default)]
pub struct Cache {
    hits: HashMap<(Pid, u64), Stable>,
}

/// The cached identity of one process.
#[derive(Clone, Default, Debug)]
pub struct Stable {
    pub uid: u32,
    pub cmdline: Option<Box<str>>,
    pub exe: Option<Box<str>>,
    /// The process's working directory.
    ///
    /// Cached alongside `exe` rather than read per sample: it costs a `readlink`
    /// per process, and it is display-only — nothing sorts or filters on it by
    /// default. The cost is one syscall per process per *lifetime*, not per tick.
    pub cwd: Option<Box<str>>,
    /// Login session id.
    ///
    /// Cached because it needs a syscall of its own (`getsid`) on macOS, where
    /// `proc_bsdinfo` does not carry it. Linux reads it from field 4 of
    /// `/proc/<pid>/stat`, which the scan already has, so its value does not come
    /// from here — but keeping one field for both platforms means the two backends
    /// cannot drift in what `SESSION` means.
    ///
    /// `0` is "unknown", which is what `getsid` yields for another user's process
    /// (`EPERM`). It renders `-`.
    pub session: u32,
    /// Process group and the terminal's foreground group, so a thread row can
    /// inherit them instead of re-asking the kernel per thread.
    ///
    /// `tpgid` is `-1` when the terminal has no foreground job.
    pub pgrp: u32,
    pub tpgid: i64,
    /// The controlling terminal's device, in the platform's own encoding.
    ///
    /// Linux leaves this `0` because `tty_nr` arrives in the `stat` the scan
    /// already parses; macOS fills it from `e_tdev`, since `proc_bsdinfo` is a
    /// separate call per process and the thread pass should not repeat it.
    pub tty_dev: i64,
    /// Whether the executable has been replaced or deleted on disk.
    ///
    /// Linux exposes this in the `/proc/<pid>/exe` symlink target itself: once the
    /// file backing a running process is unlinked or replaced, the kernel appends
    /// ` (deleted)` to the target. That is the only signal available — comparing
    /// mtimes would need the path to still exist, which is exactly what deleting
    /// it removes.
    ///
    /// Cached with the identity because the flag cannot change without the
    /// process's exe being replaced, and re-reading the link per frame would be
    /// thousands of syscalls a second for a value that almost never changes.
    pub exe_deleted: bool,
    /// The cgroup *path*, not the raw `/proc/<pid>/cgroup` line.
    pub cgroup: Option<Box<str>>,
    /// The container's id, and the runtime that owns it.
    ///
    /// Cached here rather than recomputed per frame: classification is a few
    /// `find`s over a path, which is cheap once per process and wasteful
    /// thousands of times per second.
    #[cfg(target_os = "linux")]
    pub container: Option<Box<str>>,
    #[cfg(target_os = "linux")]
    pub container_runtime: Option<Box<str>>,
    pub is_kernel_thread: bool,
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, pid: Pid, starttime: u64) -> Option<&Stable> {
        self.hits.get(&(pid, starttime))
    }

    pub fn insert(&mut self, pid: Pid, starttime: u64, stable: Stable) {
        self.hits.insert((pid, starttime), stable);
    }

    /// Forget entries whose process is gone. Called once per tick with the set
    /// of live `(pid, starttime)` keys.
    pub fn retain_live(&mut self, live: &[(Pid, u64)]) {
        if self.hits.len() <= live.len() {
            return;
        }
        let keep: std::collections::HashSet<_> = live.iter().copied().collect();
        self.hits.retain(|k, _| keep.contains(k));
    }

    /// Drop everything. Called when the cache grows past a sane bound.
    pub fn clear(&mut self) {
        self.hits.clear();
    }

    pub fn len(&self) -> usize {
        self.hits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
}

/// Sort direction.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Dir {
    Asc,
    Desc,
}

/// A sort specification.
#[derive(Copy, Clone, Debug)]
pub struct Sort {
    pub field: crate::row::Field,
    pub dir: Dir,
}

impl Default for Sort {
    /// PID ascending, matching the shipped config default.
    ///
    /// Kept in step with `Config::default()` deliberately: a `ProcTable` built
    /// without an explicit sort (tests, and any future front end) should present
    /// the same stable order the TUI does, not silently fall back to CPU% and
    /// reintroduce the reshuffling this default exists to avoid.
    fn default() -> Self {
        Sort {
            field: crate::row::Field::Pid,
            dir: Dir::Asc,
        }
    }
}

/// The derived, displayable view over the row storage.
#[derive(Default)]
pub struct View {
    /// Indices into `rows`, already sorted and filtered.
    pub order: Vec<usize>,
}

/// Sort `order` in place using `field`, without touching `rows`.
pub fn sort_order(rows: &[Row], order: &mut [usize], sort: Sort) {
    // One comparison pass, so direction and tie-breaking are decided in one place.
    //
    // This used to be a `match` that sorted each field its own way, then a
    // `reverse()` of the whole vector for descending. Two problems came from that:
    //
    // 1. `reverse()` reversed every *tie* as well. On a quiet machine where dozens
    //    of processes sit at 0.0% CPU, "sort by CPU%, descending" produced
    //    10, 9, 8, … — a pid list running backwards, which reads as a bug rather
    //    than as "all of these are equal".
    //
    // 2. `sort_unstable_by` gives ties no guaranteed order, so equal rows could
    //    swap places between samples even without the reversal.
    //
    // Reversing the *comparison* rather than the vector, and keeping the pid
    // tie-break outside the reversal, fixes both: an equal group reads in
    // ascending pid order in either direction, and never shuffles.
    let descending = sort.dir == Dir::Desc;
    order.sort_unstable_by(|a, b| {
        let primary = compare_by(rows, sort.field, *a, *b);
        let primary = if descending { primary.reverse() } else { primary };
        primary.then_with(|| rows[*a].pid.cmp(&rows[*b].pid))
    });
}

/// Compare two rows by `field`, ascending. Used by the descending sort so the
/// reversal can be applied to the comparison instead of to the finished vector.
fn compare_by(rows: &[Row], field: crate::row::Field, a: usize, b: usize) -> std::cmp::Ordering {
    use crate::row::Field as F;
    match field {
        F::Pid => rows[a].pid.cmp(&rows[b].pid),
        F::Ppid => rows[a].ppid.cmp(&rows[b].ppid),
        F::Pgrp => rows[a].pgrp.cmp(&rows[b].pgrp),
        F::Comm => rows[a].comm.cmp(&rows[b].comm),
        F::User => rows[a].uid.cmp(&rows[b].uid),
        F::Priority => rows[a].priority.cmp(&rows[b].priority),
        F::Nice => rows[a].nice.cmp(&rows[b].nice),
        F::PercentCpu | F::PercentNormCpu => {
            rows[a].percent_cpu.total_cmp(&rows[b].percent_cpu)
        }
        F::PercentMem => rows[a].percent_mem.total_cmp(&rows[b].percent_mem),
        F::ResMem => rows[a].res.cmp(&rows[b].res),
        F::VirtMem => rows[a].virt.cmp(&rows[b].virt),
        F::Time => rows[a].time_ticks.cmp(&rows[b].time_ticks),
        F::Elapsed => rows[a].elapsed.cmp(&rows[b].elapsed),
        F::Nlwp => rows[a].nlwp.cmp(&rows[b].nlwp),
        F::MinFlt => rows[a].min_flt.cmp(&rows[b].min_flt),
        F::MajFlt => rows[a].maj_flt.cmp(&rows[b].maj_flt),
        // Identity and session fields. Each needs its own comparator: `_` is the
        // pid comparator, so a column without one sorts by pid while claiming to
        // sort by something else.
        F::Session => rows[a].session.cmp(&rows[b].session),
        F::Tty => rows[a].tty_nr.cmp(&rows[b].tty_nr),
        F::Tpgid => rows[a].tpgid.cmp(&rows[b].tpgid),
        F::StartTime => rows[a].starttime.cmp(&rows[b].starttime),
        F::Tgid => rows[a].tgid.cmp(&rows[b].tgid),
        F::SchedPolicy => rows[a].sched_policy.cmp(&rows[b].sched_policy),
        F::ProcComm => rows[a].comm.cmp(&rows[b].comm),
        // The path fields are `Option<Box<str>>`: `None` (another user's process)
        // sorts below any real path, the same rule the PSS family follows for a
        // figure that was never read.
        F::ProcExe => rows[a]
            .exe
            .as_deref()
            .unwrap_or("")
            .cmp(rows[b].exe.as_deref().unwrap_or("")),
        F::Cwd => rows[a]
            .cwd
            .as_deref()
            .unwrap_or("")
            .cmp(rows[b].cwd.as_deref().unwrap_or("")),
        F::Processor => rows[a].processor.cmp(&rows[b].processor),
        #[cfg(target_os = "linux")]
        F::Cgroup => rows[a]
            .cgroup
            .as_deref()
            .unwrap_or("")
            .cmp(rows[b].cgroup.as_deref().unwrap_or("")),
        #[cfg(target_os = "linux")]
        F::Container => {
            let key = |i: usize| {
                (
                    rows[i].container_runtime.as_deref().unwrap_or(""),
                    rows[i].container.as_deref().unwrap_or(""),
                )
            };
            key(a).cmp(&key(b))
        }
        // The PSS family. `None` (not read, or not readable) sorts below any real
        // figure in both directions, because an unknown cost is not a small one —
        // but the tie-break on pid keeps the placeholder rows in a stable order.
        //
        // Sorting by these is exactly when the sampler is told to read
        // `smaps_rollup`, so the column that orders the list is also the column
        // that pays for it.
        #[cfg(target_os = "linux")]
        F::Pss => rows[a]
            .pss
            .unwrap_or(0)
            .cmp(&rows[b].pss.unwrap_or(0)),
        #[cfg(target_os = "linux")]
        F::Epss => rows[a]
            .epss
            .unwrap_or(0)
            .cmp(&rows[b].epss.unwrap_or(0)),
        #[cfg(target_os = "linux")]
        F::Private => rows[a]
            .private
            .unwrap_or(0)
            .cmp(&rows[b].private.unwrap_or(0)),
        #[cfg(target_os = "linux")]
        F::Swap => rows[a]
            .swap_bytes
            .unwrap_or(0)
            .cmp(&rows[b].swap_bytes.unwrap_or(0)),
        #[cfg(target_os = "linux")]
        F::GpuPercent => rows[a]
            .gpu_percent
            .unwrap_or(0.0)
            .total_cmp(&rows[b].gpu_percent.unwrap_or(0.0)),
        #[cfg(target_os = "linux")]
        F::GpuTime => rows[a]
            .gpu_ns
            .unwrap_or(0)
            .cmp(&rows[b].gpu_ns.unwrap_or(0)),
        // Delay percentages. Unread (`None`) sorts as zero, which puts it with
        // processes that measured zero delay rather than above them: a row that
        // was never read must not outrank one that was.
        #[cfg(target_os = "linux")]
        F::CpuDelayPercent => rows[a]
            .cpu_delay_percent
            .unwrap_or(0.0)
            .total_cmp(&rows[b].cpu_delay_percent.unwrap_or(0.0)),
        #[cfg(target_os = "linux")]
        F::IoDelayPercent => rows[a]
            .io_delay_percent
            .unwrap_or(0.0)
            .total_cmp(&rows[b].io_delay_percent.unwrap_or(0.0)),
        #[cfg(target_os = "linux")]
        F::SwapDelayPercent => rows[a]
            .swap_delay_percent
            .unwrap_or(0.0)
            .total_cmp(&rows[b].swap_delay_percent.unwrap_or(0.0)),
        _ => rows[a].pid.cmp(&rows[b].pid),
    }
}

/// Case-insensitive fixed-string filter, `|`-separated, matching htop's F4.
#[derive(Clone, Debug)]
pub struct Filter {
    terms: Vec<String>,
}

impl Filter {
    pub fn parse(pattern: &str) -> Option<Self> {
        let terms: Vec<String> = pattern
            .split('|')
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if terms.is_empty() {
            None
        } else {
            Some(Filter { terms })
        }
    }

    /// A row matches if **any** term is a substring of its command line.
    pub fn matches(&self, row: &Row) -> bool {
        let haystack = row.display_command().to_ascii_lowercase();
        self.terms.iter().any(|t| haystack.contains(t.as_str()))
    }
}

/// Case-insensitive incremental search (F3). Unlike the filter, this only
/// *selects* the next match.
pub fn find_next(rows: &[Row], order: &[usize], from: usize, needle: &str) -> Option<usize> {
    if needle.is_empty() || order.is_empty() {
        return None;
    }
    let needle = needle.to_ascii_lowercase();
    let n = order.len();
    for step in 1..=n {
        let pos = (from + step) % n;
        if rows[order[pos]]
            .display_command()
            .to_ascii_lowercase()
            .contains(&needle)
        {
            return Some(pos);
        }
    }
    None
}

/// The complete visibility predicate: command filter, owner, kernel threads.
///
/// Kept in one place so the table, the tree view and any future export all agree
/// on what "visible" means.
#[derive(Clone, Debug)]
pub struct Visibility {
    /// Command-line terms from `F4`.
    pub text: Option<Filter>,
    /// Only this uid, from `u`.
    pub uid: Option<u32>,
    /// Whether kernel threads are shown (from `K` / `H`).
    pub show_kernel_threads: bool,
    /// Whether thread rows are shown (from `H`).
    pub show_userland_threads: bool,
    /// Whether processes inside a container are shown (htop's `O`).
    ///
    /// Linux-only in effect: `Row::container` is only ever set there, so on macOS
    /// this is a no-op rather than a key that silently hides everything.
    pub show_containerised: bool,
}

impl Default for Visibility {
    /// Everything visible.
    ///
    /// Written out rather than derived: `derive(Default)` sets every `bool` to
    /// `false`, which for `show_kernel_threads` and `show_containerised` means
    /// *hidden*. A default that quietly hides rows is a trap — the first
    /// `Visibility::default()` would have made an empty-looking table.
    fn default() -> Self {
        Self {
            text: None,
            uid: None,
            show_kernel_threads: true,
            show_userland_threads: true,
            show_containerised: true,
        }
    }
}

impl Visibility {
    pub fn allows(&self, row: &Row) -> bool {
        if !self.show_kernel_threads && row.flags.contains(RowFlags::KERNEL_THREAD) {
            return false;
        }
        if !self.show_userland_threads && row.is_thread {
            return false;
        }
        if !self.show_containerised {
            // A thread has no cgroup of its own; it is in whatever its process is
            // in, and the sampler copies that down, so this works for thread rows
            // without a special case.
            #[cfg(target_os = "linux")]
            if row.container.is_some() {
                return false;
            }
        }
        if let Some(uid) = self.uid {
            if row.uid != uid {
                return false;
            }
        }
        match &self.text {
            Some(f) => f.matches(row),
            None => true,
        }
    }
}

/// Apply the current filter to produce the display order.
pub fn apply_filter(rows: &[Row], filter: Option<&Filter>) -> Vec<usize> {
    match filter {
        None => (0..rows.len()).collect(),
        Some(f) => (0..rows.len()).filter(|&i| f.matches(&rows[i])).collect(),
    }
}

/// State of the table across ticks.
pub struct ProcTable {
    pub rows: Vec<Row>,
    pub view: View,
    pub sort: Sort,
    pub tree_mode: bool,
    pub show_kernel_threads: bool,
    pub show_userland_threads: bool,
    /// Whether processes inside a container are listed (htop's `O`).
    pub show_containerised: bool,
}

impl Default for ProcTable {
    /// Toggles mean "hidden" when `false`, so they are spelled out rather than
    /// derived — `derive(Default)` would hide kernel threads and container
    /// processes by default.
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            view: View::default(),
            sort: Sort::default(),
            tree_mode: false,
            show_kernel_threads: true,
            show_userland_threads: false,
            show_containerised: true,
        }
    }
}

impl ProcTable {
    /// Recompute the display order from the current rows.
    pub fn reorder(&mut self, visibility: &Visibility) {
        let mut order: Vec<usize> = (0..self.rows.len())
            .filter(|&i| visibility.allows(&self.rows[i]))
            .collect();

        if self.tree_mode {
            // The tree needs every row present to reconstruct parentage, so it is
            // built over the full set and the visible subset is taken afterwards.
            let full = tree::build(&mut self.rows);
            let visible: std::collections::HashSet<usize> = order.into_iter().collect();
            self.view.order = full.into_iter().filter(|i| visible.contains(i)).collect();
            return;
        }

        sort_order(&self.rows, &mut order, self.sort);
        self.view.order = order;
    }

    /// Number of rows currently displayed.
    pub fn visible(&self) -> usize {
        self.view.order.len()
    }

    pub fn row_at(&self, pos: usize) -> Option<&Row> {
        self.view.order.get(pos).map(|&i| &self.rows[i])
    }

    /// Sum of `percent_cpu` over displayed rows, used by the USED meter label.
    pub fn total_cpu(&self) -> f32 {
        self.view
            .order
            .iter()
            .map(|&i| self.rows[i].percent_cpu)
            .sum()
    }
}

/// Recompute per-process CPU% and MEM% from two samples.
///
/// `docs/DESIGN.md` §3.5.
pub fn derive_cpu(rows: &mut [Row], snapshot: &SystemSnapshot) {
    let d_total = snapshot.cpu_delta.first().map(|d| d.total()).unwrap_or(0) as f32;

    for row in rows.iter_mut() {
        let d_proc = row.ticks.proc.saturating_sub(row.prev_ticks.proc) as f32;
        row.percent_cpu = if d_total <= 0.0 {
            0.0
        } else {
            // May exceed 100% on multicore: that is correct and matches htop.
            d_proc * 100.0 / d_total
        };
        row.percent_mem = if snapshot.mem.total == 0 {
            0.0
        } else {
            row.res as f32 * 100.0 / snapshot.mem.total as f32
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Field;

    fn rows() -> Vec<Row> {
        let mut a = Row::default_for_test(1);
        a.percent_cpu = 5.0;
        a.percent_mem = 1.0;
        a.comm.try_push_str("alpha").unwrap();
        let mut b = Row::default_for_test(2);
        b.percent_cpu = 50.0;
        b.percent_mem = 10.0;
        b.comm.try_push_str("beta").unwrap();
        vec![a, b]
    }

    #[test]
    fn sort_is_a_permutation() {
        let rows = rows();
        let mut order: Vec<usize> = (0..rows.len()).collect();
        sort_order(
            &rows,
            &mut order,
            Sort {
                field: Field::PercentCpu,
                dir: Dir::Desc,
            },
        );
        assert_eq!(order, vec![1, 0]);
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1], "no duplicates, no omissions");
    }

    #[test]
    fn filter_any_term_wins() {
        let rows = rows();
        let f = Filter::parse("ALPHA|nope").unwrap();
        assert!(f.matches(&rows[0]));
        assert!(!f.matches(&rows[1]));
    }

    #[test]
    fn empty_filter_is_none() {
        assert!(Filter::parse("").is_none());
        assert!(Filter::parse(" | ").is_none());
    }

    #[test]
    fn search_wraps() {
        let rows = rows();
        let order = vec![0, 1];
        // searching from the last position wraps to the first match
        assert_eq!(find_next(&rows, &order, 1, "alpha"), Some(0));
    }

    /// Rows with equal keys must keep a fixed, ascending-pid order in *both*
    /// directions.
    ///
    /// Two bugs hid here. `reverse()` applied to the finished vector also reversed
    /// every tie, so descending CPU% on a quiet machine listed pids backwards
    /// (10, 9, 8, …) — which reads as a broken sort. And `sort_unstable_by` leaves
    /// ties unordered, so equal rows could swap between samples even without the
    /// reversal. Both show up as "the list jumps around", which is what made this
    /// worth fixing rather than tolerating.
    #[test]
    fn ties_are_broken_by_pid_in_both_directions() {
        let rows: Vec<Row> = (1..=10u32)
            .map(|pid| {
                let mut r = Row::default_for_test(pid);
                r.percent_cpu = 0.0;
                r
            })
            .collect();

        for dir in [Dir::Asc, Dir::Desc] {
            for _ in 0..5 {
                // Repeated so an unstable sort has a chance to show itself.
                let mut order: Vec<usize> = (0..rows.len()).collect();
                sort_order(&rows, &mut order, Sort { field: Field::PercentCpu, dir });
                let pids: Vec<u32> = order.iter().map(|&i| rows[i].pid).collect();
                assert_eq!(
                    pids,
                    vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                    "all-equal rows must read as ascending pid even when sorting {dir:?}"
                );
            }
        }
    }

    /// Direction must still apply to the primary key when it is not tied.
    #[test]
    fn direction_applies_to_the_primary_key() {
        let rows: Vec<Row> = (1..=10u32)
            .map(|pid| {
                let mut r = Row::default_for_test(pid);
                r.percent_cpu = if pid == 3 || pid == 7 { 5.0 } else { 0.0 };
                r
            })
            .collect();

        let mut order: Vec<usize> = (0..rows.len()).collect();
        sort_order(&rows, &mut order, Sort { field: Field::PercentCpu, dir: Dir::Desc });
        let pids: Vec<u32> = order.iter().map(|&i| rows[i].pid).collect();
        assert_eq!(&pids[..2], &[3, 7], "highest CPU first, pid-ascending among ties");
        assert_eq!(&pids[2..], &[1, 2, 4, 5, 6, 8, 9, 10], "then the zeroes");

        let mut order: Vec<usize> = (0..rows.len()).collect();
        sort_order(&rows, &mut order, Sort { field: Field::PercentCpu, dir: Dir::Asc });
        let pids: Vec<u32> = order.iter().map(|&i| rows[i].pid).collect();
        assert_eq!(&pids[..8], &[1, 2, 4, 5, 6, 8, 9, 10], "zeroes first");
        assert_eq!(&pids[8..], &[3, 7], "then the busy ones");
    }

    /// PID ascending must be exactly that: the default the app now ships.
    #[test]
    fn pid_ascending_is_a_stable_identity() {
        let rows: Vec<Row> = (1..=20u32)
            .rev()
            .map(|pid| {
                let mut r = Row::default_for_test(pid);
                r.percent_cpu = (pid % 7) as f32;
                r
            })
            .collect();
        let mut order: Vec<usize> = (0..rows.len()).collect();
        sort_order(&rows, &mut order, Sort { field: Field::Pid, dir: Dir::Asc });
        let pids: Vec<u32> = order.iter().map(|&i| rows[i].pid).collect();
        assert_eq!(pids, (1..=20u32).collect::<Vec<_>>());
    }
}

#[cfg(test)]
mod visibility_tests {
    use super::*;
    use crate::row::{Row, RowFlags};

    #[test]
    fn default_visibility_shows_everything() {
        // The toggles mean "hidden" when false, so a derived Default would hide
        // kernel threads and containers and make a fresh table look empty.
        let v = Visibility::default();
        assert!(v.show_kernel_threads);
        assert!(v.show_userland_threads);
        assert!(v.show_containerised);

        let mut kernel = Row::default_for_test(2);
        kernel.flags = kernel.flags.insert(RowFlags::KERNEL_THREAD);
        assert!(v.allows(&kernel), "kernel threads visible by default");

        let mut thread = Row::default_for_test(3);
        thread.is_thread = true;
        assert!(v.allows(&thread), "thread rows visible by default");
    }

    #[test]
    fn hiding_kernel_threads_only_hides_kernel_threads() {
        let v = Visibility {
            show_kernel_threads: false,
            ..Visibility::default()
        };
        let mut kernel = Row::default_for_test(2);
        kernel.flags = kernel.flags.insert(RowFlags::KERNEL_THREAD);
        assert!(!v.allows(&kernel));
        assert!(v.allows(&Row::default_for_test(3)));
    }

    #[test]
    fn hiding_threads_only_hides_threads() {
        let v = Visibility {
            show_userland_threads: false,
            ..Visibility::default()
        };
        let mut thread = Row::default_for_test(3);
        thread.is_thread = true;
        assert!(!v.allows(&thread));
        assert!(v.allows(&Row::default_for_test(4)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn hiding_containerised_processes_spares_host_processes() {
        let visible = Visibility::default();
        let mut containerised = Row::default_for_test(10);
        containerised.container = Some("abc123".into());
        containerised.container_runtime = Some("docker".into());
        assert!(visible.allows(&containerised), "shown by default");

        let hidden = Visibility {
            show_containerised: false,
            ..Visibility::default()
        };
        assert!(!hidden.allows(&containerised));

        // A host process must survive the filter: the whole point of `O` is to
        // leave the host's own processes readable.
        assert!(hidden.allows(&Row::default_for_test(11)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_thread_of_a_containerised_process_is_also_hidden() {
        // A thread has no cgroup of its own; the sampler copies the process's, so
        // the filter has to work on thread rows without a special case.
        let mut thread = Row::default_for_test(12);
        thread.is_thread = true;
        thread.container = Some("abc123".into());
        thread.container_runtime = Some("docker".into());

        let hidden = Visibility {
            show_containerised: false,
            ..Visibility::default()
        };
        assert!(!hidden.allows(&thread));
    }
}
