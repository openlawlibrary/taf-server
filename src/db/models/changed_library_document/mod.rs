use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod manager;

/// Trait for reading changed library documents outside a transaction.
#[async_trait]
pub trait Manager {
    /// Every document that changed in one of the given `publication_version`
    /// ids, under a library materialized-path prefix.
    ///
    /// The manager appends the `LIKE` wildcard. The backing index is `COLLATE
    /// NOCASE`, so callers must still drop rows that do not `starts_with`.
    ///
    /// Version ids are resolved in a separate round trip: joining `phpv` and
    /// `pv` here would leave the date a residual filter over the whole prefix.
    async fn find_changes_by_library_prefix_and_version_ids(
        &self,
        library_mpath_prefix: &str,
        publication_version_ids: &[String],
    ) -> anyhow::Result<Vec<LibraryDocumentChange>>;
    /// Every document ever recorded under a library materialized-path prefix.
    /// The undated collection query: this table is the only namespace bridge.
    async fn find_documents_by_library_prefix(
        &self,
        library_mpath_prefix: &str,
    ) -> anyhow::Result<Vec<LibraryDocument>>;
}

/// Trait for managing transactional changed library documents.
#[async_trait]
pub trait TxManager {
    /// Insert bulk of changed library documents.
    async fn insert_bulk(
        &mut self,
        changed_library_document: Vec<ChangedLibraryDocument>,
    ) -> anyhow::Result<()>;
}

#[derive(sqlx::FromRow, Deserialize, Serialize)]
/// Model for library (collection) change events.
pub struct ChangedLibraryDocument {
    /// Foreign key reference to `document_change` id.
    pub document_change_id: String,
    /// Materialized path to the library
    pub library_mpath: String,
}

#[derive(sqlx::FromRow, Deserialize, Serialize, Debug, Clone)]
/// Read projection: one document change, attached to the library it sits in.
pub struct LibraryDocumentChange {
    /// Materialized path of the library the changed document belongs to.
    pub library_mpath: String,
    /// Materialized path of the element that changed.
    pub doc_mpath: String,
    /// Change status as the integer stored in `document_change.status`.
    pub status: i64,
}

#[derive(sqlx::FromRow, Deserialize, Serialize, Debug, Clone)]
/// Read projection: one document attached to the library it sits in, with no
/// version selected and therefore no status.
pub struct LibraryDocument {
    /// Materialized path of the library the document belongs to.
    pub library_mpath: String,
    /// Materialized path of the element.
    pub doc_mpath: String,
}

impl LibraryDocument {
    /// Create a new undated library document row.
    #[must_use]
    pub const fn new(library_mpath: String, doc_mpath: String) -> Self {
        Self {
            library_mpath,
            doc_mpath,
        }
    }
}

impl LibraryDocumentChange {
    /// Create a new library document change row.
    #[must_use]
    pub const fn new(library_mpath: String, doc_mpath: String, status: i64) -> Self {
        Self {
            library_mpath,
            doc_mpath,
            status,
        }
    }
}

impl ChangedLibraryDocument {
    /// Create a new library change.
    #[must_use]
    pub const fn new(document_change_id: String, library_mpath: String) -> Self {
        Self {
            document_change_id,
            library_mpath,
        }
    }
}
