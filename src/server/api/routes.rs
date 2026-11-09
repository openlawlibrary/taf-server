//! A central place to register App routes.
#![expect(
    clippy::exit,
    reason = "We exit with 1 error code on any application errors"
)]
use std::sync::Arc;
use std::{process, sync::OnceLock};

use crate::fonds::{fonds::Fonds, types::repositories::Repositories};
use crate::server::api::state;
use actix_service::ServiceFactory;
use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    guard, web, App, Error, Scope,
};

use super::archive::get_blob;
use super::date::date;
use super::internal::redirects;
use super::{serve::serve, state::Global, versions::versions};

/// Name of the header to guard current documents
static HEADER_NAME: OnceLock<String> = OnceLock::new();
/// Values of the header to guard current documents
static HEADER_VALUES: OnceLock<Vec<String>> = OnceLock::new();
/// Name of the root fonds
static ROOT_NAME_VALUE: OnceLock<String> = OnceLock::new();

#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "Actix Web resource path uses `{param}` syntax which is not formatting but route pattern matching"
)]
/// Central place to register all the App routing.
///
/// Registers all routes for the given Archive
/// Static routes should be registered first, followed by dynamic routes.
///
/// # Errors
/// Errors if unable to register dynamic routes (e.g. if git repository cannot be opened)
#[tracing::instrument(skip(app, state))]
pub fn register_app<
    T: Global + Clone + 'static,
    U: MessageBody,
    V: ServiceFactory<
        ServiceRequest,
        Response = ServiceResponse<U>,
        Config = (),
        InitError = (),
        Error = Error,
    >,
>(
    mut app: App<V>,
    state: &T,
) -> anyhow::Result<App<V>> {
    app = app
        .service(
            web::scope("/_api").service(
                web::scope("/versions")
                    .service(
                        web::resource("/_publication/{publication}/_compare/{date}/{compare_date}")
                            .to(versions),
                    )
                    .service(
                        web::resource(
                            "/_publication/{publication}/_compare/{date}/{compare_date}/{path:.*}",
                        )
                        .to(versions),
                    )
                    .service(web::resource("/_publication/{publication}/_date/{date}").to(versions))
                    .service(
                        web::resource("/_publication/{publication}/_date/{date}/{path:.*}")
                            .to(versions),
                    )
                    .service(web::resource("/_publication/{publication}").to(versions))
                    .service(web::resource("/_publication/{publication}/{path:.*}").to(versions))
                    .service(web::resource("/_compare/{date}/{compare_date}").to(versions))
                    .service(
                        web::resource("/_compare/{date}/{compare_date}/{path:.*}").to(versions),
                    )
                    .service(web::resource("/_date/{date}").to(versions))
                    .service(web::resource("/_date/{date}/{path:.*}").to(versions))
                    .service(web::resource("/{path:.*}").to(versions))
                    .service(web::resource("").to(versions)),
            ),
        )
        .app_data(web::Data::new(state.clone()));

    let redirects_data_state: Arc<dyn Global> = Arc::new(state.clone());
    app = app
        .service(
            web::resource("/_internal/redirects/refresh").route(web::post().to(redirects::refresh)),
        )
        .app_data(web::Data::new(redirects_data_state));

    app = app
        .service(
            web::scope("/_date")
                .service(
                    web::resource("/{version_date}/{path:.*}")
                        .route(web::get().to(date))
                        .route(web::head().to(date)),
                )
                .service(
                    web::resource("/{version_date}")
                        .route(web::get().to(date))
                        .route(web::head().to(date)),
                ),
        )
        .app_data(web::Data::new(state.clone()));

    app = app
        .service(
            web::scope("/_publication")
                .service(
                    web::resource("/{pub_name}/_date/{version_date}/{path:.*}")
                        .route(web::get().to(date))
                        .route(web::head().to(date)),
                )
                .service(
                    web::resource("/{pub_name}/_date/{version_date}")
                        .route(web::get().to(date))
                        .route(web::head().to(date)),
                )
                .service(
                    web::resource("/{pub_name}/{path:.*}")
                        .route(web::get().to(date))
                        .route(web::head().to(date)),
                )
                .service(
                    web::resource("/{pub_name}")
                        .route(web::get().to(date))
                        .route(web::head().to(date)),
                ),
        )
        .app_data(web::Data::new(state.clone()));

    app = register_guarded_and_unguarded_routes(app, state)?;
    Ok(app)
}

/// Initialize all dynamic routes for the given Archive.
///
/// Dynamic routes are determined at runtime by looking at the fonds's `dependencies.json` and `repositories.json` files
/// in the authentication (e.g. law) repository.
///
/// # Errors
/// Errors if unable to register dynamic routes (e.g. if git repository cannot be opened)
fn register_guarded_and_unguarded_routes<
    T: MessageBody,
    U: ServiceFactory<
        ServiceRequest,
        Response = ServiceResponse<T>,
        Config = (),
        InitError = (),
        Error = Error,
    >,
    V: Global + Clone + 'static,
>(
    mut app: App<U>,
    state: &V,
) -> anyhow::Result<App<U>> {
    let config = state.archive().get_config()?;
    let fonds_guard = config
        .headers
        .and_then(|headers| headers.current_documents_guard);

    if let Some(guard) = fonds_guard {
        app = initialize_guarded_archive_route(guard.clone(), app, state)?;
        app = initialize_guarded_dynamic_routes(guard, app, state)?;
    } else {
        app = initialize_archive_route(app, state);
        app = initialize_dynamic_routes(app, state)?;
    }
    Ok(app)
}

#[expect(
    clippy::expect_used,
    reason = "If there is no root fonds, we should panic"
)]
/// Initialize all guarded archive routes for the given Archive.
/// Routes are guarded by a header value specified in the config.toml file.
///
/// # Errors
/// Errors if unable to register dynamic routes (e.g. if git repository cannot be opened)
fn initialize_guarded_archive_route<
    T: MessageBody,
    U: ServiceFactory<
        ServiceRequest,
        Response = ServiceResponse<T>,
        Config = (),
        InitError = (),
        Error = Error,
    >,
    V: Global + Clone + 'static,
>(
    guard: String,
    mut app: App<U>,
    state: &V,
) -> anyhow::Result<App<U>> {
    tracing::info!("Initializing guarded fonds routes with header: {}", guard);
    HEADER_NAME.get_or_init(|| guard);
    let archive = state.archive();
    let guard_value = archive.get_root()?.get_qualified_name();
    ROOT_NAME_VALUE.get_or_init(|| guard_value);
    let data_state: Arc<dyn Global> = Arc::new(state.clone());
    if let Some(guard_name) = HEADER_NAME.get() {
        let fonds = state
            .archive()
            .fonds_map
            .get(&archive.get_root()?.get_qualified_name());
        if let Some(_guarded_fonds) = fonds {
            let mut archive_scope = web::scope("_archive");
            archive_scope = archive_scope.guard(guard::Header(
                guard_name,
                ROOT_NAME_VALUE
                    .get()
                    .expect("ROOT_NAME_VALUE not initialized")
                    .as_str(),
            ));
            app = app
                .app_data(web::Data::new(state.archive().path.clone()))
                .app_data(web::Data::new(Arc::clone(&data_state)))
                .service(
                    archive_scope.service(
                        web::resource("/{namespace}/{name}")
                            .route(web::get().to(get_blob))
                            .route(web::head().to(get_blob)),
                    ),
                );
        }
    } else {
        let err_msg =
            "Failed to initialize guarded archive routes. Header name or value not found.";
        tracing::error!(err_msg);
        anyhow::bail!(err_msg);
    }
    Ok(app)
}

/// Initialize _archive routes for the given Archive.
///
/// # Errors
/// Errors if unable to register fonds routes (e.g. if git repository cannot be opened)
fn initialize_archive_route<
    T: MessageBody,
    U: ServiceFactory<
        ServiceRequest,
        Response = ServiceResponse<T>,
        Config = (),
        InitError = (),
        Error = Error,
    >,
    V: Global + Clone + 'static,
>(
    mut app: App<U>,
    state: &V,
) -> actix_web::App<U> {
    let data_state: Arc<dyn Global> = Arc::new(state.clone());
    app = app
        .app_data(web::Data::new(state.archive().path.clone()))
        .app_data(web::Data::new(data_state))
        .service(
            web::scope("_archive").service(
                web::resource("/{namespace}/{name}")
                    .route(web::get().to(get_blob))
                    .route(web::head().to(get_blob)),
            ),
        );
    app
}

/// Initialize all guarded dynamic routes for the given Archive.
/// Routes are guarded by a header value specified in the config.toml file.
///
/// # Errors
/// Errors if unable to register dynamic routes (e.g. if git repository cannot be opened)
fn initialize_guarded_dynamic_routes<
    T: MessageBody,
    U: ServiceFactory<
        ServiceRequest,
        Response = ServiceResponse<T>,
        Config = (),
        InitError = (),
        Error = Error,
    >,
    V: Global + Clone + 'static,
>(
    guard: String,
    mut app: App<U>,
    state: &V,
) -> anyhow::Result<App<U>> {
    tracing::info!(
        "Initializing guarded current documents with header: {}",
        guard
    );
    HEADER_NAME.get_or_init(|| guard);
    HEADER_VALUES.get_or_init(|| {
        state
            .archive()
            .fonds_map
            .keys()
            .map(ToString::to_string)
            .collect()
    });

    if let (Some(guard_name), Some(guard_values)) = (HEADER_NAME.get(), HEADER_VALUES.get()) {
        for guard_value in guard_values {
            let fonds = state.archive().fonds_map.get(guard_value);
            if let Some(guarded_fonds) = fonds {
                let shared_state = state::init_shared(guarded_fonds)?;
                let data_state: Arc<dyn Global> = Arc::new(state.clone());
                let mut fonds_scope = web::scope("");
                fonds_scope = fonds_scope.guard(guard::Header(guard_name, guard_value));
                app = app.service(
                    fonds_scope
                        .app_data(web::Data::new(shared_state))
                        .app_data(web::Data::new(Arc::clone(&data_state)))
                        .configure(|cfg| {
                            register_root_routes(cfg, guarded_fonds).unwrap_or_else(|_| {
                                tracing::error!(
                                    "Failed to initialize routes for Fonds: {}",
                                    guarded_fonds.get_qualified_name()
                                );
                                process::exit(1);
                            });
                        }),
                );
            }
        }
    } else {
        let err_msg = "Failed to initialize guarded routes. Header name or values not found.";
        tracing::error!(err_msg);
        anyhow::bail!(err_msg);
    }
    Ok(app)
}

/// Initialize all dynamic routes for the given Archive.
///
/// # Errors
/// Errors if unable to register dynamic routes (e.g. if git repository cannot be opened)
fn initialize_dynamic_routes<
    T: MessageBody,
    U: ServiceFactory<
        ServiceRequest,
        Response = ServiceResponse<T>,
        Config = (),
        InitError = (),
        Error = Error,
    >,
    V: Global + Clone + 'static,
>(
    mut app: App<U>,
    state: &V,
) -> anyhow::Result<App<U>> {
    tracing::info!("Initializing app");
    let root = state.archive().get_root()?;
    let shared_state = state::init_shared(root)?;
    let data_state: Arc<dyn Global> = Arc::new(state.clone());
    app = app.service(
        web::scope("")
            .app_data(web::Data::new(shared_state))
            .app_data(web::Data::new(Arc::clone(&data_state)))
            .configure(|cfg| {
                register_routes(cfg, state).unwrap_or_else(|_| {
                    tracing::error!(
                        // TODO: error handling
                        "Failed to initialize routes for root Fonds: {}",
                        root.get_qualified_name()
                    );
                    process::exit(1);
                });
            }),
    );
    Ok(app)
}

/// Registers all dynamic routes for the given Archive
/// Each current document routes consists of two dynamic segments: `{prefix}/{tail}`.
/// prefix: the first part of the request uri, used to determine which dependent Fonds to serve.
/// tail: the remaining glob pattern path of the request uri.
/// # Arguments
/// * `cfg` - The Actix `ServiceConfig`
/// * `state` - The application state
/// # Errors
/// Will error if unable to register routes (e.g. if git repository cannot be opened)
#[expect(
    clippy::iter_over_hash_type,
    reason = "List of repositories that are registered as routes are always sorted, even with iterating over hash type"
)]
fn register_routes<T: Global>(cfg: &mut web::ServiceConfig, state: &T) -> anyhow::Result<()> {
    for fonds in state.archive().fonds_map.values() {
        if let Some(repositories) = fonds.repositories.as_ref() {
            if fonds.is_root() {
                continue;
            }
            register_dependent_routes(cfg, fonds, repositories)?;
        }
    }
    let root = state.archive().get_root()?;
    register_root_routes(cfg, root)?;
    Ok(())
}

/// Register routes for the root Fonds
/// Root Fonds is the Fonds specified in config.toml
/// # Arguments
/// * `cfg` - The Actix `ServiceConfig`
/// * `fonds` - The root Fonds
/// # Errors
/// Will error if unable to register routes (e.g. if git repository cannot be opened)
fn register_root_routes(cfg: &mut web::ServiceConfig, fonds: &Fonds) -> anyhow::Result<()> {
    let mut root_scope: Scope = web::scope("");
    if let Some(repositories) = fonds.repositories.as_ref() {
        let sorted_repositories = repositories.get_sorted();
        for repository in sorted_repositories {
            let custom = &repository.custom;
            let repo_state = state::init_repo(repository, fonds)?;
            for route in custom.routes.iter().flat_map(|routes| routes.iter()) {
                let actix_route = format!("/{{tail:{route}}}");
                root_scope = root_scope.service(
                    web::resource(actix_route.as_str())
                        .route(web::get().to(serve))
                        .route(web::head().to(serve))
                        .app_data(web::Data::new(repo_state.clone())),
                );
            }
            if let Some(underscore_scope) = custom.scope.as_ref() {
                let actix_underscore_scope = web::scope(underscore_scope.as_str()).service(
                    web::scope("").service(
                        web::resource("/{tail:.*}")
                            .route(web::get().to(serve))
                            .route(web::head().to(serve))
                            .app_data(web::Data::new(repo_state.clone())),
                    ),
                );
                cfg.service(actix_underscore_scope);
            }
        }
        cfg.service(root_scope);
    }
    Ok(())
}

/// Register routes for dependent Fonds
/// Dependent Fonds are all Fonds' specified in the root Fonds's `dependencies.json` config file.
/// # Arguments
/// * `cfg` - The Actix `ServiceConfig`
/// * `fonds` - The root Fonds
/// * `repositories` - Data repositories of the dependent Fonds
/// # Errors
/// Will error if unable to register routes (e.g. if git repository cannot be opened)
fn register_dependent_routes(
    cfg: &mut web::ServiceConfig,
    fonds: &Fonds,
    repositories: &Repositories,
) -> anyhow::Result<()> {
    let sorted_repositories = repositories.get_sorted();
    for scope in repositories.scopes.iter().flat_map(|scopes| scopes.iter()) {
        let scope_str = format!("/{{prefix:{}}}", scope.as_str());
        let mut actix_scope = web::scope(scope_str.as_str());
        for repository in &sorted_repositories {
            let custom = &repository.custom;
            let repo_state = state::init_repo(repository, fonds)?;
            for route in custom.routes.iter().flat_map(|routes| routes.iter()) {
                if route.starts_with('_') {
                    // Ignore routes in dependent Fonds that start with underscore
                    // These routes are handled by the root Fonds.
                    continue;
                }
                let actix_route = format!("/{{tail:{route}}}");
                actix_scope = actix_scope.service(
                    web::resource(actix_route.as_str())
                        .route(web::get().to(serve))
                        .route(web::head().to(serve))
                        .app_data(web::Data::new(repo_state.clone())),
                );
            }
        }
        cfg.service(actix_scope);
    }
    Ok(())
}
