//! Converting scanned anchors into a PEP 691 document.
//!
//! Hashes come from the href fragment (PEP 503), upload times from the
//! `data-upload-time` attribute some indexes publish (PyTorch, devpi);
//! entries without one stay undatable and drop under a cooldown.

use serde_json::{Map, Value};
use url::Url;

use crate::constants::SIMPLE_HTML_CTYPE;
use crate::html::entity::unescape;
use crate::html::scan::{anchors, Anchor};

/// Whether a content type is a PEP 503 / PEP 691 HTML simple index.
pub(crate) fn is_html_simple(ctype: &str) -> bool {
    ctype.split(';').next().is_some_and(|t| {
        let t = t.trim();
        t.eq_ignore_ascii_case("text/html") || t.eq_ignore_ascii_case(SIMPLE_HTML_CTYPE)
    })
}

/// Parses a PEP 503 project page into a PEP 691 document.
///
/// `base` is the URL the page was fetched from, so relative hrefs (which PEP 503
/// permits) resolve to the same absolute URLs a JSON index would have given.
pub(crate) fn parse_simple_html(body: &str, project: &str, base: &Url) -> Value {
    let files: Vec<Value> = anchors(body)
        .iter()
        .filter_map(|a| anchor_to_file(a, base))
        .collect();

    let mut meta = Map::new();
    meta.insert("api-version".to_owned(), Value::String("1.0".to_owned()));

    let mut doc = Map::new();
    doc.insert("meta".to_owned(), Value::Object(meta));
    doc.insert("name".to_owned(), Value::String(project.to_owned()));
    doc.insert("files".to_owned(), Value::Array(files));
    Value::Object(doc)
}

/// Whether any entry carries an upload time. A cooldown cannot be honored on a
/// document where nothing is datable, and saying so beats serving an empty one.
pub(crate) fn has_upload_times(doc: &Value) -> bool {
    doc.get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|f| f.get("upload-time").and_then(Value::as_str).is_some())
}

/// Converts one anchor into a PEP 691 file entry, or `None` when it carries no
/// usable link — a malformed entry must not hide the rest of the index.
fn anchor_to_file(anchor: &Anchor, base: &Url) -> Option<Value> {
    let resolved = base.join(&unescape(anchor.attr("href")?)).ok()?;

    // PEP 503 puts the hash in the fragment as `#<algo>=<value>`.
    let mut hashes = Map::new();
    if let Some((algo, value)) = resolved.fragment().and_then(|f| f.split_once('=')) {
        if !algo.is_empty() && !value.is_empty() {
            hashes.insert(algo.to_ascii_lowercase(), Value::String(value.to_owned()));
        }
    }
    let mut url = resolved.clone();
    url.set_fragment(None);

    // The link text is the filename; fall back to the URL's last segment for
    // pages that wrap it in markup or leave it empty.
    let text = unescape(anchor.text.trim());
    let filename = if text.is_empty() {
        url.path_segments()?.next_back()?.to_owned()
    } else {
        text
    };
    if filename.is_empty() {
        return None;
    }

    let mut file = Map::new();
    file.insert("filename".to_owned(), Value::String(filename));
    file.insert("url".to_owned(), Value::String(url.into()));
    file.insert("hashes".to_owned(), Value::Object(hashes));
    insert_attr(&mut file, anchor, "data-upload-time", "upload-time");
    insert_attr(&mut file, anchor, "data-requires-python", "requires-python");

    // `data-yanked` present-but-empty means yanked without a reason.
    if let Some(y) = anchor.attr("data-yanked") {
        let y = unescape(y);
        let value = if y.is_empty() {
            Value::Bool(true)
        } else {
            Value::String(y)
        };
        file.insert("yanked".to_owned(), value);
    }

    // PEP 714 renamed the metadata attribute; accept both spellings.
    if let Some(m) = anchor
        .attr("data-core-metadata")
        .or_else(|| anchor.attr("data-dist-info-metadata"))
    {
        file.insert("core-metadata".to_owned(), core_metadata(&unescape(m)));
    }

    Some(Value::Object(file))
}

/// Copies a non-empty anchor attribute into the entry under `key`.
fn insert_attr(file: &mut Map<String, Value>, anchor: &Anchor, attr: &str, key: &str) {
    if let Some(raw) = anchor.attr(attr) {
        let value = unescape(raw);
        if !value.is_empty() {
            file.insert(key.to_owned(), Value::String(value));
        }
    }
}

/// A `core-metadata` value: `{algo: digest}` when the attribute names one, else
/// `true` for a bare "metadata is available".
fn core_metadata(raw: &str) -> Value {
    match raw.split_once('=') {
        Some((algo, digest)) if !algo.is_empty() && !digest.is_empty() => {
            let mut map = Map::new();
            map.insert(algo.to_ascii_lowercase(), Value::String(digest.to_owned()));
            Value::Object(map)
        }
        _ => Value::Bool(true),
    }
}

#[cfg(test)]
mod tests;
