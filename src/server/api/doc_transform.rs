//! Document transformation utilities for historical versioning.
//!
//! Provides functions to rewrite URLs in HTML and JSON documents so that
//! internal links point to a specific historical version context. Also includes
//! helpers for building URL prefixes, formatting dates, fetching document
//! version dates, and inserting notification banners into HTML documents.

use chrono::NaiveDate;
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::name::QName;
use quick_xml::{Reader, Writer, XmlVersion};
use serde_json::Value;
use std::borrow::Cow;
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
/// immediately after it, leaving every other byte of the document untouched.
/// The banner is a fragment of already-rendered HTML, so there is nothing to
/// gain from parsing the page to insert it.
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
/// Returns an error if the HTML is not well-formed, or if the rewritten
/// document is not valid UTF-8.
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
    let versioned_title = format!(
        "Historical version from {}",
        format_date_display(version_date)
    );

    let mut reader = Reader::from_str(html_doc_str);
    let mut writer = Writer::new(Vec::new());

    // Depth of the element currently open: 0 outside the root, 1 inside
    // `<html>`, 2 inside `<head>`.
    let mut depth: usize = 0;
    let mut in_head = false;
    let mut head_children: usize = 0;
    let mut meta_inserted = false;

    loop {
        let event = reader.read_event()?;

        if in_head && depth == 2 && !meta_inserted && is_significant(&event) {
            head_children += 1;
            if head_children > HEAD_INSERT_INDEX {
                write_historical_meta_before(&mut writer, &versioned_url)?;
                meta_inserted = true;
            }
        }

        match event {
            Event::Eof => break,
            Event::Start(start) => {
                if depth == 1 && !meta_inserted && start.name().as_ref() == b"head" {
                    in_head = true;
                    head_children = 0;
                }
                match rewrite_element(&start, &path, &versioned_url, &versioned_title) {
                    Some(updated) => writer.write_event(Event::Start(updated))?,
                    None => writer.write_event(Event::Start(start))?,
                }
                depth += 1;
            }
            Event::Empty(start) => {
                if depth == 1 && !meta_inserted && start.name().as_ref() == b"head" {
                    // A self-closing `<head/>` has no children to count, so the
                    // meta tag is appended just as it would be to an empty
                    // `<head></head>`.
                    writer.write_event(Event::Start(start.borrow()))?;
                    write_historical_meta_at_end(&mut writer, &versioned_url)?;
                    writer.write_event(Event::End(start.to_end()))?;
                    meta_inserted = true;
                } else {
                    match rewrite_element(&start, &path, &versioned_url, &versioned_title) {
                        Some(updated) => writer.write_event(Event::Empty(updated))?,
                        None => writer.write_event(Event::Empty(start))?,
                    }
                }
            }
            Event::End(end) => {
                if in_head && depth == 2 && end.name().as_ref() == b"head" {
                    if !meta_inserted {
                        write_historical_meta_at_end(&mut writer, &versioned_url)?;
                        meta_inserted = true;
                    }
                    in_head = false;
                }
                depth = depth.saturating_sub(1);
                writer.write_event(Event::End(end))?;
            }
            other @ (Event::Text(_)
            | Event::CData(_)
            | Event::Comment(_)
            | Event::Decl(_)
            | Event::PI(_)
            | Event::DocType(_)
            | Event::GeneralRef(_)) => writer.write_event(other)?,
        }
    }

    Ok(String::from_utf8(writer.into_inner())?)
}

/// Position among `<head>`'s significant children at which the
/// historical-prefix meta tag is inserted, so that it lands among the other
/// meta tags rather than at the top of the head. A `<head>` with fewer
/// children gets the tag appended instead.
const HEAD_INSERT_INDEX: usize = 13;

/// Whether an event counts as a `<head>` child when locating the insertion
/// point.
///
/// Mirrors the libxml2 implementation, which dropped whitespace-only text
/// nodes but kept comments: real pages group their meta tags with
/// `<!--Meta-->` style comments, and those shift the insertion point.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "Matching the reference avoids a `ref` binding for the text arm"
)]
fn is_significant(event: &Event<'_>) -> bool {
    match event {
        Event::Start(_)
        | Event::Empty(_)
        | Event::Comment(_)
        | Event::CData(_)
        | Event::GeneralRef(_)
        | Event::PI(_) => true,
        Event::Text(text) => !text.iter().all(u8::is_ascii_whitespace),
        Event::End(_) | Event::Decl(_) | Event::DocType(_) | Event::Eof => false,
    }
}

/// Writes the historical-prefix meta tag immediately before an existing
/// `<head>` child.
///
/// The newline and indent go *after* the tag, lining up the sibling that
/// follows it: the whitespace that originally preceded that sibling has
/// already been written and now indents the meta tag itself.
fn write_historical_meta_before(
    writer: &mut Writer<Vec<u8>>,
    versioned_url: &str,
) -> anyhow::Result<()> {
    writer.write_event(Event::Empty(historical_meta(versioned_url)))?;
    writer.write_event(Event::Text(BytesText::from_escaped("\n    ")))?;
    Ok(())
}

/// Appends the historical-prefix meta tag as the last child of `<head>`.
///
/// The leading two spaces complete the indent of the whitespace already
/// written before `</head>`.
fn write_historical_meta_at_end(
    writer: &mut Writer<Vec<u8>>,
    versioned_url: &str,
) -> anyhow::Result<()> {
    writer.write_event(Event::Text(BytesText::from_escaped("  ")))?;
    writer.write_event(Event::Empty(historical_meta(versioned_url)))?;
    writer.write_event(Event::Text(BytesText::from_escaped("\n  ")))?;
    Ok(())
}

/// Builds the `<meta itemprop="historical-prefix">` tag.
fn historical_meta(versioned_url: &str) -> BytesStart<'static> {
    let mut meta = BytesStart::new("meta");
    meta.push_attribute(Attribute {
        key: QName(b"itemprop"),
        value: Cow::Borrowed(b"historical-prefix"),
    });
    meta.push_attribute(Attribute {
        key: QName(b"content"),
        value: Cow::Owned(escape_attribute(versioned_url).into_bytes()),
    });
    meta
}

/// Returns the rewritten form of `start`, or `None` if it needs no change.
///
/// Mirrors the element and attribute pairs the libxml2 implementation walked:
/// `og:title` and `og:url` meta tags, the `full-html` and `toc-json` meta
/// tags, `a/@href`, `span/@id`, a PDF `object/@data`, and headings' `@id`.
fn rewrite_element(
    start: &BytesStart<'_>,
    path: &str,
    versioned_url: &str,
    versioned_title: &str,
) -> Option<BytesStart<'static>> {
    let qname = start.name();
    let name = qname.as_ref();

    if name == b"meta" {
        return rewrite_meta(start, path, versioned_url, versioned_title);
    }

    let attr = if name == b"a" {
        "href"
    } else if name == b"span" {
        "id"
    } else if name == b"object" {
        if attribute_value(start, "type").as_deref() != Some("application/pdf") {
            return None;
        }
        "data"
    } else if is_heading(name) {
        "id"
    } else {
        return None;
    };

    let value = attribute_value(start, attr)?;
    Some(with_attribute(
        start,
        attr,
        &rewritten_url(&value, path, versioned_url),
    ))
}

/// Rewrites the `content` attribute of the meta tags carrying a URL or the
/// page title.
#[expect(
    clippy::else_if_without_else,
    reason = "Only specific values are handled; all other cases intentionally do nothing"
)]
fn rewrite_meta(
    start: &BytesStart<'_>,
    path: &str,
    versioned_url: &str,
    versioned_title: &str,
) -> Option<BytesStart<'static>> {
    let mut content = attribute_value(start, "content")?;
    let mut changed = false;

    if let Some(property) = attribute_value(start, "property") {
        if property == "og:title" {
            content = format!("{content} | {versioned_title}");
            changed = true;
        } else if property == "og:url" {
            content = content.replace(path, &format!("{versioned_url}{path}"));
            changed = true;
        }
    }

    if let Some(itemprop) = attribute_value(start, "itemprop") {
        if itemprop == "full-html" || itemprop == "toc-json" {
            content = rewritten_url(&content, path, versioned_url);
            changed = true;
        }
    }

    changed.then(|| with_attribute(start, "content", &content))
}

/// Matches `h` followed by a single digit, as the previous `^h\d$` regex did.
fn is_heading(name: &[u8]) -> bool {
    matches!(*name, [b'h', digit] if digit.is_ascii_digit())
}

/// Rewrites one URL attribute value for a historical version.
///
/// A `.pdf` value is resolved against the document's own directory; any other
/// local value, meaning one starting with `/`, is prefixed with the historical
/// URL. Everything else is left alone.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "File extension comparison is intentionally case-sensitive; only lowercase `.pdf` URLs are expected"
)]
fn rewritten_url(value: &str, path: &str, versioned_url: &str) -> String {
    if value.ends_with(".pdf") {
        let base = path
            .replace("/index.html", "")
            .replace("/index.full.html", "")
            .trim_end_matches('/')
            .to_owned();
        // `Url::parse` rejects a bare path, so give it a dummy scheme and host.
        Url::parse(&format!("http://oll.dummy.com{base}"))
            .ok()
            .and_then(|base_url| base_url.join(value).ok())
            .map_or_else(|| value.to_owned(), |joined| joined.path().to_owned())
    } else if value.starts_with('/') {
        format!("{versioned_url}{value}")
    } else {
        value.to_owned()
    }
}

/// Returns the unescaped value of the `name` attribute on `start`.
fn attribute_value(start: &BytesStart<'_>, name: &str) -> Option<String> {
    let attribute = start
        .attributes()
        .with_checks(false)
        .flatten()
        .find(|attr| attr.key.as_ref() == name.as_bytes())?;
    attribute
        .normalized_value(XmlVersion::Implicit1_0)
        .ok()
        .map(Cow::into_owned)
}

/// Returns a copy of `start` with `name` set to `value`, keeping the other
/// attributes and their order.
///
/// The element is rebuilt, so one originally written across several lines
/// comes back on a single line. Only elements actually rewritten are
/// affected; every other byte of the document is passed through untouched.
fn with_attribute(start: &BytesStart<'_>, name: &str, value: &str) -> BytesStart<'static> {
    let mut out = BytesStart::new(String::from_utf8_lossy(start.name().as_ref()).into_owned());
    for attr in start.attributes().with_checks(false).flatten() {
        if attr.key.as_ref() == name.as_bytes() {
            out.push_attribute(Attribute {
                key: QName(name.as_bytes()),
                value: Cow::Owned(escape_attribute(value).into_bytes()),
            });
        } else if attr.value.contains(&b'"') {
            // `attr.value` is the source bytes from between the original
            // quotes, so a single-quoted value may hold a literal `"`.
            // `push_attribute` always writes double quotes, which would end
            // the value early and produce broken markup.
            out.push_attribute(Attribute {
                key: attr.key,
                value: Cow::Owned(
                    String::from_utf8_lossy(&attr.value)
                        .replace('"', "&quot;")
                        .into_bytes(),
                ),
            });
        } else {
            out.push_attribute(attr);
        }
    }
    out
}

/// Escapes an attribute value the way libxml2 did: `&`, `<`, `>` and `"`, plus
/// the whitespace that has to survive as a character reference, but not `'`.
///
/// quick-xml's own escaping also rewrites `'` as `&apos;`, which would change
/// the bytes of every page whose title contains an apostrophe.
///
/// Tab, newline and carriage return must be written as references rather than
/// literally. `normalized_value` decodes `&#10;` to a newline when the value is
/// read, and attribute-value normalization turns a literal newline back into a
/// space when it is read again - so writing one out silently replaces it. Over
/// a hundred San Mateo `og:title` values carry `&#10;`, and a DC one carries
/// `&#9;`.
fn escape_attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(character),
        }
    }
    out
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

    /// Assert that `update_doc_urls` produces exactly `expected_output`.
    ///
    /// The expected value is compared literally, byte for byte. It used to be
    /// parsed and re-serialized with libxml2 first, which normalized both
    /// sides through the same serializer and so made these tests structurally
    /// incapable of detecting a change of XML library -- which is now the one
    /// thing they have to guard.
    ///
    /// On a mismatch the actual output is printed raw, so that after an
    /// intentional change it can be copied back into the test:
    ///
    /// ```text
    /// cargo test doc_transform -- --nocapture
    /// ```
    fn assert_html_transformation(
        name: &str,
        html_input: &str,
        expected_output: &str,
        path: &str,
        version_date: &str,
    ) {
        let output = update_doc_urls(html_input, path, version_date, "")
            .expect("update_doc_urls should succeed");
        if output != expected_output {
            println!("--- actual output for {name} ---\n{output}\n--- end {name} ---");
        }
        assert_eq!(output, expected_output, "{name}: serialized output changed");
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
        assert_html_transformation("a_tag", html_input, expected_output, path, version_date);
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
        assert_html_transformation("span_tag", html_input, expected_output, path, version_date);
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
    <h2 id="introduction">Introduction</h2>
    <h6 id="/">Introduction</h6>
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
    <h2 id="introduction">Introduction</h2>
    <h6 id="/_date/02-09-2024/">Introduction</h6>
  </body>
</html>
        "#;
        let version_date = "02-09-2024";
        let path = "/some/test/path/";
        assert_html_transformation("h_tag", html_input, expected_output, path, version_date);
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
    <object data="/some/test/documents/example.pdf" type="application/pdf" width="600" height="800">
        <p>
            Your browser does not support embedded PDFs.
            <a href="/some/test/documents/example.pdf">Download the PDF</a>.
        </p>
    </object>
    <object data="/_date/02-09-2024/not/ending/on/dot/pdf" type="application/pdf" width="600" height="800">
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
        assert_html_transformation(
            "object_tag",
            html_input,
            expected_output,
            path,
            version_date,
        );
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
    <meta property="og:url" content="https://law.cityofsanmateo.org/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2020/19"/>
    <meta property="og:type" content="article" />
    <meta property="og:title" content="Ord. No. 2020-19. Emergency Ordinance – Outdoor Business Operations | City of San Mateo Law Library | Historical version from March 04, 2025"/>
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
        assert_html_transformation("meta_tag", html_input, expected_output, path, version_date);
    }

    /// A `<head>` with more than `HEAD_INSERT_INDEX` significant children, so
    /// the historical-prefix meta tag is spliced in among the others rather
    /// than appended at the end. Every real page takes this branch.
    ///
    /// The `<!--Meta-->` and `<!--Assets-->` comments are load bearing: they
    /// count as children when locating the insertion point, matching libxml2,
    /// which dropped whitespace-only text nodes but kept comments. Remove one
    /// and the meta tag moves down a line.
    ///
    /// Counting the significant children of `<head>`:
    ///  1 `<!--Meta-->`              6 `toc-json`      11 `app.css`
    ///  2 `charset`                  7 `doc-type`      12 `print.css`
    ///  3 `og:url`                   8 `doc-num`       13 `<title>`
    ///  4 `og:type`                  9 `full-html`     14 `<script>`  <- before this
    ///  5 `og:title`                10 `<!--Assets-->` 15 `<style>`
    #[test]
    fn test_meta_tag_inserted_among_a_full_head() {
        let html_input = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
    <!--Meta-->
    <meta charset="utf-8" />
    <meta property="og:url" content="https://law.cityofsanmateo.org/us/ca/cities/san-mateo/ordinances/2020/19" />
    <meta property="og:type" content="article" />
    <meta property="og:title" content="Ord. No. 2020-19 | City of San Mateo Law Library" />
    <meta itemprop="toc-json" content="/us/ca/cities/san-mateo/ordinances/2020/19/index.json" />
    <meta itemprop="doc-type" content="document" />
    <meta itemprop="doc-num" content="2020-19" />
    <meta itemprop="full-html" content="/us/ca/cities/san-mateo/ordinances/2020/19/index.full.html" />
    <!--Assets-->
    <link rel="stylesheet" href="/_assets/app.css" />
    <link rel="stylesheet" href="/_assets/print.css" />
    <title>Ord. No. 2020-19</title>
    <script src="/_assets/app.js"></script>
    <style>.icon { color: #000; }</style>
  </head>
  <body>
  </body>
</html>
        "#;
        let expected_output = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US" class="no-js">
  <head>
    <!--Meta-->
    <meta charset="utf-8" />
    <meta property="og:url" content="https://law.cityofsanmateo.org/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2020/19"/>
    <meta property="og:type" content="article" />
    <meta property="og:title" content="Ord. No. 2020-19 | City of San Mateo Law Library | Historical version from March 04, 2025"/>
    <meta itemprop="toc-json" content="/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2020/19/index.json"/>
    <meta itemprop="doc-type" content="document" />
    <meta itemprop="doc-num" content="2020-19" />
    <meta itemprop="full-html" content="/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2020/19/index.full.html"/>
    <!--Assets-->
    <link rel="stylesheet" href="/_assets/app.css" />
    <link rel="stylesheet" href="/_assets/print.css" />
    <title>Ord. No. 2020-19</title>
    <meta itemprop="historical-prefix" content="/_date/2025-03-04"/>
    <script src="/_assets/app.js"></script>
    <style>.icon { color: #000; }</style>
  </head>
  <body>
  </body>
</html>
        "#;
        let version_date = "2025-03-04";
        let path = "/us/ca/cities/san-mateo/ordinances/2020/19";
        assert_html_transformation(
            "meta_tag_full_head",
            html_input,
            expected_output,
            path,
            version_date,
        );
    }

    /// Tab, newline and carriage return have to come back out as character
    /// references. `normalized_value` decodes `&#10;` to a newline on the way
    /// in, and attribute-value normalization would turn a literal newline into
    /// a space the next time the page is read, so writing one out loses it.
    #[test]
    fn test_og_title_keeps_newline_reference() {
        let html_input = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US">
  <head>
    <meta property="og:title" content="Ord. No. 2020-19&#10;Outdoor Operations&#9;(amended)" />
  </head>
  <body>
  </body>
</html>
        "#;
        let expected_output = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US">
  <head>
    <meta property="og:title" content="Ord. No. 2020-19&#10;Outdoor Operations&#9;(amended) | Historical version from March 04, 2025"/>
    <meta itemprop="historical-prefix" content="/_date/2025-03-04"/>
  </head>
  <body>
  </body>
</html>
        "#;
        let version_date = "2025-03-04";
        let path = "/us/ca/cities/san-mateo/ordinances/2020/19";
        assert_html_transformation(
            "og_title_newline",
            html_input,
            expected_output,
            path,
            version_date,
        );
    }

    /// An attribute that is carried across unchanged is re-quoted with double
    /// quotes, so a single-quoted source value holding a literal `"` has to be
    /// re-encoded or the markup breaks at that quote.
    #[test]
    fn test_single_quoted_attribute_keeps_its_double_quotes() {
        let html_input = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US">
  <head>
  </head>
  <body>
    <a href="/us/ca/cities/san-mateo/ordinances/2020/19" title='say "hi"'>link</a>
  </body>
</html>
        "#;
        let expected_output = r#"
<!DOCTYPE html SYSTEM "about:legacy-compat">
<html lang="en-US">
  <head>
    <meta itemprop="historical-prefix" content="/_date/2025-03-04"/>
  </head>
  <body>
    <a href="/_date/2025-03-04/us/ca/cities/san-mateo/ordinances/2020/19" title="say &quot;hi&quot;">link</a>
  </body>
</html>
        "#;
        let version_date = "2025-03-04";
        let path = "/us/ca/cities/san-mateo/ordinances/2020/19";
        assert_html_transformation(
            "single_quoted_attribute",
            html_input,
            expected_output,
            path,
            version_date,
        );
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
