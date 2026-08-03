//! Test data and small helpers: packument bodies and sentinel publish times.
#![allow(dead_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map};

/// A publish time that is always older than any plausible cooldown cutoff.
pub const OLD: &str = "2000-01-01T00:00:00Z";
/// A publish time that is always newer than the cutoff (for any cooldown > 0).
pub const TOO_NEW: &str = "2999-01-01T00:00:00Z";

/// Opaque tarball payload. The proxy treats tarball bytes as opaque, so the
/// exact contents don't matter — only that they round-trip byte-for-byte.
pub const TARBALL_BYTES: &[u8] = b"\x1f\x8b\x08\x00chilled-npm-test-tarball-bytes\x00";

/// The default upstream ETag mounted by the proxy harness.
pub const ETAG: &str = "\"etag123\"";

/// Builds a full packument body: `dist-tags.latest` = last entry, a `time` map
/// with `created`/`modified`, and per-version tarball URLs under `upstream`.
pub fn packument(name: &str, versions: &[(&str, &str)], upstream: &str) -> String {
    packument_with_tags(name, versions, &[], upstream)
}

/// Like [`packument`], with extra dist-tags appended.
pub fn packument_with_tags(
    name: &str,
    versions: &[(&str, &str)],
    tags: &[(&str, &str)],
    upstream: &str,
) -> String {
    let unscoped = name.rsplit('/').next().unwrap();
    let mut time = Map::new();
    let mut vers = Map::new();
    time.insert(
        "created".into(),
        json!(versions.first().map_or(OLD, |(_, t)| *t)),
    );
    time.insert(
        "modified".into(),
        json!(versions.last().map_or(OLD, |(_, t)| *t)),
    );
    for (v, t) in versions {
        time.insert((*v).into(), json!(t));
        vers.insert(
            (*v).into(),
            json!({
                "name": name,
                "version": v,
                "dist": {
                    "tarball": format!("{upstream}{name}/-/{unscoped}-{v}.tgz"),
                    "shasum": "da39a3ee5e6b4b0d3255bfef95601890afd80709",
                    "integrity": "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg==",
                },
            }),
        );
    }
    let mut dist_tags = Map::new();
    if let Some((last, _)) = versions.last() {
        dist_tags.insert("latest".into(), json!(last));
    }
    for (tag, target) in tags {
        dist_tags.insert((*tag).into(), json!(target));
    }
    json!({"name": name, "dist-tags": dist_tags, "versions": vers, "time": time}).to_string()
}

/// Formats `now + offset_secs` as an RFC3339 UTC timestamp (`...Z`). Used by
/// boundary tests that need publish times near the cooldown cutoff.
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
