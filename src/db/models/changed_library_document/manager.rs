//! Manager for the changed library document model.
use super::{ChangedLibraryDocument, LibraryDocument, LibraryDocumentChange};
use crate::db::{models::BATCH_SIZE, DatabaseConnection, DatabaseKind, DatabaseTransaction};
use async_trait::async_trait;
use sqlx::QueryBuilder;

#[async_trait]
impl super::Manager for DatabaseConnection {
    /// Every document that changed in one of the given `publication_version`
    /// ids, under a library materialized-path prefix. Yields both collection
    /// levels: truncating `library_mpath`, and `doc_mpath` to its first segment.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_changes_by_library_prefix_and_version_ids(
        &self,
        library_mpath_prefix: &str,
        publication_version_ids: &[String],
    ) -> anyhow::Result<Vec<LibraryDocumentChange>> {
        if publication_version_ids.is_empty() {
            return Ok(vec![]);
        }
        let mut rows: Vec<LibraryDocumentChange> = vec![];
        match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                for chunk in publication_version_ids.chunks(BATCH_SIZE) {
                    let mut query_builder = QueryBuilder::new(
                        "SELECT cld.library_mpath AS library_mpath, \
                                dc.doc_mpath      AS doc_mpath, \
                                dc.status         AS status \
                         FROM changed_library_document cld \
                         JOIN document_change dc ON dc.id = cld.document_change_id \
                         WHERE cld.library_mpath LIKE ",
                    );
                    query_builder.push_bind(format!("{library_mpath_prefix}%"));
                    query_builder.push(" AND dc.publication_version_id IN (");
                    let mut separated = query_builder.separated(", ");
                    for version_id in chunk {
                        separated.push_bind(version_id);
                    }
                    separated.push_unseparated(")");
                    let found = query_builder
                        .build_query_as::<LibraryDocumentChange>()
                        .fetch_all(&mut *connection)
                        .await?;
                    rows.extend(found);
                }
            }
        }
        Ok(rows)
    }

    /// Every document ever recorded under a library materialized-path prefix.
    ///
    /// # Errors
    /// Errors if can't establish a connection to the database.
    async fn find_documents_by_library_prefix(
        &self,
        library_mpath_prefix: &str,
    ) -> anyhow::Result<Vec<LibraryDocument>> {
        let statement = "
            SELECT DISTINCT cld.library_mpath AS library_mpath,
                            dc.doc_mpath      AS doc_mpath
            FROM changed_library_document cld
            JOIN document_change dc
              ON dc.id = cld.document_change_id
            WHERE cld.library_mpath LIKE $1
        ";
        let rows = match self.kind {
            DatabaseKind::Sqlite => {
                let mut connection = self.pool.acquire().await?;
                sqlx::query_as::<_, LibraryDocument>(statement)
                    .bind(format!("{library_mpath_prefix}%"))
                    .fetch_all(&mut *connection)
                    .await?
            }
        };
        Ok(rows)
    }
}

#[async_trait]
impl super::TxManager for DatabaseTransaction {
    /// Upsert a bulk of changed library documents into the database.
    ///
    /// # Errors
    /// Errors if the changed library documents cannot be inserted into the database.
    async fn insert_bulk(
        &mut self,
        changed_library_document: Vec<ChangedLibraryDocument>,
    ) -> anyhow::Result<()> {
        let mut query_builder = QueryBuilder::new(
            "INSERT OR IGNORE INTO changed_library_document ( library_mpath, document_change_id ) ",
        );
        for chunk in changed_library_document.chunks(BATCH_SIZE) {
            query_builder.push_values(chunk, |mut bindings, cl| {
                bindings
                    .push_bind(&cl.library_mpath)
                    .push_bind(&cl.document_change_id);
            });
            let query = query_builder.build();
            query.execute(&mut *self.tx).await?;
            query_builder.reset();
        }
        Ok(())
    }
}
