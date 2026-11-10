//! Response types for the menu endpoint.
use std::collections::BTreeMap;

use serde::Serialize;

use crate::db::models::status::Status;

use super::super::versions::format_date;
use super::mpath::{last_segment, mpath_depth};
use super::tree::{Layout, NodeSet};
use super::MAX_MENU_DEPTH;

/// Token served for a `document_change.status` integer outside `0..=3`.
const UNKNOWN_STATUS_TOKEN: &str = "unknown";

/// Which driver produced this response.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Driven by `document_change` filtered to one version date.
    Date,
    /// Driven by the `document_element` tree, with no version selected.
    Tree,
}

/// What the requested path resolved to.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RootKind {
    /// A document element.
    Document,
    /// A library (collection).
    Collection,
}

/// Presentation facts for one node, independent of which table supplied them.
#[derive(Debug, Default, Clone)]
pub struct Facts {
    /// Unique document identifier.
    pub doc_id: Option<String>,
    /// Url of the element. `None` renders as a non-clickable group header.
    pub url: Option<String>,
}

/// One node of the cascading menu.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// Materialized path of this node.
    pub mpath: String,
    /// Last `|`-delimited segment of [`Node::mpath`].
    pub segment: String,
    /// Unique document identifier.
    pub doc_id: Option<String>,
    /// Url this node links to, `null` when no element row backs it.
    pub url: Option<String>,
    /// **Absolute** mpath depth, not relative to the requested root. The tree is
    /// not uniform, so no caller may assume "depth N == sections".
    pub depth: usize,
    /// Advisory label: `segment`, else `docId`. Composed at response time, so
    /// the frontend can build a better one from the raw fields if it wants.
    pub label: String,
    /// Statuses carried by change rows whose `doc_mpath` *equals* this node's
    /// mpath, in this version. Descendant statuses never bubble up, and `[]` —
    /// meaning "present only because a descendant changed" — is meaningful.
    pub statuses: Vec<String>,
    /// Whether anything exists below this node that the client can expand to.
    /// `true` with an empty `children` is the depth frontier: fetch deeper.
    pub has_children: bool,
    /// Nested children, already natural-sorted.
    pub children: Vec<Node>,
}

/// Response for the menu endpoint.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Menu {
    /// Url path of the root, without its leading slash.
    pub path: String,
    /// Publication the menu was served from.
    pub active_publication: String,
    /// Which driver produced this response.
    pub mode: Mode,
    /// Resolved version date. `null` in tree mode.
    pub date: Option<String>,
    /// Human rendering of [`Menu::date`]. `null` in tree mode.
    pub display: Option<String>,
    /// What the requested path resolved to.
    pub root_kind: RootKind,
    /// Levels actually served, after clamping. A `collection` root always
    /// reports `2` and ignores `requestedDepth`, while `maxDepth` keeps
    /// reporting the document-mode ceiling.
    pub depth: usize,
    /// Levels the client asked for. Ignored for a `collection` root.
    pub requested_depth: u32,
    /// The server's hard depth ceiling.
    pub max_depth: usize,
    /// How many nodes this response contains.
    pub node_count: usize,
    /// Whether the node ceiling dropped anything.
    pub truncated: bool,
    /// The requested path's own node.
    pub root: Node,
}

/// Everything the response layer needs that the tree does not supply.
#[derive(Debug)]
pub struct Context {
    /// Url path of the root, without its leading slash.
    pub path: String,
    /// Publication the menu was served from.
    pub active_publication: String,
    /// Which driver produced this response.
    pub mode: Mode,
    /// Resolved version date; `None` in tree mode.
    pub date: Option<String>,
    /// What the requested path resolved to.
    pub root_kind: RootKind,
    /// Levels actually served, after clamping.
    pub depth: usize,
    /// Levels the client asked for.
    pub requested_depth: u32,
}

impl Menu {
    /// Assemble a menu response from a synthesized node set and its layout.
    #[must_use]
    pub fn build(
        context: Context,
        node_set: &NodeSet,
        layout: &Layout,
        base: &str,
        facts: &BTreeMap<String, Facts>,
    ) -> Self {
        let root = Node::build(base, node_set, layout, facts);
        Self {
            path: context.path,
            active_publication: context.active_publication,
            mode: context.mode,
            display: context.date.as_deref().map(format_date),
            date: context.date,
            root_kind: context.root_kind,
            depth: context.depth,
            requested_depth: context.requested_depth,
            max_depth: MAX_MENU_DEPTH,
            node_count: layout.node_count,
            truncated: layout.truncated,
            root,
        }
    }
}

impl Node {
    /// Build one node and, recursively, its retained children.
    ///
    /// A node with no backing row is still emitted with a null `url` and a
    /// `warn!`: dropping a synthesized ancestor would orphan its children.
    #[must_use]
    pub fn build(
        mpath: &str,
        node_set: &NodeSet,
        layout: &Layout,
        facts: &BTreeMap<String, Facts>,
    ) -> Self {
        let found = facts.get(mpath);
        if found.is_none() {
            tracing::warn!("No element row backing menu node {mpath}; serving it with a null url");
        }
        let resolved = found.cloned().unwrap_or_default();
        let segment = last_segment(mpath).to_owned();
        let children: Vec<Self> = layout
            .children
            .get(mpath)
            .map_or_else(Vec::new, |siblings| {
                siblings
                    .iter()
                    .map(|child| Self::build(child, node_set, layout, facts))
                    .collect()
            });
        Self {
            label: label_for(&segment, resolved.doc_id.as_deref()),
            depth: mpath_depth(mpath),
            statuses: status_tokens(node_set, mpath),
            has_children: node_set.has_descendant.contains(mpath),
            mpath: mpath.to_owned(),
            segment,
            doc_id: resolved.doc_id,
            url: resolved.url,
            children,
        }
    }
}

/// Advisory label: the last mpath segment, else `docId`.
fn label_for(segment: &str, doc_id: Option<&str>) -> String {
    if segment.trim().is_empty() {
        return doc_id.unwrap_or_default().to_owned();
    }
    segment.to_owned()
}

/// Render a node's direct statuses as stable lowercase API tokens, ordered by
/// [`Status::to_int`] and deduplicated by the backing `BTreeSet`.
fn status_tokens(node_set: &NodeSet, mpath: &str) -> Vec<String> {
    node_set
        .direct_statuses
        .get(mpath)
        .map_or_else(Vec::new, |statuses| {
            statuses
                .iter()
                .map(|value| {
                    Status::from_int(*value).map_or_else(
                        || {
                            tracing::warn!(
                                "Unknown document_change.status {value} on mpath {mpath}"
                            );
                            UNKNOWN_STATUS_TOKEN.to_owned()
                        },
                        |status| status.as_api_str().to_owned(),
                    )
                })
                .collect()
        })
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests"
)]
mod tests {
    use super::{label_for, status_tokens, Context, Facts, Menu, Mode, Node, RootKind};
    use crate::db::models::document_change::VersionChange;
    use crate::server::api::menu::tree::{plan, synthesize_dated};
    use std::collections::BTreeMap;

    /// Document root used by the fixtures below.
    const ROOT: &str = "City of San Mateo, Cal., Code|";

    /// Facts for an element at `url`.
    fn located(url: &str) -> Facts {
        Facts {
            doc_id: Some("City of San Mateo, Cal., Code".to_owned()),
            url: Some(url.to_owned()),
        }
    }

    #[test]
    fn label_is_the_segment_falling_back_to_doc_id() {
        assert_eq!(label_for("10", Some("Code")), "10");
        assert_eq!(label_for("10.04", None), "10.04");
        assert_eq!(label_for("", Some("Code")), "Code");
        assert_eq!(label_for("   ", Some("Code")), "Code");
        assert_eq!(label_for("", None), "");
    }

    #[test]
    fn unknown_status_integers_serve_a_token_instead_of_failing() {
        let changes = [VersionChange::new(format!("{ROOT}27|"), 7)];
        let node_set = synthesize_dated(&changes, ROOT, 1);
        assert_eq!(status_tokens(&node_set, &format!("{ROOT}27|")), ["unknown"]);
    }

    #[test]
    fn status_tokens_are_ordered_by_their_integer_not_insertion_order() {
        let changes = [
            VersionChange::new(format!("{ROOT}27|"), 3),
            VersionChange::new(format!("{ROOT}27|"), 0),
        ];
        let node_set = synthesize_dated(&changes, ROOT, 1);
        assert_eq!(
            status_tokens(&node_set, &format!("{ROOT}27|")),
            ["added", "removed"]
        );
    }

    #[test]
    fn multi_status_elements_serve_added_then_effective() {
        let changes = [
            VersionChange::new(format!("{ROOT}27|"), 1),
            VersionChange::new(format!("{ROOT}27|"), 0),
        ];
        let node_set = synthesize_dated(&changes, ROOT, 1);
        assert_eq!(
            status_tokens(&node_set, &format!("{ROOT}27|")),
            ["added", "effective"]
        );
    }

    #[test]
    fn a_node_with_no_element_row_is_served_with_null_url_and_segment_label() {
        let changes = [VersionChange::new(format!("{ROOT}27|27.21|"), 2)];
        let node_set = synthesize_dated(&changes, ROOT, 2);
        let layout = plan(&node_set, ROOT, 5_000);
        let facts: BTreeMap<String, Facts> = BTreeMap::new();

        let root = Node::build(ROOT, &node_set, &layout, &facts);
        assert!(root.url.is_none());
        assert!(root.doc_id.is_none());
        assert_eq!(root.label, "City of San Mateo, Cal., Code");
        let title = root.children.first().expect("title present");
        assert_eq!(title.label, "27");
        assert!(title.children.first().is_some());
    }

    #[test]
    fn built_tree_reports_absolute_depths_and_frontier_flags() {
        let changes = [VersionChange::new(format!("{ROOT}10|10.04|10.04.010|"), 2)];
        let node_set = synthesize_dated(&changes, ROOT, 2);
        let layout = plan(&node_set, ROOT, 5_000);
        let mut facts: BTreeMap<String, Facts> = BTreeMap::new();
        facts.insert(ROOT.to_owned(), located("/us/ca/cities/san-mateo/code"));
        facts.insert(
            format!("{ROOT}10|"),
            located("/us/ca/cities/san-mateo/code/10"),
        );
        facts.insert(
            format!("{ROOT}10|10.04|"),
            located("/us/ca/cities/san-mateo/code/10.04"),
        );

        let root = Node::build(ROOT, &node_set, &layout, &facts);
        assert_eq!(root.depth, 1);
        assert!(root.statuses.is_empty());
        assert!(root.has_children);
        assert_eq!(root.label, "City of San Mateo, Cal., Code");

        let title = root.children.first().expect("title present");
        assert_eq!(title.depth, 2);
        assert_eq!(title.label, "10");
        assert!(title.statuses.is_empty());
        assert!(title.has_children);

        let chapter = title.children.first().expect("chapter present");
        assert_eq!(chapter.depth, 3);
        assert_eq!(chapter.label, "10.04");
        assert!(chapter.has_children);
        assert!(chapter.children.is_empty());
    }

    #[test]
    fn menu_serializes_with_camel_case_keys_and_a_formatted_display_date() {
        let changes = [VersionChange::new(format!("{ROOT}27|"), 2)];
        let node_set = synthesize_dated(&changes, ROOT, 1);
        let layout = plan(&node_set, ROOT, 5_000);
        let facts: BTreeMap<String, Facts> = BTreeMap::new();
        let menu = Menu::build(
            Context {
                path: "us/ca/cities/san-mateo/code".to_owned(),
                active_publication: "current".to_owned(),
                mode: Mode::Date,
                date: Some("2026-02-03".to_owned()),
                root_kind: RootKind::Document,
                depth: 1,
                requested_depth: 1,
            },
            &node_set,
            &layout,
            ROOT,
            &facts,
        );

        assert_eq!(menu.display.as_deref(), Some("February 03, 2026"));
        let json = serde_json::to_value(&menu).expect("menu serializes");
        assert_eq!(
            json.get("mode").and_then(serde_json::Value::as_str),
            Some("date")
        );
        assert_eq!(
            json.get("rootKind").and_then(serde_json::Value::as_str),
            Some("document")
        );
        assert!(json.get("requestedDepth").is_some());
        assert!(json.get("nodeCount").is_some());
        assert!(json
            .get("root")
            .and_then(|root| root.get("hasChildren"))
            .is_some());
        assert_eq!(
            json.get("root")
                .and_then(|root| root.get("statuses"))
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(0)
        );
    }

    #[test]
    fn tree_mode_menus_carry_no_date_or_display() {
        let node_set = synthesize_dated(&[], ROOT, 1);
        let layout = plan(&node_set, ROOT, 5_000);
        let facts: BTreeMap<String, Facts> = BTreeMap::new();
        let menu = Menu::build(
            Context {
                path: "us/ca/cities/san-mateo/code".to_owned(),
                active_publication: "current".to_owned(),
                mode: Mode::Tree,
                date: None,
                root_kind: RootKind::Document,
                depth: 2,
                requested_depth: 2,
            },
            &node_set,
            &layout,
            ROOT,
            &facts,
        );
        assert!(menu.date.is_none());
        assert!(menu.display.is_none());
        assert_eq!(menu.mode, Mode::Tree);
        assert_eq!(menu.node_count, 1);
    }
}
