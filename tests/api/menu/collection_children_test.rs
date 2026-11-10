//! `hasChildren` on member documents reached through a collection.
//!
//! The bridge under-reports: `changed_library_document` records only elements
//! that changed as part of a library change, which for San Mateo is always the
//! document root. In tree mode `hasChildren` means "has any child in the
//! tree", so the element tree is the authority; in dated mode it means "has
//! changed descendants in this version", so the bridge is.

use super::{
    child, child_segments, dated_uri, fixture, has_children, statuses, tree_uri, DATE_MAIN,
    RESOLUTIONS_MPATH, RESOLUTIONS_URL,
};
use crate::common;
use actix_web::http::StatusCode;

/// Member document roots, in natural-sort order.
const MEMBERS: [&str; 3] = ["Res. No. 2026-01", "Res. No. 2026-02", "Res. No. 2026-03"];

/// A member whose element tree has deeper rows is expandable in tree mode even
/// though its only bridge row is its own depth-1 root, and it gives the same
/// answer when addressed directly.
#[actix_web::test]
async fn test_tree_collection_member_with_element_children_is_expandable() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &tree_uri(RESOLUTIONS_URL, Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);
    assert_eq!(resp.json()["mode"], "tree");
    assert_eq!(resp.json()["rootKind"], "collection");
    assert_eq!(resp.root()["mpath"], RESOLUTIONS_MPATH);

    let year = child(resp.root(), "2026");
    assert_eq!(
        child_segments(year),
        MEMBERS
            .iter()
            .map(|seg| (*seg).to_owned())
            .collect::<Vec<String>>()
    );

    let member = child(year, MEMBERS[0]);
    assert_eq!(member["mpath"], "Res. No. 2026-01|");
    assert!(has_children(member));
    assert_eq!(statuses(member), Vec::<String>::new());
    assert_eq!(
        child_segments(member),
        Vec::<String>::new(),
        "collections still stop at two levels"
    );

    let direct = super::get(
        &app,
        &tree_uri(&format!("{RESOLUTIONS_URL}/2026/1"), Some("2")),
    )
    .await;
    assert_eq!(direct.status, StatusCode::OK, "body: {}", direct.body);
    assert!(
        has_children(direct.root()),
        "the same node must not answer differently depending on how it was reached"
    );
    assert_eq!(
        child_segments(direct.root()),
        vec!["1".to_owned(), "2".to_owned()]
    );
    drop(app);
}

/// A genuinely single-element member stays a leaf, so the positive case cannot
/// be satisfied by hardcoding `true`.
#[actix_web::test]
async fn test_tree_collection_single_element_member_stays_a_leaf() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &tree_uri(RESOLUTIONS_URL, Some("2"))).await;
    let member = child(child(resp.root(), "2026"), MEMBERS[1]);
    assert_eq!(member["mpath"], "Res. No. 2026-02|");
    assert!(!has_children(member));

    let direct = super::get(
        &app,
        &tree_uri(&format!("{RESOLUTIONS_URL}/2026/2"), Some("2")),
    )
    .await;
    assert!(!has_children(direct.root()));
    drop(app);
}

/// The override also clears a `true` the bridge asserted when this fonds's
/// element tree has nothing deeper, so a client is never sent after an empty
/// response.
#[actix_web::test]
async fn test_tree_collection_clears_a_stale_bridge_true() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &tree_uri(RESOLUTIONS_URL, Some("2"))).await;
    let member = child(child(resp.root(), "2026"), MEMBERS[2]);
    assert_eq!(member["mpath"], "Res. No. 2026-03|");
    assert!(!has_children(member));

    let direct = super::get(
        &app,
        &tree_uri(&format!("{RESOLUTIONS_URL}/2026/3"), Some("2")),
    )
    .await;
    assert!(!has_children(direct.root()));
    assert_eq!(child_segments(direct.root()), Vec::<String>::new());
    drop(app);
}

/// Dated mode is unaffected: `hasChildren` still means "has changed
/// descendants in this version" and still comes from the bridge. Two of the
/// three members answer the opposite of their tree-mode answer.
#[actix_web::test]
async fn test_dated_collection_member_children_still_come_from_the_bridge() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let dated = super::get(&app, &dated_uri(DATE_MAIN, RESOLUTIONS_URL, Some("2"))).await;
    assert_eq!(dated.status, StatusCode::OK, "body: {}", dated.body);
    assert_eq!(dated.json()["mode"], "date");
    assert_eq!(dated.json()["rootKind"], "collection");
    assert_eq!(dated.json()["depth"], 2);
    let dated_year = child(dated.root(), "2026");

    let tree = super::get(&app, &tree_uri(RESOLUTIONS_URL, Some("2"))).await;
    let tree_year = child(tree.root(), "2026");

    for (segment, dated_expected, tree_expected) in [
        (MEMBERS[0], false, true),
        (MEMBERS[1], false, false),
        (MEMBERS[2], true, false),
    ] {
        assert_eq!(
            has_children(child(dated_year, segment)),
            dated_expected,
            "dated hasChildren for {segment}"
        );
        assert_eq!(
            has_children(child(tree_year, segment)),
            tree_expected,
            "tree hasChildren for {segment}"
        );
    }

    assert_eq!(
        statuses(child(dated_year, MEMBERS[0])),
        vec!["added".to_owned()]
    );
    assert_eq!(
        statuses(child(dated_year, MEMBERS[2])),
        Vec::<String>::new(),
        "only something inside 2026-03 changed, so its root has no status"
    );
    drop(app);
}
