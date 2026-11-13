//! Handlers for serving historical documents.
#![expect(
    clippy::future_not_send,
    reason = "We don't worry about git2-rs not implementing `Send` trait"
)]
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use chrono::NaiveDate;
use std::convert::Into;

use crate::{
    db::{
        models::{
            document_change, document_element, library, library_change,
            publication::{self, Publication},
        },
        DatabaseConnection,
    },
    fonds::archive::Archive,
    utils::paths::clean_path,
};

use self::response::messages;

use super::state::{App as AppState, Global as _};

/// Name of the current publication.
pub const CURRENT_PUBLICATION_NAME: &str = "Current";
/// Name of the current version.
pub const CURRENT_VERSION_NAME: &str = "Current";
/// Date of the current version.
pub const CURRENT_VERSION_DATE: &str = "current";

/// Module that maps the HTTP web request body to structs.
pub mod request;

/// Module that maps the HTTP web response to structs.
pub mod response;

/// Handler for the versions endpoint.
#[tracing::instrument(skip(req, data))]
pub async fn versions(
    req: HttpRequest,
    data: web::Data<AppState>,
    params: web::Path<request::Version>,
) -> impl Responder {
    let fonds = match get_fonds_from_request(&req, data.archive()) {
        Ok(fonds) => fonds,
        Err(err) => {
            tracing::error!("Error getting fonds from request: {err}");
            return HttpResponse::BadRequest().body(format!("Error: {err}"));
        }
    };
    let db = data.db();
    let mut publications = publication::Manager::find_all_non_revoked_publications(db, &fonds)
        .await
        .unwrap_or_default();

    let Some(current_publication) = publications.first() else {
        tracing::warn!("No publications found for fonds: {fonds}");
        return HttpResponse::NotFound().body("No publications found.");
    };

    let mut active_publication_name = params
        .publication
        .clone()
        .unwrap_or_else(|| current_publication.name.clone())
        .to_lowercase();

    let active_publication = publications
        .iter()
        .find(|pb| pb.name == active_publication_name);

    let is_current_publication = active_publication_name == current_publication.name
        || active_publication_name == CURRENT_PUBLICATION_NAME.to_lowercase();

    let url = clean_url_path(&params.path.clone().unwrap_or_default());
    let mut versions = if let Some(publication) = active_publication {
        publication_versions(db, publication, url.clone()).await
    } else if active_publication_name == "current" {
        publication_versions(db, current_publication, url.clone()).await
    } else {
        vec![]
    };

    // active version is the version the user is looking at right now
    let active_version =
        NaiveDate::parse_from_str(params.date.as_deref().unwrap_or_default(), "%Y-%m-%d")
            .map_or_else(
                |_| {
                    if is_current_publication {
                        CURRENT_VERSION_DATE.to_owned()
                    } else {
                        // Historical publications have no "current" version: default to
                        // the publication's own latest (real, dated) version instead.
                        versions
                            .first()
                            .map_or_else(|| CURRENT_VERSION_DATE.to_owned(), |ver| ver.date.clone())
                    }
                },
                |date| date.to_string(),
            );
    let active_compare_to = params.compare_date.clone().map(|date| {
        NaiveDate::parse_from_str(&date, "%Y-%m-%d")
            .map_or_else(|_| date, |active_date| active_date.to_string())
    });

    let messages = messages::historical(
        &versions,
        current_publication.name.as_str(),
        &active_publication_name,
        &params.date,
        &active_compare_to,
        active_publication.is_some(),
    );

    if active_publication_name == current_publication.name.clone() && params.publication.is_none() {
        CURRENT_PUBLICATION_NAME
            .to_lowercase()
            .clone_into(&mut active_publication_name);
    }

    response::Version::insert_if_not_present(&mut versions, params.date.clone());
    response::Version::insert_if_not_present(&mut versions, active_compare_to.clone());

    finalize_versions(&mut versions, is_current_publication);

    let current_publication_name = current_publication.name.clone();
    // duplicate current publication with current label
    publications.insert(
        0,
        Publication::new(
            current_publication.id.clone(),
            CURRENT_PUBLICATION_NAME.to_lowercase(),
            current_publication.date.clone(),
            current_publication.fonds.clone(),
        ),
    );

    HttpResponse::Ok().json(response::Versions::build(
        &active_publication_name,
        active_version,
        active_compare_to,
        &url,
        &publications,
        &current_publication_name,
        &versions,
        messages,
    ))
}

/// Get all the versions of a publication.
#[expect(
    clippy::module_name_repetitions,
    reason = "publication_versions is more descriptive than versions_publication"
)]
pub async fn publication_versions(
    db: &DatabaseConnection,
    publication: &Publication,
    url: String,
) -> Vec<response::Version> {
    tracing::debug!("Fetching publication versions for '{url}'");
    let mut versions = vec![];
    let doc_mpath =
        document_element::Manager::find_doc_mpath_by_url(db, &url, &publication.fonds).await;
    if let Ok(mpath) = doc_mpath {
        let doc_versions =
            document_change::Manager::find_all_document_versions_by_mpath_and_publication(
                db,
                &mpath,
                &publication.id,
            )
            .await
            .unwrap_or_default();
        versions = doc_versions.into_iter().map(Into::into).collect();
    } else {
        let lib_mpath = library::Manager::find_lib_mpath_by_url(db, &url, &publication.fonds).await;
        if let Ok(mpath) = lib_mpath {
            let coll_versions =
                library_change::Manager::find_all_collection_versions_by_mpath_and_publication(
                    db,
                    &mpath,
                    &publication.id,
                )
                .await
                .unwrap_or_default();
            versions = coll_versions.into_iter().map(Into::into).collect();
        }
    }
    tracing::debug!("Found {} versions", versions.len());
    versions
}

/// Extracts the fonds from the request.
/// If the `X-Fonds` header is present, it will return the value of the header.
/// Otherwise, it will return the root fonds.
///
/// # Errors
/// Errors if X-Fonds is in invalid format
pub fn get_fonds_from_request(req: &HttpRequest, archive: &Archive) -> anyhow::Result<String> {
    let req_headers = req.headers();
    let fonds = archive.get_root()?.get_qualified_name();

    req_headers.get("X-Fonds").map_or_else(
        || Ok(fonds),
        |value| {
            value.to_str().map_or_else(
                |_| anyhow::bail!("Invalid X-Fonds header value"),
                |str| Ok(str.to_owned()),
            )
        },
    )
}

/// Finalize a list of versions for the response: assign display strings and
/// descending indices, flag the newest entry as "(last modified)", and,
/// only when `include_current` is true, insert the synthetic "Current"
/// version. Historical (non-current) publications are frozen, so they must
/// never be labelled "Current".
fn finalize_versions(versions: &mut Vec<response::Version>, include_current: bool) {
    let versions_size = versions.len();
    for (idx, version) in versions.iter_mut().enumerate() {
        version.display = format_date(&version.date.clone());
        version.index = versions_size - idx;
    }
    if let Some(ver) = versions.first_mut() {
        ver.display.push_str(" (last modified)");
    }

    if include_current {
        let current_version = response::Version::new(
            CURRENT_VERSION_DATE.to_owned(),
            CURRENT_VERSION_NAME.to_owned(),
            versions.first().map_or(0, |ver| ver.index),
        );
        versions.insert(versions_size - current_version.index, current_version);
    }
}

/// Format a date from %Y-%m-%d to %B %d, %Y.
/// Any other input is returned HTML-escaped, since messages are rendered as HTML.
fn format_date(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_or_else(
        |_| {
            date.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&#39;")
        },
        |found_date| found_date.format("%B %d, %Y").to_string(),
    )
}

/// Clean the url path by removing the trailing slash.
fn clean_url_path(path: &str) -> String {
    let mut url = String::from('/');
    let url_parts = clean_path(path);
    url.push_str(&url_parts);
    url
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::inline_modules,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests"
)]
mod tests {
    use super::{
        finalize_versions, format_date, response, CURRENT_VERSION_DATE, CURRENT_VERSION_NAME,
    };

    #[test]
    fn format_date_when_not_a_date_expect_html_escaped() {
        let actual = format_date(r#"<img src="x">&'"#);
        let expected = "&lt;img src=&quot;x&quot;&gt;&amp;&#39;";
        assert_eq!(actual, expected);
    }

    fn sample_versions() -> Vec<response::Version> {
        vec![
            response::Version::new("2024-06-01".to_owned(), String::new(), 0),
            response::Version::new("2024-01-01".to_owned(), String::new(), 0),
        ]
    }

    #[test]
    fn finalize_versions_includes_current_when_current_publication() {
        let mut versions = sample_versions();

        finalize_versions(&mut versions, true);

        let current = versions
            .iter()
            .find(|ver| ver.date == CURRENT_VERSION_DATE)
            .expect("current version should be present");
        assert_eq!(current.display, CURRENT_VERSION_NAME);
        assert_eq!(current.index, 2);
        assert_eq!(versions.len(), 3);
    }

    #[test]
    fn finalize_versions_omits_current_when_historical_publication() {
        let mut versions = sample_versions();

        finalize_versions(&mut versions, false);

        assert!(versions.iter().all(|ver| ver.date != CURRENT_VERSION_DATE));
        assert_eq!(versions.len(), 2);
        assert!(versions
            .first()
            .unwrap()
            .display
            .ends_with("(last modified)"));
    }

    #[test]
    fn finalize_versions_indices_and_last_modified_unaffected_by_current_flag() {
        let mut with_current = sample_versions();
        let mut without_current = sample_versions();

        finalize_versions(&mut with_current, true);
        finalize_versions(&mut without_current, false);

        for (with_ver, without_ver) in with_current
            .iter()
            .filter(|ver| ver.date != CURRENT_VERSION_DATE)
            .zip(without_current.iter())
        {
            assert_eq!(with_ver.date, without_ver.date);
            assert_eq!(with_ver.display, without_ver.display);
            assert_eq!(with_ver.index, without_ver.index);
        }
    }
}
