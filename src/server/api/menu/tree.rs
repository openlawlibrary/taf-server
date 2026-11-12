//! Pure node-set synthesis and layout for the menu endpoint.
//!
//! No DB, no HTTP, no actix types. Ancestors are synthesized from the changed
//! descendants' mpaths; there is no query for them. The containers are
//! `BTreeMap`/`BTreeSet` because they must be iterated and because the status
//! set then orders by `Status::to_int()` for free.

use std::collections::{BTreeMap, BTreeSet};

use crate::db::models::document_change::VersionChange;

use super::mpath::{mpath_depth, truncate_to_depth};
use super::sort::sorted_siblings;

/// What a menu request's rows imply about the shape of the tree.
#[derive(Debug, Default)]
pub struct NodeSet {
    /// Every mpath that must appear as a node, including the base itself.
    pub nodes: BTreeSet<String>,
    /// Statuses recorded *directly* on a node, keyed by mpath. Descendant
    /// statuses never bubble up into an ancestor's entry.
    pub direct_statuses: BTreeMap<String, BTreeSet<i64>>,
    /// Nodes that have at least one changed descendant in this version (dated
    /// mode) or at least one child in the element tree (tree mode).
    pub has_descendant: BTreeSet<String>,
}

/// An ordered, ceiling-bounded plan for rendering one menu response.
#[derive(Debug, Default)]
pub struct Layout {
    /// Child mpaths per parent mpath, natural-sorted and already pruned to the
    /// nodes that survived the ceiling.
    pub children: BTreeMap<String, Vec<String>>,
    /// How many nodes the response will contain.
    pub node_count: usize,
    /// Whether the node ceiling dropped anything.
    pub truncated: bool,
}

impl Layout {
    /// Every mpath that will appear in the response, including `base`.
    #[must_use]
    pub fn retained(&self, base: &str) -> Vec<String> {
        let mut retained: BTreeSet<String> = BTreeSet::new();
        retained.insert(base.to_owned());
        for siblings in self.children.values() {
            retained.extend(siblings.iter().cloned());
        }
        retained.into_iter().collect()
    }
}

/// Synthesize the node set for a **dated** request from the version's change
/// rows. `depth` is the clamped number of levels below `base`.
#[must_use]
pub fn synthesize_dated(changes: &[VersionChange], base: &str, depth: usize) -> NodeSet {
    let window_floor = mpath_depth(base);
    let window_ceil = window_floor + depth;
    let mut node_set = NodeSet::default();
    // The base is always a node, even when nothing under it changed.
    node_set.nodes.insert(base.to_owned());

    for change in changes {
        // Mandatory, not defensive: the SQL prefix is a `LIKE`, which is
        // ASCII-case-insensitive and treats `%`/`_` in the base as wildcards.
        if !change.doc_mpath.starts_with(base) {
            continue;
        }
        let row_depth = mpath_depth(&change.doc_mpath);

        // The node that actually changed, if it falls inside the window.
        if row_depth >= window_floor && row_depth <= window_ceil {
            let node = truncate_to_depth(&change.doc_mpath, row_depth);
            node_set.nodes.insert(node.clone());
            node_set
                .direct_statuses
                .entry(node)
                .or_default()
                .insert(change.status);
        }

        // Every ancestor inside the window becomes a node marked as having a
        // changed descendant. The `min` with `window_ceil` *is* the depth
        // frontier: an unbounded `row_depth - 1` would leak sub-frontier nodes.
        let ancestor_ceil = window_ceil.min(row_depth.saturating_sub(1));
        for level in window_floor..=ancestor_ceil {
            let ancestor = truncate_to_depth(&change.doc_mpath, level);
            node_set.nodes.insert(ancestor.clone());
            node_set.has_descendant.insert(ancestor);
        }
    }
    node_set
}

/// Synthesize the node set for an **undated (tree-mode)** request.
///
/// `mpaths` must have been fetched to `depth + 1` levels below `base`; the
/// extra level answers `hasChildren` at the frontier. No node carries statuses.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "`synthesize_tree` pairs with `synthesize_dated`; the suffix names the mode, not the module"
)]
pub fn synthesize_tree(mpaths: &[String], base: &str, depth: usize) -> NodeSet {
    let window_floor = mpath_depth(base);
    let window_ceil = window_floor + depth;
    let mut node_set = NodeSet::default();
    node_set.nodes.insert(base.to_owned());

    for mpath in mpaths {
        if !mpath.starts_with(base) {
            continue;
        }
        let row_depth = mpath_depth(mpath);
        if row_depth > window_ceil + 1 || row_depth < window_floor {
            continue;
        }
        if row_depth <= window_ceil {
            node_set.nodes.insert(truncate_to_depth(mpath, row_depth));
        }
        // Synthesize the ancestor chain as dated mode does: the element tree is
        // not complete (64 dev-archive rows have no parent row), and without
        // this `plan()` would drop them as orphans and delete whole subtrees.
        let ancestor_ceil = window_ceil.min(row_depth.saturating_sub(1));
        for level in window_floor..=ancestor_ceil {
            node_set.nodes.insert(truncate_to_depth(mpath, level));
        }
        if row_depth > window_floor {
            node_set
                .has_descendant
                .insert(truncate_to_depth(mpath, row_depth - 1));
        }
    }
    node_set
}

/// Nest a node set into parent → sorted-children form, then apply the node
/// ceiling breadth-first so shallow levels stay complete.
///
/// Parents come from mpath truncation, so this is for single-namespace trees
/// only; collections supply explicit links to [`plan_from_links`].
#[must_use]
pub fn plan(node_set: &NodeSet, base: &str, max_nodes: usize) -> Layout {
    let base_depth = mpath_depth(base);
    let mut links: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in &node_set.nodes {
        let node_depth = mpath_depth(node);
        if node_depth <= base_depth {
            continue;
        }
        let parent = truncate_to_depth(node, node_depth - 1);
        if !node_set.nodes.contains(&parent) {
            // Unreachable from either synthesis function, which both insert the
            // whole ancestor chain; a guard against a hand-built node set.
            tracing::warn!("Menu node {node} has no parent {parent} in the node set; dropping it");
            continue;
        }
        links.entry(parent).or_default().push(node.clone());
    }
    plan_from_links(&links, base, max_nodes)
}

/// Lay out an explicit parent → children map: natural-sort each sibling set,
/// then apply the node ceiling breadth-first from `base`.
#[must_use]
pub fn plan_from_links(
    links: &BTreeMap<String, Vec<String>>,
    base: &str,
    max_nodes: usize,
) -> Layout {
    let mut raw_children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (parent, siblings) in links {
        raw_children.insert(parent.clone(), sorted_siblings(siblings));
    }

    let (retained, truncated) = breadth_first_retain(&raw_children, base, max_nodes);
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (parent, siblings) in &raw_children {
        if !retained.contains(parent) {
            continue;
        }
        let kept: Vec<String> = siblings
            .iter()
            .filter(|child| retained.contains(*child))
            .cloned()
            .collect();
        if !kept.is_empty() {
            children.insert(parent.clone(), kept);
        }
    }
    Layout {
        children,
        node_count: retained.len(),
        truncated,
    }
}

/// Walk the tree breadth-first from `base`, keeping at most `max_nodes` nodes.
///
/// Returns the retained mpaths and whether anything was dropped.
fn breadth_first_retain(
    children: &BTreeMap<String, Vec<String>>,
    base: &str,
    max_nodes: usize,
) -> (BTreeSet<String>, bool) {
    let mut retained: BTreeSet<String> = BTreeSet::new();
    let mut frontier: Vec<String> = vec![base.to_owned()];
    let mut truncated = false;
    while !frontier.is_empty() {
        let mut next: Vec<String> = vec![];
        for node in frontier {
            if retained.len() >= max_nodes {
                truncated = true;
                break;
            }
            retained.insert(node.clone());
            if let Some(siblings) = children.get(&node) {
                next.extend(siblings.iter().cloned());
            }
        }
        if truncated {
            break;
        }
        frontier = next;
    }
    (retained, truncated)
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests"
)]
mod tests {
    use super::{plan, synthesize_dated, synthesize_tree, NodeSet};
    use crate::db::models::document_change::VersionChange;
    use std::collections::BTreeSet;

    /// Document root used by the San Mateo-shaped fixtures.
    const ROOT: &str = "City of San Mateo, Cal., Code|";
    /// A generous ceiling, so the ceiling never interferes with a shape test.
    const NO_CEILING: usize = 100_000;

    /// Build a change row with the "changed" status (2).
    fn changed(mpath: &str) -> VersionChange {
        VersionChange::new(mpath.to_owned(), 2)
    }

    /// Collect a node set's mpaths into a plain sorted vector.
    fn node_list(node_set: &NodeSet) -> Vec<String> {
        node_set.nodes.iter().cloned().collect()
    }

    /// Read a node's direct statuses as a vector, `[]` when it has none.
    fn statuses_of(node_set: &NodeSet, mpath: &str) -> Vec<i64> {
        node_set
            .direct_statuses
            .get(mpath)
            .map(|found| found.iter().copied().collect())
            .unwrap_or_default()
    }

    #[test]
    fn base_is_always_a_node_even_when_nothing_changed() {
        let node_set = synthesize_dated(&[], ROOT, 2);
        assert_eq!(node_list(&node_set), [ROOT]);
        assert!(node_set.has_descendant.is_empty());
        assert!(statuses_of(&node_set, ROOT).is_empty());
    }

    #[test]
    fn pure_ancestors_are_synthesized_with_no_statuses() {
        // One change at depth 4, requested depth 2 from depth 1.
        let changes = [changed("City of San Mateo, Cal., Code|10|10.04|10.04.010|")];
        let node_set = synthesize_dated(&changes, ROOT, 2);

        assert_eq!(
            node_list(&node_set),
            [
                "City of San Mateo, Cal., Code|",
                "City of San Mateo, Cal., Code|10|",
                "City of San Mateo, Cal., Code|10|10.04|",
            ]
        );
        for ancestor in &node_set.nodes {
            assert!(
                statuses_of(&node_set, ancestor).is_empty(),
                "{ancestor} must carry no direct status"
            );
            assert!(node_set.has_descendant.contains(ancestor));
        }
    }

    #[test]
    fn the_frontier_node_is_marked_and_the_deeper_node_is_absent() {
        // This is the `min(window_ceil, row_depth - 1)` bound. Depth 2 from
        // depth 1 means the window is depths 1..=3; the depth-4 change is below
        // it and must only mark the depth-3 frontier.
        let changes = [changed("City of San Mateo, Cal., Code|10|10.04|10.04.010|")];
        let node_set = synthesize_dated(&changes, ROOT, 2);

        let frontier = "City of San Mateo, Cal., Code|10|10.04|";
        assert!(node_set.nodes.contains(frontier));
        assert!(node_set.has_descendant.contains(frontier));
        assert!(!node_set
            .nodes
            .contains("City of San Mateo, Cal., Code|10|10.04|10.04.010|"));
    }

    #[test]
    fn a_change_leaf_inside_the_window_has_statuses_but_no_descendants() {
        let changes = [changed("City of San Mateo, Cal., Code|10|10.04|")];
        let node_set = synthesize_dated(&changes, ROOT, 2);

        let leaf = "City of San Mateo, Cal., Code|10|10.04|";
        assert_eq!(statuses_of(&node_set, leaf), [2]);
        assert!(!node_set.has_descendant.contains(leaf));
        assert!(node_set
            .has_descendant
            .contains("City of San Mateo, Cal., Code|10|"));
    }

    #[test]
    fn a_node_can_both_change_itself_and_have_changed_descendants() {
        let changes = [
            changed("City of San Mateo, Cal., Code|27|"),
            changed("City of San Mateo, Cal., Code|27|27.21|II|27.21.100|"),
        ];
        let node_set = synthesize_dated(&changes, ROOT, 2);

        let title = "City of San Mateo, Cal., Code|27|";
        assert_eq!(statuses_of(&node_set, title), [2]);
        assert!(node_set.has_descendant.contains(title));
    }

    #[test]
    fn descendant_statuses_do_not_bubble_into_ancestors() {
        // status 3 == removed.
        let changes = [VersionChange::new(
            "City of San Mateo, Cal., Code|10|10.04|10.04.010|".to_owned(),
            3,
        )];
        let node_set = synthesize_dated(&changes, ROOT, 3);

        assert_eq!(
            statuses_of(
                &node_set,
                "City of San Mateo, Cal., Code|10|10.04|10.04.010|"
            ),
            [3]
        );
        assert!(statuses_of(&node_set, "City of San Mateo, Cal., Code|10|").is_empty());
        assert!(statuses_of(&node_set, ROOT).is_empty());
    }

    #[test]
    fn statuses_are_deduplicated_and_ordered_by_their_integer() {
        let changes = [
            VersionChange::new("City of San Mateo, Cal., Code|27|".to_owned(), 3),
            VersionChange::new("City of San Mateo, Cal., Code|27|".to_owned(), 0),
            VersionChange::new("City of San Mateo, Cal., Code|27|".to_owned(), 0),
            VersionChange::new("City of San Mateo, Cal., Code|27|".to_owned(), 1),
        ];
        let node_set = synthesize_dated(&changes, ROOT, 1);
        assert_eq!(
            statuses_of(&node_set, "City of San Mateo, Cal., Code|27|"),
            [0, 1, 3]
        );
    }

    #[test]
    fn rows_outside_the_base_prefix_are_dropped() {
        // `LIKE` is ASCII-case-insensitive and `_` is a wildcard; both
        // over-matches must be filtered in Rust.
        let changes = [
            changed("A_B|child|"),
            changed("AXB|child|"),
            changed("a_b|child|"),
        ];
        let node_set = synthesize_dated(&changes, "A_B|", 2);
        assert_eq!(node_list(&node_set), ["A_B|", "A_B|child|"]);
    }

    #[test]
    fn depth_is_counted_on_the_mpath_not_the_url() {
        // A Mohican opinion is a depth-1 mpath behind a seven-segment URL.
        let changes = [changed("1997 SMTCA 1|")];
        let node_set = synthesize_dated(&changes, "1997 SMTCA 1|", 2);
        assert_eq!(node_list(&node_set), ["1997 SMTCA 1|"]);
        assert!(!node_set.has_descendant.contains("1997 SMTCA 1|"));
    }

    #[test]
    fn non_uniform_depths_coexist_in_one_response() {
        let changes = [
            changed("City of San Mateo, Cal., Code|27|27.60|27.60.180|"),
            changed("City of San Mateo, Cal., Code|27|27.21|II|27.21.100|"),
        ];
        // Window is depths 1..=4: the depth-4 section is inside it, the depth-5
        // one is not, and both live under the same depth-2 title.
        let node_set = synthesize_dated(&changes, ROOT, 3);
        assert!(node_set
            .nodes
            .contains("City of San Mateo, Cal., Code|27|27.60|27.60.180|"));
        assert!(node_set
            .nodes
            .contains("City of San Mateo, Cal., Code|27|27.21|II|"));
        assert!(!node_set
            .nodes
            .contains("City of San Mateo, Cal., Code|27|27.21|II|27.21.100|"));
        assert!(node_set
            .has_descendant
            .contains("City of San Mateo, Cal., Code|27|27.21|II|"));
    }

    #[test]
    fn a_deeper_request_reveals_what_the_shallower_one_marked_as_frontier() {
        let changes = [changed("City of San Mateo, Cal., Code|10|10.04|10.04.010|")];
        let shallow = synthesize_dated(&changes, ROOT, 2);
        let deep = synthesize_dated(&changes, ROOT, 3);

        assert!(shallow
            .has_descendant
            .contains("City of San Mateo, Cal., Code|10|10.04|"));
        assert!(deep
            .nodes
            .contains("City of San Mateo, Cal., Code|10|10.04|10.04.010|"));
        assert!(!deep
            .has_descendant
            .contains("City of San Mateo, Cal., Code|10|10.04|10.04.010|"));
    }

    #[test]
    fn unchanged_siblings_never_appear() {
        let changes = [changed("City of San Mateo, Cal., Code|10|10.04|")];
        let node_set = synthesize_dated(&changes, ROOT, 3);
        assert!(!node_set.nodes.contains("City of San Mateo, Cal., Code|11|"));
        assert!(!node_set
            .nodes
            .contains("City of San Mateo, Cal., Code|10|10.08|"));
    }

    #[test]
    fn plan_nests_and_natural_sorts_siblings() {
        let changes = [
            changed("City of San Mateo, Cal., Code|27|"),
            changed("City of San Mateo, Cal., Code|10|10.100|"),
            changed("City of San Mateo, Cal., Code|10|10.04|"),
            changed("City of San Mateo, Cal., Code|2|"),
        ];
        let node_set = synthesize_dated(&changes, ROOT, 2);
        let layout = plan(&node_set, ROOT, NO_CEILING);

        assert_eq!(
            layout.children.get(ROOT).cloned().unwrap_or_default(),
            [
                "City of San Mateo, Cal., Code|2|",
                "City of San Mateo, Cal., Code|10|",
                "City of San Mateo, Cal., Code|27|",
            ]
        );
        assert_eq!(
            layout
                .children
                .get("City of San Mateo, Cal., Code|10|")
                .cloned()
                .unwrap_or_default(),
            [
                "City of San Mateo, Cal., Code|10|10.04|",
                "City of San Mateo, Cal., Code|10|10.100|",
            ]
        );
        assert_eq!(layout.node_count, 6);
        assert!(!layout.truncated);
    }

    #[test]
    fn plan_is_stable_when_the_rows_arrive_in_reverse() {
        let forward = [
            changed("City of San Mateo, Cal., Code|2|"),
            changed("City of San Mateo, Cal., Code|10|"),
            changed("City of San Mateo, Cal., Code|27|"),
        ];
        let mut reverse = forward.to_vec();
        reverse.reverse();

        let left = plan(&synthesize_dated(&forward, ROOT, 1), ROOT, NO_CEILING);
        let right = plan(&synthesize_dated(&reverse, ROOT, 1), ROOT, NO_CEILING);
        assert_eq!(left.children, right.children);
    }

    #[test]
    fn the_node_ceiling_truncates_breadth_first_and_keeps_shallow_levels_complete() {
        // 4 titles, each with 4 chapters: 1 + 4 + 16 = 21 nodes uncapped.
        let mut changes: Vec<VersionChange> = vec![];
        for title in 1_i32..5_i32 {
            for chapter in 1_i32..5_i32 {
                changes.push(changed(&format!(
                    "City of San Mateo, Cal., Code|{title}|{title}.{chapter:02}|"
                )));
            }
        }
        let node_set = synthesize_dated(&changes, ROOT, 2);
        let uncapped = plan(&node_set, ROOT, NO_CEILING);
        assert_eq!(uncapped.node_count, 21);
        assert!(!uncapped.truncated);

        let capped = plan(&node_set, ROOT, 7);
        assert_eq!(capped.node_count, 7);
        assert!(capped.truncated);
        // Root plus all four titles survive; the cut lands inside the chapters.
        assert_eq!(
            capped.children.get(ROOT).cloned().unwrap_or_default().len(),
            4
        );
        let kept_chapters: usize = capped
            .children
            .iter()
            .filter(|(parent, _)| parent.as_str() != ROOT)
            .map(|(_, siblings)| siblings.len())
            .sum();
        assert_eq!(kept_chapters, 2);
    }

    #[test]
    fn the_node_ceiling_counts_the_root_itself() {
        let changes = [changed("City of San Mateo, Cal., Code|10|")];
        let layout = plan(&synthesize_dated(&changes, ROOT, 1), ROOT, 1);
        assert_eq!(layout.node_count, 1);
        assert!(layout.truncated);
        assert!(layout.children.is_empty());
    }

    #[test]
    fn tree_mode_synthesizes_missing_intermediate_levels() {
        // The element tree is not guaranteed complete: 64 of the dev archive's
        // 47,950 rows have a parent mpath with no row of its own, every one at
        // Maryland's `attachments` level. Without ancestor synthesis the leaf
        // is orphaned and `plan()` deletes the whole subtree silently.
        let root = "Code of Maryland Regulations|";
        let mpaths: Vec<String> = [
            "Code of Maryland Regulations|",
            "Code of Maryland Regulations|02|",
            "Code of Maryland Regulations|02|06|",
            // `…|02|06|03|attachments|` is deliberately absent.
            "Code of Maryland Regulations|02|06|03|attachments|02060303-form|",
        ]
        .iter()
        .map(|found| (*found).to_owned())
        .collect();

        let node_set = synthesize_tree(&mpaths, root, 5);
        for expected in [
            "Code of Maryland Regulations|02|06|03|",
            "Code of Maryland Regulations|02|06|03|attachments|",
            "Code of Maryland Regulations|02|06|03|attachments|02060303-form|",
        ] {
            assert!(
                node_set.nodes.contains(expected),
                "{expected} must be synthesized"
            );
        }

        let layout = plan(&node_set, root, NO_CEILING);
        assert_eq!(
            layout
                .children
                .get("Code of Maryland Regulations|02|06|03|attachments|")
                .cloned()
                .unwrap_or_default(),
            ["Code of Maryland Regulations|02|06|03|attachments|02060303-form|"],
            "the attachment must survive the layout, not be dropped as an orphan"
        );
        // Root, 02, 02|06, 02|06|03, …|attachments|, …|02060303-form|.
        assert_eq!(layout.node_count, 6);
    }

    #[test]
    fn tree_mode_marks_children_from_the_extra_level() {
        let mpaths: Vec<String> = [
            "City of San Mateo, Cal., Code|",
            "City of San Mateo, Cal., Code|10|",
            "City of San Mateo, Cal., Code|10|10.04|",
            "City of San Mateo, Cal., Code|10|10.04|10.04.010|",
        ]
        .iter()
        .map(|found| (*found).to_owned())
        .collect();

        let node_set = synthesize_tree(&mpaths, ROOT, 2);
        assert_eq!(
            node_list(&node_set),
            [
                "City of San Mateo, Cal., Code|",
                "City of San Mateo, Cal., Code|10|",
                "City of San Mateo, Cal., Code|10|10.04|",
            ]
        );
        assert!(node_set
            .has_descendant
            .contains("City of San Mateo, Cal., Code|10|10.04|"));
        assert!(node_set.direct_statuses.is_empty());
    }

    #[test]
    fn tree_mode_leaves_have_no_children() {
        let mpaths: Vec<String> = [
            "City of San Mateo, Cal., Code|",
            "City of San Mateo, Cal., Code|10|",
        ]
        .iter()
        .map(|found| (*found).to_owned())
        .collect();
        let node_set = synthesize_tree(&mpaths, ROOT, 2);
        assert!(!node_set
            .has_descendant
            .contains("City of San Mateo, Cal., Code|10|"));
        assert!(node_set.has_descendant.contains(ROOT));
    }

    #[test]
    fn tree_mode_drops_rows_outside_the_prefix_and_below_the_extra_level() {
        let mpaths: Vec<String> = [
            "City of San Mateo, Cal., Code|10|",
            "city of san mateo, cal., code|99|",
            "City of San Mateo, Cal., Code|10|10.04|10.04.010|20|",
        ]
        .iter()
        .map(|found| (*found).to_owned())
        .collect();
        let node_set = synthesize_tree(&mpaths, ROOT, 2);
        let expected: BTreeSet<String> = [
            "City of San Mateo, Cal., Code|",
            "City of San Mateo, Cal., Code|10|",
        ]
        .iter()
        .map(|found| (*found).to_owned())
        .collect();
        assert_eq!(node_set.nodes, expected);
    }
}
