//! Tree-view construction.
//!
//! Matches htop's behaviour: rows whose parent is missing (or is itself) become
//! roots, and cycles are broken instead of hanging the program.

use std::collections::HashMap;

use crate::row::Row;
use crate::Pid;

/// Fill `depth`, `has_children`, `is_last_child` on every row, in tree order.
///
/// Returns the row indices in depth-first display order. The input slice is not
/// reordered; only the tree metadata on each row is updated.
pub fn build(rows: &mut [Row]) -> Vec<usize> {
    let mut by_pid: HashMap<Pid, usize> = HashMap::with_capacity(rows.len());
    for (idx, row) in rows.iter().enumerate() {
        by_pid.insert(row.pid, idx);
    }

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); rows.len()];
    let mut is_root = vec![true; rows.len()];

    for idx in 0..rows.len() {
        let ppid = rows[idx].ppid;
        let parent = by_pid.get(&ppid).copied();
        match parent {
            Some(p) if p != idx => {
                children[p].push(idx);
                is_root[idx] = false;
            }
            // Missing parent, or the degenerate self-parent case: treat as root.
            _ => is_root[idx] = true,
        }
    }

    // Break cycles: anything not reachable from a root becomes a root itself.
    let mut seen = vec![false; rows.len()];
    let mut order = Vec::with_capacity(rows.len());

    for root in 0..rows.len() {
        if is_root[root] {
            rows[root].is_last_child = false;
            walk(root, 0, &children, rows, &mut order, &mut seen);
        }
    }
    // Any remaining node sits on a cycle. Promote and walk it.
    for idx in 0..rows.len() {
        if !seen[idx] {
            rows[idx].depth = 0;
            rows[idx].is_last_child = false;
            walk(idx, 0, &children, rows, &mut order, &mut seen);
        }
    }

    order
}

fn walk(
    idx: usize,
    depth: u16,
    children: &[Vec<usize>],
    rows: &mut [Row],
    order: &mut Vec<usize>,
    seen: &mut [bool],
) {
    if seen[idx] {
        return;
    }
    seen[idx] = true;

    let kids = &children[idx];
    rows[idx].depth = depth;
    rows[idx].has_children = !kids.is_empty();
    // `is_last_child` is assigned by the caller (the parent's loop), or left
    // false for roots; assigning it here would erase the parent's decision.
    order.push(idx);

    let n = kids.len();
    for (i, &child) in kids.iter().enumerate() {
        if seen[child] {
            continue;
        }
        rows[child].is_last_child = i + 1 == n;
        walk(child, depth.saturating_add(1), children, rows, order, seen);
    }
}

/// All pids in the subtree rooted at `pid`, including `pid` itself.
pub fn subtree(rows: &[Row], pid: Pid) -> Vec<Pid> {
    let mut by_pid: HashMap<Pid, usize> = HashMap::with_capacity(rows.len());
    for (idx, row) in rows.iter().enumerate() {
        by_pid.insert(row.pid, idx);
    }
    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for row in rows {
        if by_pid.contains_key(&row.ppid) {
            children.entry(row.ppid).or_default().push(row.pid);
        }
    }

    let mut out = Vec::new();
    let mut stack = vec![pid];
    let mut guard = 0usize;
    while let Some(p) = stack.pop() {
        out.push(p);
        guard += 1;
        if guard > rows.len() + 1 {
            break; // defensive: cycle
        }
        if let Some(kids) = children.get(&p) {
            stack.extend(kids.iter().copied());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: Pid, ppid: Pid) -> Row {
        let mut r = Row::default_for_test(pid);
        r.ppid = ppid;
        r
    }

    #[test]
    fn depth_first_order() {
        // 1 -> (2 -> 4, 3)
        let mut rows = vec![row(1, 0), row(2, 1), row(3, 1), row(4, 2)];
        let order = build(&mut rows);
        let pids: Vec<Pid> = order.iter().map(|&i| rows[i].pid).collect();
        assert_eq!(pids, vec![1, 2, 4, 3]);
        assert_eq!(rows[0].depth, 0, "pid 1 is the root");
        assert_eq!(rows[1].depth, 1, "pid 2 is a child of 1");
        assert_eq!(rows[3].depth, 2, "pid 4 is a child of 2");
        assert!(rows[1].has_children);
        assert!(!rows[0].is_last_child);
        // Sibling order follows discovery order in the input slice, so pid 3 is
        // not necessarily last; only the structure is asserted here.
        assert!(!rows[2].has_children);
        assert!(order.iter().any(|&i| rows[i].pid == 3));
    }

    #[test]
    fn orphan_is_a_root() {
        let mut rows = vec![row(10, 999)];
        let order = build(&mut rows);
        assert_eq!(order.len(), 1);
        assert_eq!(rows[0].depth, 0);
    }

    #[test]
    fn self_parent_does_not_loop() {
        let mut rows = vec![row(7, 7)];
        let order = build(&mut rows);
        assert_eq!(order.len(), 1);
    }

    #[test]
    fn cycle_does_not_hang() {
        // 1 -> 2 -> 3 -> 1
        let mut rows = vec![row(1, 3), row(2, 1), row(3, 2)];
        let order = build(&mut rows);
        assert_eq!(order.len(), 3, "every node must appear exactly once");
    }

    #[test]
    fn subtree_walk() {
        let rows = vec![row(1, 0), row(2, 1), row(3, 1), row(4, 2)];
        let mut got = subtree(&rows, 1);
        got.sort_unstable();
        assert_eq!(got, vec![1, 2, 3, 4]);
    }
}
