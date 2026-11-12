//! Node-set synthesis: the rollup, the depth frontier, and mpath-based depth.

use super::{
    child, child_segments, contains_mpath, dated_uri, find_mpath, fixture, has_children, statuses,
    CODE_DOC, CODE_URL, DATE_LATE, DATE_MAIN, MOHICAN_DOC, MOHICAN_URL,
};
use crate::common;
use actix_web::http::StatusCode;

/// A change recorded only at depth 4 makes its depth-2 and depth-3 ancestors
/// appear with `statuses: []` and `hasChildren: true`, and nothing else.
#[actix_web::test]
async fn test_menu_change_at_depth_four_expect_ancestors_rolled_up() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, CODE_URL, Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);

    assert_eq!(statuses(resp.root()), Vec::<String>::new());
    assert!(has_children(resp.root()));
    assert_eq!(child_segments(resp.root()), vec!["10".to_owned()]);

    let title = child(resp.root(), "10");
    assert_eq!(title["mpath"], format!("{CODE_DOC}|10|"));
    assert_eq!(title["depth"], 2);
    assert_eq!(statuses(title), Vec::<String>::new());
    assert!(has_children(title));

    let chapter = child(title, "10.04");
    assert_eq!(chapter["depth"], 3);
    assert_eq!(statuses(chapter), Vec::<String>::new());
    assert!(has_children(chapter));

    // Titles with no changed descendants are absent entirely.
    assert!(!contains_mpath(resp.root(), &format!("{CODE_DOC}|5|")));
    assert_eq!(resp.json()["nodeCount"], 4);
    assert_eq!(resp.json()["truncated"], false);
    drop(app);
}

/// The frontier: a change below the served window marks the frontier node
/// `hasChildren: true, children: []` and the deeper node does not leak in.
#[actix_web::test]
async fn test_menu_change_below_window_expect_frontier_and_no_leak() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, CODE_URL, Some("2"))).await;
    let chapter = child(child(resp.root(), "10"), "10.04");
    assert!(has_children(chapter));
    assert_eq!(child_segments(chapter), Vec::<String>::new());
    assert!(!contains_mpath(
        resp.root(),
        &format!("{CODE_DOC}|10|10.04|10.04.010|")
    ));
    drop(app);
}

/// Asking one level deeper materialises exactly what the shallower response
/// flagged as frontier, and the depth-4 node is then a change leaf.
#[actix_web::test]
async fn test_menu_deeper_request_expect_frontier_nodes_materialise() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, CODE_URL, Some("3"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);
    assert_eq!(resp.json()["depth"], 3);

    let section = find_mpath(resp.root(), &format!("{CODE_DOC}|10|10.04|10.04.010|"))
        .expect("the depth-4 section is inside a depth=3 window");
    assert_eq!(section["depth"], 4);
    assert_eq!(statuses(section), vec!["changed".to_owned()]);
    assert!(!has_children(section));
    assert_eq!(resp.json()["nodeCount"], 6);
    drop(app);
}

/// Levels are counted on the mpath, never on URL segments: a depth-1 node
/// behind a seven-segment URL, whose unchanged child element must not make it
/// look expandable.
#[actix_web::test]
async fn test_menu_depth_one_mpath_behind_deep_url() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_LATE, MOHICAN_URL, Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);
    assert_eq!(resp.root()["mpath"], format!("{MOHICAN_DOC}|"));
    assert_eq!(resp.root()["segment"], MOHICAN_DOC);
    assert_eq!(resp.root()["depth"], 1);
    assert_eq!(statuses(resp.root()), vec!["changed".to_owned()]);
    assert!(!has_children(resp.root()));
    assert_eq!(resp.json()["nodeCount"], 1);
    drop(app);
}
