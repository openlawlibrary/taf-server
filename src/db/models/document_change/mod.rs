use super::version::Version;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod manager;

/// Trait for managing document changes.
#[async_trait]
pub trait Manager {
    /// All dates on which given document changed.
    async fn find_all_document_versions_by_mpath_and_publication(
        &self,
        mpath: &str,
        publication: &str,
    ) -> anyhow::Result<Vec<Version>>;
    /// Every element that changed in one version of one publication, under a
    /// materialized-path prefix.
    ///
    /// The manager appends the `LIKE` wildcard. Callers must still drop rows
    /// that do not `starts_with(base)`: `LIKE` is ASCII-case-insensitive and
    /// treats `%`/`_` inside the base as wildcards.
    async fn find_changes_in_version_by_prefix(
        &self,
        mpath_prefix: &str,
        publication_id: &str,
        version: &str,
    ) -> anyhow::Result<Vec<VersionChange>>;
}

/// Trait for managing transactional document changes.
#[async_trait]
pub trait TxManager {
    /// Insert a bulk of document changes.
    async fn insert_bulk(&mut self, document_changes: Vec<DocumentChange>) -> anyhow::Result<()>;
}

#[derive(sqlx::FromRow, Deserialize, Serialize)]
/// Model for document change events.
pub struct DocumentChange {
    /// A hashed identifier for the document change.
    /// The hash is generated from the `publication_version` id, `doc_mpath`, and `status` (as integer) field.
    pub id: String,
    /// Change status of the document.
    /// Currently could be 'Element added' = 0, 'Element effective' = 1, 'Element changed' = 2 or 'Element removed' = 3.
    pub status: i64,
    /// Optional reason for the change event.
    pub change_reason: Option<String>,
    /// Foreign key reference to the `publication_version` id.
    pub publication_version_id: String,
    /// Materialized path to the document
    pub doc_mpath: String,
}

#[derive(sqlx::FromRow, Deserialize, Serialize, Debug, Clone)]
/// Read projection: one change row inside one selected publication version.
pub struct VersionChange {
    /// Materialized path of the element that changed.
    pub doc_mpath: String,
    /// Change status as the integer stored in `document_change.status`.
    pub status: i64,
}

impl VersionChange {
    /// Create a new version change row. Used by tests and by pure synthesis.
    #[must_use]
    pub const fn new(doc_mpath: String, status: i64) -> Self {
        Self { doc_mpath, status }
    }
}

impl DocumentChange {
    /// Create a new document change.
    #[must_use]
    pub const fn new(
        id: String,
        status: i64,
        change_reason: Option<String>,
        publication_version_id: String,
        doc_mpath: String,
    ) -> Self {
        Self {
            id,
            status,
            change_reason,
            publication_version_id,
            doc_mpath,
        }
    }
}
