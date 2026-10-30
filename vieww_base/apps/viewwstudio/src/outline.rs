//! The inspector as an outline (ST-06): collapsible rows, a filter,
//! keyboard navigation and a multi-row selection, over the flat
//! `(depth, node)` snapshot the inspector already captures.
//!
//! Everything here is a pure function of the snapshot and the outline's
//! state, so it is tested without a window; the pane and the key
//! dispatcher call it.
//!
//! **Keys survive a re-render.** A row is named by its *path* — the child
//! index at each level from the root, `"0.2.1"` — not by its position in
//! the list, so a collapsed card stays collapsed when the guest rebuilds
//! and the rows above it change count.

use std::collections::BTreeSet;

/// One captured row: its depth and its child count (what the outline needs
/// of a `NodeDescription`).
pub trait Row {
    fn depth(&self) -> usize;
    fn children(&self) -> usize;
    fn name(&self) -> &str;
}

impl Row for (usize, vieww_render::NodeDescription) {
    fn depth(&self) -> usize {
        self.0
    }

    fn children(&self) -> usize {
        self.1.children
    }

    fn name(&self) -> &str {
        self.1.name
    }
}

/// Each row's path key (`"0"`, `"0.1"`, `"0.1.0"`…), by position.
#[must_use]
pub fn paths<R: Row>(rows: &[R]) -> Vec<String> {
    let mut stack: Vec<usize> = Vec::new();
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let d = r.depth();
        if d < stack.len() {
            stack.truncate(d + 1);
            if let Some(last) = stack.last_mut() {
                *last += 1;
            }
        } else {
            stack.resize(d + 1, 0);
        }
        // A row deeper than its predecessor by more than one (a truncated
        // capture) still gets a unique key.
        out.push(
            stack
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("."),
        );
    }
    out
}

/// The parent row of `index`, if any.
#[must_use]
pub fn parent<R: Row>(rows: &[R], index: usize) -> Option<usize> {
    let d = rows.get(index)?.depth();
    (0..index).rev().find(|&i| rows[i].depth() < d)
}

/// The rows the outline shows, in order: those under no collapsed row,
/// and, with a filter, those whose name contains it (case-insensitively)
/// together with their ancestors so each match keeps its place. A filter
/// shows matches inside collapsed rows too: searching is how a reader
/// finds what folding hid.
#[must_use]
pub fn visible<R: Row>(rows: &[R], collapsed: &BTreeSet<String>, filter: &str) -> Vec<usize> {
    let keys = paths(rows);
    let needle = filter.trim().to_lowercase();
    if !needle.is_empty() {
        let mut keep = vec![false; rows.len()];
        for (i, r) in rows.iter().enumerate() {
            if r.name().to_lowercase().contains(&needle) {
                keep[i] = true;
                let mut p = parent(rows, i);
                while let Some(j) = p {
                    keep[j] = true;
                    p = parent(rows, j);
                }
            }
        }
        return (0..rows.len()).filter(|&i| keep[i]).collect();
    }
    let mut out = Vec::with_capacity(rows.len());
    let mut hidden_below: Option<usize> = None;
    for (i, r) in rows.iter().enumerate() {
        if let Some(d) = hidden_below {
            if r.depth() > d {
                continue;
            }
            hidden_below = None;
        }
        out.push(i);
        if r.children() > 0 && collapsed.contains(&keys[i]) {
            hidden_below = Some(r.depth());
        }
    }
    out
}

/// A navigation key in the outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    /// Collapse an open row, or go to its parent.
    Left,
    /// Expand a closed row, or go to its first child.
    Right,
    Home,
    End,
}

/// What a key does: the new selected row, and the row to collapse or
/// expand (by path key), if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub select: Option<usize>,
    pub collapse: Option<String>,
    pub expand: Option<String>,
}

/// Apply `nav` with `current` selected.
#[must_use]
pub fn navigate<R: Row>(
    rows: &[R],
    collapsed: &BTreeSet<String>,
    filter: &str,
    current: Option<usize>,
    nav: Nav,
) -> Step {
    let shown = visible(rows, collapsed, filter);
    let keys = paths(rows);
    let none = Step {
        select: current,
        collapse: None,
        expand: None,
    };
    if shown.is_empty() {
        return none;
    }
    let at = current.and_then(|c| shown.iter().position(|&i| i == c));
    let pick = |i: usize| Step {
        select: Some(i),
        collapse: None,
        expand: None,
    };
    match (nav, at) {
        (Nav::Home, _) | (Nav::Down | Nav::Right | Nav::Left | Nav::Up, None) => pick(shown[0]),
        (Nav::End, _) => pick(shown[shown.len() - 1]),
        (Nav::Down, Some(p)) => pick(shown[(p + 1).min(shown.len() - 1)]),
        (Nav::Up, Some(p)) => pick(shown[p.saturating_sub(1)]),
        (Nav::Left, Some(p)) => {
            let i = shown[p];
            if rows[i].children() > 0 && !collapsed.contains(&keys[i]) && filter.trim().is_empty() {
                Step {
                    select: Some(i),
                    collapse: Some(keys[i].clone()),
                    expand: None,
                }
            } else {
                parent(rows, i).map_or(none, pick)
            }
        }
        (Nav::Right, Some(p)) => {
            let i = shown[p];
            if rows[i].children() == 0 {
                none
            } else if collapsed.contains(&keys[i]) {
                Step {
                    select: Some(i),
                    collapse: None,
                    expand: Some(keys[i].clone()),
                }
            } else {
                shown.get(p + 1).map_or(none, |&n| pick(n))
            }
        }
    }
}

/// A click on a row with the selection `(primary, set)`: plain replaces
/// the selection, toggle (⌘/Ctrl) adds or removes the row, range (Shift)
/// selects every shown row from the primary to it.
#[must_use]
pub fn click(
    shown: &[usize],
    primary: Option<usize>,
    set: &BTreeSet<usize>,
    index: usize,
    toggle: bool,
    range: bool,
) -> (Option<usize>, BTreeSet<usize>) {
    if range {
        if let Some(anchor) = primary {
            let a = shown.iter().position(|&i| i == anchor);
            let b = shown.iter().position(|&i| i == index);
            if let (Some(a), Some(b)) = (a, b) {
                let (lo, hi) = (a.min(b), a.max(b));
                return (Some(anchor), shown[lo..=hi].iter().copied().collect());
            }
        }
    }
    if toggle {
        let mut next = set.clone();
        if !next.remove(&index) {
            next.insert(index);
        }
        let primary = if next.contains(&index) {
            Some(index)
        } else {
            next.iter().next().copied()
        };
        return (primary, next);
    }
    if primary == Some(index) && set.len() <= 1 {
        // Clicking the only selected row clears it, as the pane always did.
        return (None, BTreeSet::new());
    }
    (Some(index), BTreeSet::from([index]))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct R(usize, usize, &'static str);
    impl Row for R {
        fn depth(&self) -> usize {
            self.0
        }
        fn children(&self) -> usize {
            self.1
        }
        fn name(&self) -> &str {
            self.2
        }
    }

    /// root
    ///   card      (0.0)
    ///     Text a  (0.0.0)
    ///     Text b  (0.0.1)
    ///   card      (0.1)
    ///     Image   (0.1.0)
    fn fixture() -> Vec<R> {
        vec![
            R(0, 2, "Root"),
            R(1, 2, "Card"),
            R(2, 0, "TextA"),
            R(2, 0, "TextB"),
            R(1, 1, "Card"),
            R(2, 0, "Image"),
        ]
    }

    #[test]
    fn paths_name_rows_by_position_in_the_tree() {
        assert_eq!(
            paths(&fixture()),
            ["0", "0.0", "0.0.0", "0.0.1", "0.1", "0.1.0"]
        );
        assert_eq!(parent(&fixture(), 3), Some(1));
        assert_eq!(parent(&fixture(), 4), Some(0));
        assert_eq!(parent(&fixture(), 0), None);
    }

    #[test]
    fn collapsing_hides_descendants_and_a_filter_finds_them_again() {
        let rows = fixture();
        let collapsed = BTreeSet::from(["0.0".to_owned()]);
        assert_eq!(visible(&rows, &collapsed, ""), [0, 1, 4, 5]);
        assert_eq!(visible(&rows, &collapsed, "textb"), [0, 1, 3]);
        assert_eq!(visible(&rows, &BTreeSet::new(), "image"), [0, 4, 5]);
    }

    #[test]
    fn arrows_move_left_folds_right_unfolds() {
        let rows = fixture();
        let mut collapsed = BTreeSet::new();
        let s = navigate(&rows, &collapsed, "", Some(1), Nav::Left);
        assert_eq!(s.collapse.as_deref(), Some("0.0"));
        collapsed.insert("0.0".to_owned());
        assert_eq!(
            navigate(&rows, &collapsed, "", Some(1), Nav::Down).select,
            Some(4)
        );
        let s = navigate(&rows, &collapsed, "", Some(1), Nav::Right);
        assert_eq!(s.expand.as_deref(), Some("0.0"));
        collapsed.clear();
        assert_eq!(
            navigate(&rows, &collapsed, "", Some(1), Nav::Right).select,
            Some(2)
        );
        assert_eq!(
            navigate(&rows, &collapsed, "", Some(3), Nav::Left).select,
            Some(1)
        );
        assert_eq!(
            navigate(&rows, &collapsed, "", None, Nav::Down).select,
            Some(0)
        );
        assert_eq!(
            navigate(&rows, &collapsed, "", Some(0), Nav::End).select,
            Some(5)
        );
    }

    #[test]
    fn clicks_select_toggle_and_range() {
        let shown = [0, 1, 2, 3, 4, 5];
        let (p, s) = click(&shown, None, &BTreeSet::new(), 2, false, false);
        assert_eq!((p, s.clone()), (Some(2), BTreeSet::from([2])));
        let (p, s) = click(&shown, p, &s, 4, true, false);
        assert_eq!((p, s.clone()), (Some(4), BTreeSet::from([2, 4])));
        let (p, s) = click(&shown, Some(1), &s, 3, false, true);
        assert_eq!((p, s), (Some(1), BTreeSet::from([1, 2, 3])));
        let (p, s) = click(&shown, Some(2), &BTreeSet::from([2]), 2, false, false);
        assert_eq!((p, s), (None, BTreeSet::new()));
    }
}
