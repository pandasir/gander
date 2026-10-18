//! Tree-view prefixes.
//!
//! htop draws the tree with `├─`, `└─` and `│ ` segments. We precompute the
//! prefix string for each visible row so the renderer stays allocation-cheap and
//! the prefix logic is unit-testable without a terminal.

use gander_core::row::Row;

/// Characters used for tree drawing, matching htop.
pub const MARKERS: [&str; 4] = ["├─", "└─", "│ ", "  "];

/// One row's tree decoration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub data_index: usize,
    pub prefix: String,
}

/// Build the prefix for every row in `order`.
///
/// `order` must come from `gander_core::tree::build`, which fills `depth` and
/// `is_last_child` on each row.
pub fn compose_tree(rows: &[Row], order: &[usize]) -> Vec<Mark> {
    // `has_next[d]` records whether the node currently on the path at depth `d`
    // has a following sibling. Because `order` is depth-first pre-order, every
    // ancestor's entry is already correct when a descendant is reached.
    let mut has_next: Vec<bool> = Vec::new();
    let mut out = Vec::with_capacity(order.len());

    for &idx in order {
        let row = &rows[idx];
        let depth = row.depth as usize;

        has_next.resize(depth + 2, false);

        let mut prefix = String::with_capacity(depth * 2);
        // Continuation columns for the ancestors strictly between the root and
        // this node's parent. The elbow occupies the last column pair itself.
        for continues in has_next.iter().take(depth).skip(1) {
            prefix.push_str(if *continues { MARKERS[2] } else { MARKERS[3] });
        }
        // The elbow for this node.
        if depth > 0 {
            prefix.push_str(if row.is_last_child {
                MARKERS[1]
            } else {
                MARKERS[0]
            });
        }

        // This node continues into its own column if it has following siblings.
        has_next[depth] = !row.is_last_child;

        out.push(Mark {
            data_index: idx,
            prefix,
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gander_core::row::Row;
    use gander_core::tree;

    fn row(pid: u32, ppid: u32) -> Row {
        let mut r = Row::default_for_test(pid);
        r.ppid = ppid;
        r
    }

    #[test]
    fn simple_tree_prefixes() {
        // 1
        // ├─ 2
        // │  └─ 4
        // └─ 3
        let mut rows = vec![row(1, 0), row(2, 1), row(3, 1), row(4, 2)];
        let order = tree::build(&mut rows);
        let marks = compose_tree(&rows, &order);
        let prefixes: Vec<&str> = marks.iter().map(|m| m.prefix.as_str()).collect();
        assert_eq!(prefixes, vec!["", "├─", "│ └─", "└─"]);
    }

    #[test]
    fn single_child_chain() {
        let mut rows = vec![row(1, 0), row(2, 1), row(3, 2)];
        let order = tree::build(&mut rows);
        let marks = compose_tree(&rows, &order);
        let prefixes: Vec<&str> = marks.iter().map(|m| m.prefix.as_str()).collect();
        assert_eq!(prefixes, vec!["", "└─", "  └─"]);
    }

    #[test]
    fn one_mark_per_row() {
        let mut rows: Vec<Row> = (1..=20)
            .map(|p| row(p, if p == 1 { 0 } else { 1 }))
            .collect();
        let order = tree::build(&mut rows);
        let marks = compose_tree(&rows, &order);
        assert_eq!(marks.len(), rows.len());
    }
}
