//! Date handling: scoping, and the three outcomes that must stay distinct.

use super::{
    child_segments, contains_mpath, dated_uri, fixture, has_children, statuses, CODE_DOC, CODE_URL,
    DATE_ABSENT, DATE_LATE, DATE_MAIN,
};
use crate::common;
use actix_web::http::StatusCode;

/// A node whose only change is on another date must not appear.
#[actix_web::test]
async fn test_menu_node_changed_on_another_date_expect_absent() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, CODE_URL, Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);
    assert!(!contains_mpath(resp.root(), &format!("{CODE_DOC}|1|")));
    drop(app);
}

/// A date with no `publication_version` row is a 404 naming the date.
#[actix_web::test]
async fn test_menu_date_absent_from_publication_expect_not_found() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_ABSENT, CODE_URL, Some("2"))).await;
    assert_eq!(resp.status, StatusCode::NOT_FOUND, "body: {}", resp.body);
    assert!(resp.body.contains(DATE_ABSENT), "body: {}", resp.body);
    drop(app);
}

/// A date that exists but has no changes under this path is a 200 with a lone
/// root -- the distinction the frontend needs, and not a 404.
#[actix_web::test]
async fn test_menu_date_exists_but_subtree_quiet_expect_empty_root() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_LATE, CODE_URL, Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);
    assert_eq!(resp.root()["mpath"], format!("{CODE_DOC}|"));
    assert_eq!(statuses(resp.root()), Vec::<String>::new());
    assert!(!has_children(resp.root()));
    assert_eq!(child_segments(resp.root()), Vec::<String>::new());
    assert_eq!(resp.json()["nodeCount"], 1);
    drop(app);
}

/// A path backing neither a document element nor a library is a 404.
#[actix_web::test]
async fn test_menu_unknown_path_expect_not_found() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, "/no/such/document", Some("2"))).await;
    assert_eq!(resp.status, StatusCode::NOT_FOUND, "body: {}", resp.body);
    drop(app);
}

/// Date absence is a 400 naming the legal forms, never a quiet whole-tree
/// response -- the failure mode the endpoint is designed to make unreachable.
#[actix_web::test]
async fn test_menu_without_date_expect_bad_request_naming_legal_forms() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &format!("/_api/menu{CODE_URL}")).await;
    assert_eq!(resp.status, StatusCode::BAD_REQUEST, "body: {}", resp.body);
    assert!(
        resp.body.contains("_date") && resp.body.contains("_tree"),
        "body: {}",
        resp.body
    );
    drop(app);
}
