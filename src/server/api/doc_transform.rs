//! Document transformation utilities for historical versioning.
//!
//! Provides functions to rewrite URLs in HTML and JSON documents so that
//! internal links point to a specific historical version context. Also includes
//! helpers for building URL prefixes, formatting dates, fetching document
//! version dates, and inserting notification banners into HTML documents.

use chrono::NaiveDate;
use libxml::parser::Parser;
use libxml::tree::SaveOptions;
use libxml::tree::{Document, Node, NodeType};
use libxml::xpath::Context;
use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::{
    db::{models::publication::Publication, DatabaseConnection},
    server::api::versions::{publication_versions, CURRENT_VERSION_DATE},
};

/// Builds the URL prefix used to rewrite historical document links.
///
/// Combines an optional publication name and an optional date into a prefix:
/// - Both present: `/_publication/{pub_name}/_date/{date}`
/// - Date only:    `/_date/{date}`
/// - Pub only:     `/_publication/{pub_name}`
/// - Neither:      `""`
#[must_use]
pub fn build_url_prefix(pub_name: &str, date: &str) -> String {
    let pub_part = if pub_name.is_empty() {
        String::new()
    } else {
        format!("/_publication/{pub_name}")
    };
    let date_part = if date.is_empty() {
        String::new()
    } else {
        format!("/_date/{date}")
    };
    format!("{pub_part}{date_part}")
}

/// Formats a `YYYY-MM-DD` date string for display as `Month DD, YYYY`.
/// Returns the original string unchanged if parsing fails.
#[must_use]
pub fn format_date_display(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_or_else(|_| date.to_owned(), |dt| dt.format("%B %d, %Y").to_string())
}

/// Returns all version dates for a document in ascending order.
///
/// Calls `publication_versions` directly against the database and strips the
/// synthetic `"current"` sentinel value so only real codified dates remain.
pub async fn get_doc_version_dates(
    db: &DatabaseConnection,
    publication: &Publication,
    path: &str,
) -> Vec<String> {
    let url = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{}", path.trim_start_matches('/'))
    };
    let mut dates: Vec<String> = publication_versions(db, publication, url)
        .await
        .into_iter()
        .map(|ver| ver.date)
        .filter(|dt| dt != CURRENT_VERSION_DATE)
        .collect();
    dates.reverse(); // publication_versions returns newest-first; we need ascending
    dates
}

/// Returns the `(start_date, end_date, current_date)` surrounding `date` in a
/// sorted list of version dates.
///
/// - `start_date` – the version on or just before `date` (when the doc became valid).
/// - `end_date`   – the next version after `date` (`None` means `date` is the latest).
/// - `current_date` – the most recent version date in the list.
///
/// Mirrors Python's `_get_document_version_start_end_current_dates`.
#[must_use]
pub fn get_version_start_end_current(
    versions: &[String],
    date: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let current = versions.last().cloned();

    // partition_point is equivalent to bisect_left: first index where versions[i] >= date
    let idx = versions.partition_point(|version| version.as_str() < date);

    if idx >= versions.len() {
        // All versions are older than date (IndexError case in Python)
        return (current.clone(), None, current);
    }

    let Some(version_at_idx) = versions.get(idx) else {
        return (current.clone(), None, current);
    };
    if version_at_idx.as_str() > date {
        // date falls between versions[idx-1] and versions[idx]
        let start = if idx == 0 {
            None
        } else {
            versions.get(idx - 1).cloned()
        };
        (start, Some(version_at_idx.clone()), current)
    } else {
        // Exact match: versions[idx] == date
        let end = versions.get(idx + 1).cloned();
        (Some(version_at_idx.clone()), end, current)
    }
}

/// Inserts a notification HTML banner as the first child of
/// `<main id="area__content">` in the document.
///
/// Uses string search to locate the opening tag and injects the notification
/// immediately after it. libxml-rust's reference-counting model prevents safe
/// cross-document DOM insertion before shared nodes, so string manipulation is
/// used here instead.
///
/// If the element is not found, the document is returned unchanged.
#[must_use]
pub fn insert_notification(html_str: &str, notification_html: &str) -> String {
    let mut search_start = 0;
    while let Some(rel_pos) = html_str
        .get(search_start..)
        .and_then(|html_slice| html_slice.find("<main"))
    {
        let pos = search_start + rel_pos;
        let Some(suffix) = html_str.get(pos..) else {
            break;
        };
        if let Some(tag_end_offset) = suffix.find('>') {
            let Some(tag) = html_str.get(pos..=(pos + tag_end_offset)) else {
                break;
            };
            if tag.contains(r#"id="area__content""#) || tag.contains("id='area__content'") {
                let insert_pos = pos + tag_end_offset + 1;
                // Count leading spaces on the <main> line and apply the same
                // indentation to every line of the notification fragment.
                let main_indent: String = html_str
                    .get(..pos)
                    .unwrap_or("")
                    .lines()
                    .last()
                    .unwrap_or("")
                    .chars()
                    .take_while(char::is_ascii_whitespace)
                    .collect();
                let indented_notification: String = notification_html
                    .lines()
                    .map(|line| {
                        if line.is_empty() {
                            String::new()
                        } else {
                            format!("  {main_indent}{line}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let Some(before) = html_str.get(..insert_pos) else {
                    break;
                };
                let Some(after) = html_str.get(insert_pos..) else {
                    break;
                };
                let mut result =
                    String::with_capacity(html_str.len() + indented_notification.len() + 1);
                result.push_str(before);
                result.push_str(&indented_notification);
                result.push_str(after);
                return result;
            }
            search_start = pos + tag_end_offset + 1;
        } else {
            break;
        }
    }
    html_str.to_owned()
}

/// Updates document URLs to point to a historical (versioned) prefix.
///
/// The function parses the provided HTML document, walks through relevant
/// elements (`meta`, `a`, `span`, headings, etc.), and rewrites selected
/// attributes so that local URLs are prefixed with the given `_date/version_date`.
/// It also injects a `<meta>` tag describing the historical prefix.
///
/// # Arguments
///
/// * `html_doc_str` – HTML document content as a UTF-8 string
/// * `path_str` – Request path used to resolve and normalize relative URLs
/// * `version_date` – Date used to construct the historical URL prefix
/// * `pub_name` – Publication name (empty string means no publication prefix)
///
/// # Returns
///
/// Returns the updated HTML document as a string.
///
/// # Errors
///
/// Returns an error if the HTML cannot be parsed or if a required regular
/// expression fails to compile.
#[expect(
    clippy::else_if_without_else,
    reason = "Only specific values are handled; all other cases intentionally do nothing"
)]
pub fn update_doc_urls(
    html_doc_str: &str,
    path_str: &str,
    version_date: &str,
    pub_name: &str,
) -> anyhow::Result<String> {
    let path = if path_str.starts_with('/') {
        path_str.to_owned()
    } else {
        format!("/{path_str}")
    };
    let versioned_url = build_url_prefix(pub_name, version_date);
    let version_date_fmt = NaiveDate::parse_from_str(version_date, "%Y-%m-%d").map_or_else(
        |_| version_date.to_owned(),
        |date| date.format("%B %d, %Y").to_string(),
    );
    let versioned_title = format!("Historical version from {version_date_fmt}");
    let heading_re = Regex::new(r"^h\d$")?;

    let parser = Parser::default();
    let doc: Document = parser.parse_string(html_doc_str)?;
    let ctx =
        Context::new(&doc).map_err(|err| anyhow::anyhow!("Failed to create Context: {err:?}"))?;

    let nodes = ctx
        .evaluate("//*")
        .map_err(|err| anyhow::anyhow!("Failed to evaluate XPath '//*': {err:?}"))?
        .get_nodes_as_vec();

    for mut node in nodes {
        let tag = node.get_name();

        match tag.as_str() {
            "meta" => {
                if let Some(prop) = node.get_attribute("property") {
                    if prop == "og:title" {
                        if let Some(content) = node.get_attribute("content") {
                            node.set_attribute(
                                "content",
                                &format!("{content} | {versioned_title}"),
                            )
                            .map_err(|_err| anyhow::anyhow!("Failed to update attribute"))?;
                        }
                    } else if prop == "og:url" {
                        if let Some(content) = node.get_attribute("content") {
                            let updated = content.replace(&path, &format!("{versioned_url}{path}"));
                            node.set_attribute("content", &updated)
                                .map_err(|_err| anyhow::anyhow!("Failed to update attribute"))?;
                        }
                    }
                }

                if let Some(itemprop) = node.get_attribute("itemprop") {
                    if itemprop == "full-html" || itemprop == "toc-json" {
                        update_el_attr(&mut node, "content", &path, &versioned_url);
                    }
                }
            }

            "a" => update_el_attr(&mut node, "href", &path, &versioned_url),

            "span" => update_el_attr(&mut node, "id", &path, &versioned_url),

            "object" => {
                if node.get_attribute("type").as_deref() == Some("application/pdf") {
                    update_el_attr(&mut node, "data", &path, &versioned_url);
                }
            }

            _ if heading_re.is_match(&tag) => {
                update_el_attr(&mut node, "id", &path, &versioned_url);
            }

            _ => {}
        }
    }

    // Insert historical-prefix meta tag
    let head = ctx
        .evaluate("/html/head")
        .map_err(|_err| anyhow::anyhow!("Failed to update attribute"))?
        .get_nodes_as_vec()
        .into_iter()
        .next();

    add_historical_meta_tag(head, &doc, &versioned_url)?;

    let options = SaveOptions {
        no_declaration: true,
        ..Default::default()
    };
    Ok(doc.to_string_with_options(options))
}

/// Adds a `<meta>` tag with the `itemprop="historical-prefix"` attribute to the
/// document `<head>`.
///
/// The meta tag's `content` attribute is set to the provided historical URL
/// prefix (typically a versioned `/_date/...` path). If a `<head>` element
/// exists, the tag is inserted at a deterministic position among its children (The element is inserted at index 13 to align with existing meta tag grouping).
/// If no `<head>` is found, the document is left unchanged.
///
/// # Arguments
///
/// * `doc` - Parsed HTML document to be modified
/// * `versioned_url` - Historical URL prefix to store in the meta tag
///
/// # Errors
///
/// Returns an error if DOM manipulation fails, such as when creating nodes,
/// setting attributes, or inserting the meta element.
fn add_historical_meta_tag(
    head_node: Option<Node>,
    doc: &Document,
    versioned_url: &str,
) -> anyhow::Result<(), anyhow::Error> {
    if let Some(mut head) = head_node {
        let mut meta = Node::new("meta", None, doc)
            .map_err(|err| anyhow::anyhow!("Failed to creat 'meta' node: {err:?}"))?;
        meta.set_attribute("itemprop", "historical-prefix")
            .map_err(|err| anyhow::anyhow!("Failed to update attribute 'itemprop': {err}"))?;
        meta.set_attribute("content", versioned_url)
            .map_err(|err| anyhow::anyhow!("Failed to update attribute 'content': {err}"))?;

        let mut element_children: Vec<Node> = head
            .get_child_nodes()
            .into_iter()
            .filter(|n| {
                n.get_type() != Some(NodeType::TextNode) || !n.get_content().trim().is_empty()
            })
            .collect();

        if let Some(ref_node) = element_children.get_mut(13) {
            // Used for formating indentations
            let mut newline = Node::new_text("\n    ", doc)
                .map_err(|err| anyhow::anyhow!("Failed to create text node: {err:?}"))?;
            ref_node
                .add_prev_sibling(&mut meta)
                .map_err(|err| anyhow::anyhow!("Failed to add_prev_sibling 'meta': {err}"))?;
            ref_node
                .add_prev_sibling(&mut newline)
                .map_err(|err| anyhow::anyhow!("Failed to add_prev_sibling 'text': {err}"))?;
        } else {
            // Used for formating indentations
            let mut newline = Node::new_text("\n  ", doc)
                .map_err(|err| anyhow::anyhow!("Failed to create text node: {err:?}"))?;
            let mut newspaces = Node::new_text("  ", doc)
                .map_err(|err| anyhow::anyhow!("Failed to create text node: {err:?}"))?;
            head.add_child(&mut newspaces)
                .map_err(|err| anyhow::anyhow!("Failed to aooend 'text' node to 'head': {err}"))?;
            head.add_child(&mut meta)
                .map_err(|err| anyhow::anyhow!("Failed to append 'meta' node to 'head': {err}"))?;
            head.add_child(&mut newline)
                .map_err(|err| anyhow::anyhow!("Failed to append 'text' node to 'head': {err}"))?;
        }
    }

    Ok(())
}

/// Updates a single HTML element attribute by rewriting local URLs.
///
/// If the attribute value ends with `.pdf`, the URL is resolved against the
/// normalized document path. Otherwise, local URLs starting with `/` are
/// prefixed with the provided historical version URL.
///
/// # Arguments
///
/// * `node` - Mutable reference to the xml node
/// * `attr` - Attribute name to update (e.g. `href`, `src`, `content`)
/// * `path` - Current document path, used for resolving relative URLs
/// * `versioned_url` - Prefix added to local URLs for historical versions
///
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "File extension comparison is intentionally case-sensitive; only lowercase `.pdf` URLs are expecte"
)]
pub fn update_el_attr(node: &mut Node, attr: &str, path: &str, versioned_url: &str) {
    let Some(value) = node.get_attribute(attr) else {
        return;
    };

    let new_value = if value.ends_with(".pdf") {
        let base = path
            .replace("/index.html", "")
            .replace("/index.full.html", "")
            .trim_end_matches('/')
            .to_owned();
        // Url:parse return Err on invalid url - so for paths we add dummy schema/base
        if let Ok(base_url) = Url::parse(&format!("http://oll.dummy.com{base}")) {
            base_url
                .join(&value)
                .map_or(value, |joined| joined.path().to_owned())
        } else {
            value
        }
    } else {
        // Prefix only local URLs
        if value.starts_with('/') {
            format!("{versioned_url}{value}")
        } else {
            value
        }
    };
    match node.set_attribute(attr, &new_value) {
        Ok(()) => {}
        Err(_) => {
            tracing::error!("Not able to update node attribute {attr}");
        }
    }
}

/// Updates all URLs in a JSON file according to a historical version.
///
/// If the file is a `manifest.json`, updates `start_url`, `scope`, and `icons`.
/// Otherwise, updates entries in an index JSON using the historical URL prefix.
///
/// # Arguments
///
/// * `json_str` - The JSON content as a string
/// * `path` - File path (used to distinguish manifest from index JSON)
/// * `date` - Historical version date used to construct URL prefixes
/// * `pub_name` - Publication name (empty string means no publication prefix)
///
/// # Returns
///
/// Returns the updated JSON as a string. If parsing fails, the original JSON is returned.
#[must_use]
pub fn update_json_content(json_str: &str, path: &str, date: &str, pub_name: &str) -> String {
    let result: anyhow::Result<String> = (|| {
        let mut json_value: Value = serde_json::from_str(json_str)?;
        let url_prefix = build_url_prefix(pub_name, date);
        if path.contains("manifest") {
            update_manifest_json(&mut json_value, &url_prefix);
        } else {
            update_index_json(&mut json_value, &url_prefix);
        }

        Ok(serde_json::to_string(&json_value)?)
    })();

    match result {
        Ok(updated) => updated,
        Err(err) => {
            tracing::error!("Error while updating json content: {err}");
            json_str.to_owned()
        }
    }
}

/// Updates URLs inside a `manifest.json` object.
///
/// Prefixes the `start_url` and `scope` fields, as well as all `src` fields
/// in the `icons` array, with the provided `url_prefix`.
///
/// # Arguments
///
/// * `manifest` - Mutable reference to the JSON manifest (`serde_json::Value`)
/// * `url_prefix` - Prefix to prepend to all relevant URL fields
///
fn update_manifest_json(manifest: &mut Value, url_prefix: &str) {
    let Some(obj) = manifest.as_object_mut() else {
        return;
    };
    if let Some(start_url) = obj
        .get("start_url")
        .and_then(Value::as_str)
        .map(str::to_owned)
    {
        obj.insert(
            "start_url".into(),
            Value::String(format!("{url_prefix}{start_url}")),
        );
    }
    if let Some(scope) = obj.get("scope").and_then(Value::as_str).map(str::to_owned) {
        obj.insert(
            "scope".into(),
            Value::String(format!("{url_prefix}{scope}")),
        );
    }

    if let Some(icons) = obj.get_mut("icons").and_then(Value::as_array_mut) {
        for icon in icons {
            if let Some(src) = icon.get("src").and_then(Value::as_str).map(str::to_owned) {
                if let Some(mut_icon) = icon.as_object_mut() {
                    mut_icon.insert("src".into(), Value::String(format!("{url_prefix}{src}")));
                }
            }
        }
    }
}

/// Recursively updates URLs inside a JSON index.
///
/// Walks through all entries in the JSON `index` and prefixes relevant
/// fields (`p`, `j`, `dj`, `fh`) with the given `url_prefix`. If an entry
/// has a `c` field (children), the function recurses into them.
///
/// # Arguments
///
/// * `index` - Mutable reference to the JSON index (`serde_json::Value`)
/// * `url_prefix` - String to prefix all relevant URL fields with
fn update_index_json(index: &mut Value, url_prefix: &str) {
    fn update_one_entry(entry: &mut Value, url_prefix: &str) {
        let Some(obj) = entry.as_object_mut() else {
            return;
        };

        let keys: Vec<String> = obj.keys().cloned().collect();
        #[expect(
            clippy::pattern_type_mismatch,
            reason = " Refactoring to avoid this lint would reduce readability without functional benefit"
        )]
        for key in &keys {
            match key.as_str() {
                "p" | "j" | "dj" | "fh" => {
                    if let Some(Value::String(val)) = obj.get(key) {
                        obj.insert(key.clone(), Value::String(format!("{url_prefix}{val}")));
                    }
                }
                "c" => {
                    if let Some(Value::Array(children)) = obj.get_mut("c") {
                        for child in children {
                            update_one_entry(child, url_prefix);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    update_one_entry(index, url_prefix);
}

#[cfg(test)]
#[expect(clippy::inline_modules, reason = "tests")]
mod tests {
    use super::*;

    fn test_html_transformation(
        html_input: &str,
        expected_output: &str,
        path: &str,
        version_date: &str,
    ) {
        let parser = Parser::default();
        let doc: Document = parser
            .parse_string(expected_output)
            .expect("parse should succeed");
        let options = SaveOptions {
            no_declaration: true,
            as_xml: true,
            ..Default::default()
        };
        let expected_output_fmt = doc.to_string_with_options(options);
        let result = update_doc_urls(html_input, path, version_date, "");
        let output = result.expect("function should succeed");
        assert_eq!(output, expected_output_fmt);
    }

    #[test]
    fn test_update_element_a_tag() {
        let html_input = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
  </head>
  <body>
    <a href="/">smth</a>
    <a href="/test">smth</a>
    <a href="/test/1">smth</a>
    <a href="test/1">smth</a>
  </body>
</html>
        "#;
        let expected_output = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
    <meta itemprop="historical-prefix" content="/_date/12-03-2025"/>
  </head>
  <body>
    <a href="/_date/12-03-2025/">smth</a>
    <a href="/_date/12-03-2025/test">smth</a>
    <a href="/_date/12-03-2025/test/1">smth</a>
    <a href="test/1">smth</a>
  </body>
</html>
        "#;
        let version_date = "12-03-2025";
        let path = "/some/test/path/";
        test_html_transformation(html_input, expected_output, path, version_date);
    }

    #[test]
    fn test_update_element_span_tag() {
        let html_input = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
  </head>
  <body>
    <span id="/">smth</span>
    <span id="/test">smth</span>
    <span id="/test/1">smth</span>
    <span id="test/1">smth</span>
    <span href="/test/2">smth</span>
  </body>
</html>
        "#;
        let expected_output = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
    <meta itemprop="historical-prefix" content="/_date/02-09-2024"/>
  </head>
  <body>
    <span id="/_date/02-09-2024/">smth</span>
    <span id="/_date/02-09-2024/test">smth</span>
    <span id="/_date/02-09-2024/test/1">smth</span>
    <span id="test/1">smth</span>
    <span href="/test/2">smth</span>
  </body>
</html>
        "#;
        let version_date = "02-09-2024";
        let path = "/some/test/path/";
        test_html_transformation(html_input, expected_output, path, version_date);
    }

    #[test]
    fn test_update_element_h_tag() {
        let html_input = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
  </head>
  <body>
    <h1 id="/test">Introduction</h1>
    <h2 id="introduction">Introduction</h1>
    <h6 id="/">Introduction</h1>
  </body>
</html>
        "#;
        let expected_output = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
    <meta itemprop="historical-prefix" content="/_date/02-09-2024"/>
  </head>
  <body>
    <h1 id="/_date/02-09-2024/test">Introduction</h1>
    <h2 id="introduction">Introduction</h1>
    <h6 id="/_date/02-09-2024/">Introduction</h1>
  </body>
</html>
        "#;
        let version_date = "02-09-2024";
        let path = "/some/test/path/";
        test_html_transformation(html_input, expected_output, path, version_date);
    }

    #[test]
    fn test_update_element_object_tag() {
        let html_input = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
  </head>
  <body>
    <object
        data="./documents/example.pdf"
        type="application/pdf"
        width="600"
        height="800">
        <p>
            Your browser does not support embedded PDFs.
            <a href="./documents/example.pdf">Download the PDF</a>.
        </p>
    </object>
    <object
        data="/not/ending/on/dot/pdf"
        type="application/pdf"
        width="600"
        height="800">
        <p>
            Your browser does not support embedded PDFs.
            <a href="/">Download the PDF</a>.
        </p>
    </object>
  </body>
</html>
        "#;
        let expected_output = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
    <meta itemprop="historical-prefix" content="/_date/02-09-2024"/>
  </head>
  <body>
    <object
        data="/some/test/documents/example.pdf"
        type="application/pdf"
        width="600"
        height="800">
        <p>
            Your browser does not support embedded PDFs.
            <a href="/some/test/documents/example.pdf">Download the PDF</a>.
        </p>
    </object>
    <object
        data="/_date/02-09-2024/not/ending/on/dot/pdf"
        type="application/pdf"
        width="600"
        height="800">
        <p>
            Your browser does not support embedded PDFs.
            <a href="/_date/02-09-2024/">Download the PDF</a>.
        </p>
    </object>
  </body>
</html>
        "#;
        let version_date = "02-09-2024";
        let path = "/some/test/path/";
        test_html_transformation(html_input, expected_output, path, version_date);
    }

    #[test]
    fn test_meta_tag_update() {
        let html_input = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
    <meta property="og:url"
        content="https://law.cityofsanmateo.org/us/ca/cities/san-mateo/ordinances/2020/19" />
    <meta property="og:type" content="article" />
    <meta property="og:title"
        content="Ord. No. 2020-19. Emergency Ordinance – Outdoor Business Operations | City of San Mateo Law Library" />
    <meta itemprop="toc-json" content="/us/ca/cities/san-mateo/ordinances/2020/19/index.json" data-document="href"/>
    <meta itemprop="doc-type" content="document" data-document=""/>
    <meta itemprop="doc-num" content="2020-19"/>
    <meta itemprop="full-html" content="/"/>
  </head>
  <body>
  </body>
</html>
        "#;
        let expected_output = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
    <meta property="og:url"
        content="https://law.cityofsanmateo.org/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2020/19" />
    <meta property="og:type" content="article" />
    <meta property="og:title"
        content="Ord. No. 2020-19. Emergency Ordinance – Outdoor Business Operations | City of San Mateo Law Library | Historical version from March 04, 2025" />
    <meta itemprop="toc-json" content="/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2020/19/index.json" data-document="href"/>
    <meta itemprop="doc-type" content="document" data-document=""/>
    <meta itemprop="doc-num" content="2020-19"/>
    <meta itemprop="full-html" content="/_date/2025-03-04/"/>
    <meta itemprop="historical-prefix" content="/_date/2025-03-04"/>
  </head>
  <body>
  </body>
</html>
        "#;
        let version_date = "2025-03-04";
        let path = "/us/ca/cities/san-mateo/ordinances/2020/19";
        test_html_transformation(html_input, expected_output, path, version_date);
    }

    #[test]
    fn test_json_update_on_index_json() {
        let json_input = r#"
{
    "p" : "/us/ca/cities/san-mateo/ordinances/2025/2#1",
    "j" : "/us/ca/cities/san-mateo/ordinances/2025/2#2",
    "dj" : "/us/ca/cities/san-mateo/ordinances/2025/2#3",
    "fh" : "/us/ca/cities/san-mateo/ordinances/2025/2#4",
    "q" : "/us/ca/cities/san-mateo/ordinances/2025/2#5",
    "c" : [
        {
            "p" : "/us/ca/cities/san-mateo/ordinances/2025/2#6",
            "j" : "/us/ca/cities/san-mateo/ordinances/2025/2#7",
            "n" : [
                {
                    "p" : "/us/ca/cities/san-mateo/ordinances/2025/2#8"
                }
            ]
        },
        {
            "c": [
                {
                    "p" : "/us/ca/cities/san-mateo/ordinances/2025/2#9"
                }
            ]
        }
    ]
}
        "#;
        let expected_output = r#"
{
    "p" : "/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2025/2#1",
    "j" : "/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2025/2#2",
    "dj" : "/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2025/2#3",
    "fh" : "/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2025/2#4",
    "q" : "/us/ca/cities/san-mateo/ordinances/2025/2#5",
    "c" : [
        {
            "p" : "/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2025/2#6",
            "j" : "/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2025/2#7",
            "n" : [
                {
                    "p" : "/us/ca/cities/san-mateo/ordinances/2025/2#8"
                }
            ]
        },
        {
            "c": [
                {
                    "p" : "/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2025/2#9"
                }
            ]
        }
    ]
}
        "#;
        let version_date = "2025-03-04";
        let path = "/us/ca/cities/san-mateo/ordinances/2020/19/index.json";
        let json_value: Value =
            serde_json::from_str(expected_output).expect("expect to parse json");
        let expected_output_fmt =
            serde_json::to_string(&json_value).expect("expect to convert json to string");
        let output = update_json_content(json_input, path, version_date, "");
        assert_eq!(output, expected_output_fmt.to_owned());
    }

    #[test]
    fn test_json_update_on_manifest_json() {
        let json_input = r#"
{
  "name": "City of San Mateo Law Library",
  "icons": [
    {
      "src": "/us/ca/cities/san-mateo/_document/v2/images/favicons/android-chrome-48x48.png",
      "sizes": "48x48",
      "type": "image/png"
    },
    {
      "src": "/us/ca/cities/san-mateo/_document/v2/images/favicons/android-chrome-96x96.png",
      "sizes": "96x96",
      "type": "image/png"
    }
  ],
  "display": "standalone",
  "start_url": "/",
  "scope": "/"
}
        "#;
        let expected_output = r#"
{
  "name": "City of San Mateo Law Library",
  "icons": [
    {
      "src": "/_date/2025-03-04/us/ca/cities/san-mateo/_document/v2/images/favicons/android-chrome-48x48.png",
      "sizes": "48x48",
      "type": "image/png"
    },
    {
      "src": "/_date/2025-03-04/us/ca/cities/san-mateo/_document/v2/images/favicons/android-chrome-96x96.png",
      "sizes": "96x96",
      "type": "image/png"
    }
  ],
  "display": "standalone",
  "start_url": "/_date/2025-03-04/",
  "scope": "/_date/2025-03-04/"
}
        "#;
        let version_date = "2025-03-04";
        let path = "/manifest.json";
        let json_value: Value =
            serde_json::from_str(expected_output).expect("expect to parse json");
        let expected_output_fmt =
            serde_json::to_string(&json_value).expect("expect to convert json to string");
        let output = update_json_content(json_input, path, version_date, "");
        assert_eq!(output, expected_output_fmt.to_owned());
    }

    #[test]
    fn test_build_url_prefix_both() {
        assert_eq!(
            build_url_prefix("my-pub", "2025-03-04"),
            "/_publication/my-pub/_date/2025-03-04"
        );
    }

    #[test]
    fn test_build_url_prefix_date_only() {
        assert_eq!(build_url_prefix("", "2025-03-04"), "/_date/2025-03-04");
    }

    #[test]
    fn test_build_url_prefix_pub_only() {
        assert_eq!(build_url_prefix("my-pub", ""), "/_publication/my-pub");
    }

    #[test]
    fn test_build_url_prefix_neither() {
        assert_eq!(build_url_prefix("", ""), "");
    }

    #[test]
    fn test_format_date_display_valid() {
        assert_eq!(format_date_display("2025-03-04"), "March 04, 2025");
    }

    #[test]
    fn test_format_date_display_invalid_passthrough() {
        assert_eq!(format_date_display("not-a-date"), "not-a-date");
    }

    #[test]
    fn test_version_start_end_exact_match() {
        let versions = vec![
            "2024-01-01".to_owned(),
            "2024-06-01".to_owned(),
            "2025-01-01".to_owned(),
        ];
        let (start, end, current) = get_version_start_end_current(&versions, "2024-06-01");
        assert_eq!(start, Some("2024-06-01".to_owned()));
        assert_eq!(end, Some("2025-01-01".to_owned()));
        assert_eq!(current, Some("2025-01-01".to_owned()));
    }

    #[test]
    fn test_version_start_end_between_versions() {
        let versions = vec![
            "2024-01-01".to_owned(),
            "2024-06-01".to_owned(),
            "2025-01-01".to_owned(),
        ];
        let (start, end, current) = get_version_start_end_current(&versions, "2024-09-01");
        assert_eq!(start, Some("2024-06-01".to_owned()));
        assert_eq!(end, Some("2025-01-01".to_owned()));
        assert_eq!(current, Some("2025-01-01".to_owned()));
    }

    #[test]
    fn test_version_start_end_latest() {
        let versions = vec![
            "2024-01-01".to_owned(),
            "2024-06-01".to_owned(),
            "2025-01-01".to_owned(),
        ];
        let (start, end, current) = get_version_start_end_current(&versions, "2025-01-01");
        assert_eq!(start, Some("2025-01-01".to_owned()));
        assert_eq!(end, None);
        assert_eq!(current, Some("2025-01-01".to_owned()));
    }

    #[test]
    fn test_version_start_end_newer_than_all() {
        let versions = vec!["2024-01-01".to_owned(), "2024-06-01".to_owned()];
        let (start, end, current) = get_version_start_end_current(&versions, "2099-01-01");
        assert_eq!(start, current.clone());
        assert_eq!(end, None);
        assert_eq!(current, Some("2024-06-01".to_owned()));
    }

    #[test]
    fn test_version_start_end_empty_list() {
        let versions: Vec<String> = vec![];
        let (start, end, current) = get_version_start_end_current(&versions, "2025-01-01");
        assert_eq!(start, None);
        assert_eq!(end, None);
        assert_eq!(current, None);
    }

    #[test]
    fn test_insert_notification_basic() {
        let html = r#"<html><body>
    <main id="area__content">
      <p>content</p>
    </main>
  </body></html>"#;
        let notification = r#"<div class="banner">Notice</div>"#;
        let result = insert_notification(html, notification);
        assert!(result.contains(r#"<main id="area__content">"#));
        assert!(result.contains(r#"<div class="banner">Notice</div>"#));
        // notification must appear before existing content
        let main_pos = result.find(r#"<main id="area__content""#).unwrap();
        let banner_pos = result.find(r#"<div class="banner">"#).unwrap();
        let content_pos = result.find("<p>content</p>").unwrap();
        assert!(main_pos < banner_pos);
        assert!(banner_pos < content_pos);
    }

    #[test]
    fn test_insert_notification_single_quote_attr() {
        let html = r#"<html><body>
    <main id='area__content'>
      <p>content</p>
    </main>
  </body></html>"#;
        let notification = r#"<div class="banner">Notice</div>"#;
        let result = insert_notification(html, notification);
        assert!(result.contains(r#"<div class="banner">Notice</div>"#));
    }

    #[test]
    fn test_insert_notification_no_match_returns_unchanged() {
        let html = "<html><body><main id=\"other\"><p>content</p></main></body></html>";
        let notification = "<div>Notice</div>";
        let result = insert_notification(html, notification);
        assert_eq!(result, html);
    }

    // --- update_doc_urls with pub_name ---

    #[test]
    fn test_update_doc_urls_with_pub_name() {
        let html_input = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US">
  <head></head>
  <body>
    <a href="/us/ca/cities/san-mateo/ordinances/2020/19">link</a>
  </body>
</html>
        "#;
        let result = update_doc_urls(
            html_input,
            "/us/ca/cities/san-mateo/ordinances/2020/19",
            "2025-03-04",
            "2020-01-01",
        )
        .expect("should succeed");
        assert!(
            result.contains("/_publication/2020-01-01/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2020/19"),
            "expected pub+date prefixed href in: {result}"
        );
        assert!(
            result.contains(r#"content="/_publication/2020-01-01/_date/2025-03-04""#),
            "expected historical-prefix meta in: {result}"
        );
    }

    // --- update_json_content with pub_name ---

    #[test]
    fn test_json_update_index_with_pub_name() {
        let json_input = r#"{"p":"/us/ca/test","j":"/us/ca/test.json"}"#;
        let output = update_json_content(
            json_input,
            "/us/ca/test/index.json",
            "2025-03-04",
            "2013-01-31",
        );
        let parsed: Value = serde_json::from_str(&output).expect("valid json");
        assert_eq!(
            parsed["p"].as_str().unwrap(),
            "/_publication/2013-01-31/_date/2025-03-04/us/ca/test"
        );
        assert_eq!(
            parsed["j"].as_str().unwrap(),
            "/_publication/2013-01-31/_date/2025-03-04/us/ca/test.json"
        );
    }

    #[test]
    fn test_json_update_manifest_with_pub_name() {
        let json_input = r#"{"start_url":"/","scope":"/","icons":[{"src":"/img/icon.png"}]}"#;
        let output = update_json_content(json_input, "/manifest.json", "2025-03-04", "2013-01-31");
        let parsed: Value = serde_json::from_str(&output).expect("valid json");
        assert_eq!(
            parsed["start_url"].as_str().unwrap(),
            "/_publication/2013-01-31/_date/2025-03-04/"
        );
        assert_eq!(
            parsed["scope"].as_str().unwrap(),
            "/_publication/2013-01-31/_date/2025-03-04/"
        );
        assert_eq!(
            parsed["icons"][0]["src"].as_str().unwrap(),
            "/_publication/2013-01-31/_date/2025-03-04/img/icon.png"
        );
    }
}
