//! Performance guards, `#[ignore]` by default.
//!
//! They never point at `C:/OLLGIT/lib-dev`: connecting through
//! `DatabaseConnection` would run migrations against it, which is a write.
//! Make a copy first, then run them against that:
//!
//! ```text
//! python -I -c "import sqlite3; src = sqlite3.connect('file:C:/OLLGIT/lib-dev/.taf/db.sqlite3?mode=ro', uri=True); dst = sqlite3.connect(r'<scratch>/.taf/db.sqlite3'); src.backup(dst); dst.close()"
//! TAF_MENU_PERF_DB=<scratch> cargo nextest run --all --run-ignored all -E 'test(menu_perf)'
//! ```
//!
//! They assert the shape of the result (rows in, nodes out) and print the
//! elapsed time; a wall-clock assertion in CI would just be flaky.

use std::path::PathBuf;
use std::time::Instant;
use taf_server::db::models::document_change::Manager as _;
use taf_server::db::models::publication::Manager as _;
use taf_server::db::DatabaseConnection;
use taf_server::server::api::menu::tree::{plan, synthesize_dated};
use taf_server::server::api::menu::MAX_MENU_NODES;

/// Environment variable naming the archive directory holding the copy.
const PERF_DB_ENV: &str = "TAF_MENU_PERF_DB";

/// Resolve the archive directory to measure, or skip.
///
/// Panics rather than silently passing if the caller points this at the live
/// dev archive.
fn perf_archive() -> Option<PathBuf> {
    let raw = std::env::var(PERF_DB_ENV).ok()?;
    let path = PathBuf::from(&raw);
    let normalised = raw.replace('\\', "/").to_lowercase();
    assert!(
        !normalised.contains("ollgit/lib-dev"),
        "{PERF_DB_ENV} points at the product owner's dev archive ({raw}). Connecting \
         would run pending migrations against it, which is a write. Copy the database \
         first; see this module's docs."
    );
    Some(path)
}

/// Connect to the copy.
///
/// `sqlx::migrate!` is deliberately NOT used: the dev archive's database has
/// `20261030090000_add_document_build_reason` applied and that file does not
/// exist on this branch, so sqlx refuses the whole run with
/// `VersionMissing`. The menu queries need no schema change.
async fn connect(archive: &std::path::Path) -> DatabaseConnection {
    let db_path = archive.join(".taf/db.sqlite3");
    assert!(
        db_path.exists(),
        "no database at {}; copy one there first",
        db_path.display()
    );
    let url = format!("sqlite:///{}?mode=rw", db_path.to_string_lossy());
    let db = DatabaseConnection::connect_with_journal_mode(&url, "DELETE")
        .await
        .unwrap();
    db
}

/// Run one dated menu synthesis end to end and report its shape.
async fn measure(db: &DatabaseConnection, fonds: &str, base: &str, date: &str, depth: usize) {
    let publications = db.find_all_non_revoked_publications(fonds).await.unwrap();
    let publication = publications
        .first()
        .unwrap_or_else(|| panic!("no non-revoked publication for {fonds}"));

    let started = Instant::now();
    let rows = db
        .find_changes_in_version_by_prefix(base, &publication.id, date)
        .await
        .unwrap();
    let queried = started.elapsed();

    let kept: Vec<_> = rows
        .into_iter()
        .filter(|row| row.doc_mpath.starts_with(base))
        .collect();
    let node_set = synthesize_dated(&kept, base, depth);
    let layout = plan(&node_set, base, MAX_MENU_NODES);
    let total = started.elapsed();

    println!(
        "{fonds} {date} depth={depth} base={base:?}: {} change rows -> {} nodes \
         (truncated={}); query {:?}, total {:?}",
        kept.len(),
        layout.node_count,
        layout.truncated,
        queried,
        total
    );
    assert!(
        layout.node_count <= MAX_MENU_NODES,
        "the node ceiling must bound every response"
    );
}

/// Case 53. San Mateo `/code` on a typical date.
#[actix_web::test]
#[ignore = "requires a copy of the dev archive; see module docs"]
async fn menu_perf_san_mateo_typical_date() {
    let Some(archive) = perf_archive() else {
        return;
    };
    let db = connect(&archive).await;
    measure(
        &db,
        "cityofsanmateo/law",
        "City of San Mateo, Cal., Code|",
        "2026-02-03",
        2,
    )
    .await;
    drop(db);
}

/// Case 54. The San Mateo library bootstrap date -- the one shape where the
/// node ceiling and the synthesis cost actually matter.
#[actix_web::test]
#[ignore = "requires a copy of the dev archive; see module docs"]
async fn menu_perf_san_mateo_bootstrap_date() {
    let Some(archive) = perf_archive() else {
        return;
    };
    let db = connect(&archive).await;
    for depth in [2_usize, 6] {
        measure(
            &db,
            "cityofsanmateo/law",
            "City of San Mateo, Cal., Code|",
            "2018-03-05",
            depth,
        )
        .await;
    }
    drop(db);
}

/// Case 55. Maryland, including its bootstrap date, at depth 2 and depth 6.
#[actix_web::test]
#[ignore = "requires a copy of the dev archive; see module docs"]
async fn menu_perf_maryland_bootstrap_date() {
    let Some(archive) = perf_archive() else {
        return;
    };
    let db = connect(&archive).await;
    for depth in [2_usize, 6] {
        measure(
            &db,
            "maryland-dsd/law",
            "Code of Maryland Regulations|",
            "2025-08-22",
            depth,
        )
        .await;
    }
    drop(db);
}
