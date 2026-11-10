use std::path::Path;
use std::sync::{Arc, RwLock};

use actix_http::header::LOCATION;
use actix_http::StatusCode;
use actix_web::test;
use taf_server::db::models::redirects::Manager as _;
use taf_server::fonds::archive::Archive;
use taf_server::server::api::state::App;
use taf_server::server::app;
use tempfile::TempDir;

use crate::archive_testtools::config::{ArchiveType, Jurisdiction};
use crate::archive_testtools::get_repository;
use crate::common;
use crate::common::db_data::{insert_fonds, insert_publication};

const FONDS: &str = "test_org/law";
const OLD_PUB: &str = "2020-01-01";
const LATEST_PUB: &str = "2024-01-01";

/// Archive whose `law-html` repo has `doc/index.html`, with two publications
/// and one commit, codified on 2015-01-01, for the older publication.
async fn initialize_date_archive() -> TempDir {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let html_path = archive_path.path().join("test_org");
    let git_repo = get_repository(&html_path, "law-html");
    let doc_page = r#"<html><body><main id="area__content"><p>doc</p></main></body></html>"#;
    git_repo
        .add_file(
            &html_path.join("law-html").join("doc"),
            "index.html",
            doc_page,
        )
        .unwrap();
    let commit = git_repo.commit(None, "Adding doc").unwrap();

    let db = common::get_db(archive_path.path()).await;
    insert_fonds(&db, FONDS).await;
    for name in [OLD_PUB, LATEST_PUB] {
        insert_publication(
            &db,
            name,
            name,
            name,
            FONDS,
            false,
            Some("test_org/law-html"),
        )
        .await;
    }
    sqlx::query(
        "INSERT INTO data_repo_commits (commit_hash, codified_date, build_date, repo_type, auth_commit_hash, auth_commit_timestamp, publication_id) VALUES (?, '2015-01-01', '2015-01-01', 'html', 'auth', '2015-01-01 00:00:00 UTC', ?)",
    )
    .bind(commit.to_string())
    .bind(OLD_PUB)
    .execute(&db.pool)
    .await
    .unwrap();
    archive_path
}

async fn get_date(archive_path: &Path, uri: &str) -> (StatusCode, Option<String>, String) {
    let archive = Archive::parse(archive_path.to_path_buf(), archive_path, false).unwrap();
    let db = common::get_db(archive_path).await;
    let repos_with_redirects = db.repos_with_redirects().await.unwrap_or_default();
    let state = App {
        archive,
        db,
        repos_with_redirects: Arc::new(RwLock::new(repos_with_redirects)),
    };
    let app = test::init_service(app::init(&state).unwrap()).await;
    let req = test::TestRequest::get()
        .uri(uri)
        .insert_header(("X-Fonds", FONDS))
        .insert_header(("Forwarded", "host=other.example;proto=javascript"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let status = resp.status();
    let location = resp
        .headers()
        .get(LOCATION)
        .map(|value| value.to_str().unwrap().to_owned());
    let body = String::from_utf8(test::read_body(resp).await.to_vec()).unwrap();
    (status, location, body)
}

#[actix_web::test]
async fn test_date_where_publication_is_outdated_expect_relative_banner_link() {
    let archive_path = initialize_date_archive().await;

    let (status, _, body) = get_date(
        archive_path.path(),
        "/_publication/2020-01-01/_date/2020-01-01/doc",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#"<a href="/_publication/2024-01-01/_date/2020-01-01/doc">here</a>"#),
        "{body}"
    );
    assert!(!body.contains("other.example"), "{body}");
}

#[actix_web::test]
async fn test_date_where_date_is_not_canonical_expect_bad_request() {
    let archive_path = initialize_date_archive().await;

    for date in ["2019-02-29", "2021-2-20", "+2021-02-20", "1%3Cimg%3E"] {
        let uri = format!("/_publication/2020-01-01/_date/{date}/doc");
        let (actual, _, _) = get_date(archive_path.path(), &uri).await;
        let expected = StatusCode::BAD_REQUEST;
        assert_eq!(actual, expected, "date {date}");
    }
}

#[actix_web::test]
async fn test_date_where_date_precedes_first_version_expect_not_found() {
    let archive_path = initialize_date_archive().await;

    let (actual, _, _) = get_date(
        archive_path.path(),
        "/_publication/2020-01-01/_date/1900-01-01/doc",
    )
    .await;
    let expected = StatusCode::NOT_FOUND;
    assert_eq!(actual, expected);
}

#[actix_web::test]
async fn test_date_where_publication_is_unknown_expect_redirect_to_latest() {
    let archive_path = initialize_date_archive().await;

    let (status, location, _) = get_date(
        archive_path.path(),
        "/_publication/unknown%22%3E/_date/2020-01-01/doc",
    )
    .await;

    let actual = (status, location.as_deref());
    let expected = (
        StatusCode::FOUND,
        Some("/_publication/2024-01-01/_date/2020-01-01/doc"),
    );
    assert_eq!(actual, expected);
}

#[actix_web::test]
async fn test_date_where_commit_has_no_codified_date_expect_found() {
    let archive_path = initialize_date_archive().await;
    let db = common::get_db(archive_path.path()).await;
    sqlx::query(
        "INSERT INTO data_repo_commits (commit_hash, codified_date, build_date, repo_type, auth_commit_hash, auth_commit_timestamp, publication_id) SELECT commit_hash, NULL, NULL, 'html', 'auth-undated', auth_commit_timestamp, ? FROM data_repo_commits",
    )
    .bind(LATEST_PUB)
    .execute(&db.pool)
    .await
    .unwrap();

    let (actual, _, _) = get_date(
        archive_path.path(),
        "/_publication/2024-01-01/_date/2020-01-01/doc",
    )
    .await;
    let expected = StatusCode::OK;
    assert_eq!(actual, expected);
}
