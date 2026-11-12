use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod manager;

/// Trait for managing collection changes.
#[async_trait]
pub trait Manager {
    /// Find one library materialized path by url.
    async fn find_lib_mpath_by_url(&self, url: &str, fonds: &str) -> anyhow::Result<String>;
    /// Find urls for an explicit set of library materialized paths.
    async fn find_labels_by_mpaths(
        &self,
        mpaths: &[String],
        fonds: &str,
    ) -> anyhow::Result<Vec<LibraryLabel>>;
}

/// Trait for managing transactions on publication versions.
#[async_trait]
pub trait TxManager {
    /// Insert bulk libraries.
    async fn insert_bulk(&mut self, libraries: Vec<Library>) -> anyhow::Result<()>;
}

#[derive(sqlx::FromRow, Deserialize, Serialize)]
/// Model for library (collection).
pub struct Library {
    /// Materialized path to the collection
    pub mpath: String,
    /// Url to the collection.
    pub url: String,
    /// Reference to the fonds.
    pub fonds: String,
}

#[derive(sqlx::FromRow, Deserialize, Serialize, Debug, Clone)]
/// Read projection carrying everything the menu needs to label one library node.
#[expect(
    clippy::module_name_repetitions,
    reason = "`LibraryLabel` names the row it projects; `Label` alone would be ambiguous across models"
)]
pub struct LibraryLabel {
    /// Materialized path of the library.
    pub mpath: String,
    /// Url of the library. Nullable in the schema, so nullable here.
    pub url: Option<String>,
}

impl Library {
    /// Create a new library.
    #[must_use]
    pub const fn new(mpath: String, url: String, fonds: String) -> Self {
        Self { mpath, url, fonds }
    }
}
