//! The redirects module contains tools for interacting with the redirects of the Fonds.
// The redirects module contains logic for inserting redirect objects into the database.
use crate::db::models::redirects;
use crate::db::models::redirects::RedirectPair;
use crate::db::DatabaseTransaction;
use crate::fonds::fonds::Fonds;
use crate::server::api::state;

/// Inserts multiple redirect mappings for a repository within an existing transaction.
///
/// This function delegates to the transactional redirect manager and ensures
/// that all redirect pairs are inserted atomically as part of the provided
/// database transaction.
///
/// # Arguments
///
/// * `tx` - The active database transaction.
/// * `fonds` - The fonds (tenant / namespace) identifier.
/// * `repo_name` - The repository name.
/// * `pairs` - A collection of redirect mappings to insert.
///
/// # Errors
///
/// Returns an error if the bulk insert fails
pub async fn insert_redirect_pairs(
    tx: &mut DatabaseTransaction,
    fonds: &str,
    repo_name: &str,
    pairs: Vec<RedirectPair>,
) -> anyhow::Result<()> {
    redirects::TxManager::insert_bulk(tx, fonds, repo_name, pairs).await?;
    Ok(())
}

/// Replaces the redirects of a fonds with those of its repositories, within an existing
/// transaction.
///
/// # Arguments
///
/// * `tx` - The active database transaction.
/// * `fonds` - The fonds to process redirects for.
///
/// # Errors
///
/// Returns an error if repositories cannot be read or if any bulk insert fails
pub async fn insert_redirects_for_fonds(
    tx: &mut DatabaseTransaction,
    fonds: &mut Fonds,
) -> anyhow::Result<()> {
    redirects::TxManager::delete_for_fonds(tx, &fonds.get_qualified_name()).await?;
    let Some(repositories) = fonds.get_repositories()? else {
        return Ok(());
    };
    let all_repositories = repositories.get_all();
    for repository in all_repositories {
        let repo_state = state::init_repo(repository, fonds)?;
        let pairs: Vec<RedirectPair> = repo_state
            .get_redirects()
            .into_iter()
            .map(|(from, to)| RedirectPair {
                from_url: from,
                to_url: to,
            })
            .collect();
        insert_redirect_pairs(tx, &fonds.get_qualified_name(), &repo_state.name, pairs).await?;
    }
    Ok(())
}
