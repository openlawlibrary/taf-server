use async_trait::async_trait;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{any::AnyRow, FromRow, Row as _};

pub mod manager;

/// Trait for managing publications.
#[async_trait]
pub trait Manager {
    /// Find all publications which are not revoked for a given fonds.
    async fn find_all_non_revoked_publications(
        &self,
        fonds: &str,
    ) -> anyhow::Result<Vec<Publication>>;
    /// Find the last non-revoked publication for a given fonds.
    async fn find_last_inserted(&self, fonds: &str) -> anyhow::Result<Option<Publication>>;
    /// Find a non-revoked publication by name and fonds.
    async fn find_by_name_and_fonds(
        &self,
        name: &str,
        fonds: &str,
    ) -> anyhow::Result<Option<Publication>>;
}

/// Trait for managing transactions on publications.
#[async_trait]
pub trait TxManager {
    /// Create a new publication.
    #[expect(
        clippy::too_many_arguments,
        reason = "Publication has many required fields; a dedicated builder struct can be added later."
    )]
    async fn create(
        &mut self,
        hash_id: &str,
        name: &str,
        date: &NaiveDate,
        fonds: &str,
        last_valid_publication_id: Option<String>,
        last_valid_version: Option<String>,
        html_data_repo_name: Option<String>,
    ) -> anyhow::Result<Option<i64>>;
    /// Update a publication by name and set revoked to true.
    async fn update_by_name_and_fonds_set_revoked_true(
        &mut self,
        name: &str,
        fonds: &str,
    ) -> anyhow::Result<()>;
    /// Find the last inserted publication for a given fonds.
    async fn find_last_inserted(&mut self, fonds: &str) -> anyhow::Result<Option<Publication>>;
    /// Find a publication by name and fonds.
    async fn find_by_name_and_fonds(
        &mut self,
        name: &str,
        fonds: &str,
    ) -> anyhow::Result<Option<Publication>>;
    /// Find the first publication by name and fonds that is not revoked.
    async fn find_first_by_name_and_fonds_non_revoked(
        &mut self,
        name: &str,
        fonds: &str,
    ) -> anyhow::Result<Publication>;
    /// Set `html_data_repo_name` on all publications for the given fonds whose date is
    /// strictly earlier than `boundary_date`.  Used to backfill older publications when
    /// the boundary (last-archived) publication is encountered.
    async fn set_html_data_repo_name_for_prior_publications(
        &mut self,
        fonds: &str,
        boundary_date: &NaiveDate,
        html_data_repo_name: &str,
    ) -> anyhow::Result<()>;
    /// Find all by date and fonds and sort by name in descending order.
    /// Used in revocation logic to find the latest publication.
    async fn find_all_by_date_and_fonds_order_by_name_desc(
        &mut self,
        date: String,
        fonds: String,
    ) -> anyhow::Result<Vec<Publication>>;
    /// Find the names of the non-revoked publications for a given fonds.
    async fn find_non_revoked_names(&mut self, fonds: &str) -> anyhow::Result<Vec<String>>;
    /// Count the non-revoked publications for a given fonds that carry no
    /// `html_data_repo_name`.
    async fn count_non_revoked_missing_html_data_repo_name(
        &mut self,
        fonds: &str,
    ) -> anyhow::Result<usize>;
}

#[derive(Deserialize, Serialize, Debug)]
/// Model for a Fonds.
pub struct Publication {
    /// A hashed identifier for the publication.
    /// The hash is generated from the `name` and `fonds` fields of the publication.
    pub id: String,
    /// Name of the publication in %YYYY-%MM-%DD format
    /// with optionally incrementing version numbers
    /// when two publications exist on same date.
    pub name: String,
    /// Date of the publication.
    pub date: String,
    /// Foreign key reference to fonds by name.
    pub fonds: String,
    /// Whether the publication has been revoked.
    /// A publication is revoked if another publication exists
    /// on the same date with a higher version number.
    pub revoked: i64,
    /// If a publication is derived from another publication,
    /// represents the last publication name that was valid before this publication.
    pub last_valid_publication_id: Option<String>,
    /// If a publication is derived from another publication,
    /// represents the last publication version (codified date) from the previous publication
    /// that the current publication is derived from.
    pub last_valid_version: Option<String>,
    /// Name of the HTML data repository used to build this publication.
    /// Only set when the publication was built against an archived (non-current) HTML data repo.
    /// If None, the publication uses the current/default HTML data repository.
    pub html_data_repo_name: Option<String>,
}

impl FromRow<'_, AnyRow> for Publication {
    fn from_row(row: &AnyRow) -> anyhow::Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            date: row.try_get("date")?,
            fonds: row.try_get("fonds")?,
            revoked: row.try_get("revoked")?,
            last_valid_publication_id: row.try_get("last_valid_publication_id").ok(),
            last_valid_version: row.try_get("last_valid_version").ok(),
            html_data_repo_name: row.try_get("html_data_repo_name").ok(),
        })
    }
}

impl Publication {
    /// Create a new publication.
    #[must_use]
    pub const fn new(id: String, name: String, date: String, fonds: String) -> Self {
        Self {
            id,
            name,
            date,
            fonds,
            revoked: 0,
            last_valid_publication_id: None,
            last_valid_version: None,
            html_data_repo_name: None,
        }
    }
}
