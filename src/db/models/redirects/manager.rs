//! Manager for the redirects model.
use std::collections::HashSet;

use crate::db::{
    models::{redirects::RedirectPair, BATCH_SIZE},
    DatabaseConnection, DatabaseKind, DatabaseTransaction,
};
use async_trait::async_trait;
use sqlx::QueryBuilder;

#[async_trait]
impl super::Manager for DatabaseConnection {
    /// Finds a redirect target for a given URL.
    ///
    /// Returns `Ok(None)` when no redirect exists for the given URL (i.e. the
    /// query returns no row). This is expected for most requests and is not
    /// treated as an error.
    ///
    /// # Errors
    /// Errors if a connection to the database can't be established, or the
    /// query fails for any reason other than the row not being found.
    async fn find_redirect_for_url(
        &self,
        fonds: String,
        repo_name: String,
        from_url: String,
    ) -> anyhow::Result<Option<String>> {
        let statement = "
            SELECT from_url, to_url
            FROM redirects
            WHERE fonds_name = $1
              AND repo_name = $2
              AND from_url = $3
        ";
        let result = match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                sqlx::query_as::<_, RedirectPair>(statement)
                    .bind(fonds)
                    .bind(repo_name)
                    .bind(from_url)
                    .fetch_one(&mut *connection)
                    .await
            }
        };

        match result {
            Ok(row) => Ok(Some(row.to_url)),
            // No redirect configured for this URL - not an error.
            Err(sqlx::Error::RowNotFound) => Ok(None),
            // Any other error (e.g. a connection issue) is a real failure and
            // must not be silently treated as "no redirect".
            Err(err) => {
                tracing::error!(error = %err, "Failed to query redirect for URL");
                Err(err.into())
            }
        }
    }

    /// Returns the set of `(fonds_name, repo_name)` pairs that currently have
    /// at least one redirect configured.
    ///
    /// # Errors
    /// Errors if a connection to the database can't be established or the
    /// query fails.
    async fn repos_with_redirects(&self) -> anyhow::Result<HashSet<(String, String)>> {
        let statement = "SELECT DISTINCT fonds_name, repo_name FROM redirects";
        let rows: Vec<(String, String)> = match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                sqlx::query_as(statement)
                    .fetch_all(&mut *connection)
                    .await?
            }
        };
        Ok(rows.into_iter().collect())
    }
}

#[async_trait]
impl super::TxManager for DatabaseTransaction {
    /// Upsert a bulk of redirects into the database.
    ///
    /// # Errors
    /// Errors if the redirects cannot be inserted into the database.
    async fn insert_bulk(
        &mut self,
        fonds: &str,
        repo_name: &str,
        redirect_pairs: Vec<RedirectPair>,
    ) -> anyhow::Result<()> {
        let mut query_builder = QueryBuilder::new(
            "INSERT OR IGNORE INTO redirects ( fonds_name, repo_name, from_url, to_url ) ",
        );
        for chunk in redirect_pairs.chunks(BATCH_SIZE) {
            query_builder.push_values(chunk, |mut bindings, rp| {
                bindings
                    .push_bind(fonds)
                    .push_bind(repo_name)
                    .push_bind(&rp.from_url)
                    .push_bind(&rp.to_url);
            });
            let query = query_builder.build();
            query.execute(&mut *self.tx).await?;
            query_builder.reset();
        }
        Ok(())
    }

    /// Deletes all redirects of a fonds.
    ///
    /// # Errors
    /// Errors if the delete statement fails.
    async fn delete_for_fonds(&mut self, fonds: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM redirects WHERE fonds_name = $1")
            .bind(fonds)
            .execute(&mut *self.tx)
            .await?;
        Ok(())
    }
}
