//! API endpoint for serving git blobs.
/// The `_archive` endpoint provides access to the contents of files stored within
/// repositories.
///
/// This endpoint only permits access to files that belong to **public** repositories.
/// Attempts to access files in private or restricted repositories will result in a
/// `404 Not Found` response, the same as for repositories that do not exist.
///
/// # Overview
/// - Resolves repositories and files based on the provided path and query parameters.
/// - Verifies that the target repository is listed by a fonds and that no fonds
///   listing it is private.
/// - Resolves `commitish`, which must be `HEAD` (the default), a local branch name,
///   or the full ID of a commit on one of the repository's local branches.
/// - Returns the requested file content if access is allowed.
///
/// # Restrictions
/// - Only public repositories are accessible.
/// - Authorization headers or guards may further restrict access.
///
/// # Example
/// ```http
/// GET /_archive/org/repo?path=/README.md&commitish=main
/// ```
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::fonds::fonds::Fonds;
use crate::server::headers;
use crate::server::headers::etag_matches_if_none_match;
use crate::utils::git::{Blob, Repo, Revision, GIT_REQUEST_NOT_FOUND};
use crate::utils::http::get_contenttype;
use crate::utils::paths::clean_path;
use actix_http::header::{CONTENT_SECURITY_POLICY, IF_NONE_MATCH, X_CONTENT_TYPE_OPTIONS};
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use git2::{self, ErrorCode};
use request::ArchiveQueryData;

use super::super::errors::HTTPError;
use super::state::Global;

/// Module that maps the HTTP web request body to structs.
pub mod request;

/// Return the content in the fonds archive in the `{namespace}/{name}`
/// repo at the `commitish` commit at the `path` path.
/// Return 404 if any are not found or there are any errors.
#[tracing::instrument(name = "Retrieving a Git blob", skip(path, data, query))]
#[expect(
    clippy::future_not_send,
    reason = "We don't worry about git2-rs not implementing `Send` trait"
)]
pub async fn get_blob(
    req: HttpRequest,
    path: web::Path<(String, String)>,
    query: web::Query<ArchiveQueryData>,
    data: web::Data<PathBuf>,
    archive_data: web::Data<Arc<dyn Global>>,
) -> impl Responder {
    let (namespace, name) = path.into_inner();
    let query_data: ArchiveQueryData = query.into_inner();
    let commitish = query_data.commitish.unwrap_or_else(|| String::from("HEAD"));
    let file_path = query_data.path.unwrap_or_default();

    let Ok(revision) = Revision::parse(&commitish) else {
        return HttpResponse::BadRequest().body(HTTPError::BadRequest.to_string());
    };

    let full_name = format!("{namespace}/{name}");
    let listing_fonds: Vec<&Fonds> = archive_data
        .archive()
        .fonds_map
        .values()
        .filter(|fonds| lists_repository(fonds, &full_name))
        .collect();
    if listing_fonds.is_empty() || listing_fonds.iter().any(|fonds| fonds.is_private_fonds()) {
        return HttpResponse::NotFound().body(format!("repo {namespace}/{name} does not exist"));
    }

    let blob = find_blob_on_local_branch(&data, &namespace, &name, &revision, &file_path);
    let blob_path = clean_path(&file_path);
    let contenttype = get_contenttype(&blob_path);
    match blob {
        Ok(found_blob) => {
            let content = found_blob.content;
            let filepath = found_blob.path;
            let blob_hash = found_blob.blob_hash;
            if let Some(inm) = req.headers().get(IF_NONE_MATCH) {
                if inm.to_str().ok().is_some_and(|val| {
                    etag_matches_if_none_match(val, blob_hash.to_string().as_str())
                }) {
                    return HttpResponse::NotModified()
                        .insert_header((headers::HTTP_E_TAG, blob_hash.to_string()))
                        .body("");
                }
            }
            HttpResponse::Ok()
                .insert_header(contenttype)
                .insert_header((X_CONTENT_TYPE_OPTIONS, "nosniff"))
                .insert_header((CONTENT_SECURITY_POLICY, "sandbox"))
                .insert_header((headers::HTTP_X_FILE_PATH, filepath))
                .insert_header((headers::HTTP_E_TAG, blob_hash.to_string()))
                .body(content)
        }
        Err(error) => blob_error_response(&error, &namespace, &name),
    }
}

/// Check whether `fonds` lists `full_name` in its `repositories.json` at `HEAD`.
fn lists_repository(fonds: &Fonds, full_name: &str) -> bool {
    match fonds.get_repositories_for_commitish("HEAD") {
        Ok(Some(repositories)) => repositories.repositories.contains_key(full_name),
        Ok(None) => false,
        Err(err) => {
            tracing::error!(
                "Error fetching repositories of {}: {err}",
                fonds.get_qualified_name()
            );
            false
        }
    }
}

/// Find the blob at `file_path` in the `{namespace}/{name}` repository at
/// `revision`.
///
/// A commit ID is only resolved if the commit is on one of the repository's
/// local branches, so that objects which are present in the repository but
/// not part of its history (for example unreferenced commits or
/// remote-tracking refs) are not served.
fn find_blob_on_local_branch(
    archive_path: &Path,
    namespace: &str,
    name: &str,
    revision: &Revision,
    file_path: &str,
) -> anyhow::Result<Blob> {
    let repo = Repo::new(archive_path, namespace, name)?;
    let commit = repo.resolve_commit(revision)?;
    if matches!(*revision, Revision::Commit(_)) && !repo.is_on_local_branch(commit.id())? {
        anyhow::bail!(GIT_REQUEST_NOT_FOUND);
    }
    repo.get_bytes_at_commit(&commit, &clean_path(file_path))
}

/// A centralised place to match potentially unsafe internal errors to safe user-facing error responses
#[expect(clippy::wildcard_enum_match_arm, reason = "Allows _ for enum matching")]
#[tracing::instrument(name = "Error with Git blob request", skip(error, namespace, name))]
fn blob_error_response(error: &anyhow::Error, namespace: &str, name: &str) -> HttpResponse {
    tracing::error!("{error}",);
    if let Some(git_error) = error.downcast_ref::<git2::Error>() {
        return match git_error.code() {
            // TODO: check this is the right error
            ErrorCode::NotFound => {
                HttpResponse::NotFound().body(format!("repo {namespace}/{name} does not exist"))
            }
            _ => HttpResponse::InternalServerError().body("Unexpected Git error"),
        };
    }
    match error {
        // TODO: Obviously it's better to use custom `Error` types
        _ if error.to_string() == GIT_REQUEST_NOT_FOUND => {
            HttpResponse::NotFound().body(HTTPError::NotFound.to_string())
        }
        _ => HttpResponse::InternalServerError().body(HTTPError::InternalServerError.to_string()),
    }
}
