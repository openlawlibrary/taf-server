//! Manager for the data repo commit model.
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::NaiveDate;
use sqlx::QueryBuilder;

use crate::db::{models::BATCH_SIZE, DatabaseConnection, DatabaseTransaction};

use super::DataRepoCommits;

#[async_trait]
impl super::Manager for DatabaseConnection {
    /// Finds the most recent authentication commit for a given publication
    /// that is valid for the specified version date.
    ///
    /// If `version_date` is a valid ISO date (`YYYY-MM-DD`), the commit returned
    /// is the latest one whose `codified_date` is **less than or equal to**
    /// the provided date.
    /// If `version_date` is not a valid ISO date, the latest available commit
    /// for the publication is returned regardless of date.
    ///
    /// # Errors
    /// Returns an error if the commit cannot be retrieved from the database
    /// or if the query execution fails.
    async fn find_commit_by_pub_id_and_version_date(
        &self,
        publication_id: &str,
        version_date: &str,
    ) -> anyhow::Result<DataRepoCommits> {
        let mut connection = self.pool.acquire().await?;
        let is_iso = NaiveDate::parse_from_str(version_date, "%Y-%m-%d").is_ok();
        let query = if is_iso {
            "
            SELECT *
            FROM data_repo_commits
            WHERE publication_id = $1
            AND codified_date <= $2
            ORDER BY codified_date DESC, auth_commit_timestamp DESC
            LIMIT 1
            "
        } else {
            "
            SELECT *
            FROM data_repo_commits
            WHERE publication_id = $1
            ORDER BY codified_date DESC, auth_commit_timestamp DESC
            LIMIT 1
            "
        };

        let mut commit: Option<DataRepoCommits> = if is_iso {
            sqlx::query_as::<_, DataRepoCommits>(query)
                .bind(publication_id)
                .bind(version_date)
                .fetch_optional(&mut *connection)
                .await?
        } else {
            sqlx::query_as::<_, DataRepoCommits>(query)
                .bind(publication_id)
                .fetch_optional(&mut *connection)
                .await?
        };

        if commit.is_none() {
            let fallback_query = "
                SELECT *
                FROM data_repo_commits
                WHERE publication_id = $1
                ORDER BY build_date DESC, auth_commit_timestamp DESC
                LIMIT 1
            ";

            commit = sqlx::query_as::<_, DataRepoCommits>(fallback_query)
                .bind(publication_id)
                .fetch_optional(&mut *connection)
                .await?;
        }

        commit.ok_or_else(|| {
            anyhow!(
                "No commit found for publication_id={publication_id} and version_date={version_date}")
        })
    }
}

#[async_trait]
impl super::TxManager for DatabaseTransaction {
    /// Find all authentication commits for a given fonds and data repository.
    ///
    /// # Errors
    /// Errors if the commits cannot be found.
    async fn find_all_auth_commits_for_fonds_and_data_repo(
        &mut self,
        fonds_id: &str,
        data_repo_name: &str,
    ) -> anyhow::Result<Vec<DataRepoCommits>> {
        let query = "
            SELECT dc.*
            FROM data_repo_commits dc
            LEFT JOIN publication p ON dc.publication_id = p.id
            LEFT JOIN fonds s ON p.fonds = s.name
            WHERE s.name = $1
            AND p.html_data_repo_name = $2
        ";
        let data_repo_commits = sqlx::query_as::<_, DataRepoCommits>(query)
            .bind(fonds_id)
            .bind(data_repo_name)
            .fetch_all(&mut *self.tx)
            .await?;
        Ok(data_repo_commits)
    }
    /// Find the most-recently-recorded authentication commit hash for a given fonds
    /// and data repository.
    ///
    /// # Errors
    /// Errors if the query fails.
    async fn find_last_auth_commit_for_fonds(
        &mut self,
        fonds_id: &str,
        data_repo_name: &str,
    ) -> anyhow::Result<Option<String>> {
        let query = "
            SELECT dc.auth_commit_hash
            FROM data_repo_commits dc
            LEFT JOIN publication p ON dc.publication_id = p.id
            WHERE p.fonds = $1
            AND p.html_data_repo_name = $2
            ORDER BY dc.auth_commit_timestamp DESC
            LIMIT 1
        ";
        let row: Option<(String,)> = sqlx::query_as(query)
            .bind(fonds_id)
            .bind(data_repo_name)
            .fetch_optional(&mut *self.tx)
            .await?;
        Ok(row.map(|(hash,)| hash))
    }

    /// Upsert a bulk of data repository commits into the database.
    ///
    /// # Errors
    /// Errors if the commits cannot be inserted.
    async fn insert_bulk(&mut self, data_repo_commits: Vec<DataRepoCommits>) -> anyhow::Result<()> {
        let mut query_builder = QueryBuilder::new("INSERT OR IGNORE INTO data_repo_commits ( commit_hash, codified_date, build_date, repo_type, auth_commit_hash, auth_commit_timestamp, publication_id ) ");
        for chunk in data_repo_commits.chunks(BATCH_SIZE) {
            query_builder.push_values(chunk, |mut bindings, dc| {
                bindings
                    .push_bind(&dc.commit_hash)
                    .push_bind(&dc.codified_date)
                    .push_bind(&dc.build_date)
                    .push_bind(&dc.repo_type)
                    .push_bind(&dc.auth_commit_hash)
                    .push_bind(&dc.auth_commit_timestamp)
                    .push_bind(&dc.publication_id);
            });
            let query = query_builder.build();
            query.execute(&mut *self.tx).await?;
            query_builder.reset();
        }
        Ok(())
    }
}
