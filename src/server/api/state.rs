//! Centralized state management for the Actix web server
use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug},
    path::PathBuf,
    sync::{Arc, PoisonError, RwLock},
};

use crate::{
    db,
    fonds::{archive::Archive, fonds::Fonds, types::repositories::Repository},
    server::api::utils::convert_vec_u8_to_hashmap,
    utils::archive::get_name_parts,
};

use crate::utils::git::Repo;

/// The filename for repository-level redirects.
///
/// This JSON file contains mappings of "old" paths to "new" paths
/// within the repository. It is used by the server to handle
/// HTTP redirects so that old URLs still resolve to their
/// current locations.
///
/// Example `redirects.json` contents:
/// ```json
/// [
///     ["/old-path", "/new-path"],
///     ["/outdated", "/current"]
/// ]
/// ```
pub const REDIRECTS_JSON: &str = "redirects.json";

/// Global, read-only state
pub trait Global: Debug {
    /// Fully initialized Fonds archive
    fn archive(&self) -> &Archive;
    /// Database connection
    fn db(&self) -> &db::DatabaseConnection;
    /// Whether the given repository has any redirects configured.
    fn has_redirects(&self, fonds: &str, repo_name: &str) -> bool;
    /// Replaces the cached set of `(fonds_name, repo_name)` pairs that have at
    /// least one redirect configured.
    ///
    /// Used to pick up redirects inserted into the database after start-up
    /// (e.g. by `taf-server update`) without requiring a server restart.
    fn set_repos_with_redirects(&self, repos: HashSet<(String, String)>);
}

/// Application state
#[derive(Debug, Clone)]
pub struct App {
    /// Fully initialized Fonds archive
    pub archive: Archive,
    /// Database connection
    pub db: db::DatabaseConnection,
    /// `(fonds_name, repo_name)` pairs that have at least one redirect.
    ///
    /// Computed once at startup, and refreshed on demand (via
    /// [`Global::set_repos_with_redirects`]). Wrapped in `Arc<RwLock<_>>` so
    /// every clone of `App` - one per Actix worker/scope - shares and
    /// observes the same underlying cache.
    pub repos_with_redirects: Arc<RwLock<HashSet<(String, String)>>>,
}

impl Global for App {
    fn archive(&self) -> &Archive {
        &self.archive
    }

    fn db(&self) -> &db::DatabaseConnection {
        &self.db
    }

    fn has_redirects(&self, fonds: &str, repo_name: &str) -> bool {
        self.repos_with_redirects
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&(fonds.to_owned(), repo_name.to_owned()))
    }

    fn set_repos_with_redirects(&self, repos: HashSet<(String, String)>) {
        *self
            .repos_with_redirects
            .write()
            .unwrap_or_else(PoisonError::into_inner) = repos;
    }
}

/// Repository to serve
pub struct RepoData {
    /// Path to the archive
    pub archive_path: PathBuf,
    /// Path to the Fonds
    pub path: PathBuf,
    /// Repo organization
    pub org: String,
    /// Repo name
    pub name: String,
    // /// path to the git repository
    // pub repo_path: PathBuf;
    ///Latest or historical
    pub serve: String,
}

impl RepoData {
    /// Create a new Repo state object
    #[must_use]
    pub fn new(archive_path: &str, org: &str, name: &str, serve: &str) -> Self {
        let mut repo_path = archive_path.to_owned();
        repo_path = format!("{repo_path}/{org}/{name}");
        Self {
            archive_path: PathBuf::from(archive_path),
            path: PathBuf::from(&repo_path),
            org: org.to_owned(),
            name: name.to_owned(),
            serve: serve.to_owned(),
        }
    }

    /// Reads redirects list from redirects.json file
    #[must_use]
    pub fn get_redirects(&self) -> HashMap<String, String> {
        let archive_path_pth = &self.archive_path;
        match Repo::find_blob(
            archive_path_pth,
            &self.org,
            &self.name,
            REDIRECTS_JSON,
            "HEAD",
        ) {
            Ok(blob) => convert_vec_u8_to_hashmap(&blob.content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }
}

/// Shared, read-only app state
pub struct Shared {
    /// Repository to fall back to if the current one is not found
    pub fallback: Option<RepoData>,
}

impl fmt::Debug for RepoData {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "Repo for {} in the archive at {}",
            self.name,
            self.path.display()
        )
    }
}

impl fmt::Debug for Shared {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let fb = &self.fallback;
        match fb.as_ref() {
            Some(fallback) => write!(
                formatter,
                "Repo for {} in the archive at {}",
                fallback.name,
                fallback.path.display()
            ),
            None => write!(formatter, "No fallback repo"),
        }
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Use implicit trait implementation"
)]
impl Clone for RepoData {
    fn clone(&self) -> Self {
        Self {
            archive_path: self.archive_path.clone(),
            path: self.path.clone(),
            org: self.org.clone(),
            name: self.name.clone(),
            serve: self.serve.clone(),
        }
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Use implicit trait implementation"
)]
impl Clone for Shared {
    fn clone(&self) -> Self {
        Self {
            fallback: self.fallback.clone(),
        }
    }
}

/// Initialize the data repository used in the Actix route
/// Each Actix route has its own data repository
///
/// # Errors
/// Will error if unable to initialize the data repository
pub fn init_repo(repo: &Repository, fonds: &Fonds) -> anyhow::Result<RepoData> {
    let custom = &repo.custom;
    let (org, name) = get_name_parts(&repo.name)?;
    Ok(RepoData::new(
        &fonds.archive_path.to_string_lossy(),
        &org,
        &name,
        &custom.serve,
    ))
}

/// Initialize the shared application state
/// Currently shared application state consists of:
///     - fallback: used as a data repository to resolve data when no other url matches the request
/// # Returns
/// Returns a `SharedState` object
/// # Errors
/// Will error if unable to open the git repo for the fallback data repository
pub fn init_shared(fonds: &Fonds) -> anyhow::Result<Shared> {
    let fallback = fonds
        .get_fallback_repo()
        .map(|repo| {
            let (org, name) = get_name_parts(&repo.name)?;
            Ok::<RepoData, anyhow::Error>(RepoData::new(
                &fonds.archive_path.to_string_lossy(),
                &org,
                &name,
                &repo.custom.serve,
            ))
        })
        .transpose()?;
    Ok(Shared { fallback })
}
