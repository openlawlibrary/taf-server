//! Legacy git microserver.
use actix_http::header::{IF_NONE_MATCH, X_CONTENT_TYPE_OPTIONS};
use actix_web::{get, route, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use git2::{self, ErrorCode};
use std::path::PathBuf;
use tracing_actix_web::TracingLogger;

use super::errors::{CliError, HTTPError, TafServerError};
use crate::server::headers;
use crate::server::headers::etag_matches_if_none_match;
use crate::utils::git::{Repo, GIT_INVALID_REVISION, GIT_REQUEST_NOT_FOUND};
use crate::utils::http::get_contenttype;
use crate::{server::tracing::TafServerRootSpanBuilder, utils::paths::clean_path};

/// Global, read-only state passed into the actix app
struct AppState {
    /// path to the Fonds archive
    archive_path: PathBuf,
}

/// Root index path
#[get("/")]
async fn index() -> &'static str {
    "Welcome to Taf Server"
}

/// Just for development purposes at the moment
#[get("{path}")]
async fn misc(path: web::Path<String>) -> actix_web::Result<&'static str, TafServerError> {
    match path.as_str() {
        "error" => Err(TafServerError::GitError),
        _ => Ok("\u{2728}"),
    }
}

/// Return the content in the fonds archive in the `{namespace}/{name}`
/// repo at the `commitish` commit at the `remainder` path.
/// `commitish` must be `HEAD`, a local branch name or a full commit ID.
/// Return 400 for any other `commitish`, and 404 if any are not found.
#[route(
    "/{namespace}/{name}/{commitish}{remainder:/+([^{}]*?)?/*}",
    method = "GET",
    method = "HEAD"
)]
#[tracing::instrument(name = "Retrieving a Git blob", skip(path, data))]
#[expect(
    clippy::future_not_send,
    reason = "We don't worry about git2-rs not implementing `Send` trait"
)]
async fn get_blob(
    req: HttpRequest,
    path: web::Path<(String, String, String, String)>,
    data: web::Data<AppState>,
) -> impl Responder {
    let (namespace, name, commitish, remainder) = path.into_inner();
    let archive_path = &data.archive_path;
    let blob = Repo::find_blob(archive_path, &namespace, &name, &remainder, &commitish);
    let blob_path = clean_path(&remainder);
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
                .insert_header((headers::HTTP_X_FILE_PATH, filepath))
                .insert_header((headers::HTTP_E_TAG, blob_hash.to_string()))
                .body(content)
        }
        Err(error) => blob_error_response(&error, &namespace, &name),
    }
}

/// A centralised place to match potentially unsafe internal errors to safe user-facing error responses
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "Default to wildcard match in case of unexpected errors"
)]
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
        _ if error.to_string() == GIT_INVALID_REVISION => {
            HttpResponse::BadRequest().body(HTTPError::BadRequest.to_string())
        }
        _ => HttpResponse::InternalServerError().body(HTTPError::InternalServerError.to_string()),
    }
}

/// Serve git repositories in the Fonds archive.
#[actix_web::main] // or #[tokio::main]
pub async fn serve_git(
    raw_archive_path: &str,
    archive_path: PathBuf,
    port: u16,
) -> Result<(), CliError> {
    let bind = "127.0.0.1";
    let message = "Serving content from the Fonds archive at";
    tracing::info!("{message} '{raw_archive_path}' on http://{bind}:{port}.",);

    HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::<TafServerRootSpanBuilder>::new())
            .service(index)
            .service(misc)
            .service(get_blob)
            .app_data(web::Data::new(AppState {
                archive_path: archive_path.clone(),
            }))
    })
    .bind((bind, port))
    .map_err(|err| {
        tracing::error!("Failed to bind Git server to {bind}:{port}: {err}");
        CliError::GenericError
    })?
    .run()
    .await
    .map_err(|err| {
        tracing::error!("Error running Git server: {err:?}");
        CliError::GenericError
    })
}
