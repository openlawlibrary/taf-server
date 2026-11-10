use crate::{
    archive_testtools::{
        self, add_redirects_json_file,
        config::{ArchiveType, MultihostConfig},
        utils,
    },
    common,
    common::db_data::insert_redirects,
    common::get_db,
};
use actix_http::header::IF_NONE_MATCH;
use actix_http::StatusCode;
use actix_web::test;
use std::path::PathBuf;
use taf_server::server::headers::HTTP_E_TAG;

#[actix_web::test]
async fn test_resolve_both_guarded_fonds_law_html_request_with_full_path_expect_success() {
    let archive_path =
        common::initialize_archive(ArchiveType::Multihost(MultihostConfig::Public)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    for guard_value in [
        "fonds_1/law",
        "fonds_2/law",
        "fonds_1_1/law",
        "fonds_1_2/law",
    ] {
        for request_uri in &["/a/b/c.html", "/a/b/", "/a/b/c/", "/a/d/"] {
            let req = test::TestRequest::get()
                .insert_header(("X-Current-Documents-Guard", guard_value))
                .uri(request_uri)
                .to_request();
            let resp = test::call_service(&app, req).await;
            let actual = resp.status().is_success();
            let expected = true;
            assert_eq!(actual, expected);
        }
    }
}

#[actix_web::test]
async fn test_resolve_guarded_fonds_law_html_request_where_header_value_is_incorrect_expect_error()
{
    let archive_path =
        common::initialize_archive(ArchiveType::Multihost(MultihostConfig::Public)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    let req = test::TestRequest::get()
        .insert_header(("X-Current-Documents-Guard", "xxx/xxx"))
        .uri("/a/b/c.html")
        .to_request();
    let resp = test::call_service(&app, req).await;
    let actual = resp.status().is_client_error();
    let expected = true;
    assert_eq!(actual, expected);
}

#[actix_web::test]
async fn test_resolve_guarded_fonds_law_html_request_where_header_name_is_incorrect_expect_error() {
    let archive_path =
        common::initialize_archive(ArchiveType::Multihost(MultihostConfig::Public)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    let req = test::TestRequest::get()
        .insert_header(("X-Incorrect-Header-Name", "fonds_1/law"))
        .uri("/a/b/c.html")
        .to_request();
    let resp = test::call_service(&app, req).await;
    let actual = resp.status().is_client_error();
    let expected = true;
    assert_eq!(actual, expected);
}

#[actix_web::test]
async fn test_resolve_guarded_fonds_law_rdf_request_content_expect_rdf_document_retrieved() {
    let archive_path =
        common::initialize_archive(ArchiveType::Multihost(MultihostConfig::Public)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    for guard_value in [
        "fonds_1/law",
        "fonds_2/law",
        "fonds_1_1/law",
        "fonds_1_2/law",
    ] {
        for request_uri in &[
            "/_rdf/index.rdf",
            "/_rdf/a/b/c.rdf",
            "/_rdf/a/b/index.rdf",
            "/_rdf/a/d/index.rdf",
            "/_rdf/a/b/c/index.rdf",
        ] {
            let req = test::TestRequest::get()
                .insert_header(("X-Current-Documents-Guard", guard_value))
                .uri(request_uri)
                .to_request();
            let actual = test::call_and_read_body(&app, req).await;
            let expected = "<?xml version=\"1.0\"?>";
            assert!(
                common::blob_to_string(actual.to_vec()).starts_with(expected),
                "doesn't start with {expected}"
            );
        }
    }
}

#[actix_web::test]
async fn test_law_other_data_request_content_expect_other_document_retrieved() {
    let archive_path =
        common::initialize_archive(ArchiveType::Multihost(MultihostConfig::Public)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    for guard_value in [
        "fonds_1/law",
        "fonds_2/law",
        "fonds_1_1/law",
        "fonds_1_2/law",
    ] {
        for request_uri in &[
            "/_prefix/a/index.html",
            "/a/_doc/e/index.html",
            "/a/e/_doc/f/index.html",
        ] {
            let req = test::TestRequest::get()
                .insert_header(("X-Current-Documents-Guard", guard_value))
                .uri(request_uri)
                .to_request();
            let actual = test::call_and_read_body(&app, req).await;
            let expected = "<!DOCTYPE html>";
            assert!(
                common::blob_to_string(actual.to_vec()).starts_with(expected),
                "doesn't start with {expected}"
            );
        }
    }
}

#[actix_web::test]
async fn test_law_other_data_request_content_with_cycle_expect_other_document_retrieved() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Multihost(MultihostConfig::Public))
            .unwrap();
    // Add a cycle
    // fonds_1 -> fonds_1_1 -> fonds_1
    // Expect that the cycle is resolved
    archive_testtools::add_dependencies(archive_path.path(), "fonds_1_1", vec!["fonds_1"], None)
        .unwrap();
    utils::make_all_git_repos_bare_recursive(&archive_path).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    for guard_value in [
        "fonds_1/law",
        "fonds_2/law",
        "fonds_1_1/law",
        "fonds_1_2/law",
    ] {
        for request_uri in &[
            "/_prefix/a/index.html",
            "/a/_doc/e/index.html",
            "/a/e/_doc/f/index.html",
        ] {
            let req = test::TestRequest::get()
                .insert_header(("X-Current-Documents-Guard", guard_value))
                .uri(request_uri)
                .to_request();
            let actual = test::call_and_read_body(&app, req).await;
            let expected = "<!DOCTYPE html>";
            assert!(
                common::blob_to_string(actual.to_vec()).starts_with(expected),
                "doesn't start with {expected}"
            );
        }
    }
}

#[actix_web::test]
async fn get_law_html_request_with_no_if_no_match_header_expect_new_etag() {
    let archive_path =
        common::initialize_archive(ArchiveType::Multihost(MultihostConfig::Public)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    for guard_value in [
        "fonds_1/law",
        "fonds_2/law",
        "fonds_1_1/law",
        "fonds_1_2/law",
    ] {
        for request_uri in &["/a/b/c.html", "/a/b/", "/a/b/c/", "/a/d/"] {
            let req = test::TestRequest::get()
                .insert_header(("X-Current-Documents-Guard", guard_value))
                .uri(request_uri)
                .to_request();
            let resp = test::call_service(&app, req).await;
            let etag = resp.headers().get(HTTP_E_TAG);
            assert!(etag.is_some(), "ETag header is missing");
        }
    }
}

#[actix_web::test]
async fn get_law_html_request_with_if_no_match_header_expect_not_modified() {
    let archive_path =
        common::initialize_archive(ArchiveType::Multihost(MultihostConfig::Public)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    let file_hash = "d8356a575732fe015dddcf3fa2f23f2ace98b712";
    for guard_value in [
        "fonds_1/law",
        "fonds_2/law",
        "fonds_1_1/law",
        "fonds_1_2/law",
    ] {
        for request_uri in &["/a/b/c.html", "/a/b/", "/a/b/c/", "/a/d/"] {
            let req = test::TestRequest::get()
                .insert_header(("X-Current-Documents-Guard", guard_value))
                .uri(request_uri)
                .append_header((IF_NONE_MATCH, file_hash))
                .to_request();
            let resp = test::call_service(&app, req).await;
            let etag = resp.headers().get(HTTP_E_TAG);
            assert!(etag.is_some(), "ETag header is missing");

            assert_eq!(
                resp.status(),
                StatusCode::NOT_MODIFIED,
                "Expected 304 Not Modified"
            );
            assert_eq!(etag.expect("error"), file_hash);
        }
    }
}

#[actix_web::test]
async fn get_law_html_request_with_old_if_no_match_header_expect_new_tag() {
    let archive_path =
        common::initialize_archive(ArchiveType::Multihost(MultihostConfig::Public)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    let file_hash = "d8356a575732fe015dddcf3fa2f23f2ace98b712";
    let old_file_hash = "0000000000000000000000000000000000000000000";
    for guard_value in [
        "fonds_1/law",
        "fonds_2/law",
        "fonds_1_1/law",
        "fonds_1_2/law",
    ] {
        for request_uri in &["/a/b/c.html", "/a/b/", "/a/b/c/", "/a/d/"] {
            let req = test::TestRequest::get()
                .insert_header(("X-Current-Documents-Guard", guard_value))
                .uri(request_uri)
                .append_header((IF_NONE_MATCH, old_file_hash))
                .to_request();
            let resp = test::call_service(&app, req).await;
            let etag = resp.headers().get(HTTP_E_TAG);
            assert!(etag.is_some(), "ETag header is missing");

            assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");
            assert_eq!(etag.expect("error"), file_hash);
        }
    }
}

#[actix_web::test]
async fn test_redirect_dependant_fonds_law_html_request_with_correct_redirects_expect_success() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Multihost(MultihostConfig::Public))
            .unwrap();

    let fonds_1_html_repo_path: PathBuf = archive_path.path().join("fonds_1/law-html");

    let db = get_db(archive_path.path()).await;

    insert_redirects(
        &db,
        "fonds_1/law",
        "law-html",
        vec![("/not/a/good/path", "/")],
    )
    .await;

    let app = common::initialize_app(archive_path.path()).await;

    let request_uri = "/not/a/good/path";
    let req = test::TestRequest::get()
        .uri(request_uri)
        .insert_header(("X-Current-Documents-Guard", "fonds_1/law"))
        .insert_header(("X-Fonds", "fonds_1/law"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);

    let location: &str = resp
        .headers()
        .get("Location")
        .expect("Location header missing")
        .to_str()
        .expect("Location header is not valid UTF-8");

    assert_eq!(location, "/");

    // follow redirect
    let req2 = test::TestRequest::get()
        .insert_header(("X-Current-Documents-Guard", "fonds_1/law"))
        .uri(location)
        .to_request();
    let resp2 = test::call_service(&app, req2).await;

    // assert final response
    assert!(resp2.status().is_success());

    // test on other dependant fonds
    let request_uri = "/not/a/good/path";
    let req3 = test::TestRequest::get()
        .uri(request_uri)
        .insert_header(("X-Current-Documents-Guard", "fonds_2/law"))
        .to_request();
    let resp = test::call_service(&app, req3).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    drop(db);
}

#[actix_web::test]
async fn test_redirect_dependant_fonds_law_html_requests_with_correct_redirects_expect_success() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Multihost(MultihostConfig::Public))
            .unwrap();

    let db = get_db(archive_path.path()).await;
    let fonds_1_html_repo_path: PathBuf = archive_path.path().join("fonds_1/law-html");
    let fonds_2_html_repo_path: PathBuf = archive_path.path().join("fonds_2/law-html");
    let fonds_1_1_html_repo_path: PathBuf = archive_path.path().join("fonds_1_1/law-html");

    insert_redirects(
        &db,
        "fonds_1/law",
        "law-html",
        vec![("/not/a/good/path", "/")],
    )
    .await;

    insert_redirects(
        &db,
        "fonds_2/law",
        "law-html",
        vec![("/not/a/good/path", "/a/")],
    )
    .await;

    insert_redirects(
        &db,
        "fonds_1_1/law",
        "law-html",
        vec![("/not/a/good/path", "/a/b")],
    )
    .await;

    db.pool.close().await;

    let app = common::initialize_app(archive_path.path()).await;

    let cases = [
        ("fonds_1/law", "/"),
        ("fonds_2/law", "/a/"),
        ("fonds_1_1/law", "/a/b"),
    ];

    for (guard_value, expected_location) in cases {
        let request_uri = "/not/a/good/path";
        let req = test::TestRequest::get()
            .uri(request_uri)
            .insert_header(("X-Current-Documents-Guard", guard_value))
            .insert_header(("X-Fonds", guard_value))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);

        let location: &str = resp
            .headers()
            .get("Location")
            .expect("Location header missing")
            .to_str()
            .expect("Location header is not valid UTF-8");

        assert_eq!(location, expected_location);
    }

    // test on other dependant fonds
    let request_uri = "/not/a/good/path";
    let req3 = test::TestRequest::get()
        .uri(request_uri)
        .insert_header(("X-Current-Documents-Guard", "fonds_1_2/law"))
        .to_request();
    let resp = test::call_service(&app, req3).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    drop(db);
}

#[actix_web::test]
async fn test_resolve_guarded_fonds_where_other_fonds_repositories_invalid_expect_success() {
    for repositories in [
        r#"{ "repositories": { "fonds_2/law-html": { "custom": { "serve": "latest", "routes": ["(unclosed"] } } } }"#,
        r#"{ "repositories": { "law-html": { "custom": { "serve": "latest", "routes": [".*"] } } } }"#,
    ] {
        let archive_path = common::initialize_archive_without_bare(ArchiveType::Multihost(
            MultihostConfig::Public,
        ))
        .unwrap();
        archive_testtools::write_to_file(
            &archive_path.path().join("fonds_2/law"),
            repositories.to_owned(),
            "repositories.json".to_owned(),
        )
        .unwrap();
        utils::make_all_git_repos_bare_recursive(&archive_path).unwrap();
        let app = common::initialize_app(archive_path.path()).await;

        let req = test::TestRequest::get()
            .insert_header(("X-Current-Documents-Guard", "fonds_1/law"))
            .uri("/a/b/c.html")
            .to_request();
        let actual = test::call_service(&app, req).await.status();
        let expected = StatusCode::OK;
        assert_eq!(actual, expected, "repositories.json {repositories}");
    }
}
