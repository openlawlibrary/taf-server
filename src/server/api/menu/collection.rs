//! Collection (library) roots for the menu endpoint.
//!
//! Libraries and documents live in different mpath namespaces, bridged only by
//! `changed_library_document`, so a collection serves exactly two levels and
//! builds its parent → children links explicitly rather than by truncation.

use std::collections::{BTreeMap, BTreeSet};

use crate::db::models::changed_library_document::{LibraryDocument, LibraryDocumentChange};

use super::mpath::{mpath_depth, truncate_to_depth};
use super::tree::NodeSet;

/// Levels a collection root serves: child libraries, then member documents.
pub const COLLECTION_MENU_DEPTH: usize = 2;

/// A synthesized collection menu, before labels are attached.
#[derive(Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "`CollectionTree` is the collection-shaped counterpart of `tree::NodeSet`; the repetition is the point"
)]
pub struct CollectionTree {
    /// Statuses and `hasChildren` for every node.
    pub node_set: NodeSet,
    /// Explicit parent → children links across the two namespaces.
    pub links: BTreeMap<String, Vec<String>>,
    /// Child library mpaths, for the library label lookup.
    pub library_mpaths: Vec<String>,
    /// Member document root mpaths, for the document element label lookup.
    pub document_mpaths: Vec<String>,
}

/// Synthesize the two collection levels from one version's bridge rows.
#[must_use]
pub fn synthesize(rows: &[LibraryDocumentChange], base_lib: &str) -> CollectionTree {
    synthesize_rows(
        rows.iter().map(|row| {
            (
                row.library_mpath.as_str(),
                row.doc_mpath.as_str(),
                Some(row.status),
            )
        }),
        base_lib,
    )
}

/// Synthesize the two collection levels with no version selected. No node
/// carries statuses.
#[must_use]
pub fn synthesize_undated(rows: &[LibraryDocument], base_lib: &str) -> CollectionTree {
    synthesize_rows(
        rows.iter()
            .map(|row| (row.library_mpath.as_str(), row.doc_mpath.as_str(), None)),
        base_lib,
    )
}

/// Rebuild a collection tree's member-document `hasChildren` flags from the
/// element tree, which the bridge under-reports.
///
/// **Undated mode only.** In dated mode `hasChildren` means "has changed
/// descendants in this version", which the bridge answers correctly.
#[must_use]
pub fn with_member_children(
    built: CollectionTree,
    expandable: &BTreeSet<String>,
) -> CollectionTree {
    let mut has_descendant = built.node_set.has_descendant;
    for root in &built.document_mpaths {
        if expandable.contains(root) {
            has_descendant.insert(root.clone());
        } else {
            has_descendant.remove(root);
        }
    }
    CollectionTree {
        node_set: NodeSet {
            nodes: built.node_set.nodes,
            direct_statuses: built.node_set.direct_statuses,
            has_descendant,
        },
        links: built.links,
        library_mpaths: built.library_mpaths,
        document_mpaths: built.document_mpaths,
    }
}

/// Shared body of [`synthesize`] and [`synthesize_undated`]. A `None` status
/// means no version is selected, which keeps tree-mode nodes status-less.
fn synthesize_rows<'row>(
    rows: impl Iterator<Item = (&'row str, &'row str, Option<i64>)>,
    base_lib: &str,
) -> CollectionTree {
    let base_depth = mpath_depth(base_lib);
    let mut node_set = NodeSet::default();
    node_set.nodes.insert(base_lib.to_owned());
    let mut links: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut libraries: BTreeSet<String> = BTreeSet::new();
    let mut documents: BTreeSet<String> = BTreeSet::new();

    for (library_mpath, doc_mpath, status) in rows {
        if !library_mpath.starts_with(base_lib) {
            continue;
        }
        let doc_root = truncate_to_depth(doc_mpath, 1);
        if doc_root.is_empty() || !doc_root.ends_with('|') {
            // Truncates to the empty string, so it would enter the node set as
            // a phantom that `plan()` drops, hiding an element that did change.
            tracing::warn!("Skipping malformed document materialized path {doc_mpath:?}");
            continue;
        }
        let library_depth = mpath_depth(library_mpath);
        let parent = if library_depth > base_depth {
            let child_library = truncate_to_depth(library_mpath, base_depth + 1);
            node_set.nodes.insert(child_library.clone());
            libraries.insert(child_library.clone());
            links
                .entry(base_lib.to_owned())
                .or_default()
                .insert(child_library.clone());
            node_set.has_descendant.insert(child_library.clone());
            child_library
        } else {
            base_lib.to_owned()
        };
        node_set.has_descendant.insert(base_lib.to_owned());

        node_set.nodes.insert(doc_root.clone());
        documents.insert(doc_root.clone());
        links.entry(parent).or_default().insert(doc_root.clone());

        if mpath_depth(doc_mpath) == 1 {
            // The row is about the document root itself.
            if let Some(found) = status {
                node_set
                    .direct_statuses
                    .entry(doc_root)
                    .or_default()
                    .insert(found);
            }
        } else {
            // Something inside the document: reachable by requesting its path.
            node_set.has_descendant.insert(doc_root);
        }
    }

    CollectionTree {
        node_set,
        links: links
            .into_iter()
            .map(|(parent, children)| (parent, children.into_iter().collect()))
            .collect(),
        library_mpaths: libraries.into_iter().collect(),
        document_mpaths: documents.into_iter().collect(),
    }
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests"
)]
mod tests {
    use super::{synthesize, synthesize_undated, with_member_children, COLLECTION_MENU_DEPTH};
    use crate::db::models::changed_library_document::{LibraryDocument, LibraryDocumentChange};
    use crate::server::api::menu::tree::plan_from_links;
    use std::collections::BTreeSet;

    /// Library root used by the fixtures below.
    const ORDINANCES: &str = "cityofsanmateo/law-rdf|us/ca/cities/san-mateo/ordinances|";

    /// Build a bridge row with the "changed" status (2).
    fn row(library: &str, document: &str) -> LibraryDocumentChange {
        LibraryDocumentChange::new(library.to_owned(), document.to_owned(), 2)
    }

    /// Build an undated bridge row.
    fn undated(library: &str, document: &str) -> LibraryDocument {
        LibraryDocument::new(library.to_owned(), document.to_owned())
    }

    #[test]
    fn collections_serve_exactly_two_levels() {
        assert_eq!(COLLECTION_MENU_DEPTH, 2);
    }

    #[test]
    fn undated_collections_carry_members_but_no_statuses() {
        // Tree mode's own entry point is `/`, which is a library in every
        // fonds, so this is the shape `/_api/menu/_tree` serves.
        let rows = [
            undated(&format!("{ORDINANCES}2025|"), "Ordinance 2025-1|"),
            undated(&format!("{ORDINANCES}2025|"), "Ordinance 2025-1|section-1|"),
            undated(&format!("{ORDINANCES}2024|"), "Ordinance 2024-7|"),
        ];
        let collection = synthesize_undated(&rows, ORDINANCES);
        let layout = plan_from_links(&collection.links, ORDINANCES, 5_000);

        assert_eq!(
            layout.children.get(ORDINANCES).cloned().unwrap_or_default(),
            [format!("{ORDINANCES}2024|"), format!("{ORDINANCES}2025|")]
        );
        assert_eq!(
            layout
                .children
                .get(&format!("{ORDINANCES}2025|"))
                .cloned()
                .unwrap_or_default(),
            ["Ordinance 2025-1|"]
        );
        assert!(
            collection.node_set.direct_statuses.is_empty(),
            "tree mode has no selected version, so no node may carry statuses"
        );
        // A row deeper than the document root still means "expandable".
        assert!(collection
            .node_set
            .has_descendant
            .contains("Ordinance 2025-1|"));
        assert!(!collection
            .node_set
            .has_descendant
            .contains("Ordinance 2024-7|"));
        assert!(collection
            .node_set
            .has_descendant
            .contains(&format!("{ORDINANCES}2024|")));
    }

    #[test]
    fn member_children_override_corrects_the_bridge_false_negative() {
        // The San Mateo code reaches the root menu through the bridge with only
        // a depth-1 row, so the bridge says `hasChildren: false` — while the
        // same document addressed directly answers `true` with 27 titles. The
        // element tree is the authority in tree mode.
        let rows = [
            undated(ORDINANCES, "City of San Mateo, Cal., Code|"),
            undated(ORDINANCES, "Ordinance 2025-1|"),
        ];
        let built = synthesize_undated(&rows, ORDINANCES);
        assert!(
            !built
                .node_set
                .has_descendant
                .contains("City of San Mateo, Cal., Code|"),
            "the bridge alone cannot see the code's titles"
        );

        let expandable: BTreeSet<String> = ["City of San Mateo, Cal., Code|".to_owned()]
            .into_iter()
            .collect();
        let corrected = with_member_children(built, &expandable);

        assert!(corrected
            .node_set
            .has_descendant
            .contains("City of San Mateo, Cal., Code|"));
        assert!(
            !corrected
                .node_set
                .has_descendant
                .contains("Ordinance 2025-1|"),
            "a single-section ordinance must stay a leaf"
        );
    }

    #[test]
    fn member_children_override_also_clears_a_stale_true() {
        // A bridge row deeper than the document root sets `hasChildren` before
        // the probe runs. If the element tree disagrees, the element tree wins:
        // a `true` with no reachable children would make the client fetch
        // deeper and get nothing.
        let rows = [undated(ORDINANCES, "Ordinance 2025-1|section-1|")];
        let built = synthesize_undated(&rows, ORDINANCES);
        assert!(built.node_set.has_descendant.contains("Ordinance 2025-1|"));

        let corrected = with_member_children(built, &BTreeSet::new());
        assert!(!corrected
            .node_set
            .has_descendant
            .contains("Ordinance 2025-1|"));
    }

    #[test]
    fn member_children_override_leaves_library_nodes_alone() {
        let rows = [undated(&format!("{ORDINANCES}2025|"), "Ordinance 2025-1|")];
        let built = synthesize_undated(&rows, ORDINANCES);
        let corrected = with_member_children(built, &BTreeSet::new());

        assert!(corrected.node_set.has_descendant.contains(ORDINANCES));
        assert!(corrected
            .node_set
            .has_descendant
            .contains(&format!("{ORDINANCES}2025|")));
    }

    #[test]
    fn a_malformed_document_mpath_is_skipped_rather_than_becoming_a_phantom_node() {
        // `doc_mpath = 'None'` exists in the dev archive with 178 change rows
        // behind it. It has no `|`, so it truncates to the empty string.
        let rows = [
            row(&format!("{ORDINANCES}2025|"), "None"),
            row(&format!("{ORDINANCES}2025|"), "Ordinance 2025-1|"),
        ];
        let collection = synthesize(&rows, ORDINANCES);

        assert_eq!(collection.document_mpaths, ["Ordinance 2025-1|"]);
        assert!(!collection.node_set.nodes.contains(""));
        assert!(!collection.node_set.nodes.contains("None"));
    }

    #[test]
    fn child_libraries_carry_their_changed_member_documents() {
        let rows = [
            row(&format!("{ORDINANCES}2025|"), "Ordinance 2025-1|"),
            row(&format!("{ORDINANCES}2025|"), "Ordinance 2025-2|"),
            row(&format!("{ORDINANCES}2024|"), "Ordinance 2024-7|"),
        ];
        let collection = synthesize(&rows, ORDINANCES);
        let layout = plan_from_links(&collection.links, ORDINANCES, 5_000);

        assert_eq!(
            layout.children.get(ORDINANCES).cloned().unwrap_or_default(),
            [format!("{ORDINANCES}2024|"), format!("{ORDINANCES}2025|")]
        );
        assert_eq!(
            layout
                .children
                .get(&format!("{ORDINANCES}2025|"))
                .cloned()
                .unwrap_or_default(),
            ["Ordinance 2025-1|", "Ordinance 2025-2|"]
        );
        assert_eq!(layout.node_count, 6);
        assert_eq!(
            collection.library_mpaths,
            [format!("{ORDINANCES}2024|"), format!("{ORDINANCES}2025|")]
        );
        assert_eq!(
            collection.document_mpaths,
            [
                "Ordinance 2024-7|",
                "Ordinance 2025-1|",
                "Ordinance 2025-2|"
            ]
        );
    }

    #[test]
    fn a_change_on_the_document_root_is_a_status_not_a_descendant() {
        let rows = [row(&format!("{ORDINANCES}2025|"), "Ordinance 2025-1|")];
        let collection = synthesize(&rows, ORDINANCES);

        assert!(collection
            .node_set
            .direct_statuses
            .get("Ordinance 2025-1|")
            .is_some());
        assert!(!collection
            .node_set
            .has_descendant
            .contains("Ordinance 2025-1|"));
    }

    #[test]
    fn a_change_inside_a_document_marks_it_as_expandable_without_a_status() {
        let rows = [row(
            &format!("{ORDINANCES}2025|"),
            "Ordinance 2025-1|section-1|",
        )];
        let collection = synthesize(&rows, ORDINANCES);

        assert!(collection
            .node_set
            .has_descendant
            .contains("Ordinance 2025-1|"));
        assert!(collection
            .node_set
            .direct_statuses
            .get("Ordinance 2025-1|")
            .is_none());
        assert!(collection.node_set.nodes.contains("Ordinance 2025-1|"));
    }

    #[test]
    fn documents_attached_at_the_root_library_skip_the_child_level() {
        let rows = [row(ORDINANCES, "Ordinance 2025-1|")];
        let collection = synthesize(&rows, ORDINANCES);
        let layout = plan_from_links(&collection.links, ORDINANCES, 5_000);

        assert_eq!(
            layout.children.get(ORDINANCES).cloned().unwrap_or_default(),
            ["Ordinance 2025-1|"]
        );
        assert!(collection.library_mpaths.is_empty());
    }

    #[test]
    fn a_date_with_no_member_changes_yields_the_root_alone() {
        let collection = synthesize(&[], ORDINANCES);
        let layout = plan_from_links(&collection.links, ORDINANCES, 5_000);

        assert_eq!(layout.node_count, 1);
        assert!(layout.children.is_empty());
        assert!(!collection.node_set.has_descendant.contains(ORDINANCES));
    }

    #[test]
    fn rows_outside_the_base_library_prefix_are_dropped() {
        let rows = [
            row(&format!("{ORDINANCES}2025|"), "Ordinance 2025-1|"),
            row(
                "cityofsanmateo/law-rdf|us/ca/cities/san-mateo/charter|2025|",
                "Charter 1|",
            ),
        ];
        let collection = synthesize(&rows, ORDINANCES);
        assert_eq!(collection.document_mpaths, ["Ordinance 2025-1|"]);
    }

    #[test]
    fn deeply_nested_libraries_collapse_onto_the_first_child_level() {
        let rows = [row(
            &format!("{ORDINANCES}2025|q1|"),
            "Ordinance 2025-1|section-1|",
        )];
        let collection = synthesize(&rows, ORDINANCES);
        assert_eq!(collection.library_mpaths, [format!("{ORDINANCES}2025|")]);
    }

    #[test]
    fn statuses_are_deduplicated_and_ordered_by_their_integer() {
        let rows = [
            LibraryDocumentChange::new(
                format!("{ORDINANCES}2025|"),
                "Ordinance 2025-1|".to_owned(),
                3,
            ),
            LibraryDocumentChange::new(
                format!("{ORDINANCES}2025|"),
                "Ordinance 2025-1|".to_owned(),
                0,
            ),
        ];
        let collection = synthesize(&rows, ORDINANCES);
        let statuses: Vec<i64> = collection
            .node_set
            .direct_statuses
            .get("Ordinance 2025-1|")
            .map(|found| found.iter().copied().collect())
            .unwrap_or_default();
        assert_eq!(statuses, [0, 3]);
    }
}
