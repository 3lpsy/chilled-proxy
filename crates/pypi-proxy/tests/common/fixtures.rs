//! Test data and small helpers: PEP 691 simple-index bodies, sentinel
//! upload-times, and boundary timestamp formatting.
#![allow(dead_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

/// An `upload-time` always older than any plausible cooldown cutoff.
pub const OLD: &str = "2000-01-01T00:00:00Z";
/// An `upload-time` always newer than the cutoff (for any cooldown > 0).
pub const TOO_NEW: &str = "2999-01-01T00:00:00Z";

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

/// Formats `now + offset_secs` as an RFC3339 UTC timestamp (`...Z`). Used by
/// the boundary test, which needs an upload-time exactly at the cutoff.
pub fn rfc3339_from_now(offset_secs: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    rfc3339(now + offset_secs)
}

fn rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Days since 1970-01-01 → civil date (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
