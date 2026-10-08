//! Serve documents in a Fonds archive.
#![expect(
    clippy::exit,
    reason = "We exit with 1 error code on any application errors"
)]
use crate::db;
use crate::db::models::redirects::Manager as _;
use crate::fonds::archive::Archive;
use crate::server::api::state::App as AppState;
use crate::server::errors::CliError;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{App, Error, HttpServer};
use tracing_actix_web::TracingLogger;

use std::{
    collections::HashSet,
    path::PathBuf,
    process,
    sync::{Arc, RwLock},
};

use actix_http::body::MessageBody;
use actix_service::ServiceFactory;

use super::api::state::Global;
use super::tracing::TafServerRootSpanBuilder;
use crate::server::api::routes;

/// Serve documents in a Fonds archive.
#[actix_web::main]
#[tracing::instrument(skip(raw_archive_path, archive_path, port, individual))]
pub async fn serve_archive(
    raw_archive_path: &str,
    archive_path: PathBuf,
    port: u16,
    individual: bool,
    bind_to: &str,
) -> Result<(), CliError> {
    let message = "Running Publish Server on a Fonds archive at";
    tracing::info!("{message} '{raw_archive_path}' on http://{bind_to}:{port}.",);

    let db = match db::init::connect(&archive_path).await {
        Ok(db) => db,
        Err(err) => {
            tracing::error!(
                "error: could not connect to database. Confirm that DATABASE_URL env var is set correctly."
            );
            tracing::error!("Error: {:?}", err);
            return Err(CliError::DatabaseConnectionError);
        }
    };

    let archive = match Archive::parse(
        archive_path.clone(),
        &PathBuf::from(raw_archive_path),
        individual,
    ) {
        Ok(archive) => archive,
        Err(err) => {
            tracing::error!("Unable to parse archive at '{raw_archive_path}'.");
            tracing::error!("Error: {err:?}");
            return Err(CliError::ArchiveParseError);
        }
    };

    let repos_with_redirects = db.repos_with_redirects().await.unwrap_or_else(|err| {
        tracing::error!(error = %err, "Failed to load repos with redirects, assuming none");
        HashSet::new()
    });

    let state = AppState {
        archive,
        db,
        repos_with_redirects: Arc::new(RwLock::new(repos_with_redirects)),
    };

    HttpServer::new(move || {
        init(&state).unwrap_or_else(|err| {
            tracing::error!("Unable to initialize app.");
            tracing::error!("Error: {err:?}");
            // NOTE: We should not need to exit code 1 here (or in any of the closures in `routes.rs`).
            // We should be able to return an error and let the caller handle it.
            // However, Actix does not allow us to instantiate the app outside of the closure,
            // because the opaque type `App` does not implement `Clone`.
            // Figure out a way to handle this without exiting the process.
            process::exit(1)
        })
    })
    .bind((bind_to, port))
    .map_err(|err| {
        tracing::error!("Failed to bind server to {bind_to}:{port}: {err}");
        CliError::GenericError
    })?
    .run()
    .await
    .map_err(|err| {
        tracing::error!("Error running server: {err:?}");
        CliError::GenericError
    })
}

/// Initialize the application and all possible routing at start-up time.
///
/// # Arguments
/// * `state` - The application state
/// # Errors
/// Will error if unable to initialize the application
pub fn init<T: Global + Clone + 'static>(
    state: &T,
) -> anyhow::Result<
    App<
        impl ServiceFactory<
            ServiceRequest,
            Response = ServiceResponse<impl MessageBody>,
            Config = (),
            InitError = (),
            Error = Error,
        >,
    >,
> {
    let app = App::new().wrap(TracingLogger::<TafServerRootSpanBuilder>::new());
    let registered_app = routes::register_app(app, state)?;
    Ok(registered_app)
}
