//! Natural sort, keyed on the last mpath segment.

use super::{child, child_segments, dated_uri, fixture, DATE_MAIN};
use crate::common;
use actix_web::http::StatusCode;

/// Titles order by magnitude rather than bytes, and a shorter fraction run
/// sorts before a longer one.
#[actix_web::test]
async fn test_menu_natural_sort() {
    let fix = fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, "/sort", Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);
    assert_eq!(
        child_segments(resp.root()),
        vec![
            "1".to_owned(),
            "2".to_owned(),
            "10".to_owned(),
            "27".to_owned()
        ]
    );
    assert_eq!(
        child_segments(child(resp.root(), "10")),
        vec!["10.04".to_owned(), "10.100".to_owned()]
    );
    drop(app);
}
