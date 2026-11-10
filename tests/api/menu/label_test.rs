//! Labels. `label` is the last mpath segment, falling back to `docId`.

use super::{child, dated_uri, fixture, CODE_DOC, CODE_URL, DATE_MAIN};
use crate::common;
use actix_web::http::StatusCode;

/// Every node labels itself from its own mpath segment.
#[actix_web::test]
async fn test_menu_label_falls_back_to_the_mpath_segment() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, CODE_URL, Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);

    assert_eq!(resp.root()["segment"], CODE_DOC);
    assert_eq!(resp.root()["label"], CODE_DOC);
    assert_eq!(resp.root()["docId"], CODE_DOC);
    assert_eq!(resp.root()["url"], CODE_URL);

    let title = child(resp.root(), "10");
    assert_eq!(title["segment"], "10");
    assert_eq!(title["label"], "10");
    assert_eq!(title["url"], format!("{CODE_URL}/10"));
    drop(app);
}
