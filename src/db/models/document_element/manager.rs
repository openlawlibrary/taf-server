//! Manager for the document element model.
use async_trait::async_trait;
use sqlx::QueryBuilder;

use crate::db::{models::BATCH_SIZE, DatabaseConnection, DatabaseKind, DatabaseTransaction};

use super::{DocumentElement, ElementLabel};

#[async_trait]
impl super::Manager for DatabaseConnection {
    /// Find one document materialized path by url.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_doc_mpath_by_url(&self, url: &str, fonds: &str) -> anyhow::Result<String> {
        let statement = "
            SELECT de.doc_mpath
            FROM document_element de
            WHERE de.url = $1 AND de.fonds = $2
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

    /// Find the presentation labels for an explicit set of materialized paths.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_labels_by_mpaths(
        &self,
        doc_mpaths: &[String],
        fonds: &str,
    ) -> anyhow::Result<Vec<ElementLabel>> {
        if doc_mpaths.is_empty() {
            return Ok(vec![]);
        }
        let mut labels: Vec<ElementLabel> = vec![];
        match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                for chunk in doc_mpaths.chunks(BATCH_SIZE) {
                    let mut query_builder = QueryBuilder::new(
                        "SELECT de.doc_mpath AS doc_mpath, \
                                de.url       AS url, \
                                de.doc_id    AS doc_id \
                         FROM document_element de \
                         WHERE de.doc_mpath IN (",
                    );
                    let mut separated = query_builder.separated(", ");
                    for mpath in chunk {
                        separated.push_bind(mpath);
                    }
                    separated.push_unseparated(") AND de.fonds = ");
                    query_builder.push_bind(fonds);
                    let rows = query_builder
                        .build_query_as::<ElementLabel>()
                        .fetch_all(&mut *connection)
                        .await?;
                    labels.extend(rows);
                }
            }
        }
        Ok(labels)
    }

    /// Find every document element in the `[base, upper)` materialized-path
    /// range whose depth is at most `max_depth`.
    ///
    /// `document_element(doc_mpath)` is a BINARY primary key, so the half-open
    /// range is an index scan.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_elements_in_mpath_range(
        &self,
        base: &str,
        upper: &str,
        fonds: &str,
        max_depth: i64,
    ) -> anyhow::Result<Vec<ElementLabel>> {
        let statement = "
            SELECT de.doc_mpath AS doc_mpath,
                   de.url       AS url,
                   de.doc_id    AS doc_id
            FROM document_element de
            WHERE de.doc_mpath >= $1
              AND de.doc_mpath < $2
              AND de.fonds = $3
              AND ( LENGTH(de.doc_mpath) - LENGTH(REPLACE(de.doc_mpath, '|', '')) ) <= $4
        ";
        let rows = match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                sqlx::query_as::<_, ElementLabel>(statement)
                    .bind(base)
                    .bind(upper)
                    .bind(fonds)
                    .bind(max_depth)
                    .fetch_all(&mut *connection)
                    .await?
            }
        };
        Ok(rows)
    }

    /// Which of the given depth-1 document root materialized paths have at
    /// least one strictly deeper element row.
    ///
    /// The scan is deliberate: a per-root `doc_mpath` range uses the index, but
    /// its row lookups are not covering and measured three times slower.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_roots_with_children(
        &self,
        doc_roots: &[String],
        fonds: &str,
    ) -> anyhow::Result<Vec<String>> {
        if doc_roots.is_empty() {
            return Ok(vec![]);
        }
        let mut roots: Vec<String> = vec![];
        match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                for chunk in doc_roots.chunks(BATCH_SIZE) {
                    let mut query_builder = QueryBuilder::new(
                        "SELECT DISTINCT SUBSTR(de.doc_mpath, 1, INSTR(de.doc_mpath, '|')) AS doc_root \
                         FROM document_element de \
                         WHERE de.fonds = ",
                    );
                    query_builder.push_bind(fonds);
                    query_builder.push(
                        " AND ( LENGTH(de.doc_mpath) - LENGTH(REPLACE(de.doc_mpath, '|', '')) ) > 1 \
                          AND SUBSTR(de.doc_mpath, 1, INSTR(de.doc_mpath, '|')) IN (",
                    );
                    let mut separated = query_builder.separated(", ");
                    for root in chunk {
                        separated.push_bind(root);
                    }
                    separated.push_unseparated(")");
                    let found = query_builder
                        .build_query_as::<(String,)>()
                        .fetch_all(&mut *connection)
                        .await?;
                    roots.extend(found.into_iter().map(|row| row.0));
                }
            }
        }
        Ok(roots)
    }
}

#[async_trait]
impl super::TxManager for DatabaseTransaction {
    /// Upsert a bulk of document elements into the database.
    ///
    /// # Errors
    /// Errors if the document elements cannot be inserted into the database.
    async fn insert_bulk(&mut self, document_elements: Vec<DocumentElement>) -> anyhow::Result<()> {
        let mut query_builder = QueryBuilder::new(
            "INSERT OR IGNORE INTO document_element ( doc_mpath, url, doc_id, fonds ) ",
        );
        for chunk in document_elements.chunks(BATCH_SIZE) {
            query_builder.push_values(chunk, |mut bindings, de| {
                bindings
                    .push_bind(&de.doc_mpath)
                    .push_bind(&de.url)
                    .push_bind(&de.doc_id)
                    .push_bind(&de.fonds);
            });
            let query = query_builder.build();
            query.execute(&mut *self.tx).await?;
            query_builder.reset();
        }
        Ok(())
    }
}
