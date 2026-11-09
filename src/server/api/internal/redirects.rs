//! API endpoint for refreshing the server's in-memory cache of repositories
//! that have redirects configured.
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};

use crate::db::models::redirects::Manager as _;

use super::super::state::Global;

/// Recomputes the set of `(fonds, repo)` pairs that have at least one
/// redirect configured, and replaces the server's in-memory cache with the
/// result.
///
/// Intended to be called by the publish-server update script once
/// `taf-server update` has finished inserting redirects into the database,
/// so the already-running server picks up newly added redirects without
/// needing to be restarted.
///
/// This endpoint is only meant to be reachable from the local host (the
/// update script runs on the same machine); it is expected to be blocked
/// from outside at the reverse proxy (nginx).
///
/// # Errors
/// Returns `500 Internal Server Error` if the database query fails.
#[tracing::instrument(skip(app_data))]
pub async fn refresh(app_data: web::Data<Arc<dyn Global>>) -> impl Responder {
    match app_data.db().repos_with_redirects().await {
        Ok(repos) => {
            app_data.set_repos_with_redirects(repos);
            HttpResponse::Ok().finish()
        }
        Err(err) => {
            tracing::error!(error = %err, "Failed to refresh repos-with-redirects cache");
            HttpResponse::InternalServerError().finish()
        }
    }
}
