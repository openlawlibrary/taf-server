//! Manager for the library model.
use super::{Library, LibraryLabel};
use crate::db::{models::BATCH_SIZE, DatabaseConnection, DatabaseKind, DatabaseTransaction};
use async_trait::async_trait;
use sqlx::QueryBuilder;

#[async_trait]
impl super::Manager for DatabaseConnection {
    /// Find one library materialized path by url.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_lib_mpath_by_url(&self, url: &str, fonds: &str) -> anyhow::Result<String> {
        let statement = "
            SELECT l.mpath
            FROM library l
            WHERE l.url = $1 AND l.fonds = $2
            LIMIT 1
        ";
        let row = match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                sqlx::query_as::<_, (String,)>(statement)
                    .bind(url)
                    .bind(fonds)
                    .fetch_one(&mut *connection)
                    .await?
            }
        };
        Ok(row.0)
    }

    /// Find urls for an explicit set of library materialized paths.
    ///
    /// Served by the `library(mpath)` primary-key autoindex, one seek per mpath.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_labels_by_mpaths(
        &self,
        mpaths: &[String],
        fonds: &str,
    ) -> anyhow::Result<Vec<LibraryLabel>> {
        if mpaths.is_empty() {
            return Ok(vec![]);
        }
        let mut labels: Vec<LibraryLabel> = vec![];
        match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                for chunk in mpaths.chunks(BATCH_SIZE) {
                    let mut query_builder = QueryBuilder::new(
                        "SELECT l.mpath AS mpath, l.url AS url FROM library l WHERE l.mpath IN (",
                    );
                    let mut separated = query_builder.separated(", ");
                    for mpath in chunk {
                        separated.push_bind(mpath);
                    }
                    separated.push_unseparated(") AND l.fonds = ");
                    query_builder.push_bind(fonds);
                    let rows = query_builder
                        .build_query_as::<LibraryLabel>()
                        .fetch_all(&mut *connection)
                        .await?;
                    labels.extend(rows);
                }
            }
        }
        Ok(labels)
    }
}

#[async_trait]
impl super::TxManager for DatabaseTransaction {
    /// Upsert a bulk of libraries into the database.
    ///
    /// # Errors
    /// Errors if the libraries cannot be inserted into the database.
    async fn insert_bulk(&mut self, libraries: Vec<Library>) -> anyhow::Result<()> {
        let mut query_builder =
            QueryBuilder::new("INSERT OR IGNORE INTO library ( mpath, url, fonds ) ");
        for chunk in libraries.chunks(BATCH_SIZE) {
            query_builder.push_values(chunk, |mut bindings, lb| {
                bindings
                    .push_bind(&lb.mpath)
                    .push_bind(&lb.url)
                    .push_bind(&lb.fonds);
            });
            let query = query_builder.build();
            query.execute(&mut *self.tx).await?;
            query_builder.reset();
        }
        Ok(())
    }
}
