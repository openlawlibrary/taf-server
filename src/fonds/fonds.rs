//! The Fonds module contains the Fonds object for interacting with
//! Fonds.

use std::path::{Path, PathBuf};

use super::types::{repositories::Repository, targets_metadata::TargetsMetadata};
use crate::{
    fonds::types::{dependencies::Dependencies, repositories::Repositories},
    utils::git::Repo,
};
use anyhow::Context as _;
use git2::Repository as GitRepository;
use serde_derive::{Deserialize, Serialize};
use serde_json;

/// Fonds
#[derive(Debug, Clone)]
pub struct Fonds {
    /// Path to the containing Fonds archive.
    pub archive_path: PathBuf,
    /// Fonds's repositories (as specified in repositories.json).
    pub repositories: Option<Repositories>,
    /// Indicates whether or not the Fonds is the root Fonds.
    pub root: bool,
    /// Fonds's authentication repo.
    pub auth_repo: Repo,
}

impl Fonds {
    /// Create a new Fonds object
    /// # Errors
    /// Will error if unable to find or parse repositories file at `targets/repositories.json`
    /// # Panics
    /// Will panic if unable to determine the current root Fonds.
    #[expect(clippy::shadow_reuse, reason = "Use same param as field name")]
    pub fn new(
        archive_path: &Path,
        name: Option<String>,
        org: Option<String>,
        path: Option<PathBuf>,
        root: bool,
    ) -> anyhow::Result<Self> {
        let name = name.unwrap_or_else(|| "law".into());
        let org = if let Some(org) = org {
            org
        } else {
            path.as_ref()
                .context("path is None")?
                .file_name()
                .context("file_name is None")?
                .to_str()
                .context("to_str failed")?
                .into()
        };
        let path = path.unwrap_or_else(|| archive_path.join(&org));
        let mut fonds = Self {
            archive_path: archive_path.to_path_buf(),
            repositories: None,
            root,
            auth_repo: Repo {
                archive_path: archive_path.to_string_lossy().to_string(),
                path: path.join(&name),
                org,
                name: name.clone(),
                repo: GitRepository::open(path.join(&name))?,
            },
        };
        fonds.get_repositories()?;
        Ok(fonds)
    }

    /// Get Fonds's dependencies.
    /// # Errors
    /// Will error if unable to parse dependencies file from `targets/dependencies.json`
    pub fn get_dependencies(&self) -> anyhow::Result<Option<Dependencies>> {
        let Ok(blob) = self
            .auth_repo
            .get_bytes_at_path("HEAD", "targets/dependencies.json")
        else {
            return Ok(None);
        };
        let dependencies_str = String::from_utf8(blob.content)?;
        let dependencies = serde_json::from_str(&dependencies_str)?;
        Ok(Some(dependencies))
    }

    /// Get Fonds's repositories.
    /// # Errors
    /// Will error if unable to find, parse or validate the repositories file at
    /// `targets/repositories.json` (see [`Repositories::validate`])
    pub fn get_repositories(&mut self) -> anyhow::Result<Option<Repositories>> {
        let Ok(blob) = self
            .auth_repo
            .get_bytes_at_path("HEAD", "targets/repositories.json")
        else {
            return Ok(None);
        };
        let repositories_str = String::from_utf8(blob.content)?;
        let repositories: Repositories = serde_json::from_str(&repositories_str)?;
        repositories.validate()?;
        self.repositories = Some(repositories.clone());
        Ok(Some(repositories))
    }

    /// Check if Fonds's private access file exists.
    /// Returns true if `targets/private.json` exists, false otherwise.
    #[must_use]
    pub fn is_private_fonds(&self) -> bool {
        self.auth_repo
            .get_bytes_at_path("HEAD", "targets/private.json")
            .is_ok()
    }

    /// Get Fonds's repositories for specific commitish.
    /// # Errors
    /// Will error if unable to find or parse repositories file at `targets/repositories.json`
    pub fn get_repositories_for_commitish(
        &self,
        committish: &str,
    ) -> anyhow::Result<Option<Repositories>> {
        let Ok(blob) = self
            .auth_repo
            .get_bytes_at_path(committish, "targets/repositories.json")
        else {
            return Ok(None);
        };
        let repositories_str = String::from_utf8(blob.content)?;
        let repositories: Repositories = serde_json::from_str(&repositories_str)?;
        Ok(Some(repositories))
    }

    /// Get Fonds's targets metadata file at a specific committish and filename.
    ///
    /// # Arguments
    /// * `committish` - The committish to look for the targets metadata file.
    /// * `path` - The path to the targets metadata file.
    ///
    /// # Returns
    /// Returns the targets metadata file if found, or None if not found.
    /// # Errors
    /// Will error if unable to find or parse targets metadata file at `targets/{org}/{filename}`
    pub fn get_targets_metadata_at_commit_and_filename(
        &self,
        committish: &str,
        filename: &str,
    ) -> anyhow::Result<Option<TargetsMetadata>> {
        let org = &self.auth_repo.org;
        let file_path = format!("targets/{org}/{filename}");
        let Ok(blob) = self.auth_repo.get_bytes_at_path(committish, &file_path) else {
            return Ok(None);
        };
        let targets_metadata_str = String::from_utf8(blob.content)?;
        let targets_metadata: TargetsMetadata = serde_json::from_str(&targets_metadata_str)?;
        Ok(Some(targets_metadata))
    }

    /// Get Fonds's qualified name.
    #[must_use]
    pub fn get_qualified_name(&self) -> String {
        format!(
            "{org}/{name}",
            org = self.auth_repo.org,
            name = self.auth_repo.name
        )
    }

    /// Get Fonds's fallback repo.
    /// A fallback repository is a data repository which contains `is_fallback` = true in its custom field.
    /// # Returns
    /// Returns the first fallback repository found, or None if no fallback repository is found.
    #[must_use]
    pub fn get_fallback_repo(&self) -> Option<&Repository> {
        let repositories = self.repositories.as_ref()?;
        repositories
            .repositories
            .values()
            .find(|repository| repository.custom.is_fallback.unwrap_or(false))
    }

    /// See if Fonds is a root Fonds.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.root
    }
}

/// Config object for a Fonds
#[derive(Deserialize, Serialize)]
pub struct Config {
    /// Name of the authentication repo (e.g. law).
    pub name: String,
    /// Name of the Fonds's directory, also known as Fonds's organization (e.g. openlawlibrary).
    pub org: String,
    /// The out-of-band authenticated hash of the Fonds.
    pub hash: Option<String>,
}
