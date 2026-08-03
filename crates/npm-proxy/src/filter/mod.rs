//! Packument age-gating filter and tarball URL rewriting (serde_json).
//!
//! Versions published after the cutoff are dropped from `versions` and `time`,
//! dist-tags are repaired, and every surviving `dist.tarball` URL is rewritten
//! to this proxy so downloads flow through the cooldown gate.

#[cfg(test)]
mod tests;

use bytes::Bytes;
use chilled_core::time::parse_rfc3339z;
use log::debug;
use serde_json::Value;
use url::Url;

use crate::valid;

/// Successful filter summary.
pub(crate) struct FilterOutcome {
    /// Number of versions removed by the cooldown filter.
    pub(crate) removed: usize,
}

/// Result of filtering raw packument bytes.
pub(crate) enum FilterResult {
    /// Serialized filtered+rewritten packument.
    Body(Bytes),
    /// Every version fell inside the cooldown window (serve an npm 404).
    AllFiltered,
    /// The upstream body is not JSON we can filter.
    Invalid,
}

/// Parses, filters, rewrites, and re-serializes raw packument bytes.
pub(crate) fn filter_bytes(
    data: &[u8],
    cutoff: Option<u64>,
    proxy_url: &Url,
    name: &str,
) -> FilterResult {
    let Ok(mut doc) = serde_json::from_slice::<Value>(data) else {
        return FilterResult::Invalid;
    };
    match filter_packument(&mut doc, cutoff, proxy_url, name) {
        Some(outcome) => {
            if outcome.removed > 0 {
                debug!("cooldown: removed {} version(s) of {name}", outcome.removed);
            }
            match serde_json::to_vec(&doc) {
                Ok(bytes) => FilterResult::Body(Bytes::from(bytes)),
                Err(_) => FilterResult::Invalid,
            }
        }
        None => FilterResult::AllFiltered,
    }
}

/// Filters a full packument in place against `cutoff` (`None` = no filtering)
/// and rewrites tarball URLs to the proxy. Returns `None` if every version was
/// filtered (the caller serves an npm-style 404).
pub(crate) fn filter_packument(
    doc: &mut Value,
    cutoff: Option<u64>,
    proxy_url: &Url,
    name: &str,
) -> Option<FilterOutcome> {
    // Versions we cannot map onto a servable tarball path are dropped, not
    // left pointing at upstream: an un-rewritten URL would let clients fetch
    // straight from the registry, past the download gate.
    let mut removed = unservable_versions(doc);
    if let Some(cutoff) = cutoff {
        removed.extend(too_new_versions(doc, cutoff));
    }
    for key in &removed {
        if let Some(versions) = doc.get_mut("versions").and_then(Value::as_object_mut) {
            versions.remove(key);
        }
        if let Some(time) = doc.get_mut("time").and_then(Value::as_object_mut) {
            time.remove(key);
        }
    }
    if !removed.is_empty() {
        repair_dist_tags(doc, &removed);
    }
    if doc
        .get("versions")
        .and_then(Value::as_object)
        .is_none_or(serde_json::Map::is_empty)
    {
        return None;
    }
    rewrite_tarballs(doc, proxy_url, name);
    Some(FilterOutcome {
        removed: removed.len(),
    })
}

/// Versions whose string cannot form a valid tarball path, so the proxy could
/// never serve them.
fn unservable_versions(doc: &Value) -> Vec<String> {
    let Some(versions) = doc.get("versions").and_then(Value::as_object) else {
        return Vec::new();
    };
    versions
        .keys()
        .filter(|version| !valid::is_version(version))
        .cloned()
        .collect()
}

/// Versions in the `time` map published strictly after `cutoff`. Missing or
/// unparseable stamps are treated as old (kept), matching crates' behavior.
fn too_new_versions(doc: &Value, cutoff: u64) -> Vec<String> {
    let Some(time) = doc.get("time").and_then(Value::as_object) else {
        return Vec::new();
    };
    time.iter()
        .filter(|(key, _)| key.as_str() != "created" && key.as_str() != "modified")
        .filter(|(_, stamp)| {
            stamp
                .as_str()
                .and_then(parse_rfc3339z)
                .is_some_and(|secs| secs > cutoff)
        })
        .map(|(key, _)| key.clone())
        .collect()
}

/// Drops dist-tags targeting removed versions and repoints a lost `latest` at
/// the newest surviving version per the (pruned) time map.
fn repair_dist_tags(doc: &mut Value, removed: &[String]) {
    if removed.is_empty() {
        return;
    }
    let latest = newest_survivor(doc);
    let Some(tags) = doc.get_mut("dist-tags").and_then(Value::as_object_mut) else {
        return;
    };
    tags.retain(|_, target| {
        target
            .as_str()
            .is_none_or(|t| !removed.iter().any(|r| r == t))
    });
    if !tags.contains_key("latest") {
        if let Some(latest) = latest {
            tags.insert("latest".to_owned(), Value::String(latest));
        }
    }
}

/// The surviving version with the greatest publish time, if any.
fn newest_survivor(doc: &Value) -> Option<String> {
    let versions = doc.get("versions").and_then(Value::as_object)?;
    let time = doc.get("time").and_then(Value::as_object)?;
    time.iter()
        .filter(|(key, _)| versions.contains_key(key.as_str()))
        .filter_map(|(key, stamp)| {
            stamp
                .as_str()
                .and_then(parse_rfc3339z)
                .map(|secs| (secs, key))
        })
        .max_by_key(|(secs, _)| *secs)
        .map(|(_, key)| key.clone())
}

/// Rewrites every version's `dist.tarball` to this proxy's tarball route.
fn rewrite_tarballs(doc: &mut Value, proxy_url: &Url, name: &str) {
    let unscoped = name.rsplit('/').next().unwrap_or(name).to_owned();
    let Some(versions) = doc.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for (version, entry) in versions.iter_mut() {
        // Unservable versions were dropped before this point.
        debug_assert!(valid::is_version(version));
        if let Some(tarball) = entry.get_mut("dist").and_then(|d| d.get_mut("tarball")) {
            *tarball = Value::String(format!("{proxy_url}{name}/-/{unscoped}-{version}.tgz"));
        }
    }
}
