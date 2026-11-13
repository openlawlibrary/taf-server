//! Manager for the publication model.
use crate::db::{DatabaseConnection, DatabaseKind, DatabaseTransaction};
use async_trait::async_trait;
use chrono::NaiveDate;

use super::Publication;

#[async_trait]
impl super::Manager for DatabaseConnection {
    /// Find all publications which are not revoked for a given fonds.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_all_non_revoked_publications(
        &self,
        fonds: &str,
    ) -> anyhow::Result<Vec<Publication>> {
        let statement = "
            SELECT *
            FROM publication
            WHERE revoked = 0 AND fonds = $1
            ORDER BY name DESC
        ";
        let rows = match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                sqlx::query_as::<_, Publication>(statement)
                    .bind(fonds)
                    .fetch_all(&mut *connection)
                    .await?
            }
        };
        Ok(rows)
    }

    /// Find the last non-revoked publication by `fonds`.
    ///
    /// # Errors
    /// Errors if the connection cannot be acquired or the query fails.
    async fn find_last_inserted(&self, fonds: &str) -> anyhow::Result<Option<Publication>> {
        let statement = "
            SELECT *
            FROM publication
            WHERE revoked = 0 AND fonds = $1
            ORDER BY date DESC
            LIMIT 1
        ";
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query_as::<_, Publication>(statement)
            .bind(fonds)
            .fetch_optional(&mut *connection)
            .await?;
        Ok(row)
    }

    /// Find a non-revoked publication by `name` and `fonds`.
    ///
    /// # Errors
    /// Errors if the connection cannot be acquired or the query fails.
    async fn find_by_name_and_fonds(
        &self,
        name: &str,
        fonds: &str,
    ) -> anyhow::Result<Option<Publication>> {
        let statement = "
            SELECT *
            FROM publication
            WHERE name = $1 AND fonds = $2 AND revoked = 0
        ";
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query_as::<_, Publication>(statement)
            .bind(name)
            .bind(fonds)
            .fetch_optional(&mut *connection)
            .await?;
        Ok(row)
    }
}

#[async_trait]
impl super::TxManager for DatabaseTransaction {
    /// Upsert a new publication into the database.
    ///
    /// # Errors
    /// Errors if the publication cannot be inserted into the database.
    async fn create(
        &mut self,
        hash_id: &str,
        name: &str,
        date: &NaiveDate,
        fonds: &str,
        last_valid_publication_id: Option<String>,
        last_valid_version: Option<String>,
        html_data_repo_name: Option<String>,
    ) -> anyhow::Result<Option<i64>> {
        let statement = "
            INSERT OR IGNORE INTO publication ( id, name, date, fonds, revoked, last_valid_publication_id, last_valid_version, html_data_repo_name )
            VALUES ( $1, $2, $3, $4, FALSE, $5, $6, $7)
        ";
        let id = sqlx::query(statement)
            .bind(hash_id)
            .bind(name)
            .bind(date.to_string())
            .bind(fonds)
            .bind(last_valid_publication_id)
            .bind(last_valid_version)
            .bind(html_data_repo_name)
            .execute(&mut *self.tx)
            .await?
            .last_insert_id();
        Ok(id)
    }
    /// Update a publication by name and fonds to be revoked.
    ///
    /// # Errors
    /// Errors if the publication cannot be updated.
    async fn update_by_name_and_fonds_set_revoked_true(
        &mut self,
        name: &str,
        fonds: &str,
    ) -> anyhow::Result<()> {
        let statement = "
            UPDATE publication
            SET revoked = TRUE
            WHERE name = $1 AND fonds = $2
        ";
        sqlx::query(statement)
            .bind(name)
            .bind(fonds)
            .execute(&mut *self.tx)
            .await?;
        Ok(())
    }
    /// Find the last non-revoked publication by `fonds_id`.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_last_inserted(&mut self, fonds: &str) -> anyhow::Result<Option<Publication>> {
        let statement = "
            SELECT *
            FROM publication
            WHERE revoked = 0 AND fonds = $1
            ORDER BY date DESC
            LIMIT 1
        ";
        let row = sqlx::query_as::<_, Publication>(statement)
            .bind(fonds)
            .fetch_one(&mut *self.tx)
            .await
            .ok();
        Ok(row)
    }

    /// Find a publication by `name` and `fonds_id`.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_by_name_and_fonds(
        &mut self,
        name: &str,
        fonds: &str,
    ) -> anyhow::Result<Option<Publication>> {
        let statement = "
            SELECT *
            FROM publication
            WHERE name = $1 AND fonds = $2 AND revoked = 0
        ";
        let row = sqlx::query_as::<_, Publication>(statement)
            .bind(name)
            .bind(fonds)
            .fetch_one(&mut *self.tx)
            .await
            .ok();
        Ok(row)
    }

    /// Check whether a publication, revoked or not, is stored for a fonds.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn exists_by_name_and_fonds(&mut self, name: &str, fonds: &str) -> anyhow::Result<bool> {
        let statement = "SELECT EXISTS(SELECT 1 FROM publication WHERE name = $1 AND fonds = $2)";
        let exists: i64 = sqlx::query_scalar(statement)
            .bind(name)
            .bind(fonds)
            .fetch_one(&mut *self.tx)
            .await?;
        Ok(exists != 0)
    }

    /// Filter publications by `name` and `fonds_id` which is not revoked.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_first_by_name_and_fonds_non_revoked(
        &mut self,
        name: &str,
        fonds: &str,
    ) -> anyhow::Result<Publication> {
        let statement = "
            SELECT *
            FROM publication
            WHERE name = $1 AND fonds = $2 AND revoked = 0
        ";
        let row = sqlx::query_as::<_, Publication>(statement)
            .bind(name)
            .bind(fonds)
            .fetch_one(&mut *self.tx)
            .await?;
        Ok(row)
    }

    /// Set `html_data_repo_name` on all publications for the given fonds whose date is
    /// strictly earlier than `boundary_date`.
    ///
    /// # Errors
    /// Errors if the update cannot be executed.
    async fn set_html_data_repo_name_for_prior_publications(
        &mut self,
        fonds: &str,
        boundary_date: &NaiveDate,
        html_data_repo_name: &str,
    ) -> anyhow::Result<()> {
        let statement = "
            UPDATE publication
            SET html_data_repo_name = $1
            WHERE fonds = $2 AND date < $3
        ";
        sqlx::query(statement)
            .bind(html_data_repo_name)
            .bind(fonds)
            .bind(boundary_date.to_string())
            .execute(&mut *self.tx)
            .await?;
        Ok(())
    }

    /// Find the names of the non-revoked publications for a given fonds.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_non_revoked_names(&mut self, fonds: &str) -> anyhow::Result<Vec<String>> {
        let statement = "
            SELECT name
            FROM publication
            WHERE revoked = 0 AND fonds = $1
        ";
        let names = sqlx::query_scalar(statement)
            .bind(fonds)
            .fetch_all(&mut *self.tx)
            .await?;
        Ok(names)
    }

    /// Count the non-revoked publications for a given fonds that carry no
    /// `html_data_repo_name`.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn count_non_revoked_missing_html_data_repo_name(
        &mut self,
        fonds: &str,
    ) -> anyhow::Result<usize> {
        let statement = "
            SELECT COUNT(*) as count
            FROM publication
            WHERE revoked = 0 AND fonds = $1 AND html_data_repo_name IS NULL
        ";
        let row: (i64,) = sqlx::query_as(statement)
            .bind(fonds)
            .fetch_one(&mut *self.tx)
            .await?;
        Ok(usize::try_from(row.0).unwrap_or(0))
    }

    /// Find all publication names by date and fonds.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_all_by_date_and_fonds_order_by_name_desc(
        &mut self,
        date: String,
        fonds: String,
    ) -> anyhow::Result<Vec<Publication>> {
        let statement = "
            SELECT *
            FROM publication
            WHERE date = $1 AND fonds = $2
            ORDER BY name DESC
        ";
        let rows = sqlx::query_as::<_, Publication>(statement)
            .bind(date)
            .bind(fonds)
            .fetch_all(&mut *self.tx)
            .await?;
        Ok(rows)
    }
}
