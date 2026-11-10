//! The fixture tables. Nothing here runs a test; it is data.

use super::{
    ADDED, CHANGED, CODE_DOC, CODE_URL, FONDS, MOHICAN_DOC, MOHICAN_URL, OTHER_FONDS, PV_EARLY,
    PV_LATE, PV_MAIN, RESOLUTIONS_URL,
};
use crate::common::db_data::{ChangeRow, ElementRow};

/// Shorthand for one `document_element` row in the root fonds.
fn elem(mpath: &str, url: &str, doc_id: &str) -> ElementRow {
    ElementRow::new(mpath, url, doc_id, FONDS)
}

/// Every `document_element` row in the shared fixture.
pub fn elements() -> Vec<ElementRow> {
    let code = |suffix: &str, url_tail: &str| {
        elem(
            &format!("{CODE_DOC}|{suffix}"),
            &format!("{CODE_URL}{url_tail}"),
            CODE_DOC,
        )
    };
    let mut rows = vec![
        // San Mateo code. Title 1 changes only on DATE_EARLY, Title 5 never
        // changes, Title 10's sections change on DATE_MAIN at depth 4.
        code("", ""),
        code("1|", "/1"),
        code("5|", "/5"),
        code("5|5.04|", "/5.04"),
        code("10|", "/10"),
        code("10|10.04|", "/10.04"),
        code("10|10.04|10.04.010|", "/10.04.010"),
        code("10|10.100|", "/10.100"),
        code("10|10.100|10.100.010|", "/10.100.010"),
        // Mohican: depth-1 mpath behind a seven-segment URL. The child element
        // exists but never changes.
        elem(&format!("{MOHICAN_DOC}|"), MOHICAN_URL, MOHICAN_DOC),
        elem(
            &format!("{MOHICAN_DOC}|decision|"),
            &format!("{MOHICAN_URL}#decision"),
            MOHICAN_DOC,
        ),
        // `_` is a LIKE wildcard, so the pattern `A_B|%` also matches `AXB|`.
        elem("A_B|", "/prefix/a_b", "A_B"),
        elem("A_B|child|", "/prefix/a_b/child", "A_B"),
        elem("AXB|", "/prefix/axb", "AXB"),
        elem("AXB|leak|", "/prefix/axb/leak", "AXB"),
    ];
    rows.extend(sort_elements());
    rows.extend(resolution_elements());
    rows
}

/// Sibling sets for the natural-sort test: titles at depth 2, chapters at 3.
fn sort_elements() -> Vec<ElementRow> {
    let node = |suffix: &str, url_tail: &str| {
        elem(
            &format!("Sort Doc|{suffix}"),
            &format!("/sort{url_tail}"),
            "Sort Doc",
        )
    };
    let mut rows = vec![node("", "")];
    for title in ["1", "2", "10", "27"] {
        rows.push(node(&format!("{title}|"), &format!("/{title}")));
    }
    for chapter in ["10.04", "10.100"] {
        rows.push(node(&format!("10|{chapter}|"), &format!("/10/{chapter}")));
    }
    rows
}

/// Member documents of the resolutions collection; see `insert_resolutions`.
fn resolution_elements() -> Vec<ElementRow> {
    vec![
        elem(
            "Res. No. 2026-01|",
            &format!("{RESOLUTIONS_URL}/2026/1"),
            "Res. No. 2026-01",
        ),
        elem(
            "Res. No. 2026-01|1|",
            &format!("{RESOLUTIONS_URL}/2026/1#1"),
            "Res. No. 2026-01",
        ),
        elem(
            "Res. No. 2026-01|2|",
            &format!("{RESOLUTIONS_URL}/2026/1#2"),
            "Res. No. 2026-01",
        ),
        elem(
            "Res. No. 2026-02|",
            &format!("{RESOLUTIONS_URL}/2026/2"),
            "Res. No. 2026-02",
        ),
        elem(
            "Res. No. 2026-03|",
            &format!("{RESOLUTIONS_URL}/2026/3"),
            "Res. No. 2026-03",
        ),
        // The only element below 2026-03 belongs to another fonds, so a client
        // of the root fonds has nothing to expand into.
        ElementRow::new(
            "Res. No. 2026-03|ghost|",
            "/other-fonds/resolutions/2026/3#ghost",
            "Res. No. 2026-03",
            OTHER_FONDS,
        ),
    ]
}

/// Every `document_change` row in the shared fixture.
pub fn changes() -> Vec<ChangeRow> {
    let code = |mpath_tail: &str| format!("{CODE_DOC}|{mpath_tail}");
    let mut rows = vec![
        ChangeRow::new("c-early-1", CHANGED, PV_EARLY, &code("1|")),
        ChangeRow::new(
            "c-main-10-04-010",
            CHANGED,
            PV_MAIN,
            &code("10|10.04|10.04.010|"),
        ),
        ChangeRow::new(
            "c-main-10-100-010",
            CHANGED,
            PV_MAIN,
            &code("10|10.100|10.100.010|"),
        ),
        ChangeRow::new("c-main-a_b", CHANGED, PV_MAIN, "A_B|child|"),
        ChangeRow::new("c-main-axb", CHANGED, PV_MAIN, "AXB|leak|"),
        ChangeRow::new(
            "c-late-mohican",
            CHANGED,
            PV_LATE,
            &format!("{MOHICAN_DOC}|"),
        ),
        // Resolutions: the first two bridge only their own root, which is the
        // real San Mateo shape; the third bridges a depth-2 element.
        ChangeRow::new("c-res-01", ADDED, PV_MAIN, "Res. No. 2026-01|"),
        ChangeRow::new("c-res-02", ADDED, PV_MAIN, "Res. No. 2026-02|"),
        ChangeRow::new(
            "c-res-03-ghost",
            CHANGED,
            PV_MAIN,
            "Res. No. 2026-03|ghost|",
        ),
    ];
    for title in ["1", "2", "10", "27"] {
        rows.push(ChangeRow::new(
            &format!("c-sort-{title}"),
            CHANGED,
            PV_MAIN,
            &format!("Sort Doc|{title}|"),
        ));
    }
    for chapter in ["10.04", "10.100"] {
        rows.push(ChangeRow::new(
            &format!("c-sort-10-{chapter}"),
            CHANGED,
            PV_MAIN,
            &format!("Sort Doc|10|{chapter}|"),
        ));
    }
    rows
}
