//! The node ceiling: truncation is breadth-first, so shallow levels survive.

use crate::archive_testtools::config::{ArchiveType, Jurisdiction};
use crate::common::{
    self,
    db_data::{
        insert_document, insert_document_changes, insert_document_elements, insert_fonds,
        insert_publication, insert_publication_has_publication_version, insert_publication_version,
        insert_version, ChangeRow, ElementRow,
    },
    get_db,
};
use actix_web::http::StatusCode;

use super::{
    child, child_segments, dated_uri, has_children, DATE_MAIN, FONDS, PUB_CURRENT_ID, PV_MAIN,
};

/// The constant from `menu::MAX_MENU_NODES`, duplicated on purpose: importing
/// it could not catch the constant itself being changed.
const MAX_MENU_NODES: i64 = 5_000;

/// Document id of the oversized fixture.
const WIDE_DOC: &str = "Wide Doc";
/// URL of its root.
const WIDE_URL: &str = "/wide";
/// Depth-2 children.
const BRANCHES: usize = 100;
/// Depth-3 children per branch: `1 + 100 + 100*60 = 6101` nodes at `depth=2`.
const LEAVES: usize = 60;

/// One very wide, very busy document in its own archive.
async fn wide_fixture() -> super::Fixture {
    let archive =
        common::initialize_archive_without_bare(ArchiveType::Basic(Jurisdiction::Single)).unwrap();
    let db = get_db(archive.path()).await;

    insert_fonds(&db, FONDS).await;
    insert_publication(
        &db,
        PUB_CURRENT_ID,
        super::PUB_CURRENT,
        super::PUB_CURRENT,
        FONDS,
        false,
        Some("test_org/law-html"),
    )
    .await;
    insert_version(&db, DATE_MAIN).await;
    insert_publication_version(&db, PV_MAIN, DATE_MAIN, PUB_CURRENT_ID).await;
    insert_publication_has_publication_version(&db, PUB_CURRENT_ID, PV_MAIN).await;
    insert_document(&db, WIDE_DOC).await;

    let mut elements = vec![ElementRow::new(
        &format!("{WIDE_DOC}|"),
        WIDE_URL,
        WIDE_DOC,
        FONDS,
    )];
    let mut changes = Vec::new();
    for branch in 0..BRANCHES {
        let branch_seg = format!("{branch:03}");
        elements.push(ElementRow::new(
            &format!("{WIDE_DOC}|{branch_seg}|"),
            &format!("{WIDE_URL}/{branch_seg}"),
            WIDE_DOC,
            FONDS,
        ));
        for leaf in 0..LEAVES {
            let leaf_seg = format!("{leaf:02}");
            let mpath = format!("{WIDE_DOC}|{branch_seg}|{leaf_seg}|");
            elements.push(ElementRow::new(
                &mpath,
                &format!("{WIDE_URL}/{branch_seg}/{leaf_seg}"),
                WIDE_DOC,
                FONDS,
            ));
            changes.push(ChangeRow::new(
                &format!("c-{branch_seg}-{leaf_seg}"),
                super::CHANGED,
                PV_MAIN,
                &mpath,
            ));
        }
    }
    insert_document_elements(&db, &elements).await;
    insert_document_changes(&db, &changes).await;

    super::Fixture { db, archive }
}

/// Over the cap: `truncated: true`, capped at the constant, every depth-2 node
/// still present, and the deepest level consumed in sorted order.
#[actix_web::test]
async fn test_menu_node_ceiling_truncates_breadth_first() {
    let fix = wide_fixture().await;
    let app = common::initialize_app(fix.archive.path()).await;

    let resp = super::get(&app, &dated_uri(DATE_MAIN, WIDE_URL, Some("2"))).await;
    assert_eq!(resp.status, StatusCode::OK, "body: {}", resp.body);
    assert_eq!(resp.json()["truncated"], true);
    assert_eq!(resp.json()["nodeCount"], MAX_MENU_NODES);
    assert_eq!(child_segments(resp.root()).len(), BRANCHES);

    assert_eq!(child_segments(child(resp.root(), "000")).len(), LEAVES);
    let last = child(resp.root(), "099");
    assert_eq!(child_segments(last).len(), 0);
    assert!(has_children(last));

    // A request that fits is not flagged.
    let fits = super::get(&app, &dated_uri(DATE_MAIN, WIDE_URL, Some("1"))).await;
    assert_eq!(fits.json()["truncated"], false);
    assert_eq!(fits.json()["nodeCount"], 1 + BRANCHES as i64);
    drop(app);
}
