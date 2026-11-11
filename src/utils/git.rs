//! The git module contains structs for interacting with git repositories
//! in the Fonds Archive.
use crate::utils::paths::clean_path;
use anyhow::Result;
use git2::{BranchType, Commit, Oid, Reference, Repository, Sort};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// This is the first step towards having custom errors
pub const GIT_REQUEST_NOT_FOUND: &str = "Git object doesn't exist";

/// Error message for a revision that is not `HEAD`, a local branch name or a full commit ID.
pub const GIT_INVALID_REVISION: &str = "Invalid Git revision";

/// A revision that content can be read from.
///
/// Only `HEAD`, a local branch name or a full commit ID are accepted, so that
/// requests cannot use revision syntax such as abbreviated IDs, remote-tracking
/// refs, reflog entries, ancestry operators or commit message searches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    /// The commit `HEAD` points to.
    Head,
    /// The tip of the local branch `refs/heads/<name>`.
    Branch(String),
    /// A commit given by its full object ID.
    Commit(Oid),
}

impl Revision {
    /// Parse `commitish` into a `Revision`.
    ///
    /// # Errors
    /// Will error with [`GIT_INVALID_REVISION`] if `commitish` is not `HEAD`,
    /// a full 40 character hex commit ID, or a valid local branch name.
    pub fn parse(commitish: &str) -> Result<Self> {
        if commitish == "HEAD" {
            return Ok(Self::Head);
        }
        if commitish.len() == 40 && commitish.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(Self::Commit(Oid::from_str(commitish)?));
        }
        if !commitish.is_empty() && Reference::is_valid_name(&format!("refs/heads/{commitish}")) {
            return Ok(Self::Branch(commitish.to_owned()));
        }
        anyhow::bail!(GIT_INVALID_REVISION)
    }
}

/// Check that `segment` can be used as a single directory name in the archive.
fn is_valid_path_segment(segment: &str) -> bool {
    !matches!(segment, "" | "." | "..") && !segment.contains(['/', '\\'])
}

/// Represents a git repository within an oll archive. includes helpers for
/// for interacting with the Git Repo.
/// Expects a path to the archive, as well as the repo's organization and name.
pub struct Repo {
    /// Path to the archive
    pub archive_path: String,
    /// Path to the Fonds
    pub path: PathBuf,
    /// Repo organization
    pub org: String,
    /// Repo name
    pub name: String,
    /// git2 repository pointing to the repo in the archive.
    pub repo: Repository,
}

/// Represents a git blob returned from the archive on disk
#[derive(Debug)]
pub struct Blob {
    /// The actual content of the git blob
    pub content: Vec<u8>,
    /// Path to the blob
    pub path: String,
    /// Blob hash
    pub blob_hash: Oid,
}

impl fmt::Debug for Repo {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "Repo for {}/{} in the archive at {}",
            self.org, self.name, self.archive_path
        )
    }
}

#[expect(
    clippy::missing_trait_methods,
    clippy::unwrap_used,
    reason = "Expect to have git repo on disk"
)]
impl Clone for Repo {
    fn clone(&self) -> Self {
        Self {
            archive_path: self.archive_path.clone(),
            org: self.org.clone(),
            name: self.name.clone(),
            path: self.path.clone(),
            repo: Repository::open(self.path.clone()).unwrap(),
        }
    }
}

impl Repo {
    /// Do the work of looking for the requested Git object.
    ///
    ///
    /// # Errors
    /// Will error if the Repo couldn't be found, or if there was a problem with the Git object.
    pub fn find_blob(
        archive_path: &Path,
        namespace: &str,
        name: &str,
        remainder: &str,
        commitish: &str,
    ) -> anyhow::Result<Blob> {
        let repo = Self::new(archive_path, namespace, name)?;
        let blob_path = clean_path(remainder);
        let blob = repo.get_bytes_at_path(commitish, &blob_path)?;
        Ok(blob)
    }
    /// Create a new Repo object with helpers for interacting with a Git Repo.
    /// Expects a path to the archive, as well as the repo's org and name.
    ///
    /// # Errors
    ///
    /// Will return `Err` if `org` or `name` is not a single path segment, if
    /// git repository does not exist at `{org}/{name}` in archive, or if there
    /// is something wrong with the git repository.
    pub fn new(archive_path: &Path, org: &str, name: &str) -> anyhow::Result<Self> {
        if !is_valid_path_segment(org) || !is_valid_path_segment(name) {
            anyhow::bail!(GIT_REQUEST_NOT_FOUND);
        }
        let archive_path_str = archive_path.to_string_lossy();
        tracing::trace!(org, name, "Creating new Repo at {archive_path_str}");
        let repo_path = format!("{archive_path_str}/{org}/{name}");
        Ok(Self {
            archive_path: archive_path_str.into(),
            org: org.into(),
            name: name.into(),
            path: PathBuf::from(repo_path.clone()),
            repo: Repository::open(repo_path)?,
        })
    }

    /// Create a new Repo object from a full path to the repo in the archive.
    ///
    /// # Errors
    /// Will return `Err` if the path does not contain at least org and name,
    /// or if git repository does not exist at `{org}/{name}` in archive, or
    /// if there is something wrong with the git repository.
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        let components: Vec<&str> = path
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .collect();
        if components.len() < 2 {
            anyhow::bail!("Path must contain at least org and name");
        }
        let name = (*components
            .last()
            .ok_or_else(|| anyhow::anyhow!("Missing repo name"))?)
        .to_owned();
        let org = (*components
            .get(components.len() - 2)
            .ok_or_else(|| anyhow::anyhow!("Missing repo org"))?)
        .to_owned();
        let archive_path_slice = components.get(..components.len() - 2).ok_or_else(|| {
            anyhow::anyhow!("Path does not contain enough components for archive_path")
        })?;
        let archive_path = archive_path_slice.iter().collect::<PathBuf>();
        Self::new(&archive_path, &org, &name)
    }

    /// Resolve `revision` to a commit in the repository.
    ///
    /// # Errors
    /// Will error with [`GIT_REQUEST_NOT_FOUND`] if the revision does not
    /// resolve to a commit.
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "Matching on a reference to the enum is clearer than dereferencing it"
    )]
    pub fn resolve_commit(&self, revision: &Revision) -> Result<Commit<'_>> {
        let commit = match revision {
            Revision::Head => self.repo.head().and_then(|head| head.peel_to_commit()),
            Revision::Branch(name) => self
                .repo
                .find_reference(&format!("refs/heads/{name}"))
                .and_then(|reference| reference.peel_to_commit()),
            Revision::Commit(oid) => self.repo.find_commit(*oid),
        };
        commit.map_err(|err| {
            tracing::debug!(?revision, "Couldn't resolve revision: {err}");
            anyhow::anyhow!(GIT_REQUEST_NOT_FOUND)
        })
    }

    /// Check whether the commit `oid` is the tip of a local branch or one of
    /// its ancestors.
    ///
    /// # Errors
    /// Will error if the local branches cannot be listed or the commit graph
    /// cannot be read.
    pub fn is_on_local_branch(&self, oid: Oid) -> Result<bool> {
        for branch in self.repo.branches(Some(BranchType::Local))? {
            let (local_branch, _) = branch?;
            let Some(tip) = local_branch.get().target() else {
                continue;
            };
            if tip == oid || self.repo.graph_descendant_of(tip, oid)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Returns bytes of blob found in the commit `commitish` at path `path`
    /// if a blob is not found at path, it will try adding ".html", "index.html,
    /// and "/index.html".
    ///
    /// `commitish` must be `HEAD`, a local branch name or a full commit ID
    /// (see [`Revision`]).
    ///
    /// Example usage:
    ///
    //// let content: Vec<u8> = repo.get_bytes_at_path(
    ////    "0f2f1ef9fa213dcf83e269bc832ab63435cbd4b1",
    ////    "us/ca/cities/san-mateo"
    //// );
    ///
    /// # Errors
    ///
    /// Will return `Err` if `commitish` is not a valid revision or does not
    /// exist in repo, if a blob does not exist in commit at `path`, or if there
    /// is a problem with reading repo.
    pub fn get_bytes_at_path(&self, commitish: &str, path: &str) -> Result<Blob> {
        let revision = Revision::parse(commitish)?;
        let commit = self.resolve_commit(&revision)?;
        self.get_bytes_at_commit(&commit, path)
    }

    /// Returns bytes of blob found in `commit` at path `path`, trying the same
    /// suffixes as [`Repo::get_bytes_at_path`].
    ///
    /// # Errors
    ///
    /// Will return `Err` if a blob does not exist in `commit` at `path`, or if
    /// the commit's tree cannot be read.
    pub fn get_bytes_at_commit(&self, commit: &Commit<'_>, path: &str) -> Result<Blob> {
        let tree = commit.tree()?;
        for postfix in ["", "/index.html", ".html", "index.html"] {
            let filepath = format!("{path}{postfix}");
            let Ok(entry) = tree.get_path(Path::new(filepath.trim_start_matches('/'))) else {
                continue;
            };
            if let Ok(blob) = self.repo.find_blob(entry.id()) {
                tracing::trace!(filepath, "Found Git object");
                return Ok(Blob {
                    content: blob.content().to_owned(),
                    path: filepath,
                    blob_hash: blob.id(),
                });
            }
        }
        tracing::debug!(commit = %commit.id(), path, "Couldn't find requested Git object");
        anyhow::bail!(GIT_REQUEST_NOT_FOUND)
    }

    /// Instantiates a git revwalk from the beginning of the repository.
    /// Return an iterator over the commits.
    ///
    /// # Errors
    /// Will error if the revwalk could not be instantiated
    pub fn iter_commits(&self) -> anyhow::Result<impl Iterator<Item = Commit<'_>>> {
        let mut revwalk = self.repo.revwalk()?;
        revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
        revwalk.push_head()?;
        Ok(revwalk
            .filter_map(|found_oid| {
                let oid = found_oid.ok()?;
                self.repo.find_commit(oid).ok()
            })
            .collect::<Vec<Commit>>()
            .into_iter())
    }
}
