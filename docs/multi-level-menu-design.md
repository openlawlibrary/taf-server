# Multi-level (cascading) menu endpoint — design (REVISED, supersedes v1)

---

## ADDENDUM — coordinator decisions, measurements and product-owner answers

**This section supersedes anything it contradicts below. Read it first.**

### A1. Product-owner answers (received after the revision was written)

| Question | Answer |
|---|---|
| Undated / tree mode (§1.2, risk 1) | **Support BOTH modes.** Build dated mode as specified, AND tree mode behind the explicit `/_api/menu/_tree/{path:.*}` route the design itself proposes. Date-absence on a non-`_tree` route stays a 400 naming both legal forms. |
| Date range / `compareDate` (risk: not raised) | **Single date only for this phase.** No `_compare` variants. Purely additive later. |
| Removed / repealed nodes (risk 3) | **Return them with `removed` visible in `statuses`.** Matches the design's recommendation. Server stays presentation-neutral. |
| Does `/menu` also return the date list? | **No.** The frontend keeps calling `/_api/versions` for the dropdown. No duplication of publication/display-string logic. |
| Attachments (risk 4) | **Return them**, `ptag` is in the response, frontend filters. Design recommendation accepted, no change. |

Tree mode is a real deliverable, not optional. It reinstates v1's node query — a BINARY range scan on `document_element(doc_mpath)` bounded by `LENGTH(x)-LENGTH(REPLACE(x,'|',''))`, plus one extra level to compute `hasChildren` — sets `"mode": "tree"`, and keeps labels, natural sort, nesting and the depth clamp identical. In tree mode `hasChildren` reverts to "has any child in the tree", and nodes carry no `statuses` (there is no selected version); emit `statuses: []`. Keep the two drivers in separate functions; do not branch inside one query builder.

### A2. Per-date measurements — closes §0.2

Run by the coordinator against `C:\OLLGIT\lib-dev\.taf\db.sqlite3` (stdlib `sqlite3` via `python -I`; no `sqlite3` CLI on this box). Change rows per version date, scoped to the latest non-revoked publication:

| Fonds | Publication | Dates | Bootstrap date | Every other date | Median |
|---|---|---|---|---|---|
| `maryland-dsd/law` (`Code of Maryland Regulations|%`) | 2025-11-12 | 8 | **2025-08-22 → 33,877 rows** | ≤ 107 | 87 |
| `cityofsanmateo/law` (`City of San Mateo, Cal., Code|%`) | 2026-04-21 | 68 | **2018-03-05 → 2,770 rows** | ≤ 89 | 7 |

The design's structural hypothesis is **confirmed for Maryland**: the library bootstrap date holds 99% of the publication's change rows (33,877 of 34,328); 6 of 8 dates are ≤ 100 rows.

End-to-end timing of Query 1 (§5.2) as written, best of 3:

| Case | Rows | Time |
|---|---|---|
| Maryland bootstrap `2025-08-22` | 33,877 | 21.1 ms |
| Maryland typical `2025-09-29` | 62 | < 0.1 ms |
| San Mateo `2026-02-03` | 24 | < 0.1 ms |
| San Mateo deep base `Code\|27\|` on `2026-02-03` | 4 | < 0.1 ms |

### A3. Migration §3.2 is CANCELLED — do not write it

`20261115120100_add_publication_version_version_idx` must **not** be created. Measured on a consistent backup copy of the dev DB, with `ANALYZE` run before each pass:

```
WITHOUT the index   MD bootstrap 33,877 rows  21.1 ms
  EQP: SEARCH phpv USING COVERING INDEX ...(publication_id=?)
     -> SEARCH pv USING INDEX publication_version_id_idx (id=?)
     -> SEARCH dc USING INDEX document_change_publication_version_idx (publication_version_id=?)

WITH    the index   MD bootstrap 33,877 rows  19.4 ms
  EQP: SEARCH pv USING INDEX publication_version_version_idx (version=?)
     -> SEARCH phpv USING COVERING INDEX ...(publication_id=? AND publication_version_id=?)
     -> SEARCH dc USING INDEX document_change_publication_version_idx (publication_version_id=?)
```

The index does change the plan — it makes `pv.version` the driver, exactly as §5.2 predicted. It buys **1.7 ms on the single worst row-count in the entire dev archive, and nothing at all on every other date** (both 0.0 ms either way). That does not justify an index that every ingest must maintain. Ship **one** migration: §3.1 (`cite`/`ptag`).

The pre-approved remedy index `document_change_pv_mpath_status_idx` in §5.2 is likewise **not** to be added. The plan already drives through `document_change_publication_version_idx`, and the worst case is 21 ms.

### A4. Node ceiling (§5.5) — RESOLVED, cap confirmed

The product owner was shown the measured bootstrap-date figures and **confirmed `MAX_MENU_NODES = 5_000`**, superseding their earlier "no breadth cap" (which was decided against the tree-first design, where tree width bounded breadth). Implement exactly as §5.5 specifies: applied after natural sorting, breadth-first so shallow levels are always complete, with `"truncated": true` surfaced in the response when it fires. Keep the limit as a single named constant.

### A4b. Re-ingest of `cite`/`ptag` — DO NOT RUN

The product owner has asked to be consulted before anything rewrites `C:\OLLGIT\lib-dev\.taf\db.sqlite3`. **No agent may run `cargo run ... update` against `C:/OLLGIT/lib-dev`, with or without `--force`.** Verify against a throwaway copy made with python's `sqlite3` backup API in the scratchpad directory. The coordinator will ask the owner when the code is ready.

Note the ceiling is load-bearing in **tree** mode too, and more so: an undated `depth=6` request at the Maryland root returns the whole 33,912-node tree. In dated mode only the bootstrap date approaches it.

### A5. Unchanged from the design

§3.1 migration, §4 ingest in full (including the empty-vs-absent `cite` normalisation and the `INSERT OR IGNORE` → upsert fix), §4.6 `Status::from_int`/`as_api_str` plus the declaration-order trap, §5.1 mpath helpers, §5.3 node-set synthesis, §5.4 IN-list label lookup, §7 natural sort, §10 lint traps.

---

Persist as `C:\OLLGIT\taf-server\docs\multi-level-menu-design.md`, replacing the previous version entirely.

**What changed from v1 and why.** The product owner's correction inverts the feature: the menu is **date-first**, not tree-first. The top level is the existing version-date picker; hovering a date expands into the subtree of elements that **changed in that version**, cascading down levels (date → titles that changed → chapters that changed → sections that changed). Consequently `date` is the primary SQL filter, the node set is derived from `document_change` rather than `document_element`, per-node date lists are gone, and `hasChildren` now means "has changed descendants in this version". §§5-6, 8-9 are rewritten. §3 (migration), §4 (ingest, `Status`), §7 (natural sort), the mpath helpers and §10 (lint traps) carry forward.

Sources read in the repo: `src\server\api\versions\mod.rs`, `src\server\api\routes.rs`, `src\db\models\document_change\manager.rs`, `src\db\models\document_element\manager.rs`, `src\db\models\library\manager.rs`, `src\db\models\library_change\manager.rs`, `src\db\models\changed_library_document\mod.rs`, `src\db\models\status\mod.rs`, `src\db\models\mod.rs`, `src\history\changes.rs`, `src\history\rdf\namespaces.rs`, `src\history\rdf\graph.rs`, `migrations\sqlite\20240115152953_initial_db.up.sql`, `src\fonds\archive.rs`, `src\lib.rs`.

---

## 0. Measurements

### 0.1 Measured by me, this session

I have no shell in this agent (Read/Grep/Glob only), so I measured the RDF source rather than the DB. The RDF is the DB's input, and the two agree to within one row, which cross-validates both.

San Mateo, publication `2026-04-21`, document `code`: **68 version dates** (Glob over `_publication\2026-04-21\us\ca\cities\san-mateo\code\_version\*\index.rdf` — matches `CLAUDE.md`'s "all 68 versions"), **3,681 `oll:documentMaterializedPath` change blocks in total**. The coordinator's SQL measured **3,682 `document_change` rows** for `doc_mpath LIKE 'City of San Mateo, Cal., Code|%'` scoped to one publication. 1:1 agreement.

Per-date distribution (the number that actually matters now):

| Version date | Change blocks | Note |
|---|---|---|
| **2018-03-05** | **2,769** | library start date — bootstrap of the whole code |
| 2019-12-18 | 89 | busiest *real* date |
| 2022-01-05 | 80 | |
| 2025-12-03 | 79 | |
| 2025-05-07 | 56 | |
| 2024-03-21 | 50 | |
| 2023-01-01 | 44 | |
| 2021-07-01 | 43 | |
| 2026-02-03 | 24 | the `CLAUDE.md` both-reasons fixture |
| 2025-02-20 | 7 | |
| median of all 68 | ~7 | |
| 24 of the 68 dates | ≤ 5 | |

**The structural finding: the whole-publication figure (3,682 rows) collapses to ≤ 89 rows for every date except one.** The library start date alone holds 75% of the subtree's change rows. The worst case in dated mode is therefore not "a busy date" — it is **the library bootstrap date, and there is exactly one of those per library**.

### 0.2 NOT measured — must be run before merge

Maryland per-date counts. The coordinator measured 34,328 rows for the whole publication at 123 ms; the per-date breakdown is unknown. If San Mateo's shape holds, the Maryland bootstrap date dominates and typical dates are small, but **do not assume it — measure it**. There is no `sqlite3` CLI on this box; use stdlib `sqlite3` via python, and run it with `-I` (isolated) per the environment rules:

```powershell
python -I -c @"
import sqlite3
db = sqlite3.connect(r'C:\OLLGIT\lib-dev\.taf\db.sqlite3')
cur = db.cursor()
for fonds, prefix in (('maryland-dsd/law', 'Code of Maryland Regulations|%'),
                      ('cityofsanmateo/law', 'City of San Mateo, Cal., Code|%')):
    pub = cur.execute(
        'SELECT id, name FROM publication WHERE fonds=? AND revoked=0 ORDER BY name DESC LIMIT 1',
        (fonds,)).fetchone()
    print('===', fonds, pub)
    rows = cur.execute('''
        SELECT pv.version, COUNT(*) AS n
        FROM document_change dc
        JOIN publication_has_publication_versions phpv
          ON phpv.publication_version_id = dc.publication_version_id
        JOIN publication_version pv ON pv.id = phpv.publication_version_id
        WHERE dc.doc_mpath LIKE ? AND phpv.publication_id = ?
        GROUP BY pv.version ORDER BY n DESC LIMIT 15
    ''', (prefix, pub[0])).fetchall()
    for row in rows:
        print(row)
    print('total dates:', cur.execute('''
        SELECT COUNT(DISTINCT pv.version)
        FROM document_change dc
        JOIN publication_has_publication_versions phpv
          ON phpv.publication_version_id = dc.publication_version_id
        JOIN publication_version pv ON pv.id = phpv.publication_version_id
        WHERE dc.doc_mpath LIKE ? AND phpv.publication_id = ?
    ''', (prefix, pub[0])).fetchone()[0])
"@
```

Also required before merge: `EXPLAIN QUERY PLAN` for Query 1 (§5.2) both with and without the new `publication_version_version_idx`, pasted into the PR.

---

## 1. Endpoint contract

### 1.1 Name

**`/_api/menu`**, handler `menu()`. The name still fits — arguably better than before, since this is literally the hover-expansion of the existing version picker. `/_api/changes` was considered and rejected: it would collide conceptually with `document_change` and implies a changelog, not a navigable tree.

### 1.2 Mode: date is required, never inferred

The endpoint has exactly one mode in this phase: **dated**. A request without a date is a **400**, never a fallback to the whole tree.

This is a deliberate contract, not an oversight. The failure mode to design out is "date omitted → server quietly returns the entire 34k-node tree". Making absence an error eliminates it by construction, and makes any future expensive mode an explicit, greppable opt-in rather than a default.

**Answer to the open question the coordinator is putting to the product owner** (design is ready for either outcome):

- *If the PO says a date is always required* — ship exactly what is specified below. Nothing to add.
- *If the PO also wants undated whole-tree browsing* — add it as a **separately named route segment, never as date-absence**: `/_api/menu/_tree/{path:.*}`. It would reinstate v1's node query (a BINARY range scan on `document_element(doc_mpath)` bounded by `LENGTH(x)-LENGTH(REPLACE(x,'|',''))`, plus one extra level for `hasChildren`), set `"mode": "tree"`, and keep everything else — labels, natural sort, nesting, depth clamp — identical. Bare `/_api/menu/{path}` with neither `_date` nor `_tree` stays a 400 that names both legal forms.

**Recommendation: ship dated mode only.** The two modes have different drivers, different queries, different `hasChildren` semantics and different performance profiles; one handler serving both silently is two features wearing one name. If tree mode is wanted, it is ~120 lines added later behind its own route, with no change to the dated contract.

### 1.3 Routes and parameters

```
GET|HEAD /_api/menu/_publication/{publication}/_date/{date}/{path:.*}
GET|HEAD /_api/menu/_publication/{publication}/_date/{date}
GET|HEAD /_api/menu/_date/{date}/{path:.*}
GET|HEAD /_api/menu/_date/{date}
GET|HEAD /_api/menu/_publication/{publication}/{path:.*}   -> 400, no date
GET|HEAD /_api/menu/_publication/{publication}             -> 400, no date
GET|HEAD /_api/menu/{path:.*}                              -> 400, no date
GET|HEAD /_api/menu                                        -> 400, no date
```

The dateless variants are registered deliberately so the client gets a 400 naming the legal form, rather than an opaque actix 404.

| Parameter | Where | Type | Default | Role |
|---|---|---|---|---|
| `date` | path, after `_date` | `YYYY-MM-DD` or `current` | **required** | **Primary filter.** Enters the SQL predicate as `pv.version = ?`. |
| `path` | path tail | string | `""` | URL of the subtree root. Normalised by the existing `clean_url_path`. |
| `publication` | path, after `_publication` | string | current publication | Resolved exactly as `versions()` does: lowercase match over `find_all_non_revoked_publications`, `"current"` aliases the newest. |
| `depth` | **query** | `u32` | `2` | Relative levels below the root. Clamped to `[1, 6]`. |
| `X-Fonds` | header | string | root fonds | Reuse `get_fonds_from_request()` verbatim. |

`compareDate` and the `_compare/...` route variants are **not** part of this endpoint. Comparison is a diff feature; if it is ever wanted here it is purely additive.

`date` is a path segment, not a query param, to mirror `/_api/versions/_date/{date}/{path}` and `/_api/versions/_publication/{p}/_date/{d}/{path}` — the frontend already constructs those URLs. `depth` must stay a query param because `{path:.*}` greedily consumes the entire tail. `web::Query` is already in use at `C:\OLLGIT\taf-server\src\server\api\archive\mod.rs:56`.

### 1.4 Date resolution

1. `date == "current"` (the `CURRENT_VERSION_DATE` sentinel from `versions/mod.rs:31`) → resolve to the publication's newest version date via a new tiny manager method `find_latest_version_for_publication(publication_id) -> anyhow::Result<Option<String>>`:
   ```sql
   SELECT MAX(pv.version) AS codified_date
   FROM publication_has_publication_versions phpv
   JOIN publication_version pv ON pv.id = phpv.publication_version_id
   WHERE phpv.publication_id = $1
   ```
   `None` → 404.
2. Otherwise parse with `NaiveDate::parse_from_str(date, "%Y-%m-%d")`. Parse failure → **400**.
3. Verify the version exists in this publication:
   ```sql
   SELECT 1
   FROM publication_has_publication_versions phpv
   JOIN publication_version pv ON pv.id = phpv.publication_version_id
   WHERE phpv.publication_id = $1 AND pv.version = $2
   LIMIT 1
   ```
   No row → **404** `"No version {date} in publication {name}"`.

Step 3 matters: it separates *"that date does not exist here"* (404) from *"that date exists but nothing under this path changed"* (200 with an empty root). Without it the frontend cannot tell a typo'd date from a quiet one. Steps 1 and 3 collapse into a single round trip when `date == "current"`.

### 1.5 Response shape

```jsonc
{
  "path": "us/ca/cities/san-mateo/code",
  "activePublication": "current",
  "mode": "date",
  "date": "2026-02-03",
  "display": "February 03, 2026",
  "rootKind": "document",          // "document" | "collection"
  "depth": 2,                      // levels ACTUALLY served, post-clamp
  "requestedDepth": 2,
  "maxDepth": 6,
  "nodeCount": 32,   // measured against the real archive; an earlier draft said 17
  "truncated": false,
  "root": { /* Node */ }
}
```

`Node`:

```jsonc
{
  "mpath": "City of San Mateo, Cal., Code|10|",
  "segment": "10",
  "cite": "Title 10",
  "ptag": "container",
  "docId": "City of San Mateo, Cal., Code",
  "url": "/us/ca/cities/san-mateo/code/10",
  "depth": 2,
  "label": "Title 10",
  "statuses": ["changed"],
  "hasChildren": true,
  "children": [ /* Node, ... */ ]
}
```

**The per-node `versions` array from v1 is gone.** The date is fixed by the request, so it belongs once at the top level, not repeated on every node. Each node instead carries `statuses` — the status(es) that node has **in that one version**.

Nested rather than flat, for the same reasons as v1: a cascading menu renders nesting directly, a hover lazy-load replaces one node's `children` wholesale, and the N-level bound is self-evident.

`depth` on a node is the **absolute** mpath depth, not relative. This is required because the tree is not uniform: San Mateo sections sit at depth 4 under most titles but at depth 5 under `Code|27|27.21|II|`, and Maryland reaches depth 6 at `…|19A|04|attachments|A|appendix-a|`. **No code path may assume "depth N == sections".**

### 1.6 `statuses` — exact semantics

`statuses` holds **only the statuses of change rows whose `doc_mpath` equals this node's mpath**, in this version. Descendant statuses do **not** bubble up.

- `["changed"]` — this element itself changed in this version.
- `["added", "effective"]` — both on one element, which is real: the library bootstrap date carries `Element added` + `Element effective` together (`CLAUDE.md`), and `document_change`'s PK is `md5(pub_version_hash + doc_mpath + status)`, so two statuses are two rows.
- `[]` — **this node appears only because a descendant changed.** This is the common case for titles and chapters. It is meaningful and must be representable; do not collapse it to `null` and do not omit the field.

Deduplicated and ordered ascending by `Status::to_int()`, so always `["added","effective"]`, never `["effective","added"]`.

### 1.7 `hasChildren` — exact semantics, and the subtle bug to avoid

**`hasChildren` = "this node has at least one changed descendant in this version."** It is *not* "this node has any children in the tree".

A title with 40 sections, none of which changed on the hovered date, is simply **absent from the response**. If it is present at all, something under it changed.

The lazy-load rule for the frontend is uniform and falls out of this one definition:

| `hasChildren` | `children` | Meaning |
|---|---|---|
| `false` | `[]` | Change leaf. Nothing deeper changed. Terminal. |
| `true` | non-empty | Changed descendants, already included. Render them. |
| `true` | `[]` | **Depth frontier.** Changed descendants exist below what was served — fetch deeper on hover. |

So the client rule is exactly: *if `hasChildren && children.is_empty()`, request the next slice.*

Two traps the implementer must not fall into:

1. Do **not** compute `hasChildren` from `document_element` (v1 did). In dated mode that would be true for every container regardless of whether anything changed, and the menu would offer hover-expansions that return empty.
2. `hasChildren` and `statuses` are independent. A title can both have changed itself and have changed sections: `statuses: ["changed"], hasChildren: true`. A node can also have `statuses: [], hasChildren: true` (pure ancestor) or `statuses: ["changed"], hasChildren: false` (change leaf).

### 1.8 Labels — four raw fields plus one advisory (carried forward unchanged)

The response carries `cite`, `segment`, `mpath` and `url` as **separate independent fields**, because verified data makes any single one insufficient:

- San Mateo: `cite` is ideal — `Title 10` / `Chapter 10.04` / `Section 10.04.010`.
- Maryland: `cite` is **ambiguous across depths** — `Code of Maryland Regulations|01|`, `…|01|02|` and `…|01|01|1971|` all carry `cite='01'`. A menu labelled from `cite` alone renders Maryland as a column of identical `01` entries.
- `segment` (the last mpath segment) distinguishes them: `01`, `02`, `1971`, `.11`. It also matches the trailing component of the URL.

`label` is **advisory, not authoritative**, computed at **response time** (never at ingest):

1. `cite` if non-null and non-blank, else
2. `segment`, else
3. `doc_id`.

Response-time rather than ingest-time keeps stored data honest — an un-reingested row and a genuinely blank cite stay distinguishable in the DB — and lets the policy change without a re-ingest.

### 1.9 Worked example — `/us/ca/cities/san-mateo/code`, date `2026-02-03`, depth 2

`GET /_api/menu/_date/2026-02-03/us/ca/cities/san-mateo/code?depth=2` with `X-Fonds: cityofsanmateo`.

That date has **24 change blocks** in the code subtree (measured, §0.1). Root is depth 1, so the window is depth 2 (titles) and depth 3 (chapters); the 24 changed sections at depth 4 are below the frontier and drive `hasChildren` on their chapters. Dates/cites are real; which specific titles changed is illustrative.

```jsonc
{
  "path": "us/ca/cities/san-mateo/code",
  "activePublication": "current",
  "mode": "date",
  "date": "2026-02-03",
  "display": "February 03, 2026",
  "rootKind": "document",
  "depth": 2,
  "requestedDepth": 2,
  "maxDepth": 6,
  "nodeCount": 32,   // measured against the real archive; an earlier draft said 17
  "truncated": false,
  "root": {
    "mpath": "City of San Mateo, Cal., Code|",
    "segment": "City of San Mateo, Cal., Code",
    "cite": null,
    "ptag": "document",
    "docId": "City of San Mateo, Cal., Code",
    "url": "/us/ca/cities/san-mateo/code",
    "depth": 1,
    "label": "City of San Mateo, Cal., Code",
    "statuses": [],
    "hasChildren": true,
    "children": [
      {
        "mpath": "City of San Mateo, Cal., Code|10|",
        "segment": "10",
        "cite": "Title 10",
        "ptag": "container",
        "docId": "City of San Mateo, Cal., Code",
        "url": "/us/ca/cities/san-mateo/code/10",
        "depth": 2,
        "label": "Title 10",
        "statuses": [],
        "hasChildren": true,
        "children": [
          {
            "mpath": "City of San Mateo, Cal., Code|10|10.04|",
            "segment": "10.04",
            "cite": "Chapter 10.04",
            "ptag": "container",
            "docId": "City of San Mateo, Cal., Code",
            "url": "/us/ca/cities/san-mateo/code/10.04",
            "depth": 3,
            "label": "Chapter 10.04",
            "statuses": ["changed"],
            "hasChildren": true,
            "children": []
          },
          {
            "mpath": "City of San Mateo, Cal., Code|10|10.100|",
            "segment": "10.100",
            "cite": "Chapter 10.100",
            "ptag": "container",
            "docId": "City of San Mateo, Cal., Code",
            "url": "/us/ca/cities/san-mateo/code/10.100",
            "depth": 3,
            "label": "Chapter 10.100",
            "statuses": [],
            "hasChildren": true,
            "children": []
          }
        ]
      },
      {
        "mpath": "City of San Mateo, Cal., Code|27|",
        "segment": "27",
        "cite": "Title 27",
        "ptag": "container",
        "docId": "City of San Mateo, Cal., Code",
        "url": "/us/ca/cities/san-mateo/code/27",
        "depth": 2,
        "label": "Title 27",
        "statuses": ["changed"],
        "hasChildren": true,
        "children": [
          {
            "mpath": "City of San Mateo, Cal., Code|27|27.21|",
            "segment": "27.21",
            "cite": "Chapter 27.21",
            "ptag": "container",
            "docId": "City of San Mateo, Cal., Code",
            "url": "/us/ca/cities/san-mateo/code/27.21",
            "depth": 3,
            "label": "Chapter 27.21",
            "statuses": [],
            "hasChildren": true,
            "children": []
          }
        ]
      }
    ]
  }
}
```

Read the example for the three distinct node kinds it demonstrates:
- `Code|` and `Title 10` — `statuses: []`, pure ancestors, present only because descendants changed.
- `Title 27` — `statuses: ["changed"]` **and** `hasChildren: true`: changed itself *and* has changed descendants.
- Every depth-3 chapter — `hasChildren: true, children: []`: the frontier. Hovering any of them triggers `?depth=1` (or `depth=2`) against that chapter's own path.

Titles 1-9, 11-26 are **absent entirely**. Nothing under them changed on 2026-02-03. That absence is the feature.

---

## 2. Route registration plan

In `C:\OLLGIT\taf-server\src\server\api\routes.rs`, inside the existing `/_api` scope, after the `/versions` scope service:

```rust
.service(
    web::scope("/menu")
        .service(
            web::resource("/_publication/{publication}/_date/{date}/{path:.*}")
                .route(web::get().to(menu))
                .route(web::head().to(menu)),
        )
        .service(
            web::resource("/_publication/{publication}/_date/{date}")
                .route(web::get().to(menu))
                .route(web::head().to(menu)),
        )
        .service(
            web::resource("/_date/{date}/{path:.*}")
                .route(web::get().to(menu))
                .route(web::head().to(menu)),
        )
        .service(
            web::resource("/_date/{date}")
                .route(web::get().to(menu))
                .route(web::head().to(menu)),
        )
        .service(
            web::resource("/_publication/{publication}/{path:.*}")
                .route(web::get().to(menu))
                .route(web::head().to(menu)),
        )
        .service(
            web::resource("/_publication/{publication}")
                .route(web::get().to(menu))
                .route(web::head().to(menu)),
        )
        .service(
            web::resource("/{path:.*}")
                .route(web::get().to(menu))
                .route(web::head().to(menu)),
        )
        .service(
            web::resource("")
                .route(web::get().to(menu))
                .route(web::head().to(menu)),
        ),
)
```

Extend the import at `routes.rs:21` to `use super::{menu::menu, serve::serve, state::Global, versions::versions};`.

**Registration order is load-bearing** and mirrors `/versions` exactly: `_publication` + `_date` first, then `_date`, then `_publication`, then `{path:.*}`, then `""`. Actix matches in registration order and `{path:.*}` would otherwise swallow `_date/...`.

Method guards are explicit `get`/`head`, matching the `_date` and `_publication` scopes at `routes.rs:98-135`. `/_api/versions` registers bare `.to(versions)` (all methods); it is the outlier, not the pattern. Mention this in the PR so the reviewer does not read it as an inconsistency.

The existing `.app_data(web::Data::new(state.clone()))` on the `/_api` chain already supplies `web::Data<AppState>`. No new app data.

---

## 3. Migrations

Two migration pairs, one per concern, so each is independently revertible.

### 3.1 Labels — `20261115120000_add_document_element_labels.{up,down}.sql`

Timestamp sorts after the newest committed migration (`20260323000000_add_html_data_repo_name`) **and** after the in-flight `20261030090000_add_document_build_reason` described in `CLAUDE.md`, so the branches merge without reordering.

`.up.sql`:
```sql
-- Add up migration script here
PRAGMA foreign_keys = OFF;

ALTER TABLE document_element
  ADD COLUMN cite TEXT;

ALTER TABLE document_element
  ADD COLUMN ptag TEXT;

PRAGMA foreign_keys = ON;
PRAGMA optimize;
```

`.down.sql`:
```sql
-- Add down migration script here
PRAGMA foreign_keys = OFF;

ALTER TABLE document_element DROP COLUMN ptag;
ALTER TABLE document_element DROP COLUMN cite;

PRAGMA foreign_keys = ON;
PRAGMA optimize;
```

Both columns nullable with no `DEFAULT`, so a DB ingested by older code keeps serving with a degraded label (`cite` NULL → `label` falls back to `segment`). `ADD COLUMN` with no non-constant default is O(1) metadata-only, which matters: `document_element` holds 34,093 rows for Maryland alone.

`DROP COLUMN` needs SQLite ≥ 3.35.0; sqlx 0.9 (`Cargo.toml:39`) bundles a far newer libsqlite3-sys. This is a *real* down, unlike the no-op `20260323000000_add_html_data_repo_name.down.sql` — deliberately, because an unreversible down makes the migration un-rerunnable in dev and these are leaf payload columns with no FK or index attached.

**No index on `cite` or `ptag`.** They never appear in a `WHERE`, `JOIN` or `ORDER BY` — they are projected payload only, and ordering is computed in Rust on the mpath segment (§7). The `COLLATE NOCASE` vs `BINARY` question is therefore moot for them.

### 3.2 Date lookup index — `20261115120100_add_publication_version_version_idx.{up,down}.sql`

**New in this revision**, and the direct consequence of `date` becoming the primary filter.

`.up.sql`:
```sql
-- Add up migration script here
PRAGMA foreign_keys = OFF;

CREATE INDEX IF NOT EXISTS publication_version_version_idx
  ON publication_version(version);

PRAGMA foreign_keys = ON;
PRAGMA optimize;
```

`.down.sql`:
```sql
-- Add down migration script here
PRAGMA foreign_keys = OFF;

DROP INDEX IF EXISTS publication_version_version_idx;

PRAGMA foreign_keys = ON;
```

**Justification — which query uses it.** Query 1 (§5.2) filters `pv.version = $3`. `20240115152953_initial_db.up.sql:95` creates only `publication_version_id_idx ON publication_version(id)`, which merely duplicates the PK — **there is currently no index on `publication_version.version` at all**. Without this index, `pv.version = ?` can only be a residual filter applied after rows are reached some other way, forcing SQLite to drive Query 1 from the `doc_mpath` prefix and scan the whole publication's change set (the 3,682 / 34,328-row shape) just to keep ≤ 89 of them. With it, SQLite can start from the handful of `publication_version` rows carrying that date, follow `document_change_publication_version_idx` to their change rows, and apply the prefix as a residual.

**Collation.** BINARY, i.e. no `COLLATE` clause. `pv.version` is a `YYYY-MM-DD` string compared with `=` against a bound parameter, and the column is declared without a collation, so the comparison is BINARY — a `COLLATE NOCASE` index could not serve it (the same trap `CLAUDE.md` records for `document_build_reason`, read in the same direction). Dates have no case, so NOCASE would also be semantically pointless.

Cost: `publication_version` holds one row per (publication, date, fonds) — thousands, not millions. The index is cheap to build and adds negligible ingest cost to `publication_version::TxManager::create`.

**The implementer must paste `EXPLAIN QUERY PLAN` for Query 1, before and after this index, into the PR.** If SQLite still prefers the prefix index, say so and keep the index anyway only if it demonstrably helps some shape; otherwise drop this migration rather than carry a dead index.

---

## 4. Ingest change (carried forward from v1, unchanged)

### 4.1 `src\history\rdf\namespaces.rs`

Add two predicates to the `oll` `namespace!` macro list, beside `url` since they come off the same change-bag item:

```rust
namespace! {
    "https://open.law/us/ngo/oll/_ontology/v0.1/ontology.owl#",
    CollectionVersion,
    DocumentVersion,
    docId,
    codifiedDate,
    lastValidPublication,
    lastValidCodifiedDate,
    hasChanges,
    documentMaterializedPath,
    url,
    cite,
    ptag,
    reason,
    status,
    libraryMaterializedPath,
    archivedHtml
}
```

`cite` and `ptag` are valid Rust identifiers, so the macro expands cleanly.

### 4.2 `src\history\changes.rs` — `insert_document_changes()`

Inside the `for change in changes.items()?` loop (currently lines 519-557), after the `url` binding (lines 525-526) and before `document_elements_bulk.push(...)`:

```rust
let cite = optional_label(pub_graph, &change, oll::cite);
let ptag = optional_label(pub_graph, &change, oll::ptag);
if cite.is_none() {
    missing_cite += 1;
}
if ptag.is_none() {
    missing_ptag += 1;
}
document_elements_bulk.push(DocumentElement::new(
    doc_mpath.clone(),
    url.clone(),
    doc_id.clone(),
    publication.fonds.clone(),
    cite,
    ptag,
));
```

with a private helper in the same file:

```rust
/// Read an optional presentation label off a change-bag item.
///
/// Returns `None` both when the predicate is absent entirely (Maryland document
/// roots) and when it is present but blank (`<oll:cite xml:lang="en"/>`, San Mateo
/// document roots). Storing `""` would make the column indistinguishable from a
/// genuinely-absent value at read time.
fn optional_label(
    pub_graph: &FondsGraph,
    change: &SimpleTerm<'_>,
    predicate: NsTerm<'_>,
) -> Option<String> {
    pub_graph
        .literal_from_triple_matching(Some(change), Some(predicate), None)
        .ok()
        .filter(|value| !value.trim().is_empty())
}
```

`missing_cite` / `missing_ptag` are `usize` counters declared beside `document_elements_bulk` (line 484), logged once after the outer loop:

```rust
tracing::debug!(
    "[{}] | Ingested document elements with no cite: {missing_cite}, no ptag: {missing_ptag}",
    publication.fonds
);
```

### 4.3 `.ok()` not `?` — rationale

The surrounding code already draws this line: `doc_mpath` (line 520), `url` (line 525) and `status` (line 536) use `?` because without them the row is structurally meaningless; `reason` (lines 533-535) uses `.ok()` because it is optional metadata. `cite` and `ptag` are presentation labels. A missing one must degrade a label, never abort ingest of an entire publication — ingest is incremental, so an abort leaves the fonds half-loaded. This is not silent swallowing: the per-publication `debug!` counter records it, and the documented fallback chain (§1.8) makes the degradation deterministic and visible.

**Two distinct absent shapes exist in real data and both must map to `None`:**
- predicate absent entirely → `literal_from_triple_matching` returns `Err` → `.ok()` → `None`. (Maryland document roots.)
- predicate present but empty → returns `Ok("")` → `.filter(...)` → `None`. (San Mateo document roots: 341 of 5,441 blocks, exactly the `ptag='document'` set.)

The `.filter()` is the single most forgettable line in this change. Flag it in review.

### 4.4 Model change

`C:\OLLGIT\taf-server\src\db\models\document_element\mod.rs`:

```rust
#[derive(sqlx::FromRow, Deserialize, Serialize)]
pub struct DocumentElement {
    pub doc_mpath: String,
    pub url: String,
    pub doc_id: String,
    pub fonds: String,
    /// Human citation for this element, e.g. "Title 10". `None` when the RDF
    /// omits `oll:cite` or carries it blank, and on rows written before this
    /// column existed.
    pub cite: Option<String>,
    /// Structural type from `oll:ptag`: section | container | document | attachment.
    pub ptag: Option<String>,
}
```

`DocumentElement::new` gains the two trailing params and stays `const fn`. **Grep for `DocumentElement::new` before editing** — the only production call site is `changes.rs:527`; update any test constructors in the same commit.

### 4.5 `INSERT OR IGNORE` is a trap — switch to upsert

`document_element::TxManager::insert_bulk` (`src\db\models\document_element\manager.rs:43`) uses `INSERT OR IGNORE`. On an already-ingested DB the rows exist, so a re-ingest would **skip them and never backfill `cite`/`ptag`**, defeating the `update --force` backfill the product owner specified:

```rust
let mut query_builder = QueryBuilder::new(
    "INSERT INTO document_element ( doc_mpath, url, doc_id, fonds, cite, ptag ) ",
);
for chunk in document_elements.chunks(BATCH_SIZE) {
    query_builder.push_values(chunk, |mut bindings, de| {
        bindings
            .push_bind(&de.doc_mpath)
            .push_bind(&de.url)
            .push_bind(&de.doc_id)
            .push_bind(&de.fonds)
            .push_bind(&de.cite)
            .push_bind(&de.ptag);
    });
    query_builder.push(
        " ON CONFLICT(doc_mpath) DO UPDATE SET \
           url  = excluded.url, \
           cite = COALESCE(excluded.cite, document_element.cite), \
           ptag = COALESCE(excluded.ptag, document_element.ptag)",
    );
    let query = query_builder.build();
    query.execute(&mut *self.tx).await?;
    query_builder.reset();
}
```

`COALESCE(excluded.x, document_element.x)` so a later publication carrying a **NULL** `cite` never erases a good one already stored.

> **CORRECTION — verified by test.** This paragraph originally claimed `COALESCE` also protects against a *blank* cite. **It does not.** `COALESCE` short-circuits on NULL only, so an empty-string `cite` reaching this upsert **does** overwrite a stored `"Title 10"`. Proven by `basic::document_element_test::test_insert_bulk_blank_cite_overwrites_so_ingest_must_normalise_it`.
>
> Consequence: the `.filter(|value| !value.trim().is_empty())` in `optional_label` (§4.2) is **load-bearing, not belt-and-braces** — it is the only thing stopping San Mateo's 341 blank-cite document roots from writing `""` into the column. Normalisation MUST happen at ingest; this upsert cannot be relied on to catch it.
 `query_builder.reset()` rewinds to the `QueryBuilder::new` prefix, so appending the suffix inside the loop is correct.

Verify before merging whether `update --force` already truncates `document_element` for the fonds (in which case `OR IGNORE` would have sufficed). The upsert is correct either way and costs nothing — make the change regardless, but state in the PR which mechanism actually performs the backfill.

### 4.6 `Status` — int → enum, and the declaration-order trap

`src\db\models\status\mod.rs` has only `from_string` and `to_int`. **The enum's declaration order (`ElementAdded, ElementChanged, ElementRemoved, ElementEffective`) does NOT match `to_int()` (`Added=0, Effective=1, Changed=2, Removed=3`), and the per-variant doc comments are wrong** — `/// 1 - Element changed.` sits on `ElementChanged`, whose `to_int()` is `2`. Anyone writing `from_int` from the declaration order or the doc comments produces a silently-wrong mapping, and in this design that means every node in the menu reporting the wrong status.

```rust
/// Convert the integer stored in `document_change.status` back into a `Status`.
///
/// Returns `None` for values outside 0..=3, which indicates corrupt data.
/// Mirrors [`Status::to_int`] exactly — note that the mapping is *not* the
/// enum's declaration order.
#[must_use]
pub const fn from_int(value: i64) -> Option<Self> {
    match value {
        0 => Some(Self::ElementAdded),
        1 => Some(Self::ElementEffective),
        2 => Some(Self::ElementChanged),
        3 => Some(Self::ElementRemoved),
        _ => None,
    }
}

/// Stable lowercase token used in JSON API responses.
#[must_use]
pub const fn as_api_str(&self) -> &'static str {
    match *self {
        Self::ElementAdded => "added",
        Self::ElementEffective => "effective",
        Self::ElementChanged => "changed",
        Self::ElementRemoved => "removed",
    }
}
```

Fix the three wrong variant doc comments in the same commit.

`Option`, not `anyhow::Result`: one corrupt status int must not 500 an entire menu. The response layer maps `None` to the token `"unknown"` and emits `tracing::warn!("Unknown document_change.status {value} on mpath {mpath}")`. Explicit handling, not swallowing.

JSON emits `added|effective|changed|removed`, not the RDF phrasing `"Element added"` — the RDF wording is an ingest detail and the frontend renders its own copy.

**Required test:** `for n in 0..4 { assert_eq!(Status::from_int(n).unwrap().to_int(), n) }` plus `assert!(Status::from_int(4).is_none())`.

---

## 5. Query design

**The driver has inverted.** In v1, `document_element` drove and `document_change` supplied version info. Now `document_change` drives — it is the only table that knows what changed in a version — and `document_element` is reduced to a bounded label lookup.

Three queries for a document root (date existence check, changes, labels), three for a collection root.

Throughout: `base` is the resolved base mpath (always ends with `|`), `base_depth = base.matches('|').count()`, `N` is the clamped relative depth.

### 5.1 mpath helpers (pure, `src\server\api\menu\mpath.rs`)

`clippy::string_slice` and `clippy::indexing_slicing` are denied crate-wide, so none of these may use `&s[..n]`:

```rust
/// Number of `|`-delimited segments in a materialized path.
fn mpath_depth(mpath: &str) -> usize {
    mpath.matches('|').count()
}

/// Materialized-path prefix of `mpath` containing exactly `depth` segments.
/// `truncate_to_depth("a|b|c|", 2) == "a|b|"`.
fn truncate_to_depth(mpath: &str, depth: usize) -> String {
    mpath.split_inclusive('|').take(depth).collect()
}

/// Last `|`-delimited segment. `"a|b|c|" -> "c"`, `"a|" -> "a"`.
fn last_segment(mpath: &str) -> &str {
    mpath.trim_end_matches('|').rsplit('|').next().unwrap_or(mpath)
}

/// Exclusive upper bound of the set of strings prefixed by `base`.
/// `None` when `base` is not a well-formed mpath (does not end with `|`).
fn prefix_upper_bound(base: &str) -> Option<String> {
    base.strip_suffix('|').map(|stem| format!("{stem}}}"))
}
```

`unwrap_or` not `unwrap` (`clippy::unwrap_used` is denied); `rsplit` always yields at least one item, so the fallback is unreachable but free.

`prefix_upper_bound` replaces the trailing `|` (0x7C) with `}` (0x7D), giving the exact exclusive successor of the prefix set for **arbitrary UTF-8**: any string beginning with `base` agrees on every earlier byte then compares smaller at that position. It is retained for the collection path (§5.6) and for tree mode should the PO ask for it; the dated document path no longer needs a range scan.

### 5.2 Query 1 (driver) — elements that changed in this version, under this base

```sql
SELECT dc.doc_mpath AS doc_mpath,
       dc.status    AS status
FROM document_change dc
JOIN publication_has_publication_versions phpv
  ON phpv.publication_version_id = dc.publication_version_id
JOIN publication_version pv
  ON pv.id = phpv.publication_version_id
WHERE dc.doc_mpath LIKE $1
  AND phpv.publication_id = $2
  AND pv.version = $3
```

Binds: `$1 = format!("{base}%")`, `$2 = publication.id`, `$3 = resolved_date`.

This is the existing `/versions` join shape (`document_change\manager.rs:22-28`) plus the one new predicate `pv.version = $3`. Joining `pv` through `phpv` rather than directly off `dc` is deliberate and matches the existing code: `document_change.publication_version_id` may point at a `publication_version` row owned by an *earlier* publication, because `insert_shared_publication_versions_for_publication` (`changes.rs:648`) extends `phpv` with the `last_valid_publication_id` chain. Filtering `phpv.publication_id` is what scopes to the publication being viewed.

**Consequence worth stating:** if two `publication_version` rows carrying the same `version` date are both mapped into this publication via `phpv`, their change sets are **unioned**. That is consistent with `/versions`, which does `SELECT DISTINCT pv.version`, and is almost certainly correct (the same calendar date really did have those changes). Do not add a `pv.publication_id = $2` predicate — that would break lightweight publications.

**No `ESCAPE` clause.** An `ESCAPE` clause unconditionally disables SQLite's LIKE optimization. The cost is that a literal `%` or `_` in an mpath acts as a wildcard; `/versions` already lives with this unmitigated.

**Mandatory Rust-side guard:** drop any row where `!row.doc_mpath.starts_with(base)`. This fixes two real over-match sources at zero cost — `%`/`_` inside the base, and `LIKE` being ASCII-case-insensitive so base `ABC|` matches `abc|…`. A genuine correctness improvement over `/versions`.

**Indexes, honestly.** Three candidate drive paths exist:
- `publication_version_version_idx` (§3.2, new) on `pv.version` → `document_change_publication_version_idx` → prefix as residual. **This is the plan the index exists to enable**, and it is the right one: the date is now the most selective predicate.
- `document_change_doc_mpath_idx ON document_change(doc_mpath COLLATE NOCASE)` serves the `LIKE` prefix (NOCASE index + no ESCAPE + parameter-bound non-wildcard-leading pattern), with the date as residual. This is what runs today without the new index, and it means scanning the whole publication's subtree change set.
- `publication_has_publication_versions_publication_id_idx`, the worst option.

**Do not assert which SQLite picks — measure it.** §0.2 gives the command. What *is* established: the result set is small. San Mateo, measured per-date, is ≤ 89 rows for 67 of 68 dates, median ~7, versus 3,682 for the whole publication. Even if the planner picks the prefix path and ignores the date index, the work is bounded by the publication-wide figures the coordinator already measured (21 ms San Mateo, 123 ms Maryland) — which is the *ceiling*, not the expectation.

**Pre-approved remedy** if EQP shows a bad plan on both paths — a covering index so the date range never touches the table:
```sql
CREATE INDEX document_change_pv_mpath_status_idx
  ON document_change(publication_version_id, doc_mpath, status);
```
Add only with before/after EQP output in the PR. Do not add speculatively; it overlaps `document_change_publication_version_idx`.

**Rejected:** `... FROM document_element de JOIN document_change dc ON dc.doc_mpath LIKE de.doc_mpath || '%'`. The LIKE optimization requires a literal or parameter on the right-hand side; `de.doc_mpath || '%'` is neither, so no index applies and it degenerates to a nested loop of nodes × changes. Explicitly rejected — and in the revised design it is moot anyway, since `document_element` no longer drives.

**Rejected:** a `WITH RECURSIVE` ancestor rollup in SQL using `instr`/`substr`. It materializes `rows × (N+1)` in a temp B-tree, is unreadable and untestable, and buys nothing — Rust already holds the rows in order to nest and sort them.

### 5.3 Node-set synthesis (pure Rust, `src\server\api\menu\tree.rs`)

Ancestors are **synthesized from the changed descendants' mpaths**. There is no query for them. For each surviving row `(mpath, status)` with `row_depth = mpath_depth(mpath)`:

```
window_floor = base_depth
window_ceil  = base_depth + N

// 1. The node that actually changed, if it falls inside the window.
if row_depth >= window_floor && row_depth <= window_ceil:
    node_set.insert(truncate_to_depth(mpath, row_depth))
    direct_statuses[that node].insert(status)

// 2. Every ancestor of it inside the window becomes a node, and is marked
//    as having a changed descendant.
for k in window_floor ..= min(window_ceil, row_depth - 1):
    ancestor = truncate_to_depth(mpath, k)
    node_set.insert(ancestor)
    has_changed_descendant.insert(ancestor)

// 3. A change BELOW the frontier marks the frontier node — this is what
//    makes `hasChildren == true, children == []` the lazy-load signal.
//    Covered by step 2 when row_depth > window_ceil, since then
//    min(window_ceil, row_depth - 1) == window_ceil.
```

Step 3 needs no separate branch: when `row_depth > window_ceil`, step 2's upper bound is `window_ceil`, so the frontier node is inserted into `has_changed_descendant` and the deeper node is never inserted into `node_set`. That single `min` is the whole frontier mechanism — call it out in review, because getting it wrong (using `row_depth - 1` unbounded) silently leaks sub-frontier nodes into the response.

Then:
- `node.statuses = direct_statuses.get(mpath)` or `[]`.
- `node.hasChildren = has_changed_descendant.contains(mpath)`.
- `base` itself is always in `node_set` as the root, even when nothing changed.

Containers: `direct_statuses: HashMap<String, BTreeSet<i64>>` (BTreeSet orders by `to_int()`, which is exactly the required output order, and dedups), `has_changed_descendant: HashSet<String>`, `node_set: HashSet<String>`. Built inside a pure function taking `&[ChangeRow]` and returning the triple; it never mutates anything owned by a caller, satisfying the immutability rule. `clippy::iter_over_hash_type` is denied — only `.contains()` / `.get()` these maps, or collect-and-sort before iterating.

**Cost.** `rows × N` truncations. Worst measured case, San Mateo bootstrap date at depth 2: 2,769 × 2 ≈ 5,500 short-string allocations. Sub-millisecond. A typical date is ~7 × 2 = 14.

**Node-set size is bounded by `min(rows × N, tree width in the window)`** — both small. San Mateo `/code` at depth 2 can never exceed 27 titles + 285 chapters = 312 nodes no matter how busy the date, and on 2026-02-03 it is 17.

### 5.4 Query 2 (lookup) — labels for the synthesized node set

```sql
SELECT de.doc_mpath AS doc_mpath,
       de.url       AS url,
       de.doc_id    AS doc_id,
       de.cite      AS cite,
       de.ptag      AS ptag
FROM document_element de
WHERE de.doc_mpath IN ( ?, ?, ... )
  AND de.fonds = ?
```

Built with `sqlx::QueryBuilder` and `separated(", ")`; chunk the IN-list at `BATCH_SIZE` (1000, `src\db\models\mod.rs:4`) to stay clear of SQLite's parameter ceiling. Served by `sqlite_autoindex_document_element_1` on `document_element(doc_mpath)` (BINARY, from `PRIMARY KEY (doc_mpath)` with no `COLLATE` clause) — point lookups, one index seek each. The `fonds` predicate is a residual correctness guard; `doc_mpath` is globally unique, so it needs no index of its own.

**Why an IN-list and not v1's range scan.** The IN-list is proportional to the *answer* (17 rows on 2026-02-03); the range scan was proportional to the *subtree* (~3,000 rows for San Mateo `/code`, 33,912 for Maryland at depth 2..5). In dated mode the answer is one to three orders of magnitude smaller than the subtree, so the IN-list wins decisively. It also removes v1's "+1 extra level" fetch entirely, because `hasChildren` now comes from change data rather than from the element tree — a real simplification, not just a swap.

**Missing-row handling.** Tree completeness is verified, so every synthesized ancestor should have a `document_element` row. If one is missing anyway: **synthesize the node with `cite: null`, `ptag: null`, `url: null`, `docId: null`, `label: segment`, and emit `tracing::warn!`**. Never drop it — dropping an ancestor would orphan its children and break the tree — and never 500. A `url: null` node renders as a non-clickable group header.

### 5.5 Node ceiling

```rust
/// Hard server-side cap on nodes in one menu response.
pub const MAX_MENU_NODES: usize = 5_000;
```

Applied after natural sorting, breadth-first, so shallow levels are always complete; `"truncated": true` is surfaced in the response when it fires.

Why this exists despite the product owner's "no breadth cap": the cap was agreed for the *tree-first* design, where breadth is bounded by the tree. In dated mode the depth clamp does **not** bound the pathological case. The library bootstrap date is "everything changed at once" — 2,769 changes in the San Mateo code subtree alone, and presumably ~30k for Maryland. At depth 2 that is harmless (312 / 580 nodes, bounded by tree width), but at `depth=6` from a Maryland root it is the entire 34k-node tree in one JSON body. That is a memory and bandwidth hazard, not a usable menu.

The cap never fires on a normal request. **Flagged in §9 for product-owner sign-off**, and trivially disabled by raising the constant.

### 5.6 Collection roots

**Decision: supported, serving the library levels plus their member document roots** — effectively two levels — signalled by `"rootKind": "collection"` and `"depth": 2`. Descending into a document's internals requires a second request against that document's own path.

Rationale. Collections are a real top-level nav entry (`/us/ca/cities/san-mateo/ordinances` → years → ordinances that changed on this date), so refusing them makes the menu unable to start where users start. But collections live in a **different mpath namespace** from documents — `cityofsanmateo/law-rdf|us/ca/cities/san-mateo/ordinances|2025|` versus `Ordinance 2025-1|` — bridged only through `changed_library_document`. Recursing across that bridge arbitrarily deep in one request is the largest complexity risk in the feature, and the menu is lazy-loaded anyway.

Resolution order in the handler mirrors `publication_versions()` (`versions\mod.rs:163-188`): try `document_element::Manager::find_doc_mpath_by_url`; on `Err`, try `library::Manager::find_lib_mpath_by_url`; on `Err` again, 404.

**Query C1 — changed documents in this version, under this library subtree**

```sql
SELECT cld.library_mpath AS library_mpath,
       dc.doc_mpath      AS doc_mpath,
       dc.status         AS status
FROM changed_library_document cld
JOIN document_change dc
  ON dc.id = cld.document_change_id
JOIN publication_has_publication_versions phpv
  ON phpv.publication_version_id = dc.publication_version_id
JOIN publication_version pv
  ON pv.id = phpv.publication_version_id
WHERE cld.library_mpath LIKE $1
  AND phpv.publication_id = $2
  AND pv.version = $3
```

`$1 = format!("{base_lib}%")`. The `LIKE` prefix is served by `changed_library_document_library_mpath_idx` (NOCASE) — and note this is one of the cases where `LIKE` is correct and a `>=`/`<` rewrite would be wrong, because that index is NOCASE and a BINARY range comparison could not use it. Apply the same Rust-side `starts_with(base_lib)` guard. `dc.id = cld.document_change_id` uses the `document_change` PK; `pv.version` benefits from the new index.

**This single query yields both collection levels.** Child libraries come from `truncate_to_depth(library_mpath, base_lib_depth + 1)`; member documents come from `truncate_to_depth(doc_mpath, 1)` attached under their own `library_mpath`'s child-library ancestor. A document's root element is always its depth-1 mpath — confirmed by the existing code at `document_change\manager.rs:75-76`, which computes exactly `mpath.split('|').next() + '|'`.

**Query C2 — labels for the child libraries**

```sql
SELECT l.mpath AS mpath, l.url AS url
FROM library l
WHERE l.mpath IN ( ?, ?, ... )
  AND l.fonds = ?
```
Served by the `library(mpath)` PK autoindex. ~20 rows per fonds; cost irrelevant. Libraries have no `cite`/`ptag`, so `label` falls back to `segment` for them — note this in the contract so the frontend expects a bare year like `2025`.

**Query C3 — labels for the member document roots**

Identical to Query 2 (§5.4), with the document-root mpaths as the IN-list.

Three queries, all bounded, no N+1.

`hasChildren` in collection mode: a child library has it when it contributed any `doc_mpath` row; a member document root has it when any of its change rows sits at depth > 1 (i.e. something inside the document changed, which the client can reach by requesting that document's path).

### 5.7 Performance conclusions, revised

| Shape | v1 (tree-first) | Revised (date-first) |
|---|---|---|
| Driver | `document_element` range scan | `document_change` filtered to one version |
| San Mateo `/code`, typical date, depth 2 | ~3,000 element rows + 3,682 change rows | **~7-89 change rows**, 17-ish nodes |
| San Mateo `/code`, bootstrap date, depth 2 | same | 2,769 change rows → ≤ 312 nodes |
| Maryland root, depth 2 | 33,912 element rows / 13 ms + 34,328 change rows / 123 ms | **unmeasured — run §0.2**; expected to collapse the same way |
| Label lookup | n/a (came free with the scan) | IN-list of ≤ node-count point seeks |
| Rust rollup | rows × depth | rows × depth, far fewer rows |

The 123 ms Maryland figure is now a **ceiling that only the bootstrap date can approach**, not the cost of a typical hover. Confirm with the §0.2 measurement before merge rather than taking this on trust.

---

## 6. Module layout

New, mirroring `src\server\api\versions\`:

| Path | Contents | Est. lines |
|---|---|---|
| `C:\OLLGIT\taf-server\src\server\api\menu\mod.rs` | `menu()` handler; `DEFAULT_MENU_DEPTH` / `MIN_MENU_DEPTH` / `MAX_MENU_DEPTH` / `MAX_MENU_NODES` consts; fonds, publication, date and path resolution; document-vs-collection branch; orchestration only | ~200 |
| `C:\OLLGIT\taf-server\src\server\api\menu\request\mod.rs` | `Path` struct (`publication`, `date`, `path`), `Query` struct (`depth: Option<u32>`); `Query::resolved_depth()` doing the clamp | ~60 |
| `C:\OLLGIT\taf-server\src\server\api\menu\response\mod.rs` | `Menu`, `Node` serde structs (`#[serde(rename_all = "camelCase")]`); `Menu::build`; `Node::label` fallback chain; `Status` → token mapping incl. `"unknown"` | ~150 |
| `C:\OLLGIT\taf-server\src\server\api\menu\mpath.rs` | §5.1 helpers. Pure, no deps, unit-tested | ~90 |
| `C:\OLLGIT\taf-server\src\server\api\menu\tree.rs` | §5.3 node-set synthesis; nesting into `children`; node-ceiling truncation. No DB, no HTTP, no actix types. The bulk of the unit tests live here | ~240 |
| `C:\OLLGIT\taf-server\src\server\api\menu\sort.rs` | `natural_cmp` + `sort_children`. Pure, unit-tested | ~120 |
| `C:\OLLGIT\taf-server\src\server\api\menu\collection.rs` | §5.6 collection branch | ~170 |

Modified:

| Path | Change |
|---|---|
| `src\server\api\mod.rs` | `pub mod menu;` |
| `src\server\api\routes.rs` | §2 |
| `src\db\models\document_element\mod.rs` | `cite`/`ptag` fields; `new()` signature; `ElementLabel` read projection; `Manager` trait gains `find_labels_by_mpaths` |
| `src\db\models\document_element\manager.rs` | Query 2; upsert in `insert_bulk` |
| `src\db\models\document_change\mod.rs` | `VersionChange` read projection (`doc_mpath`, `status`); `Manager` trait gains `find_changes_in_version_by_prefix` |
| `src\db\models\document_change\manager.rs` | Query 1 |
| `src\db\models\publication_version\mod.rs` + `manager.rs` | `find_latest_version_for_publication`, `version_exists_in_publication` (§1.4) |
| `src\db\models\library\mod.rs` + `manager.rs` | `find_labels_by_mpaths` (C2) |
| `src\db\models\changed_library_document\mod.rs` + `manager.rs` | `Manager` trait (currently `TxManager` only) + `find_changes_in_version_by_library_prefix` (C1) |
| `src\db\models\status\mod.rs` | `from_int`, `as_api_str`, fix the three wrong variant doc comments |
| `src\server\api\versions\mod.rs` | `fn format_date` → `pub fn format_date`, so `menu::response` reuses it for the top-level `display` rather than duplicating |
| `src\history\rdf\namespaces.rs` | `cite`, `ptag` |
| `src\history\changes.rs` | §4.2 |

Keep every manager method in the existing idiom: raw SQL `&str`, `match self.kind { DatabaseKind::Sqlite => { … } }`, `sqlx::query_as::<_, T>`, `#[async_trait]`, `anyhow::Result`.

### 6.1 Where the constants live — decision

**Plain constants in `src\server\api\menu\mod.rs`**, not archive config:

```rust
/// Levels below the requested node returned when the client does not ask.
pub const DEFAULT_MENU_DEPTH: usize = 2;
/// Smallest meaningful menu depth; one level below the root.
pub const MIN_MENU_DEPTH: usize = 1;
/// Hard server-side ceiling. Maryland, the deepest corpus in the dev archive,
/// bottoms out at mpath depth 6, so this covers a whole-tree request from the
/// root while still bounding the per-request synthesis cost.
pub const MAX_MENU_DEPTH: usize = 6;
/// Hard cap on nodes in one response. Only reachable on a library bootstrap
/// date at large depth; see the design doc's node-ceiling rationale.
pub const MAX_MENU_NODES: usize = 5_000;
```

Rejecting a new `Option<Menu>` section on `fonds::archive::Config` (`src\fonds\archive.rs:168`) for three reasons:

1. `Config` is **serialized back out** by `fonds::archive::init()` (`archive.rs:199-209`), so any added field appears in every freshly written `config.toml`. Suppressing that churn needs `skip_serializing_if` + `#[serde(default)]` + a constant fallback — three mechanisms for one integer.
2. Every deployed `config.toml`, including `C:\OLLGIT\lib-dev\.taf\config.toml`, lacks the section, so the fallback constant is the value that actually runs everywhere. The config would be a second source of truth that is empty in practice.
3. These are **safety rails, not operator preferences**. Operators should not be able to raise them; that is the point.

Option (b) remains a clean, backward-compatible follow-up: adding `#[serde(default)]` config that falls back to these constants changes no endpoint contract. Do it when an operator actually asks.

Placement matches `versions\mod.rs:27-31` (`CURRENT_PUBLICATION_NAME` et al.) and gives the bare numbers a documented home so `clippy::restriction` does not trip on a literal in the handler.

### 6.2 Exact `depth` behaviour

| Input | Behaviour |
|---|---|
| absent | `2` (`DEFAULT_MENU_DEPTH`) |
| `1` … `6` | used as-is |
| `0` | **clamped to 1**; response reports `"depth": 1, "requestedDepth": 0` |
| `7`, `99` | **clamped to 6**; `"depth": 6, "requestedDepth": 99` |
| `-1` | **400** — rejected by serde during `web::Query<Query>` extraction, since the field is `Option<u32>`. Actix's default `QueryConfig` produces it; no handler code |
| `abc`, `` (empty) | **400**, same mechanism |
| `depth=2&depth=3` | **400 Bad Request.** *(Corrected: originally predicted "serde takes the last". It does not — `serde_urlencoded` behind `web::Query` rejects a duplicate key for a non-sequence field with `duplicate field 'depth'`, which actix maps to 400. The observed behaviour is the defensible one: a repeated parameter is malformed and the handler must not guess. Pinned by `test_menu_repeated_depth_parameter_expect_bad_request`.)* |

**Clamp for 0 and >max, 400 for non-numeric.** The distinction is between a *malformed* request (not a number — the client is broken; fail fast, per the input-validation rule) and a *well-formed but out-of-policy* one (a number outside the server's rails — serve the closest thing we will serve). Clamping is not a silent lie, because the response echoes both `depth` and `requestedDepth`. An idempotent GET nav endpoint that hard-fails on `depth=0` turns a cosmetic frontend bug into a blank menu.

Implement as `requested.clamp(MIN_MENU_DEPTH, MAX_MENU_DEPTH)` in `Query::resolved_depth()`, converting `u32` → `usize` with `usize::try_from(...)` (`clippy::as_conversions` is denied), falling back to `DEFAULT_MENU_DEPTH` on the impossible conversion failure.

---

## 7. Natural sort (carried forward from v1, unchanged)

### 7.1 Key: the last mpath segment. Confirmed, not `cite`.

Sorting is applied to each `children` vector after assembly, bottom-up, in `sort.rs`, keyed on `last_segment(node.mpath)`.

Why not `cite`:

- **Maryland makes `cite` useless as a sort key.** `Code of Maryland Regulations|01|`, `…|01|02|` and `…|01|01|1971|` all carry `cite='01'`. Sorting Maryland siblings by `cite` is a no-op and leaves sibling order **undefined**.
- `cite` is NULL on document roots in both corpora (absent in Maryland, empty-string in San Mateo) and NULL on every row of a DB ingested before this feature. Sorting would be unstable exactly where data is oldest.
- `cite` carries a level-name prefix (`Title `/`Chapter `/`Section `). Within one sibling set the prefix is usually constant, but not guaranteed — a `container` and a `section` can be siblings, and then `"Chapter …"` sorts before `"Section …"` as a pure prefix artifact rather than a meaningful order.
- `segment` is always present, always unique among siblings, and is the archive's own ordering material.

### 7.2 Comparator

```
natural_cmp(a: &str, b: &str) -> Ordering:
  split a and b into maximal runs: each run is either all-ASCII-digits or all-non-digits
  for (ra, rb) in zip(runs(a), runs(b)):
      if ra is digits and rb is digits:
          sa = ra with leading '0' bytes stripped
          sb = rb with leading '0' bytes stripped
          ord = sa.len().cmp(&sb.len()).then_with(|| sa.cmp(sb))
          if ord != Equal: return ord
      else if ra is non-digits and rb is non-digits:
          ord = ra.cmp(rb)                  // plain byte order, case-sensitive
          if ord != Equal: return ord
      else:
          return if ra is digits { Less } else { Greater }   // digit runs sort first
  return runs(a).count().cmp(&runs(b).count())
            .then_with(|| a.cmp(b))         // final total-order tie-break
```

Two deliberate choices:

- **Digit runs are compared by stripped length then bytes — never parsed to an integer.** `parse::<u64>()` would fail on a 25-digit segment; length-then-lexicographic *is* numeric comparison for non-negative integers and has no overflow mode. It also keeps the function infallible, which `clippy::unwrap_used`/`expect_used` demand.
- **The final `a.cmp(b)` makes the comparator a total order**, so `sort_by` is deterministic even when the natural key ties (`01` vs `1`). Without it, sibling order depends on SQLite's row order.

Lint-clean implementation: walk `a.chars()` and `b.chars()` as two peekable iterators and consume runs in place, comparing digit runs by counting significant digits then comparing char-by-char. Never materialize substrings, never index.

### 7.3 Worked examples

**San Mateo titles — `1, 2, 10, 27`** → `1 < 2 < 10 < 27`. Single digit runs; stripped lengths 1,1,2,2.
By `cite` (`Title 1`, `Title 2`, `Title 10`, `Title 27`) natural sort gives the *same* order here, since the `"Title "` run is identical and the digit runs decide. **Assert on the segment ordering** — identical output in this case, but the segment is the specified key and the only one that works in Maryland.

**San Mateo chapters — `10.04` vs `10.100`** → runs `["10",".","04"]` vs `["10",".","100"]`; `10 == 10`; `"." == "."`; stripped `"4"` (len 1) vs `"100"` (len 3) → **`10.04 < 10.100`**. Required behaviour, satisfied.

**`1.01.010` vs `1.1.10`** → runs `["1",".","01",".","010"]` vs `["1",".","1",".","10"]`. Every run ties numerically; run counts tie (5 vs 5). Final byte tie-break: at index 2, `'0'` (0x30) < `'1'` (0x31) → **`1.01.010 < 1.1.10`**. Deterministic, documented, testable.

**Maryland — `01, 02, 1971, 1972`** → stripped `"1"`,`"2"`,`"1971"`,`"1972"`; lengths 1,1,4,4 → **`01 < 02 < 1971 < 1972`**. Leading zeros correctly ignored for ordering.

**Maryland leading-dot segment `.11`** → runs `[".", "11"]`, a non-digit run **first**. Against a purely numeric sibling `11` (runs `["11"]`), the first pair is non-digit vs digit → digit first → **`11 < .11`**. Against dotted siblings the `"."` runs tie and the digits decide: **`.2 < .10 < .11`**. Maryland's section siblings are uniformly leading-dot, so this is consistent in practice; the mixed case is specified rather than left to chance.

**San Mateo Roman numeral `II`** (depth 4, under `Code|27|27.21|`) → one non-digit run. Digit-before-non-digit puts all numerics first: **`1 < 2 < 10 < II`**. Against `III`: both non-digit, byte compare → `"II" < "III"`. **Roman numerals are not interpreted as numbers** — `IX` would sort before `V`. Accepted limitation; see §9.

---

## 8. Edge cases for the test author

Helpers do not exist yet. `C:\OLLGIT\taf-server\tests\common\db_data\mod.rs` currently has only `insert_fonds`, `insert_publication`, `insert_redirects`, `load_redirects`. **The test author must add** `insert_version`, `insert_publication_version`, `insert_publication_has_publication_version`, `insert_document`, `insert_document_element` (with `cite`/`ptag` as `Option<&str>`), `insert_document_change`, `insert_library`, `insert_library_change`, `insert_changed_library_document` — all in the existing raw-`sqlx::query` style, inserting parent rows first, because the initial migration sets `PRAGMA foreign_keys = ON`.

A compact fixture that covers most of the list: one fonds, one publication, three version dates, a San Mateo-shaped code tree of depth 4 with two titles and a Roman-numeral depth-5 branch, plus one Maryland-shaped tree with ambiguous cites and a leading-dot segment.

### Date handling (new in this revision — the highest-value group)
1. **No date at all** — `/_api/menu/us/ca/.../code` → **400**, body names both legal forms. Same for `/_api/menu`, `/_api/menu/_publication/{p}`, `/_api/menu/_publication/{p}/{path}`.
2. **`date=current`** → resolves to the publication's newest version; assert the response's `date` field is the resolved concrete date, not the string `current`.
3. **`date=current` on a publication with no versions** → 404.
4. **Malformed date** (`_date/2026-13-99`, `_date/notadate`) → **400**.
5. **Well-formed date with no `publication_version` row in this publication** → **404** `"No version … in publication …"`.
6. **Valid date, but nothing under this path changed** → **200**, root present, `statuses: []`, `hasChildren: false`, `children: []`, `nodeCount: 1`. **Must not be a 404.** This is the single most important distinction in the endpoint — assert 4/5/6 produce three different outcomes.
7. **Date exists in a different publication but not this one** → 404.
8. **Two `publication_version` rows with the same `version` date mapped into one publication via `phpv`** (the lightweight-publication case) → their changes are **unioned**. Assert this, since it is easy to "fix" into a bug later.
9. **Historical publication** (`/_publication/{name}/_date/{d}`) — a node created after that publication must be **absent entirely**, not present-with-empty-statuses. This is the §9 risk retirement; assert it directly.
10. **Unknown publication name** → falls back exactly as `versions()` does. Assert parity.

### Node set and `hasChildren`
11. **Pure ancestor** — a change at depth 4 only; request depth 2 from depth 1. Assert the depth-2 and depth-3 ancestors exist with `statuses: []` and `hasChildren: true`.
12. **Frontier** — same fixture; assert the depth-3 node has `hasChildren: true` **and** `children: []`, and that the depth-4 changed node is **absent**. This pins the `min(window_ceil, row_depth - 1)` bound in §5.3.
13. **Change leaf** — a change exactly at the frontier depth; assert `statuses` non-empty and `hasChildren: false`, `children: []`.
14. **Changed itself AND has changed descendants** — changes at both depth 2 and depth 4; assert the depth-2 node has non-empty `statuses` **and** `hasChildren: true`.
15. **Unchanged siblings absent** — a title with 40 sections, none changed on this date, must not appear anywhere in the response.
16. **Depth 1 vs depth 3** on the same fixture — assert the depth-3 response contains nodes the depth-2 one marked as frontier, and that the depth-2 frontier nodes' `hasChildren` was correct.
17. **Non-uniform depth** — San Mateo `Code|27|27.21|II|27.21.100|` (section at depth 5) alongside depth-4 sections. Assert `depth` differs per node and nothing assumed uniformity.
18. **Six-level tree** (Maryland `…|19A|04|attachments|A|appendix-a|`) at `depth=6` from the root.
19. **Depth-1 mpath with a deep URL** (Mohican `1997 SMTCA 1|` → `/us/nsn/mohican/courts/appeals/opinions/1997/1`) — levels counted on the **mpath**, so this is depth 1 with `hasChildren: false`. **The best single guard against anyone reintroducing URL-segment counting.**
20. **Flat URLs vs nested mpaths** (San Mateo `Code|27|27.60|27.60.180|` → `/code/27.60.180`) — assert child URLs are taken from `document_element.url`, never derived from the parent's URL.
21. **Path is a leaf document element** (a section) with a change on this date → 200, root only, `hasChildren: false`.
22. **Path not found** (no `document_element`, no `library`) → 404.
23. **Node ceiling** — synthesize > `MAX_MENU_NODES` changed nodes; assert `truncated: true`, `nodeCount == MAX_MENU_NODES`, and that shallow levels are complete (breadth-first truncation).

### `depth` parameter
24. Matrix from §6.2: absent → 2; `0` → served 1; `1`,`6` → as-is; `7` → 6; `-1` → 400; `abc` → 400; empty → 400. Assert `depth` and `requestedDepth` in every 200 case.

### Statuses
25. **Multi-status on one element** — `Element added` + `Element effective` on the same mpath/version (the bootstrap-date shape) → `statuses: ["added","effective"]`, in that order.
26. **Status ordering is by `to_int()`**, not insertion order — insert `removed` before `added` and assert `["added","removed"]`.
27. **Descendant statuses do not bubble** — a section with `removed`; assert its ancestor title has `statuses: []`, not `["removed"]`.
28. **Unknown `document_change.status` int** (e.g. `7`) → node serves `"unknown"`, request still 200, a `warn!` is emitted. Must not 500.
29. **`Status` round-trip** — `from_int(to_int())` identity for all four; `from_int(4) == None`. Guards the declaration-order trap in §4.6.

### Labels
30. **`cite` empty string** (San Mateo document roots, `<oll:cite xml:lang="en"/>`) → stored NULL → `cite: null`, `label == segment`.
31. **`cite` predicate absent entirely** (Maryland document roots) → same outcome. **Both must be tested; they take different paths through `optional_label` (`Ok("")` + filter vs `Err` + `.ok()`).**
32. **`cite` ambiguous across depths** (Maryland `01` at three depths) → all three serve `cite: "01"` and remain distinguishable via `segment`, `mpath`, `url`. Assert `segment` differs.
33. **`cite`/`ptag` NULL on un-reingested data** → 200, `cite: null`, `ptag: null`, `label == segment`. The "old DB still serves" guarantee.
34. **`ptag` vocabulary** — only `section | container | document | attachment` occur. Assert pass-through; an unexpected fifth value must not 500.
35. **Synthesized ancestor with no `document_element` row** → node present with `url: null`, `label == segment`, and a `warn!`. **Never dropped.**

### Sorting
36. Titles `1,2,10,27` → that order. 37. `10.04` before `10.100`. 38. `1.01.010` before `1.1.10`. 39. Maryland `01,02,1971,1972`. 40. `.2,.10,.11`, and `11` before `.11` when mixed. 41. `II` after all numeric siblings; `II` before `III`. 42. **Stability** — insert rows in reverse and assert identical output ordering.

### Prefix / collation safety
43. **mpath segment containing a LIKE wildcard** — rows `A_B|` and `AXB|` with changes under both; request `A_B` and assert the `AXB` changes do **not** leak in. Pins the Rust-side `starts_with` guard.
44. **Case-colliding mpaths** — `abc|` and `ABC|` as separate rows with separate changes. Query 1's `LIKE` is case-insensitive and returns both; the `starts_with` guard must filter. Assert it does.
45. **Literal `|` inside a segment name** — checked, **impossible by construction**: `|` is the delimiter and the RDF emits `oll:documentMaterializedPath` already delimited, so such a segment is indistinguishable from two segments. No defence is possible and none is attempted. Case 43 is the tractable adjacent hazard; test that instead.

### Collections
46. **Collection root** (`/us/ca/cities/san-mateo/ordinances`, a date on which some ordinance changed) → `rootKind: "collection"`, `depth: 2`, children = year libraries, grandchildren = changed ordinance document roots.
47. **Collection root, date with no member changes** → 200, `children: []`, `hasChildren: false`.
48. **Collection child library labels** — libraries have no `cite`/`ptag`; assert `cite: null`, `label == segment` (e.g. `2025`).
49. **`depth=5` on a collection** → still served as `depth: 2`; assert the response reports `2` and `requestedDepth: 5`.

### Ingest
50. **Backfill on re-ingest** — a `document_element` row with NULL `cite`/`ptag`, then ingest RDF supplying both; assert they are now populated. **This is the test that catches the `INSERT OR IGNORE` bug (§4.5); without it the backfill silently does nothing.**
51. **Blank cite must not overwrite a good one** — existing `cite='Title 10'`, later publication supplies `''`; assert `Title 10` survives (the `COALESCE`).
52. **Ingest must not abort** when `oll:cite`/`oll:ptag` are missing from every change block in a publication.

### Performance guards (`#[ignore]` by default, run manually against `C:\OLLGIT\lib-dev\.taf\db.sqlite3`)
53. `/_api/menu/_date/2026-02-03/us/ca/cities/san-mateo/code?depth=2` — 24 change rows in, ~17 nodes out.
54. `/_api/menu/_date/2018-03-05/us/ca/cities/san-mateo/code?depth=2` — the bootstrap date: 2,769 change rows in, ≤ 312 nodes out. **The one case where the node ceiling and the synthesis cost actually matter.**
55. Maryland busiest date (from the §0.2 measurement) at depth 2. Record the number rather than asserting a wall-clock bound in CI.

---

## 9. Open risks

### Retired
**~~Node visibility is publication-independent.~~** **Retired, confirmed.** v1 derived the node set from `document_element`, which has no publication or version column, so a historical menu would have shown nodes that did not exist then. The revised design derives the node set from `document_change`, joined through `publication_has_publication_versions` to the viewed publication and filtered to one `pv.version`. A node that did not exist in that publication-version has no change rows under it and therefore **cannot appear**. The fix is structural, not a special case. Test 9 pins it.

### Still open — for the product owner

1. **Undated / tree mode.** §1.2. Recommendation: ship dated only; if tree browsing is wanted, add it behind an explicit `/_api/menu/_tree/{path}` segment, never as date-absence. **This is the one answer that changes scope**, so it is worth getting before implementation starts — though the dated contract above does not depend on it.

2. **Node ceiling (`MAX_MENU_NODES = 5_000`).** §5.5. The PO said "no breadth cap", but that was decided against the tree-first design where breadth was bounded by the tree. In dated mode the library bootstrap date is "everything changed at once" and the depth clamp does not bound it. Recommendation: keep the cap with `truncated` surfaced. Needs sign-off, trivially disabled.

3. **Removed / repealed nodes.** `Status::ElementRemoved` exists and nothing here hides it. In a *date-first* menu this is arguably more important than in a tree menu — "what was repealed on this date" is exactly the kind of thing a user hovers for. Recommendation: **return them with `removed` visible in `statuses`** and let the frontend style them. Confirm.

4. **Attachments.** `ptag='attachment'` nodes (23 in San Mateo, 64 in Maryland) are currently returned like any other node. Filter them out of a nav menu, or return them for the frontend to filter? Recommendation: return them; the frontend has `ptag`.

5. **Collections limited to two levels.** §5.6. If drill-down from a collection straight into a document's sections in one request matters, this needs re-scoping.

6. **Caching.** Measured worst case is now a typical ~7-89 rows rather than 34,328, so the pressure is much lower than in v1 — but a hover-driven menu still fires many requests per second. There is no cache utility on `main` (the `rss_cache` in `CLAUDE.md` is uncommitted WIP). Recommendation: **ship with HTTP caching headers only** — `Cache-Control: public, max-age=…` plus an `ETag` derived from `(publication_id, date, mpath, depth)`. A publication-version's change set is immutable until the next `update`, so a long `max-age` is safe. Measure before adding server-side state.

7. **Roman numerals do not sort as numbers.** §7.3 — `IX` sorts before `V`. Only matters where Roman segments have numeric siblings beyond `II`/`III`. Confirm acceptable, or supply the real sibling sets.

8. **Maryland per-date measurement is outstanding.** §0.2. Not a product decision, but it is the one unvalidated performance assumption in the design and it must be closed before merge.

---

## 10. Lint traps (read before writing a line)

`C:\OLLGIT\taf-server\src\lib.rs` enables `clippy::restriction` crate-wide and the gate is `cargo clippy --all -- -D warnings`. The allow-list (`lib.rs:71-172`) covers `arithmetic_side_effects`, `implicit_return`, `question_mark_used`, `single_call_fn`, `mod_module_files`, `std_instead_of_core`, `cognitive_complexity` and others — but **not** these, every one of which this feature will hit:

- **`clippy::string_slice`** — `&mpath[..n]` is denied. Use `split_inclusive('|').take(n).collect()`, `strip_suffix`, `trim_end_matches`, `rsplit`. This is why §5.1 specifies those exact constructions.
- **`clippy::indexing_slicing`** — no `vec[i]`; use `.get(i)` / iterators.
- **`clippy::unwrap_used` / `expect_used`** — none outside `#[cfg(test)]` blocks carrying an `#[expect(...)]` attribute (see `versions\mod.rs:254-261` for the exact form).
- **`clippy::as_conversions`** — `depth as usize` is denied. Use `usize::try_from(...)` with explicit handling; likewise for `document_change.status: i64`.
- **`clippy::missing_docs_in_private_items`** — every private fn, struct, field and const needs a doc comment.
- **`clippy::min_ident_chars`** — no single-character bindings, including in closures. Existing code uses `pb`, `ver`, `dc`.
- **`clippy::shadow_reuse` / `shadow_same` / `shadow_unrelated`** — no rebinding the same name; `let url = clean_url_path(&url);` will fail.
- **`clippy::pattern_type_mismatch`** — match on `*self` / explicit references; `archive\mod.rs:41-44` shows the escape hatch.
- **`clippy::iter_over_hash_type`** — iterating a `HashMap`/`HashSet` directly is denied (`routes.rs:389-392` shows the `#[expect]`). The §5.3 containers are only `.contains()`/`.get()`-ed; anything that must be iterated has to be a `BTreeMap`/`BTreeSet` or collected and sorted first.
- `#[expect(clippy::future_not_send, reason = "...")]` is needed on the handler module, copying `versions\mod.rs:2-5`.

Gate commands: `just lint` (= `cargo fmt --all -- --check` then `cargo clippy --all -- -D warnings`) and `just test` (= `cargo nextest run --all --no-fail-fast && cargo test --doc`).

Pre-existing failure recorded in `CLAUDE.md`: `unused variable: cache_key` at `src/server/api/rss/mod.rs:64`. That file does not exist on `main` as checked out, so it should not affect this branch — but if `just lint` fails there, it is not this feature's regression.

---

## Appendix — note to the coordinator

Two things to pass on:

1. **I could not run the Maryland per-date measurement you asked for** — this agent has no Bash/shell tool, only Read/Grep/Glob. What I *did* measure is better than an estimate: I counted `oll:documentMaterializedPath` blocks per version date directly in the San Mateo RDF (§0.1), getting 3,681 blocks across 68 dates against your SQL's 3,682 rows — a 1:1 cross-validation of RDF and DB. The per-date table is real data and it establishes the key structural fact (one bootstrap date holds 75% of the subtree's changes; every other date is ≤ 89 rows). The equivalent Maryland command is written out verbatim in §0.2 and should be run by someone with a shell before merge.

2. **The node-ceiling decision (§5.5) contradicts the product owner's earlier "no breadth cap"**, deliberately. That decision was made against the tree-first design, where breadth was naturally bounded by the tree width at a given depth. In date-first mode the bootstrap date is unbounded by the depth clamp. I have implemented it as a flagged, sign-off-pending safety rail rather than silently dropping the PO's instruction — please put it to them explicitly alongside the undated-mode question.
