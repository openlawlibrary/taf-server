//! Handlers for the cascading (multi-level) menu endpoint.
//!
//! Date-first: the node set comes from `document_change` filtered to one
//! version. A missing date is a 400, never a fallback to the whole tree;
//! undated browsing lives behind the explicit `_tree` segment. See
//! `docs/multi-level-menu-design.md`.
#![expect(
    clippy::future_not_send,
    reason = "We don't worry about git2-rs not implementing `Send` trait"
)]
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use chrono::NaiveDate;

use crate::db::{
    models::{
        changed_library_document, document_change, document_element, library,
        publication::{self, Publication},
        publication_version,
    },
    DatabaseConnection,
};

use self::collection::COLLECTION_MENU_DEPTH;
use self::mpath::{mpath_depth, prefix_upper_bound};
use self::response::{Context, Facts, Mode, RootKind};
use self::tree::Layout;

use super::state::Global;
use super::versions::{
    clean_url_path, get_fonds_from_request, CURRENT_PUBLICATION_NAME, CURRENT_VERSION_DATE,
};

/// Collection (library) roots.
pub mod collection;
/// Pure materialized-path helpers.
pub mod mpath;
/// Module that maps the HTTP web request to structs.
pub mod request;
/// Module that maps the HTTP web response to structs.
pub mod response;
/// Natural ordering for sibling nodes.
pub mod sort;
/// Pure node-set synthesis and layout.
pub mod tree;

/// Levels below the requested node returned when the client does not ask.
pub const DEFAULT_MENU_DEPTH: usize = 2;
/// Smallest meaningful menu depth; one level below the root.
pub const MIN_MENU_DEPTH: usize = 1;
/// Hard server-side ceiling, counted *relative* to the requested root.
/// Deepest measured absolute mpath depth in the dev archive is 7 (Maryland).
pub const MAX_MENU_DEPTH: usize = 6;
/// Hard cap on nodes in one response.
pub const MAX_MENU_NODES: usize = 5_000;

/// Body returned when a non-`_tree` route carries no date, naming both legal forms.
const MISSING_DATE_MESSAGE: &str = concat!(
    "A version date is required. Use /_api/menu/_date/{date}/{path} or ",
    "/_api/menu/_publication/{publication}/_date/{date}/{path}, where {date} is ",
    "YYYY-MM-DD or 'current'. For undated browsing of the whole tree use ",
    "/_api/menu/_tree/{path}."
);

/// Body returned for an unexpected server-side failure. The detail is logged
/// rather than served, so a database error never reaches the client.
const INTERNAL_MESSAGE: &str = "Failed to build the menu.";

/// Classify a manager lookup failure: an absent row is a 404, anything else a
/// server failure that must not render as "this page has no menu".
fn missing_or_internal<T>(context: &str, err: &anyhow::Error) -> Result<Option<T>, MenuError> {
    if let Some(found) = err.downcast_ref::<sqlx::Error>() {
        if matches!(*found, sqlx::Error::RowNotFound) {
            return Ok(None);
        }
    }
    Err(MenuError::Internal(format!("{context} failed: {err}")))
}

/// Resolve a url to a document element materialized path.
async fn find_document_base(
    db: &DatabaseConnection,
    url: &str,
    fonds: &str,
) -> Result<Option<String>, MenuError> {
    match document_element::Manager::find_doc_mpath_by_url(db, url, fonds).await {
        Ok(base) => Ok(Some(base)),
        Err(err) => missing_or_internal("Document element lookup", &err),
    }
}

/// Resolve a url to a library materialized path.
async fn find_library_base(
    db: &DatabaseConnection,
    url: &str,
    fonds: &str,
) -> Result<Option<String>, MenuError> {
    match library::Manager::find_lib_mpath_by_url(db, url, fonds).await {
        Ok(base) => Ok(Some(base)),
        Err(err) => missing_or_internal("Library lookup", &err),
    }
}

/// Reject a base materialized path that is not `|`-terminated: its depth is 0,
/// so every truncation yields the empty string and the node vanishes silently.
fn validate_base(base: &str) -> Result<(), MenuError> {
    if base.ends_with('|') {
        return Ok(());
    }
    Err(MenuError::Internal(format!(
        "Malformed base materialized path '{base}'"
    )))
}

/// Why a menu request could not be served.
#[derive(Debug)]
enum MenuError {
    /// The request was malformed.
    BadRequest(String),
    /// Nothing matched.
    NotFound(String),
    /// Something failed server-side; the string is logged, not served.
    Internal(String),
}

impl MenuError {
    /// Turn a failure into the HTTP response the client sees.
    fn into_response(self) -> HttpResponse {
        match self {
            Self::BadRequest(message) => HttpResponse::BadRequest().body(message),
            Self::NotFound(message) => HttpResponse::NotFound().body(message),
            Self::Internal(detail) => {
                tracing::error!("Menu request failed: {detail}");
                HttpResponse::InternalServerError().body(INTERNAL_MESSAGE)
            }
        }
    }
}

/// Handler for the dated menu endpoint. Also serves the dateless route
/// variants, which answer 400 naming the legal forms.
#[tracing::instrument(skip(req, data))]
pub async fn menu(
    req: HttpRequest,
    data: web::Data<Arc<dyn Global>>,
    params: web::Path<request::Menu>,
    query: web::Query<request::Depth>,
) -> impl Responder {
    match serve_dated(&req, &data, &params, &query).await {
        Ok(built) => HttpResponse::Ok().json(built),
        Err(err) => err.into_response(),
    }
}

/// Handler for the undated (tree-mode) menu endpoint.
#[tracing::instrument(skip(req, data))]
#[expect(
    clippy::module_name_repetitions,
    reason = "`menu_tree` is the handler name the route table reads best with, beside `menu`"
)]
pub async fn menu_tree(
    req: HttpRequest,
    data: web::Data<Arc<dyn Global>>,
    params: web::Path<request::Menu>,
    query: web::Query<request::Depth>,
) -> impl Responder {
    match serve_tree(&req, &data, &params, &query).await {
        Ok(built) => HttpResponse::Ok().json(built),
        Err(err) => err.into_response(),
    }
}

/// Build a dated menu response, or the failure that explains why not.
async fn serve_dated(
    req: &HttpRequest,
    data: &web::Data<Arc<dyn Global>>,
    params: &request::Menu,
    query: &request::Depth,
) -> Result<response::Menu, MenuError> {
    let fonds = get_fonds_from_request(req, data.archive())
        .map_err(|err| MenuError::BadRequest(format!("Error: {err}")))?;
    let Some(requested_date) = params.date.clone() else {
        return Err(MenuError::BadRequest(MISSING_DATE_MESSAGE.to_owned()));
    };

    let db = data.db();
    let publications = publication::Manager::find_all_non_revoked_publications(db, &fonds)
        .await
        .unwrap_or_default();
    let Some(current_publication) = publications.first() else {
        tracing::warn!("No publications found for fonds: {fonds}");
        return Err(MenuError::NotFound("No publications found.".to_owned()));
    };
    let requested_name = params
        .publication
        .clone()
        .unwrap_or_else(|| current_publication.name.clone())
        .to_lowercase();
    let Some(active_publication) = resolve_publication(&publications, &requested_name) else {
        return Err(MenuError::NotFound(format!(
            "No publication {requested_name} in fonds {fonds}"
        )));
    };
    let active_publication_name =
        if active_publication.name == current_publication.name && params.publication.is_none() {
            CURRENT_PUBLICATION_NAME.to_lowercase()
        } else {
            requested_name
        };

    let version = resolve_date(db, active_publication, &requested_date).await?;
    let url = clean_url_path(params.path.as_deref().unwrap_or_default());
    let depth = query.resolved();

    if let Some(base) = find_document_base(db, &url, &fonds).await? {
        validate_base(&base)?;
        let changes = document_change::Manager::find_changes_in_version_by_prefix(
            db,
            &base,
            &active_publication.id,
            &version.date,
        )
        .await
        .map_err(|err| MenuError::Internal(format!("Query 1 failed for {base}: {err}")))?;
        let node_set = tree::synthesize_dated(&changes, &base, depth);
        let layout = tree::plan(&node_set, &base, MAX_MENU_NODES);
        let facts = document_facts(db, &fonds, &layout.retained(&base)).await?;
        return Ok(response::Menu::build(
            Context {
                path: strip_leading_slash(&url),
                active_publication: active_publication_name,
                mode: Mode::Date,
                date: Some(version.date),
                root_kind: RootKind::Document,
                depth,
                requested_depth: query.requested(),
            },
            &node_set,
            &layout,
            &base,
            &facts,
        ));
    }

    let Some(base_lib) = find_library_base(db, &url, &fonds).await? else {
        return Err(MenuError::NotFound(format!(
            "No document or collection at {url}"
        )));
    };
    validate_base(&base_lib)?;
    let rows = changed_library_document::Manager::find_changes_by_library_prefix_and_version_ids(
        db,
        &base_lib,
        &version.ids,
    )
    .await
    .map_err(|err| MenuError::Internal(format!("Query C1 failed for {base_lib}: {err}")))?;
    let built = collection::synthesize(&rows, &base_lib);
    let layout = tree::plan_from_links(&built.links, &base_lib, MAX_MENU_NODES);
    let facts = collection_facts(db, &fonds, &built, &layout, &base_lib).await?;
    Ok(response::Menu::build(
        Context {
            path: strip_leading_slash(&url),
            active_publication: active_publication_name,
            mode: Mode::Date,
            date: Some(version.date),
            root_kind: RootKind::Collection,
            depth: COLLECTION_MENU_DEPTH,
            requested_depth: query.requested(),
        },
        &built.node_set,
        &layout,
        &base_lib,
        &facts,
    ))
}

/// Build an undated (tree-mode) menu response, or the failure that explains why
/// not. Tree mode is publication-independent, so `activePublication` is always
/// the current publication alias.
async fn serve_tree(
    req: &HttpRequest,
    data: &web::Data<Arc<dyn Global>>,
    params: &request::Menu,
    query: &request::Depth,
) -> Result<response::Menu, MenuError> {
    let fonds = get_fonds_from_request(req, data.archive())
        .map_err(|err| MenuError::BadRequest(format!("Error: {err}")))?;
    let db = data.db();
    let url = clean_url_path(params.path.as_deref().unwrap_or_default());
    let depth = query.resolved();
    let context = |root_kind: RootKind| Context {
        path: strip_leading_slash(&url),
        active_publication: CURRENT_PUBLICATION_NAME.to_lowercase(),
        mode: Mode::Tree,
        date: None,
        root_kind,
        depth,
        requested_depth: query.requested(),
    };

    if let Some(base) = find_document_base(db, &url, &fonds).await? {
        validate_base(&base)?;
        let upper = upper_bound(&base)?;
        let rows = document_element::Manager::find_elements_in_mpath_range(
            db,
            &base,
            &upper,
            &fonds,
            scan_depth(&base, depth)?,
        )
        .await
        .map_err(|err| MenuError::Internal(format!("Tree scan failed for {base}: {err}")))?;
        let mpaths: Vec<String> = rows
            .iter()
            .map(|row| row.doc_mpath.clone())
            .collect::<Vec<String>>();
        let node_set = tree::synthesize_tree(&mpaths, &base, depth);
        let layout = tree::plan(&node_set, &base, MAX_MENU_NODES);
        let mut facts: BTreeMap<String, Facts> = BTreeMap::new();
        for row in rows {
            facts.insert(
                row.doc_mpath,
                Facts {
                    doc_id: row.doc_id,
                    url: row.url,
                },
            );
        }
        return Ok(response::Menu::build(
            context(RootKind::Document),
            &node_set,
            &layout,
            &base,
            &facts,
        ));
    }

    let Some(base_lib) = find_library_base(db, &url, &fonds).await? else {
        return Err(MenuError::NotFound(format!(
            "No document or collection at {url}"
        )));
    };
    validate_base(&base_lib)?;
    // Same two-level shape as dated mode, without a version. `/_api/menu/_tree`
    // with no path resolves to `/`, a library in every fonds, so this is tree
    // mode's own entry point.
    let rows = changed_library_document::Manager::find_documents_by_library_prefix(db, &base_lib)
        .await
        .map_err(|err| {
            MenuError::Internal(format!("Tree collection scan failed for {base_lib}: {err}"))
        })?;
    let synthesized = collection::synthesize_undated(&rows, &base_lib);
    let expandable =
        member_documents_with_children(db, &fonds, &synthesized.document_mpaths).await?;
    let built = collection::with_member_children(synthesized, &expandable);
    let layout = tree::plan_from_links(&built.links, &base_lib, MAX_MENU_NODES);
    let facts = collection_facts(db, &fonds, &built, &layout, &base_lib).await?;
    Ok(response::Menu::build(
        Context {
            path: strip_leading_slash(&url),
            active_publication: CURRENT_PUBLICATION_NAME.to_lowercase(),
            mode: Mode::Tree,
            date: None,
            root_kind: RootKind::Collection,
            depth: COLLECTION_MENU_DEPTH,
            requested_depth: query.requested(),
        },
        &built.node_set,
        &layout,
        &base_lib,
        &facts,
    ))
}

/// Resolve a publication name as `versions()` does: lowercase match over the
/// non-revoked publications, with `"current"` aliasing the newest.
fn resolve_publication<'all>(
    publications: &'all [Publication],
    requested_name: &str,
) -> Option<&'all Publication> {
    publications
        .iter()
        .find(|pb| pb.name == requested_name)
        .or_else(|| {
            if requested_name == CURRENT_PUBLICATION_NAME.to_lowercase() {
                publications.first()
            } else {
                None
            }
        })
}

/// A version date that exists in the requested publication, plus the
/// `publication_version` ids carrying it. Two ids union their change sets.
#[derive(Debug)]
struct ResolvedVersion {
    /// Normalised `YYYY-MM-DD` version date.
    date: String,
    /// `publication_version` ids carrying that date in this publication.
    ids: Vec<String>,
}

/// Resolve the requested date against a publication.
///
/// The existence check separates "no such date here" (404) from "that date
/// exists but nothing under this path changed" (200, empty root).
async fn resolve_date(
    db: &DatabaseConnection,
    active_publication: &Publication,
    requested_date: &str,
) -> Result<ResolvedVersion, MenuError> {
    let normalised = if requested_date == CURRENT_VERSION_DATE {
        publication_version::Manager::find_latest_version_for_publication(
            db,
            &active_publication.id,
        )
        .await
        .map_err(|err| MenuError::Internal(format!("Latest version lookup failed: {err}")))?
        .ok_or_else(|| {
            MenuError::NotFound(format!(
                "No versions in publication {}",
                active_publication.name
            ))
        })?
    } else {
        NaiveDate::parse_from_str(requested_date, "%Y-%m-%d")
            .map_err(|err| {
                MenuError::BadRequest(format!(
                    "Invalid date '{requested_date}', expected YYYY-MM-DD or 'current': {err}"
                ))
            })?
            .to_string()
    };
    let ids = publication_version::Manager::find_version_ids_in_publication(
        db,
        &active_publication.id,
        &normalised,
    )
    .await
    .map_err(|err| MenuError::Internal(format!("Version lookup failed: {err}")))?;
    if ids.is_empty() {
        return Err(MenuError::NotFound(format!(
            "No version {normalised} in publication {}",
            active_publication.name
        )));
    }
    Ok(ResolvedVersion {
        date: normalised,
        ids,
    })
}

/// Ask the element tree which member-document roots can be expanded. One
/// grouped query; tree mode only, see [`collection::with_member_children`].
async fn member_documents_with_children(
    db: &DatabaseConnection,
    fonds: &str,
    doc_roots: &[String],
) -> Result<BTreeSet<String>, MenuError> {
    let roots = document_element::Manager::find_roots_with_children(db, doc_roots, fonds)
        .await
        .map_err(|err| MenuError::Internal(format!("Member document child probe failed: {err}")))?;
    Ok(roots.into_iter().collect())
}

/// Look up presentation labels for an explicit set of document element mpaths.
async fn document_facts(
    db: &DatabaseConnection,
    fonds: &str,
    mpaths: &[String],
) -> Result<BTreeMap<String, Facts>, MenuError> {
    let labels = document_element::Manager::find_labels_by_mpaths(db, mpaths, fonds)
        .await
        .map_err(|err| MenuError::Internal(format!("Query 2 failed: {err}")))?;
    let mut facts: BTreeMap<String, Facts> = BTreeMap::new();
    for label in labels {
        facts.insert(
            label.doc_mpath,
            Facts {
                doc_id: label.doc_id,
                url: label.url,
            },
        );
    }
    Ok(facts)
}

/// Look up labels for a collection menu: the child libraries from `library`,
/// the member document roots from `document_element`.
async fn collection_facts(
    db: &DatabaseConnection,
    fonds: &str,
    built: &collection::CollectionTree,
    layout: &Layout,
    base_lib: &str,
) -> Result<BTreeMap<String, Facts>, MenuError> {
    let retained = layout.retained(base_lib);
    let libraries: Vec<String> = retained
        .iter()
        .filter(|mpath| mpath.as_str() == base_lib || built.library_mpaths.contains(mpath))
        .cloned()
        .collect();
    let documents: Vec<String> = retained
        .iter()
        .filter(|mpath| built.document_mpaths.contains(mpath))
        .cloned()
        .collect();

    let mut facts = document_facts(db, fonds, &documents).await?;
    let library_labels = library::Manager::find_labels_by_mpaths(db, &libraries, fonds)
        .await
        .map_err(|err| MenuError::Internal(format!("Query C2 failed: {err}")))?;
    for label in library_labels {
        facts.insert(
            label.mpath,
            Facts {
                doc_id: None,
                url: label.url,
            },
        );
    }
    Ok(facts)
}

/// Exclusive upper bound for a base mpath's prefix range.
fn upper_bound(base: &str) -> Result<String, MenuError> {
    prefix_upper_bound(base)
        .ok_or_else(|| MenuError::Internal(format!("Malformed base materialized path '{base}'")))
}

/// Deepest mpath level the tree-mode scan reads: the window ceiling plus one
/// extra level, which is what makes `hasChildren` answerable at the frontier.
fn scan_depth(base: &str, depth: usize) -> Result<i64, MenuError> {
    i64::try_from(mpath_depth(base) + depth + 1)
        .map_err(|err| MenuError::Internal(format!("Depth does not fit in an i64: {err}")))
}

/// Drop the leading `/` that `clean_url_path` adds, matching `/versions`.
fn strip_leading_slash(url: &str) -> String {
    url.strip_prefix('/').unwrap_or(url).to_owned()
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests"
)]
mod tests {
    use super::{
        resolve_publication, scan_depth, strip_leading_slash, upper_bound, validate_base,
        DEFAULT_MENU_DEPTH, MAX_MENU_DEPTH, MAX_MENU_NODES, MIN_MENU_DEPTH,
    };
    use crate::db::models::publication::Publication;

    /// Build a publication with the given name.
    fn publication(name: &str) -> Publication {
        Publication::new(
            format!("id-{name}"),
            name.to_owned(),
            name.to_owned(),
            "cityofsanmateo/law".to_owned(),
        )
    }

    #[test]
    fn depth_rails_are_ordered_and_cover_the_deepest_corpus() {
        assert!(MIN_MENU_DEPTH <= DEFAULT_MENU_DEPTH);
        assert!(DEFAULT_MENU_DEPTH <= MAX_MENU_DEPTH);
        // The ceiling is relative to the requested root, not an absolute mpath
        // depth. Measured absolute maxima in the dev archive: Maryland 7,
        // lco-nsn 6, San Mateo 5, sanipueblo 5, Mohican 4.
        assert_eq!(MAX_MENU_DEPTH, 6);
        assert_eq!(MAX_MENU_NODES, 5_000);
    }

    #[test]
    fn resolve_publication_matches_by_lowercase_name() {
        let publications = vec![publication("2026-04-21"), publication("2025-11-12")];
        assert_eq!(
            resolve_publication(&publications, "2025-11-12").map(|found| found.name.as_str()),
            Some("2025-11-12")
        );
    }

    #[test]
    fn resolve_publication_aliases_current_to_the_newest() {
        let publications = vec![publication("2026-04-21"), publication("2025-11-12")];
        assert_eq!(
            resolve_publication(&publications, "current").map(|found| found.name.as_str()),
            Some("2026-04-21")
        );
    }

    #[test]
    fn resolve_publication_rejects_an_unknown_name() {
        let publications = vec![publication("2026-04-21")];
        assert!(resolve_publication(&publications, "1999-01-01").is_none());
        assert!(resolve_publication(&[], "current").is_none());
    }

    #[test]
    fn validate_base_rejects_a_path_that_is_not_delimiter_terminated() {
        // `doc_mpath = 'None'` really exists in the dev archive, with 178
        // `document_change` rows behind it. Its depth is 0, so every truncation
        // yields the empty string and the node would be dropped silently.
        assert!(validate_base("City of San Mateo, Cal., Code|").is_ok());
        assert!(validate_base("None").is_err());
        assert!(validate_base("").is_err());
    }

    #[test]
    fn upper_bound_rejects_a_malformed_base() {
        assert_eq!(upper_bound("Code|").ok(), Some("Code}".to_owned()));
        assert!(upper_bound("Code").is_err());
    }

    #[test]
    fn scan_depth_adds_the_window_plus_one_extra_level() {
        assert_eq!(scan_depth("Code|", 2).ok(), Some(4));
        assert_eq!(scan_depth("Code|10|10.04|", 1).ok(), Some(5));
    }

    #[test]
    fn strip_leading_slash_matches_the_versions_path_field() {
        assert_eq!(
            strip_leading_slash("/us/ca/cities/san-mateo/code"),
            "us/ca/cities/san-mateo/code"
        );
        assert_eq!(strip_leading_slash("/"), "");
        assert_eq!(strip_leading_slash(""), "");
    }
}
