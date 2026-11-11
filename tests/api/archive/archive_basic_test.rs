use std::path::PathBuf;

use actix_http::header::IF_NONE_MATCH;
use actix_http::Request;
use actix_service::Service;
use actix_web::body::MessageBody;
use actix_web::dev::ServiceResponse;
use actix_web::{test, Error};
use taf_server::server::headers::HTTP_E_TAG;

use crate::archive_testtools::config::{ArchiveType, Jurisdiction};
use crate::archive_testtools::{
    add_private_json_file, get_repository, init_secret_repository, write_to_file,
};
use crate::common;
use actix_http::StatusCode;
use actix_web::http::header;

use super::{test_archive_paths, test_archive_paths_with_head_method};

#[actix_web::test]
async fn test_archive_api_on_all_repositories_with_full_path_expect_success() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    test_archive_paths(
        "test_org",
        "law-html",
        vec!["", "a/", "a/b/", "a/d/", "a/b/c.html", "a/b/c/"],
        "HEAD",
        &app,
        true,
        "x-fonds",
        "test_org/law",
    )
    .await;

    test_archive_paths(
        "test_org",
        "law-other",
        vec![
            "",
            "example.json",
            "a/",
            "a/e/_doc/f/",
            "a/d/",
            "a/b/",
            "a/b/c.html",
            "a/_doc/e/",
            "_prefix/",
            "_prefix/a/",
        ],
        "HEAD",
        &app,
        true,
        "x-fonds",
        "test_org/law",
    )
    .await;

    test_archive_paths(
        "test_org",
        "law-pdf",
        vec!["/example.pdf", "/a/example.pdf", "/a/b/example.pdf"],
        "HEAD",
        &app,
        true,
        "x-fonds",
        "test_org/law",
    )
    .await;

    test_archive_paths(
        "test_org",
        "law-rdf",
        vec![
            "index.rdf",
            "a/index.rdf",
            "a/b/index.rdf",
            "a/d/index.rdf",
            "a/b/c.rdf",
            "a/b/c/index.rdf",
        ],
        "HEAD",
        &app,
        true,
        "x-fonds",
        "test_org/law",
    )
    .await;

    test_archive_paths(
        "test_org",
        "law-xml",
        vec![
            "index.xml",
            "a/index.xml",
            "a/b/index.xml",
            "a/d/index.xml",
            "a/b/c.xml",
            "a/b/c/index.xml",
        ],
        "HEAD",
        &app,
        true,
        "x-fonds",
        "test_org/law",
    )
    .await;

    test_archive_paths(
        "test_org",
        "law-xml-codified",
        vec!["index.xml", "e/index.xml", "e/f/index.xml", "e/g/index.xml"],
        "HEAD",
        &app,
        true,
        "x-fonds",
        "test_org/law",
    )
    .await;
}

#[actix_web::test]
async fn test_archive_api_on_all_repositories_with_head_method_expect_success() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    test_archive_paths_with_head_method(
        "test_org",
        "law-html",
        vec!["", "a/", "a/b/", "a/d/", "a/b/c.html", "a/b/c/"],
        "HEAD",
        &app,
        true,
    )
    .await;

    test_archive_paths_with_head_method(
        "test_org",
        "law-other",
        vec![
            "",
            "example.json",
            "a/",
            "a/e/_doc/f/",
            "a/d/",
            "a/b/",
            "a/b/c.html",
            "a/_doc/e/",
            "_prefix/",
            "_prefix/a/",
        ],
        "HEAD",
        &app,
        true,
    )
    .await;

    test_archive_paths_with_head_method(
        "test_org",
        "law-pdf",
        vec!["/example.pdf", "/a/example.pdf", "/a/b/example.pdf"],
        "HEAD",
        &app,
        true,
    )
    .await;

    test_archive_paths_with_head_method(
        "test_org",
        "law-rdf",
        vec![
            "index.rdf",
            "a/index.rdf",
            "a/b/index.rdf",
            "a/d/index.rdf",
            "a/b/c.rdf",
            "a/b/c/index.rdf",
        ],
        "HEAD",
        &app,
        true,
    )
    .await;

    test_archive_paths_with_head_method(
        "test_org",
        "law-xml",
        vec![
            "index.xml",
            "a/index.xml",
            "a/b/index.xml",
            "a/d/index.xml",
            "a/b/c.xml",
            "a/b/c/index.xml",
        ],
        "HEAD",
        &app,
        true,
    )
    .await;

    test_archive_paths_with_head_method(
        "test_org",
        "law-xml-codified",
        vec!["index.xml", "e/index.xml", "e/f/index.xml", "e/g/index.xml"],
        "HEAD",
        &app,
        true,
    )
    .await;
}

#[actix_web::test]
async fn test_archive_api_on_law_html_repository_with_missing_branch_name_expect_client_error() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    test_archive_paths(
        "test_org",
        "law-html",
        vec!["", "a/index.html"],
        "",
        &app,
        false,
        "x-fonds",
        "test_org/law",
    )
    .await;
}

#[actix_web::test]
async fn test_archive_api_on_law_html_repository_with_invalid_branch_name_expect_client_error() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    test_archive_paths(
        "test_org",
        "law-html",
        vec!["", "a/index.html"],
        "notExistingBranch",
        &app,
        false,
        "x-fonds",
        "test_org/law",
    )
    .await;
}

#[actix_web::test]
async fn test_archive_api_on_law_html_repository_with_invalid_org_name_expect_client_error() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    test_archive_paths(
        "not_test_org",
        "law-html",
        vec!["", "a/index.html"],
        "HEAD",
        &app,
        false,
        "x-fonds",
        "test_org/law",
    )
    .await;
}

#[actix_web::test]
async fn test_archive_api_on_law_html_repository_with_invalid_repo_name_expect_client_error() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    test_archive_paths(
        "test_org",
        "not_law-html",
        vec!["", "a/index.html"],
        "HEAD",
        &app,
        false,
        "x-fonds",
        "test_org/law",
    )
    .await;
}

#[actix_web::test]
async fn test_archive_api_on_law_html_repository_with_incorrect_paths_expect_client_error() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    test_archive_paths(
        "test_org",
        "law-html",
        vec!["a/b/c/d", "a/index.css"],
        "HEAD",
        &app,
        false,
        "x-fonds",
        "test_org/law",
    )
    .await;
}

#[actix_web::test]
async fn test_archive_api_on_law_html_repository_with_different_files_on_different_branches_expect_success(
) {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    let mut path = archive_path.path().to_path_buf();
    path.push("test_org");
    let git_repo = get_repository(&path, "law-html");
    path.push("law-html");
    let _ = git_repo.create_branch("test_branch");
    let _ = git_repo.create_branch("default_branch");

    let _ = git_repo.checkout("default_branch");
    let _ = git_repo.add_file(&path, "test.txt", "Content for default branch");
    let _ = git_repo.commit(None, "Adding data for default branch");

    let _ = git_repo.checkout("test_branch");
    let _ = git_repo.add_file(&path, "test1.txt", "Content for test branch");
    let _ = git_repo.commit(None, "Adding data for test branch");

    test_archive_paths(
        "test_org",
        "law-html",
        vec!["/test.txt"],
        "default_branch",
        &app,
        true,
        "x-fonds",
        "test_org/law",
    )
    .await;

    test_archive_paths(
        "test_org",
        "law-html",
        vec!["/test1.txt"],
        "default_branch",
        &app,
        false,
        "x-fonds",
        "test_org/law",
    )
    .await;

    test_archive_paths(
        "test_org",
        "law-html",
        vec!["/test.txt"],
        "test_branch",
        &app,
        false,
        "x-fonds",
        "test_org/law",
    )
    .await;

    test_archive_paths(
        "test_org",
        "law-html",
        vec!["/test1.txt"],
        "test_branch",
        &app,
        true,
        "x-fonds",
        "test_org/law",
    )
    .await;
}

#[actix_web::test]
async fn test_archive_api_with_same_file_on_different_branches_expect_different_file_content_on_different_branches(
) {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    let mut path = archive_path.path().to_path_buf();
    path.push("test_org");
    let git_repo = get_repository(&path, "law-html");
    path.push("law-html");

    let _ = git_repo.create_branch("test_branch");
    let _ = git_repo.create_branch("default_branch");

    let _ = git_repo.checkout("default_branch");
    let _ = git_repo.add_file(&path, "test.txt", "Content for default branch");
    let _ = git_repo.commit(None, "Adding data for default branch");

    let _ = git_repo.checkout("test_branch");
    let _ = git_repo.add_file(&path, "test.txt", "Content for test branch");
    let _ = git_repo.commit(None, "Adding data for test branch");

    let req = test::TestRequest::get()
        .uri("/_archive/test_org/law-html?commitish=default_branch&path=/test.txt")
        .to_request();
    let actual = test::call_and_read_body(&app, req).await;
    let expected = "Content for default branch";
    assert!(
        common::blob_to_string(actual.to_vec()).starts_with(expected),
        "doesn't start with {expected}"
    );

    let req = test::TestRequest::get()
        .uri("/_archive/test_org/law-html?commitish=test_branch&path=test.txt")
        .to_request();
    let actual = test::call_and_read_body(&app, req).await;
    let expected = "Content for test branch";
    assert!(
        common::blob_to_string(actual.to_vec()).starts_with(expected),
        "doesn't start with {expected}"
    );
}

#[actix_web::test]
async fn test_archive_api_where_branch_contains_slashs_expect_resolved_content() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    let mut path = archive_path.path().to_path_buf();
    path.push("test_org");
    let git_repo = get_repository(&path, "law-html");
    path.push("law-html");
    let branch_name = "test/branch/with/slash";
    let _ = git_repo.create_branch(branch_name);

    let _ = git_repo.checkout(branch_name);
    let _ = git_repo.add_file(&path, "test.txt", "Content for test branch");
    let _ = git_repo.commit(None, "Adding data for test branch");

    let req = test::TestRequest::get()
        .uri(&format!(
            "/_archive/test_org/law-html?commitish={}&path=/test.txt",
            branch_name
        ))
        .to_request();
    let actual = test::call_and_read_body(&app, req).await;
    let expected = "Content for test branch";
    assert!(
        common::blob_to_string(actual.to_vec()).starts_with(expected),
        "doesn't start with {expected}"
    );
}

#[actix_web::test]
async fn test_archive_api_where_branch_is_commit_sha_expect_resolved_content() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    let mut path = archive_path.path().to_path_buf();
    path.push("test_org");
    let git_repo = get_repository(&path, "law-html");
    path.push("law-html");
    let branch_name = "test/branch/with/slash";
    let _ = git_repo.create_branch(branch_name);

    let _ = git_repo.checkout(branch_name);
    let _ = git_repo.add_file(&path, "test.txt", "Content for test branch");
    let commit_hash = git_repo.commit(None, "Adding data for test branch");
    let sha_string = commit_hash.unwrap().to_string();

    let req = test::TestRequest::get()
        .uri(&format!(
            "/_archive/test_org/law-html?commitish={}&path=/test.txt",
            sha_string
        ))
        .to_request();
    let actual = test::call_and_read_body(&app, req).await;
    let expected = "Content for test branch";
    assert!(
        common::blob_to_string(actual.to_vec()).starts_with(expected),
        "doesn't start with {expected}"
    );
}

async fn get_archive_status(
    app: &impl Service<Request, Response = ServiceResponse<impl MessageBody>, Error = Error>,
    commitish: &str,
    path: &str,
) -> StatusCode {
    let req = test::TestRequest::get()
        .uri(&format!(
            "/_archive/test_org/law-html?commitish={commitish}&path={path}"
        ))
        .to_request();
    test::call_service(app, req).await.status()
}

#[actix_web::test]
async fn test_archive_api_where_commit_is_not_on_local_branch_expect_not_found() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let mut path = archive_path.path().to_path_buf();
    path.push("test_org");
    let git_repo = get_repository(&path, "law-html");
    path.push("law-html");
    let head = git_repo.head().unwrap();
    let branch_ref = head.name().unwrap().to_owned();
    let parent = head.peel_to_commit().unwrap().id();
    let _ = git_repo.add_file(&path, "remote.txt", "Remote content");
    let commit = git_repo.commit(None, "Adding remote data").unwrap();
    git_repo
        .reference(&branch_ref, parent, true, "move branch back")
        .unwrap();
    git_repo
        .reference("refs/remotes/origin/main", commit, true, "remote branch")
        .unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    let commit = commit.to_string();
    for commitish in [
        commit.as_str(),
        &commit[..7],
        "origin/main",
        "refs/remotes/origin/main",
    ] {
        let actual = get_archive_status(&app, commitish, "/remote.txt").await;
        let expected = StatusCode::NOT_FOUND;
        assert_eq!(actual, expected, "commitish {commitish}");
    }
}

#[actix_web::test]
async fn test_archive_api_where_commitish_is_revision_expression_expect_bad_request() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    for commitish in [
        "HEAD~0",
        "HEAD%5E0",
        "HEAD%40%7B0%7D",
        ":/a",
        "HEAD%5E%7B/a%7D",
    ] {
        let actual = get_archive_status(&app, commitish, "/index.html").await;
        let expected = StatusCode::BAD_REQUEST;
        assert_eq!(actual, expected, "commitish {commitish}");
    }
}

#[actix_web::test]
async fn test_archive_api_where_blob_found_expect_nosniff_and_sandbox_headers() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    let req = test::TestRequest::get()
        .uri("/_archive/test_org/law-html?path=/index.html")
        .to_request();

    let resp = test::call_service(&app, req).await;

    let actual = (
        resp.headers().get(header::X_CONTENT_TYPE_OPTIONS),
        resp.headers().get(header::CONTENT_SECURITY_POLICY),
    );
    let expected = (
        Some(&header::HeaderValue::from_static("nosniff")),
        Some(&header::HeaderValue::from_static("sandbox")),
    );
    assert_eq!(actual, expected);
}

#[actix_web::test]
async fn test_archive_api_where_org_name_does_not_exists_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;

    let req = test::TestRequest::get()
        .uri("/_archive/unknown_org/law-html?&path=/index.html")
        .insert_header((
            header::HeaderName::from_static("x-fonds"),
            "unknown_org/law",
        ))
        .to_request();
    let actual = test::call_and_read_body(&app, req).await;
    let expected = "repo unknown_org/law-html does not exist";
    println!("{}", common::blob_to_string(actual.to_vec()));
    assert!(
        common::blob_to_string(actual.to_vec()).starts_with(expected),
        "doesn't start with {expected}"
    );
}

#[actix_web::test]
async fn test_archive_api_where_repo_name_is_not_in_repository_json_file_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let test_org_path: PathBuf = archive_path.path().join("test_org");
    let _ = init_secret_repository(&test_org_path);
    let app = common::initialize_app(archive_path.path()).await;

    let req = test::TestRequest::get()
        .uri("/_archive/test_org/secret_repo?path=/password.txt")
        .insert_header((header::HeaderName::from_static("x-fonds"), "test_org/law"))
        .to_request();
    let actual = test::call_and_read_body(&app, req).await;
    let expected = "repo test_org/secret_repo does not exist";
    assert!(
        common::blob_to_string(actual.to_vec()).starts_with(expected),
        "doesn't start with {expected}"
    );
}

#[actix_web::test]
async fn test_archive_api_without_header_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let test_org_path: PathBuf = archive_path.path().join("test_org");
    let _ = init_secret_repository(&test_org_path);
    let app = common::initialize_app(archive_path.path()).await;

    let req = test::TestRequest::get()
        .uri("/_archive/test_org/secret_repo?path=/password.txt")
        .to_request();
    let resp = test::call_service(&app, req).await;
    let actual = resp.status().is_client_error();
    let expected = true;
    assert_eq!(actual, expected);
}

#[actix_web::test]
async fn test_archive_api_where_private_json_file_exists_expect_error() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let fonds_path: PathBuf = archive_path.path().join("test_org");
    let auth_repo_path: PathBuf = archive_path.path().join("test_org/law");

    let file_content = r#"
    {
      "private": true
    }
    "#
    .to_string();

    let _ = add_private_json_file(&auth_repo_path, file_content);
    let _ = init_secret_repository(&fonds_path);
    let app = common::initialize_app(archive_path.path()).await;

    let req = test::TestRequest::get()
        .uri("/_archive/test_org/law-html?path=/index.html")
        .insert_header((header::HeaderName::from_static("x-fonds"), "test_org/law"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "Expected 404 Not Found"
    );

    let actual = test::read_body(resp).await;
    let expected = "repo test_org/law-html does not exist";
    assert!(
        common::blob_to_string(actual.to_vec()).starts_with(expected),
        "doesn't start with {expected}"
    );
}

#[actix_web::test]
async fn get_law_other_request_with_no_if_no_match_header_expect_new_etag() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    let req = test::TestRequest::get()
        .uri("/_archive/test_org/law-other?commitish=HEAD&path=example.json")
        .insert_header((header::HeaderName::from_static("x-fonds"), "test_org/law"))
        .to_request();

    let resp = test::call_service(&app, req).await;

    let etag = resp.headers().get(HTTP_E_TAG);
    assert!(etag.is_some(), "ETag header is missing");
}

#[actix_web::test]
async fn get_law_other_request_with_if_no_match_header_expect_not_modified() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    let file_hash = "6676f3b75a7caaafa8c2539c0e256723cf220c13";
    let req = test::TestRequest::get()
        .uri("/_archive/test_org/law-other?commitish=HEAD&path=example.json")
        .append_header((IF_NONE_MATCH, file_hash))
        .insert_header((header::HeaderName::from_static("x-fonds"), "test_org/law"))
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

#[actix_web::test]
async fn get_law_other_request_with_old_if_no_match_header_expect_new_tag() {
    let archive_path =
        common::initialize_archive(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    let file_hash = "6676f3b75a7caaafa8c2539c0e256723cf220c13";
    let old_file_hash = "0000000000000000000000000000000000000000000";
    let req = test::TestRequest::get()
        .uri("/_archive/test_org/law-other?commitish=HEAD&path=example.json")
        .append_header((IF_NONE_MATCH, old_file_hash))
        .insert_header((header::HeaderName::from_static("x-fonds"), "test_org/law"))
        .to_request();
    let resp = test::call_service(&app, req).await;

    let etag = resp.headers().get(HTTP_E_TAG);
    assert!(etag.is_some(), "ETag header is missing");

    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");
    assert_eq!(etag.expect("error"), file_hash);
}

#[actix_web::test]
async fn get_law_other_file_with_edit_expect_new_etag() {
    let archive_path =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let app = common::initialize_app(archive_path.path()).await;
    let file_hash = "6676f3b75a7caaafa8c2539c0e256723cf220c13";
    let old_file_hash = "0000000000000000000000000000000000000000000";
    let req = test::TestRequest::get()
        .uri("/_archive/test_org/law-other?commitish=HEAD&path=example.json")
        .append_header((IF_NONE_MATCH, old_file_hash))
        .insert_header((header::HeaderName::from_static("x-fonds"), "test_org/law"))
        .to_request();
    let resp = test::call_service(&app, req).await;

    let etag = resp.headers().get(HTTP_E_TAG);
    assert!(etag.is_some(), "ETag header is missing");

    assert_eq!(resp.status(), StatusCode::OK, "Expected 200 OK");
    assert_eq!(etag.expect("error"), file_hash);
    let repo_path: PathBuf = archive_path.path().join("test_org/law-other");
    let file_content = r#"
    {
      "this_is_new_content": true
    }
    "#
    .to_string();
    let _ = write_to_file(&repo_path, file_content, "example.json".to_string());

    let req = test::TestRequest::get()
        .uri("/_archive/test_org/law-other?commitish=HEAD&path=example.json")
        .append_header((IF_NONE_MATCH, file_hash))
        .insert_header((header::HeaderName::from_static("x-fonds"), "test_org/law"))
        .to_request();
    let resp = test::call_service(&app, req).await;

    let etag = resp.headers().get(HTTP_E_TAG);
    assert!(etag.is_some(), "ETag header is missing");

    assert_eq!(
        resp.status(),
        StatusCode::NOT_MODIFIED,
        "Expected 304 NOT MODIFIED"
    );
    assert_eq!(etag.expect("error"), file_hash);
}
