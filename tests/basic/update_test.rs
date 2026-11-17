//! `taf-server update` runs against a basic archive.
//!
//! `changes::insert` is `#[actix_web::main]`, so these are plain `#[test]` functions
//! that drive database queries with their own `System`.
use crate::archive_testtools::config::{ArchiveType, Jurisdiction};
use crate::common;
use git2::{Oid, Repository, Signature, Time};
use std::path::Path;
use tempfile::TempDir;

const AUTH: &str = "test_org/law";
const HTML: &str = "test_org/law-html";
const RDF: &str = "test_org/law-rdf";

/// Commit `writes` to `repo` one second after its HEAD; `None` deletes the file.
fn commit_files(td: &TempDir, repo: &str, writes: &[(&str, Option<&str>)]) -> Oid {
    let repo_dir = td.path().join(repo);
    let repo = Repository::open(&repo_dir).unwrap();
    let mut index = repo.index().unwrap();
    for (rel, content) in writes {
        let full = repo_dir.join(rel);
        if let Some(text) = content {
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, text).unwrap();
            index.add_path(Path::new(rel)).unwrap();
        } else {
            std::fs::remove_file(&full).unwrap();
            index.remove_path(Path::new(rel)).unwrap();
        }
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    let time = Time::new(parent.time().seconds() + 1, 0);
    let sig = Signature::new("test", "test@example.com", &time).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "commit", &tree, &[&parent])
        .unwrap()
}

/// A `_publication/<name>/index.rdf` with one document version per date.
fn pub_rdf(name: &str, dates: &[&str]) -> String {
    let base = format!("https://example.com/_publication/{name}");
    let available = &name[..10];
    let mut rdf = format!(
        "<?xml version=\"1.0\"?>\n<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" \
         xmlns:rdfs=\"http://www.w3.org/2000/01/rdf-schema#\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:oll=\"https://open.law/us/ngo/oll/_ontology/v0.1/ontology.owl#\">\n\
         <rdf:Description rdf:about=\"{base}\"><rdfs:label>Publication {name}</rdfs:label>\
         <dcterms:available>{available}</dcterms:available></rdf:Description>\n"
    );
    for (i, date) in dates.iter().enumerate() {
        rdf.push_str(&format!(
            "<oll:DocumentVersion rdf:about=\"{base}/dv/{i}\"><oll:codifiedDate>{date}</oll:codifiedDate>\
             <oll:docId>d</oll:docId><oll:hasChanges rdf:resource=\"{base}/changes/{i}\"/></oll:DocumentVersion>\n\
             <rdf:Bag rdf:about=\"{base}/changes/{i}\"><rdf:_1 rdf:resource=\"{base}/change/{i}\"/></rdf:Bag>\n\
             <rdf:Description rdf:about=\"{base}/change/{i}\"><oll:documentMaterializedPath>D|</oll:documentMaterializedPath>\
             <oll:url>/d</oll:url><oll:status>Element changed</oll:status></rdf:Description>\n"
        ));
    }
    rdf.push_str("</rdf:RDF>\n");
    rdf
}

fn commit_publication(td: &TempDir, name: &str, dates: &[&str]) {
    let rel = format!("_publication/{name}/index.rdf");
    commit_files(td, RDF, &[(&rel, Some(&pub_rdf(name, dates)))]);
}

/// Mark `law-html` as `serve: historical`, as production HTML repositories are.
fn make_html_historical(td: &TempDir) {
    let path = td.path().join(AUTH).join("targets/repositories.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    value["repositories"][HTML]["custom"]["serve"] = serde_json::json!("historical");
    let text = value.to_string();
    commit_files(td, AUTH, &[("targets/repositories.json", Some(&text))]);
}

/// Commit a new `law-html` build and point the auth repository's target file at it.
fn commit_html_build(td: &TempDir, codified_date: &str) -> Oid {
    let html_commit = commit_files(td, HTML, &[("doc.html", Some(codified_date))]);
    let target = serde_json::json!({
        "branch": "publication/2021-04-23",
        "build-date": codified_date,
        "commit": html_commit.to_string(),
        "codified-date": codified_date,
    });
    let target = target.to_string();
    commit_files(td, AUTH, &[("targets/test_org/law-html", Some(&target))])
}

fn run_update(td: &TempDir) -> bool {
    taf_server::history::changes::insert(
        td.path().to_str().unwrap(),
        td.path().to_path_buf(),
        &vec![],
        &vec![],
        false,
    )
    .is_ok()
}

fn count(td: &TempDir, sql: &'static str, bind: &str) -> i64 {
    actix_web::rt::System::new().block_on(async {
        let db = taf_server::db::init::connect(td.path()).await.unwrap();
        let value = sqlx::query_scalar(sql)
            .bind(bind)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        db.pool.close().await;
        value
    })
}

fn version_count(td: &TempDir, pub_name: &str) -> i64 {
    count(
        td,
        "SELECT COUNT(*) FROM publication_has_publication_versions phpv \
         JOIN publication p ON p.id = phpv.publication_id WHERE p.name = ?",
        pub_name,
    )
}

fn publication_count(td: &TempDir, pub_name: &str) -> i64 {
    count(
        td,
        "SELECT COUNT(*) FROM publication WHERE name = ? AND revoked = 0",
        pub_name,
    )
}

fn new_archive() -> TempDir {
    common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap()
}

#[test]
fn test_update_where_publication_rdf_has_doctype_expect_error_and_publication_skipped() {
    let td = new_archive();
    let rdf = pub_rdf("2021-04-23", &["2021-01-28"]).replacen(
        "<rdf:RDF",
        "<!DOCTYPE rdf:RDF [<!ENTITY a \"a\">]>\n<rdf:RDF",
        1,
    );
    commit_files(
        &td,
        RDF,
        &[("_publication/2021-04-23/index.rdf", Some(&rdf))],
    );
    assert!(!run_update(&td));
    assert_eq!(publication_count(&td, "2021-04-23"), 0);
}

#[test]
fn test_update_where_same_date_republication_added_expect_full_history() {
    let td = new_archive();
    let dates = ["2021-01-28", "2021-02-16", "2021-04-06"];
    commit_publication(&td, "2021-04-23", &dates);
    assert!(run_update(&td));
    commit_publication(&td, "2021-04-23-01", &dates);
    assert!(run_update(&td));
    assert_eq!(publication_count(&td, "2021-04-23"), 0);
    assert_eq!(version_count(&td, "2021-04-23-01"), 3);
}

#[test]
fn test_update_where_backdated_publication_added_expect_ingested() {
    let td = new_archive();
    commit_publication(&td, "2021-04-23", &["2021-01-28"]);
    assert!(run_update(&td));
    commit_publication(&td, "2021-03-01", &["2021-01-28"]);
    assert!(run_update(&td));
    assert_eq!(publication_count(&td, "2021-03-01"), 1);
}

#[test]
fn test_update_where_publication_removed_and_revoked_one_remains_expect_rebuild() {
    let td = new_archive();
    commit_publication(&td, "2021-04-23", &["2021-01-28"]);
    commit_publication(&td, "2021-04-23-01", &["2021-01-28"]);
    commit_publication(&td, "2025-02-20", &["2025-02-01"]);
    assert!(run_update(&td));
    assert_eq!(publication_count(&td, "2025-02-20"), 1);
    commit_files(&td, RDF, &[("_publication/2025-02-20/index.rdf", None)]);
    assert!(run_update(&td));
    assert_eq!(publication_count(&td, "2025-02-20"), 0);
}

#[test]
fn test_update_where_auth_repo_reset_to_earlier_commit_expect_rebuild() {
    let td = new_archive();
    make_html_historical(&td);
    commit_publication(&td, "2021-04-23", &[]);
    let first = commit_html_build(&td, "2021-01-28");
    assert!(run_update(&td));
    let second = commit_html_build(&td, "2021-08-03");
    assert!(run_update(&td));
    let auth = Repository::open(td.path().join(AUTH)).unwrap();
    let head = auth.head().unwrap().name().unwrap().to_owned();
    auth.reference(&head, first, true, "reset").unwrap();
    assert!(run_update(&td));
    let rows = "SELECT COUNT(*) FROM data_repo_commits WHERE auth_commit_hash = ?";
    assert_eq!(count(&td, rows, &second.to_string()), 0);
    assert!(count(&td, rows, &first.to_string()) >= 1);
}

#[test]
fn test_update_where_fonds_deleted_expect_cascade_lookups_use_indexes() {
    let td = new_archive();
    let lookups = [
        "EXPLAIN QUERY PLAN SELECT rowid FROM document_change WHERE doc_mpath = ?",
        "EXPLAIN QUERY PLAN SELECT rowid FROM publication_version WHERE publication_id = ?",
        "EXPLAIN QUERY PLAN SELECT rowid FROM publication WHERE last_valid_publication_id = ?",
        "EXPLAIN QUERY PLAN SELECT rowid FROM data_repo_commits WHERE publication_id = ?",
    ];
    actix_web::rt::System::new().block_on(async {
        let db = taf_server::db::init::connect(td.path()).await.unwrap();
        for lookup in lookups {
            let row = sqlx::query(lookup)
                .bind("x")
                .fetch_one(&db.pool)
                .await
                .unwrap();
            let detail: String = sqlx::Row::get(&row, "detail");
            assert!(detail.starts_with("SEARCH"), "{lookup}: {detail}");
        }
        db.pool.close().await;
    });
}

#[test]
fn test_update_where_redirects_changed_expect_only_current_redirects_stored() {
    let td = new_archive();
    commit_publication(&td, "2021-04-23", &[]);
    let old = r#"[["/a", "/old"], ["/b", "/b-target"]]"#;
    commit_files(&td, HTML, &[("redirects.json", Some(old))]);
    assert!(run_update(&td));
    commit_files(
        &td,
        HTML,
        &[("redirects.json", Some(r#"[["/a", "/new"]]"#))],
    );
    assert!(run_update(&td));
    let from = "SELECT COUNT(*) FROM redirects WHERE from_url = ?";
    let to = "SELECT COUNT(*) FROM redirects WHERE to_url = ?";
    assert_eq!(count(&td, from, "/b"), 0);
    assert_eq!(count(&td, from, "/a"), 1);
    assert_eq!(count(&td, to, "/new"), 1);
}
