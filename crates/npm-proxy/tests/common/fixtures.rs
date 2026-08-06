//! Test data and small helpers: packument bodies and sentinel publish times.
#![allow(dead_code)]

use serde_json::{json, Map};

pub use chilled_testkit::{rfc3339_from_now, OLD, TOO_NEW};

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
