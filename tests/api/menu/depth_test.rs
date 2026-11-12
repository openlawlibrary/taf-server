//! The `depth` query parameter: clamp a well-formed value, 400 a malformed one.

use super::{dated_uri, fixture, CODE_URL, DATE_MAIN};
use crate::common;
use actix_web::http::StatusCode;

/// Absent defaults to 2 and an out-of-policy value is clamped, with both the
/// served and the requested depth echoed.
#[actix_web::test]
async fn test_menu_depth_is_clamped_and_echoed() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let default = super::get(&app, &dated_uri(DATE_MAIN, CODE_URL, None)).await;
    assert_eq!(default.status, StatusCode::OK, "body: {}", default.body);
    assert_eq!(default.json()["depth"], 2);
    assert_eq!(default.json()["requestedDepth"], 2);

    let clamped = super::get(&app, &dated_uri(DATE_MAIN, CODE_URL, Some("7"))).await;
    assert_eq!(clamped.status, StatusCode::OK, "body: {}", clamped.body);
    assert_eq!(clamped.json()["depth"], 6);
    assert_eq!(clamped.json()["requestedDepth"], 7);
    assert_eq!(clamped.json()["maxDepth"], 6);
    drop(app);
}

/// A `depth` that is not a `u32` is a 400, not a clamp.
#[actix_web::test]
async fn test_menu_malformed_depth_expect_bad_request() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, CODE_URL, Some("-1"))).await;
    assert_eq!(resp.status, StatusCode::BAD_REQUEST, "body: {}", resp.body);
    drop(app);
}
