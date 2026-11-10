//! Manager for the `publication_version` model.
use super::PublicationVersion;
use crate::db::{DatabaseConnection, DatabaseKind, DatabaseTransaction};
use async_trait::async_trait;
use std::collections::HashSet;

#[async_trait]
impl super::Manager for DatabaseConnection {
    /// The newest version date mapped into a publication.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_latest_version_for_publication(
        &self,
        publication_id: &str,
    ) -> anyhow::Result<Option<String>> {
        let statement = "
            SELECT MAX(pv.version) AS codified_date
            FROM publication_has_publication_versions phpv
            JOIN publication_version pv ON pv.id = phpv.publication_version_id
            WHERE phpv.publication_id = $1
        ";
        let row = match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                sqlx::query_as::<_, (Option<String>,)>(statement)
                    .bind(publication_id)
                    .fetch_optional(&mut *connection)
                    .await?
            }
        };
        Ok(row.and_then(|found| found.0))
    }

    /// The `publication_version` ids carrying a given version date in a
    /// publication.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_version_ids_in_publication(
        &self,
        publication_id: &str,
        version: &str,
    ) -> anyhow::Result<Vec<String>> {
        let statement = "
            SELECT pv.id AS id
            FROM publication_has_publication_versions phpv
            JOIN publication_version pv ON pv.id = phpv.publication_version_id
            WHERE phpv.publication_id = $1 AND pv.version = $2
        ";
        let rows = match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                sqlx::query_as::<_, (String,)>(statement)
                    .bind(publication_id)
                    .bind(version)
                    .fetch_all(&mut *connection)
                    .await?
            }
        };
        Ok(rows.into_iter().map(|row| row.0).collect())
    }
}

#[async_trait]
impl super::TxManager for DatabaseTransaction {
    /// Upsert a new publication version into the database.
    ///
    /// # Errors
    /// Errors if the publication version cannot be inserted into the database.
    async fn create(
        &mut self,
        hash_id: &str,
        publication_id: &str,
        codified_date: &str,
    ) -> anyhow::Result<Option<i64>> {
        let statement = "
            INSERT OR IGNORE INTO publication_version ( id, publication_id, version )
            VALUES ( $1, $2, $3 )
        ";
        let id = sqlx::query(statement)
            .bind(hash_id)
            .bind(publication_id)
            .bind(codified_date)
            .execute(&mut *self.tx)
            .await?
            .last_insert_id();
        Ok(id)
    }

    /// Delete a publication version by a `publication_id` and `version`.
    ///
    /// `ON DELETE CASCADE` removes the `document_change`, `library_change` and
    /// `publication_has_publication_versions` rows that reference it, and in turn the
    /// `changed_library_document` rows that reference those document changes.
    ///
    /// # Errors
    /// Errors if the publication version cannot be deleted from the database.
    async fn delete_by_publication_id_and_version(
        &mut self,
        publication_id: &str,
        version: &str,
    ) -> anyhow::Result<()> {
        let statement = "
            DELETE FROM publication_version
            WHERE publication_id = $1 AND version = $2
        ";
        sqlx::query(statement)
            .bind(publication_id)
            .bind(version)
            .execute(&mut *self.tx)
            .await?;
        Ok(())
    }

    /// Find the last inserted publication version by `publication`.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_last_inserted_date_by_publication_id(
        &mut self,
        publication_id: &str,
    ) -> anyhow::Result<Option<PublicationVersion>> {
        let statement = "
            SELECT *
            FROM publication_version
            WHERE publication_id = $1
            ORDER BY version DESC
            LIMIT 1
        ";
        let row = sqlx::query_as::<_, PublicationVersion>(statement)
            .bind(publication_id)
            .fetch_one(&mut *self.tx)
            .await
            .ok();
        Ok(row)
    }

    /// Find a publication version by `publication_id`.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_all_by_publication_id(
        &mut self,
        publication_id: &str,
    ) -> anyhow::Result<Vec<PublicationVersion>> {
        let statement = "
            SELECT *
            FROM publication_version
            WHERE publication_id = $1
        ";
        let rows = sqlx::query_as::<_, PublicationVersion>(statement)
            .bind(publication_id)
            .fetch_all(&mut *self.tx)
            .await?;
        Ok(rows)
    }

    /// Find all publication versions that contain the in `publication_has_publication_versions` table.
    async fn find_all_in_publication_has_publication_versions(
        &mut self,
        publication_ids: Vec<String>,
    ) -> anyhow::Result<Vec<PublicationVersion>> {
        let mut builder = sqlx::QueryBuilder::new(
            "SELECT DISTINCT *
         FROM publication_has_publication_versions phpv
         JOIN publication_version pv ON pv.id = phpv.publication_version_id
         WHERE phpv.publication_id IN (",
        );
        let mut separated = builder.separated(", ");
        for id in &publication_ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(") ");

        let query = builder.build_query_as::<PublicationVersion>();
        let rows = query.fetch_all(&mut *self.tx).await?;
        Ok(rows)
    }

    /// Find a publication version by a `publication_id` and `version`.
    async fn find_by_publication_id_and_version(
        &mut self,
        publication_id: &str,
        version: &str,
    ) -> anyhow::Result<Option<PublicationVersion>> {
        let statement = "
            SELECT *
            FROM publication_version
            WHERE publication_id = $1 AND version = $2
        ";
        let row = sqlx::query_as::<_, PublicationVersion>(statement)
            .bind(publication_id)
            .bind(version)
            .fetch_one(&mut *self.tx)
            .await
            .ok();
        Ok(row)
    }

    /// Recursively find all publication versions starting from a given publication ID.
    ///
    /// This is necessary because publication versions can be the same across publications.
    /// To make versions query simpler, we walk the publication hierarchy starting from
    /// `publication_name` looking for related publications.
    /// The function returns all the `publication_version` IDs, even in simple cases where a publication
    /// has no hierarchy.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_all_recursive_for_publication(
        &mut self,
        publication_id: String,
    ) -> anyhow::Result<Vec<PublicationVersion>> {
        let mut versions: HashSet<PublicationVersion> = self
            .find_all_by_publication_id(&publication_id)
            .await?
            .into_iter()
            .collect();
        let mut checked_publication_ids = HashSet::new();
        checked_publication_ids.insert(publication_id.clone());

        let mut publication_ids_to_check = HashSet::new();
        publication_ids_to_check.insert(publication_id);

        while !publication_ids_to_check.is_empty() {
            let new_versions: HashSet<PublicationVersion> = self
                .find_all_in_publication_has_publication_versions(
                    publication_ids_to_check.clone().into_iter().collect(),
                )
                .await?
                .into_iter()
                .collect();
            versions.extend(new_versions.clone());
            checked_publication_ids.extend(publication_ids_to_check.clone());

            publication_ids_to_check = new_versions
                .clone()
                .into_iter()
                .filter(|pv| !checked_publication_ids.contains(&pv.publication_id.clone()))
                .map(|pv| pv.publication_id)
                .collect();
        }
        Ok(versions.into_iter().collect())
    }
}
