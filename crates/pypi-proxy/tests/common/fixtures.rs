//! Test data and small helpers: PEP 691 simple-index bodies, sentinel
//! upload-times, and boundary timestamp formatting.
#![allow(dead_code)]

use serde_json::json;

pub use chilled_testkit::{rfc3339_from_now, OLD, TOO_NEW};

/// The PEP 691 JSON simple content type (mirrors the crate-private constant).
pub const SIMPLE_CTYPE: &str = "application/vnd.pypi.simple.v1+json";

/// A sha256 hex sentinel used by the default fixtures.
pub const SHA: &str = "aa11bb22cc33dd44ee55ff667788990011223344556677889900aabbccddeeff";

/// Opaque wheel payload; the proxy treats file bytes as opaque.
pub const FILE_BYTES: &[u8] = b"PK\x03\x04chilled-crates-test-wheel-bytes";

/// Maps a distribution filename to its version, mirroring the proxy's rule
/// (wheel: 2nd `-` field; sdist: after the last `-` of the stem).
fn filename_version(filename: &str) -> Option<String> {
    if let Some(stem) = filename.strip_suffix(".whl") {
        return stem.split('-').nth(1).map(str::to_owned);
    }
    let stem = filename
        .strip_suffix(".tar.gz")
        .or_else(|| filename.strip_suffix(".zip"))
        .or_else(|| filename.strip_suffix(".tar.bz2"))?;
    stem.rsplit_once('-').map(|(_, v)| v.to_owned())
}

/// Builds a PEP 691 project document for `project` from
/// `(filename, upload_time, sha256)` triples. File URLs point at
/// files.pythonhosted.org-style paths; `versions` is derived from the
/// filenames. An empty `upload_time` omits the key entirely.
pub fn simple_json(project: &str, files: &[(&str, &str, &str)]) -> String {
    let mut versions = Vec::new();
    let file_objs: Vec<serde_json::Value> = files
        .iter()
        .map(|(filename, upload_time, sha256)| {
            if let Some(v) = filename_version(filename) {
                if !versions.contains(&v) {
                    versions.push(v);
                }
            }
            let mut obj = json!({
                "filename": filename,
                "url": format!("https://files.pythonhosted.org/packages/aa/bb/cc/{filename}"),
                "hashes": {"sha256": sha256},
                "requires-python": ">=3.8",
            });
            if !upload_time.is_empty() {
                obj["upload-time"] = json!(upload_time);
            }
            obj
        })
        .collect();

    json!({
        "meta": {"api-version": "1.0"},
        "name": project,
        "versions": versions,
        "files": file_objs,
    })
    .to_string()
}

/// Builds a PEP 503 HTML project page from `(filename, upload_time, sha256)`
/// triples, in the shape an HTML-only index (PyTorch, devpi) serves. An empty
/// `upload_time` omits the `data-upload-time` attribute entirely.
pub fn simple_html(project: &str, files: &[(&str, &str, &str)]) -> String {
    let mut out = format!(
        "<!DOCTYPE html><html><head><title>Links for {project}</title></head>\
         <body><h1>Links for {project}</h1>\n"
    );
    for (filename, upload_time, sha256) in files {
        let time = if upload_time.is_empty() {
            String::new()
        } else {
            format!(" data-upload-time=\"{upload_time}\"")
        };
        out.push_str(&format!(
            "<a href=\"https://files.pythonhosted.org/packages/aa/bb/cc/{filename}#sha256={sha256}\"\
             {time} data-requires-python=\"&gt;=3.8\">{filename}</a><br/>\n"
        ));
    }
    out.push_str("</body></html>");
    out
}
