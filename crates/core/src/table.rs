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
    fn default() -> Self {
        Sort {
            field: crate::row::Field::PercentCpu,
            dir: Dir::Desc,
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
#[derive(Clone, Debug, Default)]
pub struct Visibility {
    /// Command-line terms from `F4`.
    pub text: Option<Filter>,
    /// Only this uid, from `u`.
    pub uid: Option<u32>,
    /// Whether kernel threads are shown (from `K` / `H`).
    pub show_kernel_threads: bool,
    /// Whether thread rows are shown (from `H`).
    pub show_userland_threads: bool,
}

impl Visibility {
    pub fn allows(&self, row: &Row) -> bool {
        if !self.show_kernel_threads && row.flags.contains(RowFlags::KERNEL_THREAD) {
            return false;
        }
        if !self.show_userland_threads && row.is_thread {
            return false;
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
}

impl Default for ProcTable {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            view: View::default(),
            sort: Sort::default(),
            tree_mode: false,
            show_kernel_threads: true,
            show_userland_threads: false,
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
