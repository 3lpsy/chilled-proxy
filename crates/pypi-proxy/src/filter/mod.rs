//! PEP 691 age-gating filter and file-URL rewriting.

#[cfg(test)]
mod tests;

use std::collections::HashSet;

use chilled_core::time::parse_rfc3339z;
use serde_json::Value;
use url::Url;

use crate::valid;

/// Parses a PyPI `upload-time` value into unix seconds. PyPI mostly emits
/// `...Z`, but `+00:00` and bare (no-zone) forms appear too; normalize first.
pub(crate) fn parse_upload_time(s: &str) -> Option<u64> {
    let t = s.trim();
    if let Some(base) = t.strip_suffix("+00:00") {
        return parse_rfc3339z(&format!("{base}Z"));
    }
    if t.ends_with('Z') {
        parse_rfc3339z(t)
    } else {
        parse_rfc3339z(&format!("{t}Z"))
    }
}

/// Maps a distribution filename to its version. Wheels and eggs take the 2nd
/// `-`-separated field; for sdists the split point is the one whose left side
/// normalizes to `project`, so a version containing `-` survives intact.
pub(crate) fn filename_version<'a>(filename: &'a str, project: &str) -> Option<&'a str> {
    for ext in [".whl", ".egg"] {
        if let Some(stem) = filename.strip_suffix(ext) {
            let mut fields = stem.split('-');
            fields.next()?;
            return fields.next();
        }
    }
    let stem = filename
        .strip_suffix(".tar.gz")
        .or_else(|| filename.strip_suffix(".zip"))
        .or_else(|| filename.strip_suffix(".tar.bz2"))?;
    stem.match_indices('-')
        .find(|(i, _)| valid::normalize(&stem[..*i]) == project)
        .map(|(i, _)| &stem[i + 1..])
        .or_else(|| stem.rsplit_once('-').map(|(_, version)| version))
}

/// The `upload-time` of one `files[]` entry, in unix seconds.
fn file_upload_secs(file: &Value) -> Option<u64> {
    parse_upload_time(file.get("upload-time")?.as_str()?)
}

/// Rewrites one upstream file URL to `{proxy_url}files/{project}/{tail}`,
/// where `tail` is the upstream URL's path without its leading `/`.
fn rewrite_file_url(url: &str, project: &str, proxy_url: &Url) -> String {
    let tail = match Url::parse(url) {
        Ok(parsed) => parsed.path().trim_start_matches('/').to_owned(),
        Err(_) => url.trim_start_matches('/').to_owned(),
    };
    format!("{proxy_url}files/{project}/{tail}")
}

/// The set of versions the doc's files account for.
fn file_versions(doc: &Value, project: &str) -> HashSet<String> {
    doc.get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| f.get("filename")?.as_str())
        .filter_map(|name| filename_version(name, project))
        .map(str::to_owned)
        .collect()
}

/// Filters a PEP 691 project doc in place against `cutoff` (`None` = keep all)
/// and rewrites file URLs through the proxy.
pub(crate) fn filter_simple_json(
    doc: &mut Value,
    cutoff: Option<u64>,
    project: &str,
    proxy_url: &Url,
) {
    if let Some(cutoff) = cutoff {
        // The versions the files could account for *before* filtering. A
        // version upstream lists without any file (its releases were removed)
        // is none of our business and must survive untouched.
        let backed = file_versions(doc, project);

        // Fail closed: a file without a parseable upload-time is dropped.
        if let Some(files) = doc.get_mut("files").and_then(Value::as_array_mut) {
            files.retain(|f| file_upload_secs(f).is_some_and(|secs| secs <= cutoff));
        }

        // Drop only versions we actually filtered every file out of.
        let survivors = file_versions(doc, project);
        match doc.get_mut("versions").and_then(Value::as_array_mut) {
            Some(versions) => versions.retain(|v| {
                v.as_str()
                    .is_some_and(|s| survivors.contains(s) || !backed.contains(s))
            }),
            // `versions` is optional in PEP 691 1.0, and absent entirely from a
            // PEP 503 page. Derive it so every served document describes its own
            // surviving files, whatever dialect upstream spoke.
            None => {
                let mut derived: Vec<String> = survivors.into_iter().collect();
                derived.sort();
                doc["versions"] = Value::Array(derived.into_iter().map(Value::String).collect());
            }
        }
    }

    // Rewrite every surviving file URL through the proxy files route.
    if let Some(files) = doc.get_mut("files").and_then(Value::as_array_mut) {
        for file in files {
            if let Some(url) = file.get("url").and_then(Value::as_str) {
                let rewritten = rewrite_file_url(url, project, proxy_url);
                file["url"] = Value::String(rewritten);
            }
        }
    }
}
