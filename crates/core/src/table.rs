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
    pub cgroup: Option<Box<str>>,
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
    use crate::row::Field as F;

    // Numeric comparators only. Sorting on formatted text is a bug;
    // docs/DESIGN.md §9.4.
    let cmp_cpu = |a: &usize, b: &usize| rows[*a].percent_cpu.total_cmp(&rows[*b].percent_cpu);
    let cmp_mem = |a: &usize, b: &usize| rows[*a].percent_mem.total_cmp(&rows[*b].percent_mem);

    match sort.field {
        F::Pid => order.sort_unstable_by_key(|&i| rows[i].pid),
        F::Ppid => order.sort_unstable_by_key(|&i| rows[i].ppid),
        F::Pgrp => order.sort_unstable_by_key(|&i| rows[i].pgrp),
        F::Comm => order.sort_unstable_by(|a, b| rows[*a].comm.cmp(&rows[*b].comm)),
        F::User => order.sort_unstable_by_key(|&i| rows[i].uid),
        F::Priority => order.sort_unstable_by_key(|&i| rows[i].priority),
        F::Nice => order.sort_unstable_by_key(|&i| rows[i].nice),
        F::PercentCpu | F::PercentNormCpu => order.sort_unstable_by(cmp_cpu),
        F::PercentMem => order.sort_unstable_by(cmp_mem),
        F::ResMem => order.sort_unstable_by_key(|&i| rows[i].res),
        F::VirtMem => order.sort_unstable_by_key(|&i| rows[i].virt),
        F::Time => order.sort_unstable_by_key(|&i| rows[i].time_ticks),
        F::Elapsed => order.sort_unstable_by_key(|&i| rows[i].elapsed),
        F::Nlwp => order.sort_unstable_by_key(|&i| rows[i].nlwp),
        F::MinFlt => order.sort_unstable_by_key(|&i| rows[i].min_flt),
        F::MajFlt => order.sort_unstable_by_key(|&i| rows[i].maj_flt),
        F::Processor => order.sort_unstable_by_key(|&i| rows[i].processor),
        _ => order.sort_unstable_by_key(|&i| rows[i].pid),
    }

    if sort.dir == Dir::Desc {
        order.reverse();
    }
}

/// Case-insensitive fixed-string filter, `|`-separated, matching htop's F4.
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
}

impl Default for ProcTable {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            view: View::default(),
            sort: Sort::default(),
            tree_mode: false,
            show_kernel_threads: true,
        }
    }
}

impl ProcTable {
    /// Recompute the display order from the current rows.
    pub fn reorder(&mut self, filter: Option<&Filter>) {
        if self.tree_mode {
            let order = tree::build(&mut self.rows);
            self.view.order = order;
            return;
        }
        let mut order = apply_filter(&self.rows, filter);
        if !self.show_kernel_threads {
            order.retain(|&i| !self.rows[i].flags.contains(RowFlags::KERNEL_THREAD));
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
}
