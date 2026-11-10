use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod manager;

/// Trait for managing document elements.
#[async_trait]
pub trait Manager {
    /// Find one document materialized path by url.
    async fn find_doc_mpath_by_url(&self, url: &str, fonds: &str) -> anyhow::Result<String>;
    /// Find url and `doc_id` for an explicit set of materialized paths.
    async fn find_labels_by_mpaths(
        &self,
        doc_mpaths: &[String],
        fonds: &str,
    ) -> anyhow::Result<Vec<ElementLabel>>;
    /// Find every document element in the `[base, upper)` materialized-path
    /// range whose depth is at most `max_depth`. The undated node query.
    async fn find_elements_in_mpath_range(
        &self,
        base: &str,
        upper: &str,
        fonds: &str,
        max_depth: i64,
    ) -> anyhow::Result<Vec<ElementLabel>>;
    /// Which of the given depth-1 document root materialized paths have at
    /// least one strictly deeper element row.
    ///
    /// Grouped on the first `doc_mpath` segment, not `doc_id`: the two disagree
    /// on 2 of the dev archive's 47,950 rows.
    async fn find_roots_with_children(
        &self,
        doc_roots: &[String],
        fonds: &str,
    ) -> anyhow::Result<Vec<String>>;
}

/// Trait for managing transactional document elements.
#[async_trait]
pub trait TxManager {
    /// Insert a bulk of document elements.
    async fn insert_bulk(&mut self, document_elements: Vec<DocumentElement>) -> anyhow::Result<()>;
}

#[derive(sqlx::FromRow, Deserialize, Serialize)]
/// Model for document elements.
pub struct DocumentElement {
    /// Materialized path to the document
    pub doc_mpath: String,
    /// Url to the document that was changed.
    pub url: String,
    /// Unique document identifier.
    pub doc_id: String,
    /// Reference to the fonds.
    pub fonds: String,
}

#[derive(sqlx::FromRow, Deserialize, Serialize, Debug, Clone)]
/// Read projection carrying everything the menu needs to label one node.
pub struct ElementLabel {
    /// Materialized path of the element.
    pub doc_mpath: String,
    /// Url of the element. Nullable in the schema, so nullable here.
    pub url: Option<String>,
    /// Unique document identifier. Nullable in the schema, so nullable here.
    pub doc_id: Option<String>,
}

impl DocumentElement {
    /// Create a new document element.
    #[must_use]
    pub const fn new(doc_mpath: String, url: String, doc_id: String, fonds: String) -> Self {
        Self {
            doc_mpath,
            url,
            doc_id,
            fonds,
        }
    }
}
