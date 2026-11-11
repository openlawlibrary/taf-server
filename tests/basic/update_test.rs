//! `taf-server update` runs against a basic archive.
//!
//! `changes::insert` is `#[actix_web::main]`, so these are plain `#[test]` functions
//! that drive database queries with their own `System`.
use crate::archive_testtools::config::{ArchiveType, Jurisdiction};
use crate::common;
use git2::{Oid, Repository, Signature};
use std::path::Path;
use taf_server::db::DatabaseConnection;
use tempfile::TempDir;

const AUTH: &str = "test_org/law";
const HTML: &str = "test_org/law-html";
const RDF: &str = "test_org/law-rdf";

/// Commit `writes` to `repo`; `None` deletes the file.
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
    let sig = Signature::now("test", "test@example.com").unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
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
        let db: DatabaseConnection = common::get_db(td.path()).await;
        sqlx::query_scalar(sql)
            .bind(bind)
            .fetch_one(&db.pool)
            .await
            .unwrap()
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
