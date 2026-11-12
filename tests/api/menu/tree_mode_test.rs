//! Undated browse mode: `/_api/menu/_tree/{path}`.

use super::{
    child, child_segments, contains_mpath, fixture, has_children, statuses, tree_uri, CODE_DOC,
    CODE_URL,
};
use crate::common;
use actix_web::http::StatusCode;

/// Tree mode is not date-filtered: it serves nodes that never changed, and
/// `hasChildren` reverts to "has any child in the tree".
#[actix_web::test]
async fn test_menu_tree_mode_serves_unchanged_nodes() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &tree_uri(CODE_URL, Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);
    assert_eq!(resp.json()["mode"], "tree");
    assert_eq!(
        child_segments(resp.root()),
        vec!["1".to_owned(), "5".to_owned(), "10".to_owned()]
    );
    assert!(contains_mpath(resp.root(), &format!("{CODE_DOC}|5|5.04|")));

    let title_1 = child(resp.root(), "1");
    assert!(!has_children(title_1));
    assert_eq!(statuses(title_1), Vec::<String>::new());
    drop(app);
}
