//use crate::db::{DatabaseConnection, DatabaseKind, Db as _};
use taf_server::db::{DatabaseConnection, DatabaseKind, DatabaseTransaction, Tx as _};
use taf_server::fonds::fonds::Fonds;
use taf_server::redirects::insert_redirects_for_fonds;

pub async fn insert_redirects(
    connection: &DatabaseConnection,
    fonds: &str,
    repo_name: &str,
    redirects: Vec<(&str, &str)>,
) {
    for (from, to) in redirects {
        match connection.kind {
            DatabaseKind::Sqlite => {
                sqlx::query(
                    "INSERT OR IGNORE INTO redirects (fonds_name, repo_name, from_url, to_url) VALUES (?, ?, ?, ?)",
                )
                .bind(fonds)
                .bind(repo_name)
                .bind(from)
                .bind(to)
                .execute(&connection.pool)
                .await
                .unwrap();
            }
        }
    }
}

pub async fn load_redirects(connection: &DatabaseConnection, fonds: &mut Fonds) {
    let mut tx = DatabaseTransaction::begin(connection.pool.clone())
        .await
        .unwrap();
    insert_redirects_for_fonds(&mut tx, fonds).await.unwrap();
    tx.commit().await.unwrap();
}

pub async fn insert_fonds(connection: &DatabaseConnection, fonds: &str) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            sqlx::query("INSERT OR IGNORE INTO fonds (name) VALUES (?)")
                .bind(fonds)
                .execute(&connection.pool)
                .await
                .unwrap();
        }
    }
}

/// Insert a publication row directly.
///
/// Passing `None` for `html_data_repo_name` reproduces the shape of a row written
/// before that column existed, which is otherwise unreachable through the normal
/// insert path.
pub async fn insert_publication(
    connection: &DatabaseConnection,
    id: &str,
    name: &str,
    date: &str,
    fonds: &str,
    revoked: bool,
    html_data_repo_name: Option<&str>,
) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            sqlx::query(
                "INSERT OR IGNORE INTO publication ( id, name, date, fonds, revoked, html_data_repo_name )
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(name)
            .bind(date)
            .bind(fonds)
            .bind(revoked)
            .bind(html_data_repo_name)
            .execute(&connection.pool)
            .await
            .unwrap();
        }
    }
}

// ---------------------------------------------------------------------------
// Row-insert helpers for the `/_api/menu` tests.
//
// Raw `sqlx::query`, same style as above. `PRAGMA foreign_keys = ON`, so
// parents go in first:
//
//     fonds -> document -> document_element -> document_change
//     version -> publication_version -> publication_has_publication_versions
//     fonds -> library
//     document_change -> changed_library_document
// ---------------------------------------------------------------------------

/// A `document_element` row.
#[derive(Clone, Debug)]
pub struct ElementRow {
    /// Materialized path. Always ends with `|`.
    pub doc_mpath: String,
    /// URL this element is served at, with a leading slash.
    pub url: String,
    /// Owning document id.
    pub doc_id: String,
    /// Owning fonds.
    pub fonds: String,
}

impl ElementRow {
    /// Build a row.
    pub fn new(doc_mpath: &str, url: &str, doc_id: &str, fonds: &str) -> Self {
        Self {
            doc_mpath: doc_mpath.to_owned(),
            url: url.to_owned(),
            doc_id: doc_id.to_owned(),
            fonds: fonds.to_owned(),
        }
    }
}

/// A `document_change` row.
#[derive(Clone, Debug)]
pub struct ChangeRow {
    /// Primary key; tests just need it unique.
    pub id: String,
    /// `Status::to_int()`: added = 0, effective = 1, changed = 2, removed = 3.
    pub status: i64,
    /// Owning `publication_version` id.
    pub publication_version_id: String,
    /// Materialized path of the element that changed.
    pub doc_mpath: String,
}

impl ChangeRow {
    /// Build a change row.
    pub fn new(id: &str, status: i64, publication_version_id: &str, doc_mpath: &str) -> Self {
        Self {
            id: id.to_owned(),
            status,
            publication_version_id: publication_version_id.to_owned(),
            doc_mpath: doc_mpath.to_owned(),
        }
    }
}

/// Insert a `document` row; `document_element.doc_id` points at it.
pub async fn insert_document(connection: &DatabaseConnection, doc_id: &str) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            sqlx::query("INSERT OR IGNORE INTO document (doc_id) VALUES (?)")
                .bind(doc_id)
                .execute(&connection.pool)
                .await
                .unwrap();
        }
    }
}

/// Insert `document_element` rows in one transaction.
///
/// One statement per row against a DELETE-journal SQLite file is far too slow
/// for the node-ceiling fixture, which needs several thousand.
pub async fn insert_document_elements(connection: &DatabaseConnection, rows: &[ElementRow]) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            let mut tx = connection.pool.begin().await.unwrap();
            for row in rows {
                sqlx::query(
                    "INSERT OR IGNORE INTO document_element ( doc_mpath, url, doc_id, fonds )
                     VALUES (?, ?, ?, ?)",
                )
                .bind(&row.doc_mpath)
                .bind(&row.url)
                .bind(&row.doc_id)
                .bind(&row.fonds)
                .execute(&mut *tx)
                .await
                .unwrap();
            }
            tx.commit().await.unwrap();
        }
    }
}

/// Insert a `version` row; `publication_version.version` points at it.
pub async fn insert_version(connection: &DatabaseConnection, codified_date: &str) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            sqlx::query("INSERT OR IGNORE INTO version (codified_date) VALUES (?)")
                .bind(codified_date)
                .execute(&connection.pool)
                .await
                .unwrap();
        }
    }
}

/// Insert a `publication_version` row.
///
/// `publication_id` is the owner, which is not the same thing as the
/// publications the row is visible in; that is
/// `publication_has_publication_versions`.
pub async fn insert_publication_version(
    connection: &DatabaseConnection,
    id: &str,
    version: &str,
    publication_id: &str,
) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            sqlx::query(
                "INSERT OR IGNORE INTO publication_version ( id, version, publication_id )
                 VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(version)
            .bind(publication_id)
            .execute(&connection.pool)
            .await
            .unwrap();
        }
    }
}

/// Map a `publication_version` into a publication.
pub async fn insert_publication_has_publication_version(
    connection: &DatabaseConnection,
    publication_id: &str,
    publication_version_id: &str,
) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            sqlx::query(
                "INSERT OR IGNORE INTO publication_has_publication_versions
                   ( publication_id, publication_version_id )
                 VALUES (?, ?)",
            )
            .bind(publication_id)
            .bind(publication_version_id)
            .execute(&connection.pool)
            .await
            .unwrap();
        }
    }
}

/// Insert `document_change` rows in one transaction.
pub async fn insert_document_changes(connection: &DatabaseConnection, rows: &[ChangeRow]) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            let mut tx = connection.pool.begin().await.unwrap();
            for row in rows {
                sqlx::query(
                    "INSERT OR IGNORE INTO document_change
                       ( id, status, publication_version_id, doc_mpath )
                     VALUES (?, ?, ?, ?)",
                )
                .bind(&row.id)
                .bind(row.status)
                .bind(&row.publication_version_id)
                .bind(&row.doc_mpath)
                .execute(&mut *tx)
                .await
                .unwrap();
            }
            tx.commit().await.unwrap();
        }
    }
}

/// Insert a `library` (collection) row.
pub async fn insert_library(connection: &DatabaseConnection, mpath: &str, url: &str, fonds: &str) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            sqlx::query("INSERT OR IGNORE INTO library ( mpath, url, fonds ) VALUES (?, ?, ?)")
                .bind(mpath)
                .bind(url)
                .bind(fonds)
                .execute(&connection.pool)
                .await
                .unwrap();
        }
    }
}

/// Insert a `library_change` row. The column is TEXT in the schema even though
/// it holds `Status::to_int()`, and the real ingest binds it as a string.
pub async fn insert_library_change(
    connection: &DatabaseConnection,
    publication_version_id: &str,
    status: &str,
    library_mpath: &str,
) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            sqlx::query(
                "INSERT OR IGNORE INTO library_change
                   ( publication_version_id, status, library_mpath )
                 VALUES (?, ?, ?)",
            )
            .bind(publication_version_id)
            .bind(status)
            .bind(library_mpath)
            .execute(&connection.pool)
            .await
            .unwrap();
        }
    }
}

/// Link a `document_change` to the library subtree it was published under --
/// the only bridge between the document and library mpath namespaces.
pub async fn insert_changed_library_document(
    connection: &DatabaseConnection,
    library_mpath: &str,
    document_change_id: &str,
) {
    match connection.kind {
        DatabaseKind::Sqlite => {
            sqlx::query(
                "INSERT OR IGNORE INTO changed_library_document
                   ( library_mpath, document_change_id )
                 VALUES (?, ?)",
            )
            .bind(library_mpath)
            .bind(document_change_id)
            .execute(&connection.pool)
            .await
            .unwrap();
        }
    }
}
