//! Prefix safety: Query 1's `LIKE` has no `ESCAPE`, so a Rust-side
//! `starts_with` guard has to drop the over-matches.

use super::{child_segments, contains_mpath, dated_uri, fixture, DATE_MAIN};
use crate::common;
use actix_web::http::StatusCode;

/// `_` is a single-character LIKE wildcard, so `A_B|%` also matches `AXB|leak|`.
#[actix_web::test]
async fn test_menu_like_wildcard_in_mpath_expect_no_sibling_leak() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, "/prefix/a_b", Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);
    assert_eq!(resp.root()["mpath"], "A_B|");
    assert_eq!(child_segments(resp.root()), vec!["child".to_owned()]);
    assert!(!contains_mpath(resp.root(), "AXB|leak|"));
    assert_eq!(resp.json()["nodeCount"], 2);
    drop(app);
}
