//! Fixtures and JSON helpers for the `/_api/menu` tests.
//!
//! Every test here drives the real HTTP stack and asserts on the JSON body.
//! The contract is `docs/multi-level-menu-design.md`.

use crate::archive_testtools::config::{ArchiveType, Jurisdiction};
use crate::common::{
    self,
    db_data::{
        insert_changed_library_document, insert_document, insert_document_changes,
        insert_document_elements, insert_fonds, insert_library, insert_library_change,
        insert_publication, insert_publication_has_publication_version, insert_publication_version,
        insert_version,
    },
    get_db,
};
use actix_http::Request;
use actix_service::Service;
use actix_web::body::MessageBody;
use actix_web::dev::ServiceResponse;
use actix_web::http::StatusCode;
use actix_web::{test, Error};
use serde_json::Value;
use std::collections::BTreeSet;
use taf_server::db::DatabaseConnection;
use tempfile::TempDir;

mod ceiling_test;
mod collection_children_test;
mod date_test;
mod depth_test;
mod fixture_data;
mod label_test;
mod node_set_test;
mod prefix_test;
mod sort_test;
mod tree_mode_test;

/// Root fonds of `ArchiveType::Basic(Jurisdiction::Single)`.
pub const FONDS: &str = "test_org/law";
/// A second fonds, holding only the out-of-fonds element the collection
/// `hasChildren` override has to ignore.
pub const OTHER_FONDS: &str = "test_org/law-other";

/// The only publication.
pub const PUB_CURRENT: &str = "2024-03-01";
/// Id of that publication.
pub const PUB_CURRENT_ID: &str = "pub-current";

/// A date whose changes must not show up on `DATE_MAIN`.
pub const DATE_EARLY: &str = "2024-01-01";
/// The structural date: rollup, frontier, sort and collection fixtures.
pub const DATE_MAIN: &str = "2024-02-01";
/// A date with no changes under `/code`.
pub const DATE_LATE: &str = "2024-03-01";
/// A well-formed date with no `publication_version` row.
pub const DATE_ABSENT: &str = "2024-12-31";

/// `publication_version` for `DATE_EARLY`.
pub const PV_EARLY: &str = "pv-01";
/// `publication_version` for `DATE_MAIN`.
pub const PV_MAIN: &str = "pv-02";
/// `publication_version` for `DATE_LATE`.
pub const PV_LATE: &str = "pv-03";

/// San Mateo code document id, which is also its depth-1 mpath segment.
pub const CODE_DOC: &str = "City of San Mateo, Cal., Code";
/// URL of the San Mateo code root.
pub const CODE_URL: &str = "/us/ca/cities/san-mateo/code";
/// Mohican opinion: a depth-1 mpath behind a seven-segment URL.
pub const MOHICAN_DOC: &str = "1997 SMTCA 1";
/// URL of the Mohican opinion.
pub const MOHICAN_URL: &str = "/us/nsn/mohican/courts/appeals/opinions/1997/1";
/// URL of the resolutions collection.
pub const RESOLUTIONS_URL: &str = "/us/ca/cities/san-mateo/resolutions";
/// Library mpath of the resolutions collection.
pub const RESOLUTIONS_MPATH: &str = "test_org/law-rdf|us/ca/cities/san-mateo/resolutions|";

/// `Status::to_int()` for `Element added`.
pub const ADDED: i64 = 0;
/// `Status::to_int()` for `Element changed`.
pub const CHANGED: i64 = 2;

/// A migrated archive plus a connection to its database.
pub struct Fixture {
    /// Connection to `<archive>/.taf/db.sqlite3`.
    ///
    /// Declared before `archive`: fields drop in declaration order, and the
    /// pool has to release the database file before the `TempDir` deletes the
    /// directory holding it. On Windows a still-open handle makes
    /// `TempDir::drop` fail silently, littering `tests/fixtures/`.
    #[allow(dead_code)]
    pub db: DatabaseConnection,
    /// The archive directory, removed when the fixture drops.
    pub archive: TempDir,
}

/// One HTTP response, decoded both ways.
pub struct Resp {
    /// HTTP status.
    pub status: StatusCode,
    /// Raw body as UTF-8; the 400 cases are plain text.
    pub body: String,
    /// Parsed body, when it is JSON.
    pub json: Option<Value>,
}

impl Resp {
    /// The parsed body, panicking with the raw body when it is not JSON.
    pub fn json(&self) -> &Value {
        self.json
            .as_ref()
            .unwrap_or_else(|| panic!("expected a JSON body, got {}: {}", self.status, self.body))
    }

    /// The `root` node of a menu response.
    pub fn root(&self) -> &Value {
        &self.json()["root"]
    }
}

/// `GET` `uri` against `app`.
pub async fn get(
    app: &impl Service<Request, Response = ServiceResponse<impl MessageBody>, Error = Error>,
    uri: &str,
) -> Resp {
    let req = test::TestRequest::get().uri(uri).to_request();
    let resp = test::call_service(app, req).await;
    let status = resp.status();
    let bytes = test::read_body(resp).await;
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let json = serde_json::from_slice::<Value>(&bytes).ok();
    Resp { status, body, json }
}

/// Build the shared menu fixture: one archive, one database, one publication.
pub async fn fixture() -> Fixture {
    let archive =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let db = get_db(archive.path()).await;

    insert_fonds(&db, FONDS).await;
    insert_fonds(&db, OTHER_FONDS).await;
    insert_publication(
        &db,
        PUB_CURRENT_ID,
        PUB_CURRENT,
        PUB_CURRENT,
        FONDS,
        false,
        Some("test_org/law-html"),
    )
    .await;
    for (pvid, date) in [
        (PV_EARLY, DATE_EARLY),
        (PV_MAIN, DATE_MAIN),
        (PV_LATE, DATE_LATE),
    ] {
        insert_version(&db, date).await;
        insert_publication_version(&db, pvid, date, PUB_CURRENT_ID).await;
        insert_publication_has_publication_version(&db, PUB_CURRENT_ID, pvid).await;
    }

    let rows = fixture_data::elements();
    let doc_ids: BTreeSet<&str> = rows.iter().map(|row| row.doc_id.as_str()).collect();
    for doc_id in doc_ids {
        insert_document(&db, doc_id).await;
    }
    insert_document_elements(&db, &rows).await;
    insert_document_changes(&db, &fixture_data::changes()).await;
    insert_resolutions(&db).await;

    Fixture { db, archive }
}

/// The resolutions collection: library rows plus the bridge rows that
/// deliberately disagree with the element tree.
///
/// `changed_library_document` records only elements that changed as part of a
/// library change -- for San Mateo always the document root, never anything
/// deeper -- so the bridge under-reports children. The three members cover the
/// three outcomes:
///
/// | member | bridge | element tree | tree mode must say |
/// |---|---|---|---|
/// | `Res. No. 2026-01` | root only | two depth-2 rows | true |
/// | `Res. No. 2026-02` | root only | nothing deeper | false |
/// | `Res. No. 2026-03` | a depth-2 row | nothing deeper in this fonds | false |
async fn insert_resolutions(db: &DatabaseConnection) {
    insert_library(db, RESOLUTIONS_MPATH, RESOLUTIONS_URL, FONDS).await;
    insert_library(
        db,
        &format!("{RESOLUTIONS_MPATH}2026|"),
        &format!("{RESOLUTIONS_URL}/2026"),
        FONDS,
    )
    .await;
    insert_library_change(db, PV_MAIN, "0", RESOLUTIONS_MPATH).await;
    insert_library_change(db, PV_MAIN, "0", &format!("{RESOLUTIONS_MPATH}2026|")).await;
    for change_id in ["c-res-01", "c-res-02", "c-res-03-ghost"] {
        insert_changed_library_document(db, &format!("{RESOLUTIONS_MPATH}2026|"), change_id).await;
    }
}

// ---------------------------------------------------------------------------
// URI builders
// ---------------------------------------------------------------------------

/// `/_api/menu/_date/{date}{path}` with an optional `?depth=` tail. `depth` is
/// a `&str` so the malformed cases go through the same builder.
pub fn dated_uri(date: &str, path: &str, depth: Option<&str>) -> String {
    let base = format!("/_api/menu/_date/{date}{path}");
    depth.map_or(base.clone(), |value| format!("{base}?depth={value}"))
}

/// `/_api/menu/_tree{path}` -- the undated browse mode.
pub fn tree_uri(path: &str, depth: Option<&str>) -> String {
    let base = format!("/_api/menu/_tree{path}");
    depth.map_or(base.clone(), |value| format!("{base}?depth={value}"))
}

// ---------------------------------------------------------------------------
// JSON navigation
// ---------------------------------------------------------------------------

/// Direct children of a node.
pub fn children(node: &Value) -> &[Value] {
    node["children"].as_array().map_or(&[], Vec::as_slice)
}

/// The child with this `segment`. Panics with the available segments.
pub fn child<'node>(node: &'node Value, segment: &str) -> &'node Value {
    children(node)
        .iter()
        .find(|child| child["segment"] == Value::from(segment))
        .unwrap_or_else(|| {
            panic!(
                "no child {segment:?} under {:?}; children are {:?}",
                node["mpath"],
                child_segments(node)
            )
        })
}

/// `segment` of every direct child, in response order.
pub fn child_segments(node: &Value) -> Vec<String> {
    children(node)
        .iter()
        .map(|child| child["segment"].as_str().unwrap_or("<missing>").to_owned())
        .collect()
}

/// `statuses` of a node, as owned strings.
pub fn statuses(node: &Value) -> Vec<String> {
    node["statuses"]
        .as_array()
        .unwrap_or_else(|| panic!("node {:?} has no statuses array", node["mpath"]))
        .iter()
        .map(|status| status.as_str().unwrap_or("<not a string>").to_owned())
        .collect()
}

/// `hasChildren` of a node.
pub fn has_children(node: &Value) -> bool {
    node["hasChildren"]
        .as_bool()
        .unwrap_or_else(|| panic!("node {:?} has no hasChildren bool", node["mpath"]))
}

/// Every `mpath` in the subtree rooted at `node`, including `node` itself.
pub fn all_mpaths(node: &Value) -> Vec<String> {
    let mut found = vec![node["mpath"].as_str().unwrap_or("<missing>").to_owned()];
    for child in children(node) {
        found.extend(all_mpaths(child));
    }
    found
}

/// Whether `mpath` appears anywhere in the subtree.
pub fn contains_mpath(node: &Value, mpath: &str) -> bool {
    all_mpaths(node).iter().any(|found| found == mpath)
}

/// The node with this `mpath`, searched depth-first.
pub fn find_mpath<'node>(node: &'node Value, mpath: &str) -> Option<&'node Value> {
    if node["mpath"] == Value::from(mpath) {
        return Some(node);
    }
    children(node)
        .iter()
        .find_map(|child| find_mpath(child, mpath))
}
