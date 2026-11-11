//! Module for inserting changes into the database
#![expect(
    clippy::shadow_reuse,
    reason = "Bindings that shadow other bindings in the same scope are used to make the code a bit more legible in this module"
)]
#![expect(
    clippy::future_not_send,
    reason = "We don't worry about git2-rs not implementing `Send` trait"
)]
use super::rdf::graph::Bag;
use crate::db::models::changed_library_document::{self, ChangedLibraryDocument};
use crate::db::models::data_repo_commits::{self, DataRepoCommits};
use crate::db::models::document_change::{self, DocumentChange};
use crate::db::models::document_element::DocumentElement;
use crate::db::models::library::{self, Library};
use crate::db::models::library_change::{self, LibraryChange};
use crate::db::models::publication::{self, Publication};
use crate::db::models::publication_has_publication_versions::{
    self, PublicationHasPublicationVersions,
};
use crate::db::models::publication_version;
use crate::db::models::status::Status;
use crate::db::models::{document, document_element};
use crate::db::models::{fonds, version};
use crate::db::{DatabaseTransaction, Tx as _};
use crate::fonds::fonds::Fonds;
use crate::fonds::types::repositories::Repository;
use crate::history::rdf::graph::{FondsGraph, FondsTerm};
use crate::history::rdf::namespaces::{dcterms, oll};
use crate::redirects::insert_redirects_for_fonds;
use crate::server::errors::CliError;
use crate::utils::archive::get_name_parts;
use crate::utils::git::Repo;
use crate::utils::md5;
use crate::{
    db::{self, DatabaseConnection},
    fonds::archive::Archive,
};
use anyhow::Context as _;
use chrono::DateTime;
use git2::{TreeWalkMode, TreeWalkResult};
use sophia::api::ns::rdfs;
use sqlx::types::chrono::NaiveDate;
use std::{
    borrow::ToOwned,
    path::{Path, PathBuf},
    result::Result,
};

/// Inserts changes from the archive into the database
///
/// # Errors
/// Errors if the changes cannot be inserted into the archive
#[actix_web::main]
#[tracing::instrument(
    name = "Taf-server update",
    skip(raw_archive_path, archive_path, include, exclude, force)
)]
pub async fn insert(
    raw_archive_path: &str,
    archive_path: PathBuf,
    include: &Vec<String>,
    exclude: &Vec<String>,
    force: bool,
) -> Result<(), CliError> {
    if !include.is_empty() {
        tracing::info!("Following fonds are included: {:#?}", include);
    }
    if !exclude.is_empty() {
        tracing::info!("Following fonds are excluded: {:#?}", exclude);
    }
    let conn = match db::init::connect(&archive_path).await {
        Ok(conn) => conn,
        Err(err) => {
            tracing::error!(
                "error: could not connect to database.
                Confirm that `db.sqlite3` exists in `.taf` dir or that DATABASE_URL env var is set correctly."
            );
            tracing::error!("Error: {err:?}");
            return Err(CliError::DatabaseConnectionError);
        }
    };
    insert_changes_archive(
        &conn,
        raw_archive_path,
        &archive_path,
        include,
        exclude,
        force,
    )
    .await
    .map_err(|err| {
        tracing::error!("Failed to update fonds in the archive");
        tracing::error!("{err:?}");
        CliError::GenericError
    })
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Splitting would reduce readability"
)]
/// Insert changes from the archive into the database
async fn insert_changes_archive(
    conn: &DatabaseConnection,
    raw_archive_path: &str,
    archive_path: &Path,
    include: &[String],
    exclude: &[String],
    force: bool,
) -> anyhow::Result<()> {
    tracing::debug!("Inserting history into archive");

    let archive = Archive::parse(
        archive_path.to_path_buf(),
        &PathBuf::from(raw_archive_path),
        false,
    )?;
    let mut errors = Vec::new();
    for (name, mut fonds) in archive.get_all_fonds() {
        if exclude.contains(&name) || (!include.is_empty() && !include.contains(&name)) {
            tracing::info!("Skipping update for {:?}", name);
            continue;
        }
        let mut tx = DatabaseTransaction {
            tx: conn.pool.begin().await?,
        };
        match process_fonds(&mut tx, &name, &mut fonds, archive_path, force).await {
            Ok(()) => {
                tracing::debug!("Applying transaction for fonds: {name}");
                tx.commit().await?;
            }
            Err(err) => {
                tracing::error!("Rolling back transaction for fonds: {name} due to error: {err:?}");
                tx.rollback().await?;
                errors.push(format!("{name}: {err}"));
            }
        }
    }
    if !errors.is_empty() {
        let error_msg = errors.into_iter().collect::<Vec<_>>().join("\n");
        return Err(anyhow::anyhow!(
            "Errors occurred while inserting changes:\n{error_msg}"
        ));
    }
    Ok(())
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Splitting would reduce readability"
)]
/// Process the fonds and insert changes into the database
async fn process_fonds(
    tx: &mut DatabaseTransaction,
    name: &str,
    fonds: &mut Fonds,
    archive_path: &Path,
    force: bool,
) -> anyhow::Result<()> {
    let Some(repositories) = fonds.get_repositories()? else {
        tracing::warn!("No repositories found for fonds: {name}");
        return Ok(());
    };

    insert_redirects_for_fonds(tx, fonds).await?;

    let Some(rdf_repo) = repositories.get_one_by_custom_type("rdf") else {
        tracing::warn!("No RDF repository found for fonds: {name}");
        return Ok(());
    };
    let rdf_repo_path = archive_path.to_path_buf().join(&rdf_repo.name);
    if !rdf_repo_path.exists() {
        return Err(anyhow::anyhow!(
            "RDF repository should exist on disk but not found: {}",
            rdf_repo_path.display()
        ));
    }
    let (rdf_org, rdf_name) = get_name_parts(&rdf_repo.name)?;
    let rdf = Repo::new(archive_path, &rdf_org, &rdf_name)?;
    if !rdf.path.join("_publication").exists() {
        tracing::warn!(
            "[{name}]: No publications found for RDF repository: {}",
            rdf.path.display()
        );
        return Ok(());
    }
    // Find the current (non-archived) HTML repo name to stamp on each publication record.
    let current_html_repo_name = repositories
        .get_all_by_custom_type("html")
        .into_iter()
        .find(|repo| !repo.is_archived())
        .map(|repo| repo.name.clone());
    // Collect historical HTML data repos once — used for both consistency checks and
    // commit-hash insertion below.
    let data_repos: Vec<&Repository> = repositories
        .get_all_by_serve_type("historical")
        .into_iter()
        .filter(|repo| repo.custom.repository_type.as_deref() == Some("html"))
        .collect();
    // Check database consistency and rebuild if needed.
    let needs_rebuild = if force {
        tracing::info!("[{name}] | --force flag set, rebuilding database from scratch");
        true
    } else {
        is_fonds_inconsistent(
            tx,
            name,
            fonds,
            &rdf,
            &data_repos,
            current_html_repo_name.as_deref(),
        )
        .await?
    };
    if needs_rebuild {
        if !force {
            tracing::warn!("[{name}] | Rebuilding database from scratch due to inconsistency");
        }
        fonds::TxManager::delete(tx, name).await?;
    }
    insert_changes_from_rdf_repository(tx, rdf, name, current_html_repo_name.as_deref()).await?;
    // Insert commit hashes for data repositories with serve type 'historical'
    for data_repo in data_repos {
        insert_commit_hashes_from_auth_repository(tx, fonds, data_repo).await?;
    }
    Ok(())
}

/// Insert changes from the RDF repository into the database
async fn insert_changes_from_rdf_repository(
    tx: &mut DatabaseTransaction,
    rdf_repo: Repo,
    fonds_id: &str,
    current_html_repo_name: Option<&str>,
) -> anyhow::Result<()> {
    tracing::debug!("Inserting changes from RDF repository: {}", fonds_id);
    tracing::debug!("RDF repository path: {}", rdf_repo.path.display());
    load_delta_for_fonds(tx, &rdf_repo, fonds_id, current_html_repo_name).await?;
    Ok(())
}

/// Load deltas from the publications
async fn load_delta_for_fonds(
    tx: &mut DatabaseTransaction,
    rdf_repo: &Repo,
    fonds: &str,
    current_html_repo_name: Option<&str>,
) -> anyhow::Result<()> {
    fonds::TxManager::create(tx, fonds).await?;
    if let Some(publication) = publication::TxManager::find_last_inserted(tx, fonds).await? {
        tracing::info!("[{fonds}] | Inserting RDF changes from last inserted publication");
        load_delta_from_publications(
            tx,
            rdf_repo,
            fonds,
            Some(publication),
            current_html_repo_name,
        )
        .await?;
    } else {
        tracing::info!("[{fonds}] | Inserting RDF changes from beginning...");
        load_delta_from_publications(tx, rdf_repo, fonds, None, current_html_repo_name).await?;
    }
    Ok(())
}

/// Iterate and load delta from all publications in the `_publication` directory
///
/// # Errors
/// Errors if the delta cannot be loaded from the publications
#[expect(
    clippy::too_many_lines,
    reason = "It's a complex function that handles multiple operations in a single step"
)]
#[expect(
    clippy::cognitive_complexity,
    reason = "It's a complex function that handles multiple operations in a single step"
)]
async fn load_delta_from_publications(
    tx: &mut DatabaseTransaction,
    rdf_repo: &Repo,
    fonds: &str,
    last_inserted_publication: Option<Publication>,
    current_html_repo_name: Option<&str>,
) -> anyhow::Result<()> {
    let head_commit = rdf_repo.repo.head()?.peel_to_commit()?;
    let tree = head_commit.tree()?;
    let publications_dir_entry = tree.get_path(&PathBuf::from("_publication"))?;
    let publications_subtree = rdf_repo.repo.find_tree(publications_dir_entry.id())?;
    let last_inserted_date = if let Some(last_inserted_pub) = last_inserted_publication.as_ref() {
        publication_version::TxManager::find_last_inserted_date_by_publication_id(
            tx,
            &last_inserted_pub.id,
        )
        .await?
        .map(|pv| {
            NaiveDate::parse_from_str(&pv.version, "%Y-%m-%d").context("Could not parse date")
        })
        .and_then(Result::ok)
    } else {
        None
    };
    for publication_entry in &publications_subtree {
        let mut pub_graph = FondsGraph::new();
        let object = publication_entry.to_object(&rdf_repo.repo)?;
        let publication_tree = object
            .as_tree()
            .context("Expected a tree but got something else")?;
        let index_rdf = publication_tree.get_path(&PathBuf::from("index.rdf"))?;
        let blob = rdf_repo.repo.find_blob(index_rdf.id())?;
        pub_graph.add_rdf_xml(blob.content())?;
        let pub_label = pub_graph.literal_from_triple_matching(None, Some(rdfs::label), None)?;
        let pub_name = pub_label
            .strip_prefix("Publication ")
            .context("Could not strip prefix")?
            .to_owned();
        let pub_date =
            pub_graph.literal_from_triple_matching(None, Some(dcterms::available), None)?;
        let pub_date = NaiveDate::parse_from_str(pub_date.as_str(), "%Y-%m-%d")?;
        let archived_html_repo: Option<String> = pub_graph
            .literal_from_triple_matching(None, Some(oll::archivedHtml), None)
            .ok();
        // continue from last inserted publication, since that publication can contain
        // new changes (versions) that are not in db
        let is_last_inserted = last_inserted_publication
            .as_ref()
            .is_some_and(|last_inserted_pub| last_inserted_pub.name == pub_name);
        if !is_last_inserted
            && publication::TxManager::exists_by_name_and_fonds(tx, &pub_name, fonds).await?
        {
            continue;
        }
        tracing::info!("[{fonds}] | Publication: {pub_name}");
        publication_tree.walk(TreeWalkMode::PreOrder, |_, entry| {
            let path_name = entry.name().unwrap_or_default();
            if path_name.contains(".rdf") {
                match rdf_repo.repo.find_blob(entry.id()) {
                    Ok(current_blob) => {
                        if let Err(err) = pub_graph.add_rdf_xml(current_blob.content()) {
                            tracing::error!(
                                "Error adding content to graph for entry {path_name}: {err:?}"
                            );
                        }
                    }
                    Err(err) => {
                        tracing::error!("Error finding blob for entry {path_name}: {err:?}");
                    }
                }
            }
            TreeWalkResult::Ok
        })?;
        let (last_valid_pub_name, last_valid_codified_date) =
            referenced_publication_information(&pub_graph);
        let publication_hash = md5::compute(format!("{}{}", pub_name.clone(), fonds));
        let last_inserted_pub_id = if let Some(valid_pub_name) = last_valid_pub_name {
            let Some(last_inserted_pub) =
                publication::TxManager::find_by_name_and_fonds(tx, &valid_pub_name, fonds).await?
            else {
                tracing::debug!(
                    "[{fonds}] | Publication {pub_name} not found in database after creation, which indicates revocation"
                );
                continue;
            };
            Some(last_inserted_pub.id)
        } else {
            None
        };
        // Determine which HTML data repo this publication belongs to.
        // If the RDF carries oll:archivedHtml it's the boundary publication: use the
        // archived repo name it declares.  Otherwise use the current HTML repo name.
        let html_data_repo_name: Option<String> = if archived_html_repo.is_some() {
            archived_html_repo.clone()
        } else {
            current_html_repo_name.map(ToOwned::to_owned)
        };
        publication::TxManager::create(
            tx,
            &publication_hash,
            &pub_name,
            &pub_date,
            fonds,
            last_inserted_pub_id,
            last_valid_codified_date,
            html_data_repo_name,
        )
        .await?;
        // If this is the boundary publication, backfill all earlier publications for
        // this fonds with the same archived HTML repo name.
        if let Some(archived_repo) = archived_html_repo.as_deref() {
            publication::TxManager::set_html_data_repo_name_for_prior_publications(
                tx,
                fonds,
                &pub_date,
                archived_repo,
            )
            .await?;
        }
        let Some(publication) =
            publication::TxManager::find_by_name_and_fonds(tx, &pub_name, fonds).await?
        else {
            tracing::debug!(
                    "[{fonds}] | Publication {pub_name} not found in database after creation, which indicates revocation"
                );
            continue;
        };
        let resume_date = last_inserted_date.filter(|_| is_last_inserted);
        load_delta_for_publication(tx, publication, &pub_graph, resume_date).await?;
    }
    Ok(())
}

/// Load all deltas for the publication given a fonds
///
/// # Errors
/// Errors if database connection fails or if delta cannot be loaded for the publication
async fn load_delta_for_publication(
    tx: &mut DatabaseTransaction,
    publication: Publication,
    pub_graph: &FondsGraph,
    last_inserted_date: Option<NaiveDate>,
) -> anyhow::Result<()> {
    let pub_document_versions =
        pub_graph.all_iris_from_triple_matching(None, None, Some(oll::DocumentVersion))?;
    let pub_collection_versions =
        pub_graph.all_iris_from_triple_matching(None, None, Some(oll::CollectionVersion))?;

    if let Some(last_inserted_date) = last_inserted_date.as_ref() {
        publication_version::TxManager::delete_by_publication_id_and_version(
            tx,
            &publication.id,
            &last_inserted_date.to_string(),
        )
        .await?;
    }

    insert_document_changes(
        tx,
        last_inserted_date.as_ref(),
        pub_document_versions,
        pub_graph,
        &publication,
    )
    .await?;

    insert_library_changes(
        tx,
        last_inserted_date.as_ref(),
        pub_collection_versions,
        pub_graph,
        &publication,
    )
    .await?;

    insert_shared_publication_versions_for_publication(tx, &publication).await?;

    revoke_same_date_publications(tx, publication).await?;

    Ok(())
}

/// Insert document changes into the database
async fn insert_document_changes(
    tx: &mut DatabaseTransaction,
    last_inserted_date: Option<&NaiveDate>,
    pub_document_versions: Vec<FondsTerm<'_>>,
    pub_graph: &FondsGraph,
    publication: &Publication,
) -> anyhow::Result<()> {
    let mut document_elements_bulk: Vec<DocumentElement> = vec![];
    let mut document_changes_bulk: Vec<DocumentChange> = vec![];
    for version in pub_document_versions {
        let codified_date =
            pub_graph.literal_from_triple_matching(Some(version), Some(oll::codifiedDate), None)?;
        if let Some(last_inserted_date) = last_inserted_date.as_ref() {
            let codified_date = NaiveDate::parse_from_str(codified_date.as_str(), "%Y-%m-%d")?;
            if &codified_date < last_inserted_date {
                // Date already inserted
                continue;
            }
        }
        version::TxManager::create(tx, &codified_date).await?;
        let pub_version_hash = md5::compute(format!(
            "{}{}{}",
            publication.name.clone(),
            codified_date,
            publication.fonds
        ));
        publication_version::TxManager::create(
            tx,
            &pub_version_hash,
            &publication.id,
            &codified_date,
        )
        .await?;
        let doc_id =
            pub_graph.literal_from_triple_matching(Some(version), Some(oll::docId), None)?;
        document::TxManager::create(tx, &doc_id).await?;
        let Ok(changes_uri) =
            pub_graph.iri_from_triple_matching(Some(version), Some(oll::hasChanges), None)
        else {
            continue;
        };
        let changes = Bag::new(pub_graph, changes_uri);
        for change in changes.items()? {
            let doc_mpath = pub_graph.literal_from_triple_matching(
                Some(change),
                Some(oll::documentMaterializedPath),
                None,
            )?;
            let url = pub_graph.literal_from_triple_matching(Some(change), Some(oll::url), None)?;
            document_elements_bulk.push(DocumentElement::new(
                doc_mpath.clone(),
                url.clone(),
                doc_id.clone(),
                publication.fonds.clone(),
            ));
            let reason = pub_graph
                .literal_from_triple_matching(Some(change), Some(oll::reason), None)
                .ok();
            let statuses = pub_graph.all_literals_from_triple_matching(
                Some(change),
                Some(oll::status),
                None,
            )?;
            for el_status in statuses {
                let status = Status::from_string(&el_status)?;
                let document_change_hash = md5::compute(format!(
                    "{}{}{}",
                    pub_version_hash.clone(),
                    doc_mpath.clone(),
                    status.to_int()
                ));
                document_changes_bulk.push(DocumentChange::new(
                    document_change_hash,
                    status.to_int(),
                    reason.clone(),
                    pub_version_hash.clone(),
                    doc_mpath.clone(),
                ));
            }
        }
    }
    document_element::TxManager::insert_bulk(tx, document_elements_bulk).await?;
    document_change::TxManager::insert_bulk(tx, document_changes_bulk).await?;
    Ok(())
}

/// Insert library changes into the database
async fn insert_library_changes(
    tx: &mut DatabaseTransaction,
    last_inserted_date: Option<&NaiveDate>,
    pub_collection_versions: Vec<FondsTerm<'_>>,
    pub_graph: &FondsGraph,
    publication: &Publication,
) -> anyhow::Result<()> {
    let mut library_changes_bulk: Vec<LibraryChange> = vec![];
    let mut changed_library_document_bulk: Vec<ChangedLibraryDocument> = vec![];
    let mut library_bulk: Vec<Library> = vec![];
    for version in pub_collection_versions {
        let codified_date =
            pub_graph.literal_from_triple_matching(Some(version), Some(oll::codifiedDate), None)?;
        if let Some(last_inserted_date) = last_inserted_date.as_ref() {
            let codified_date = NaiveDate::parse_from_str(codified_date.as_str(), "%Y-%m-%d")?;
            if &codified_date < last_inserted_date {
                // Date already inserted
                continue;
            }
        }
        let library_mpath = pub_graph.literal_from_triple_matching(
            Some(version),
            Some(oll::libraryMaterializedPath),
            None,
        )?;
        let url = pub_graph.literal_from_triple_matching(Some(version), Some(oll::url), None)?;
        let el_status =
            pub_graph.literal_from_triple_matching(Some(version), Some(oll::status), None)?;
        let library_status = Status::from_string(&el_status)?;
        library_bulk.push(Library::new(
            library_mpath.clone(),
            url.clone(),
            publication.fonds.clone(),
        ));
        let pub_version_hash = md5::compute(format!(
            "{}{}{}",
            publication.name.clone(),
            codified_date,
            publication.fonds
        ));
        library_changes_bulk.push(LibraryChange::new(
            pub_version_hash.clone(),
            library_status.to_int(),
            library_mpath.clone(),
        ));
        let changes_uri =
            pub_graph.iri_from_triple_matching(Some(version), Some(oll::hasChanges), None)?;
        let changes = Bag::new(pub_graph, changes_uri);
        for change in changes.items()? {
            let Ok(found_status) =
                pub_graph.literal_from_triple_matching(Some(change), Some(oll::status), None)
            else {
                continue;
            };
            let Ok(doc_mpath) = pub_graph.literal_from_triple_matching(
                Some(change),
                Some(oll::documentMaterializedPath),
                None,
            ) else {
                continue;
            };
            let status = Status::from_string(&found_status)?;
            let document_change_hash = md5::compute(format!(
                "{}{}{}",
                pub_version_hash.clone(),
                doc_mpath.clone(),
                status.to_int()
            ));
            changed_library_document_bulk.push(ChangedLibraryDocument::new(
                document_change_hash,
                library_mpath.clone(),
            ));
        }
    }
    library::TxManager::insert_bulk(tx, library_bulk).await?;
    library_change::TxManager::insert_bulk(tx, library_changes_bulk).await?;
    changed_library_document::TxManager::insert_bulk(tx, changed_library_document_bulk).await?;
    Ok(())
}

/// Insert shared publication versions for the publication
/// Support for lightweight publications.
/// Populate the many-to-many mapping between change objects and publications
async fn insert_shared_publication_versions_for_publication(
    tx: &mut DatabaseTransaction,
    publication: &Publication,
) -> anyhow::Result<()> {
    let mut publication_has_publication_versions_bulk: Vec<PublicationHasPublicationVersions> =
        vec![];
    let mut publication_version_ids =
        publication_version::TxManager::find_all_recursive_for_publication(
            tx,
            publication.id.clone(),
        )
        .await?;
    if let (Some(last_valid_pub_id), Some(_)) = (
        publication.last_valid_publication_id.as_ref(),
        publication.last_valid_version.as_ref(),
    ) {
        let publication_version_ids_last_valid =
            publication_version::TxManager::find_all_recursive_for_publication(
                tx,
                last_valid_pub_id.clone(),
            )
            .await?;
        publication_version_ids.extend(publication_version_ids_last_valid);
    }
    publication_has_publication_versions_bulk.extend(publication_version_ids.iter().map(|pv| {
        PublicationHasPublicationVersions {
            publication_id: publication.id.clone(),
            publication_version_id: pv.id.clone(),
        }
    }));
    publication_has_publication_versions::TxManager::insert_bulk(
        tx,
        publication_has_publication_versions_bulk,
    )
    .await?;

    Ok(())
}

/// Get the last valid publication name and codified date from the graph
fn referenced_publication_information(pub_graph: &FondsGraph) -> (Option<String>, Option<String>) {
    let last_valid_pub = pub_graph
        .literal_from_triple_matching(None, Some(oll::lastValidPublication), None)
        .ok()
        .and_then(|pub_name: String| pub_name.strip_prefix("Publication ").map(ToOwned::to_owned));
    let last_valid_version = pub_graph
        .literal_from_triple_matching(None, Some(oll::lastValidCodifiedDate), None)
        .ok();
    (last_valid_pub, last_valid_version)
}

/// Revoke publications that have the same date as the current publication
///
/// # Errors
/// Errors if db operations fail
async fn revoke_same_date_publications(
    tx: &mut DatabaseTransaction,
    publication: Publication,
) -> anyhow::Result<()> {
    let duplicate_publications =
        publication::TxManager::find_all_by_date_and_fonds_order_by_name_desc(
            tx,
            publication.date,
            publication.fonds,
        )
        .await?;
    if let Some(duplicate_publications_slice) = duplicate_publications.get(1..) {
        for duplicate_pub in duplicate_publications_slice {
            publication::TxManager::update_by_name_and_fonds_set_revoked_true(
                tx,
                &duplicate_pub.name,
                &duplicate_pub.fonds,
            )
            .await?;
        }
    }
    Ok(())
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Splitting would reduce readability"
)]
/// Check whether the database state for a fonds is consistent with its git repositories.
///
/// Returns `true` if an inconsistency is detected, meaning the fonds should be
/// deleted from the database and fully rebuilt from scratch.
///
/// Three checks are performed:
///
/// Publication names: every non-revoked publication stored in the database must
/// still have a directory in the RDF repository's `_publication/` at HEAD.  A missing
/// one indicates that the RDF repository was rolled back or force-pushed, or that the
/// publication was removed.
///
/// `html_data_repo_name` completeness: when the fonds has a current (non-archived)
/// HTML repository, every non-revoked publication must record one.  Rows predating
/// the column cannot be filled in by an incremental run, so a rebuild is required.
///
/// Authentication commit ancestry: for each historical HTML data
/// repository, the most-recently-recorded `auth_commit_hash` in `data_repo_commits`
/// must be the auth repository's HEAD or one of its ancestors.  If it is not, the auth
/// repository was reset or force-pushed past that commit.
async fn is_fonds_inconsistent(
    tx: &mut DatabaseTransaction,
    fonds_name: &str,
    fonds: &Fonds,
    rdf_repo: &Repo,
    data_repos: &[&Repository],
    current_html_repo_name: Option<&str>,
) -> anyhow::Result<bool> {
    let db_pub_names = publication::TxManager::find_non_revoked_names(tx, fonds_name).await?;
    if db_pub_names.is_empty() {
        // Fresh fonds, nothing to be inconsistent with.
        return Ok(false);
    }

    // Publications recorded before `html_data_repo_name` existed carry NULL, and
    // nothing rewrites them in place: `create` is INSERT OR IGNORE, and the
    // publication walk skips dates already recorded.  The auth-commit check below
    // cannot reach them either, because the lookup it uses matches on
    // `html_data_repo_name` -- so a NULL row disables the very check that would
    // otherwise rebuild it.  A rebuild is the only way to fill them in.
    //
    // Only when the fonds has a current HTML repository to record.  A fonds that
    // serves no HTML legitimately has NULL here, and treating that as inconsistent
    // would rebuild it on every run, each time producing NULL again.
    if current_html_repo_name.is_some() {
        let missing =
            publication::TxManager::count_non_revoked_missing_html_data_repo_name(tx, fonds_name)
                .await?;
        if missing > 0 {
            tracing::warn!(
                "[{fonds_name}] | Inconsistency detected: {missing} non-revoked publication(s) \
                 have no html_data_repo_name, but the fonds has a current HTML repository. \
                 These predate the column and cannot be filled in without a rebuild."
            );
            return Ok(true);
        }
    }

    let head_commit = rdf_repo.repo.head()?.peel_to_commit()?;
    let tree = head_commit.tree()?;
    if let Ok(publications_dir_entry) = tree.get_path(&PathBuf::from("_publication")) {
        let publications_subtree = rdf_repo.repo.find_tree(publications_dir_entry.id())?;
        if let Some(missing) = db_pub_names
            .iter()
            .find(|name| publications_subtree.get_name(name).is_none())
        {
            tracing::warn!(
                "[{fonds_name}] | Inconsistency detected: publication {missing} is in the \
                 database but not in the RDF repository at HEAD. The RDF repository may have \
                 been force-pushed or rolled back."
            );
            return Ok(true);
        }
    }

    let auth_head = fonds.auth_repo.repo.head()?.peel_to_commit()?.id();

    // Most-recent auth_commit_hash for each data repo must exist in the auth repo.
    for data_repo in data_repos {
        let Some(last_auth_commit) = data_repo_commits::TxManager::find_last_auth_commit_for_fonds(
            tx,
            fonds_name,
            &data_repo.name,
        )
        .await?
        else {
            continue;
        };
        let oid = match git2::Oid::from_str(&last_auth_commit) {
            Ok(oid) => oid,
            Err(err) => {
                tracing::warn!(
                    "[{fonds_name}] | Could not parse stored auth commit hash \
                     '{last_auth_commit}' as a git OID: {err:?}"
                );
                return Ok(true);
            }
        };
        if fonds.auth_repo.repo.merge_base(auth_head, oid).ok() != Some(oid) {
            tracing::warn!(
                "[{fonds_name}] | Inconsistency detected: auth commit '{last_auth_commit}' \
                 for data repo '{}' is not reachable from the auth repository's HEAD. \
                 The auth repository may have been reset or force-pushed.",
                data_repo.name
            );
            return Ok(true);
        }
    }

    Ok(false)
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Splitting would reduce readability"
)]
/// Walks the authentication repository commits and processes commit hashes that are inserted into the database.
///
/// # Errors
/// Errors if the commit cannot be processed or inserted into the database.
async fn insert_commit_hashes_from_auth_repository(
    tx: &mut DatabaseTransaction,
    fonds: &Fonds,
    data_repo: &Repository,
) -> anyhow::Result<()> {
    let auth_repo = &fonds.auth_repo;
    let fonds_name = fonds.get_qualified_name();

    let mut data_repo_commits_bulk: Vec<DataRepoCommits> = vec![];

    let loaded_auth_commits =
        data_repo_commits::TxManager::find_all_auth_commits_for_fonds_and_data_repo(
            tx,
            &fonds_name,
            &data_repo.name,
        )
        .await?;

    if loaded_auth_commits.is_empty() {
        tracing::info!("[{fonds_name}] | Inserting commit hashes from the beginning...");
    } else {
        tracing::info!("[{fonds_name}] | Inserting commit hashes...");
    }

    for commit in auth_repo.iter_commits()? {
        // Skip commits that are already in the database
        if is_commit_in_loaded_auth_commits(&commit, &loaded_auth_commits) {
            continue;
        }
        match process_commit(
            &commit,
            fonds,
            data_repo,
            &fonds_name,
            tx,
            &mut data_repo_commits_bulk,
        )
        .await
        {
            Ok(()) => {}
            Err(err) => {
                tracing::error!(
                    "[{fonds_name}] | Error processing commit {}: {err:?}",
                    commit.id().to_string()
                );
            }
        }
    }
    let inserted_len = data_repo_commits_bulk.len();
    data_repo_commits::TxManager::insert_bulk(tx, data_repo_commits_bulk).await?;
    if inserted_len == 0 {
        tracing::info!("[{fonds_name}] | All hashes up to date");
        return Ok(());
    }
    tracing::info!(
        "[{fonds_name}] | Inserted {} commit hashes for: {}",
        inserted_len,
        &data_repo.name
    );
    Ok(())
}

/// Process the auth commit.
///
/// The commit is used to get the metadata target file for the data repository.
/// If the metadata target file is found, the commit is checked for a publication name
/// and a codified date. If both are found, the publication is looked up and the commit
/// hashes are inserted into the database.
///
/// # Errors
/// Errors if the metadata target file cannot be found, the publication cannot be found,
/// or the commit cannot be inserted into the database.
async fn process_commit(
    commit: &git2::Commit<'_>,
    fonds: &Fonds,
    data_repo: &Repository,
    fonds_name: &str,
    tx: &mut DatabaseTransaction,
    data_repo_commits_bulk: &mut Vec<DataRepoCommits>,
) -> anyhow::Result<()> {
    let auth_commit_hash = commit.id().to_string();
    let Some(targets_metadata) = fonds
        .get_targets_metadata_at_commit_and_filename(&auth_commit_hash, &data_repo.get_name())?
    else {
        //Skip commits without metadata target file
        return Ok(());
    };
    let Some(publication_name) = date_from_publication_parts(&targets_metadata.branch) else {
        // Skip commits that aren't on a publication
        tracing::debug!(
            "[{fonds_name}] | Skipping commits without publication branch date {}",
            &targets_metadata.branch
        );
        return Ok(());
    };
    let Ok(publication) = publication::TxManager::find_first_by_name_and_fonds_non_revoked(
        tx,
        &publication_name,
        fonds_name,
    )
    .await
    else {
        tracing::debug!(
            "[{fonds_name}] | Skipping commit {} without publication on date {}",
            &auth_commit_hash,
            publication_name
        );
        return Ok(());
    };
    let auth_commit_timestamp = DateTime::from_timestamp(commit.time().seconds(), 0)
        .unwrap_or_default()
        .to_string();

    data_repo_commits_bulk.push(DataRepoCommits::new(
        targets_metadata.commit,
        targets_metadata.codified_date,
        targets_metadata.build_date,
        data_repo.get_type().unwrap_or_default(),
        auth_commit_hash,
        auth_commit_timestamp,
        publication.id,
    ));
    Ok(())
}

/// Get the date of the `branch` entry from targets metadata.
///
/// E.g.
/// "branch": "publication/dd-MM-YYYY"
/// returns:
///     -> dd-MM-YYYY
///
fn date_from_publication_parts(publication: &str) -> Option<String> {
    publication.split('/').nth(1).map(ToOwned::to_owned)
}
/// Checks whether the passed in commit if it is already in the database
fn is_commit_in_loaded_auth_commits(
    commit: &git2::Commit,
    loaded_auth_commits: &[DataRepoCommits],
) -> bool {
    let commit_hash = commit.id().to_string();
    loaded_auth_commits
        .iter()
        .any(|ac| ac.auth_commit_hash == commit_hash)
}

#[cfg(test)]
#[expect(clippy::inline_modules, reason = "tests")]
mod tests {
    use super::*;

    #[test]
    fn test_valid_publication_date() {
        let input = "publication/12-03-2025";
        let result = date_from_publication_parts(input);
        assert_eq!(result, Some("12-03-2025".to_owned()));
    }

    #[test]
    fn test_missing_date_part() {
        let input = "publication/";
        let result = date_from_publication_parts(input);
        assert_eq!(result, Some(String::new())); // because nth(1) = Some("")
    }

    #[test]
    fn test_no_slash_in_string() {
        let input = "publication";
        let result = date_from_publication_parts(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extra_path_components() {
        let input = "publication/12-03-2025/extra";
        let result = date_from_publication_parts(input);
        assert_eq!(result, Some("12-03-2025".to_owned()));
    }

    #[test]
    fn test_unrelated_string() {
        let input = "foo/bar";
        let result = date_from_publication_parts(input);
        assert_eq!(result, Some("bar".to_owned()));
    }
}
