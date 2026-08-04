//! Per-registry runtime settings and small config parsing helpers.

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use url::Url;

use crate::cache::MEMO_BUCKET_SECS;
use crate::cooldown;
use crate::etag::Marker;

/// Settings every registry proxy shares. Built by the CLI layer, which resolves
/// per-registry overrides against the general flags.
#[derive(Debug, Clone)]
pub struct RegistrySettings {
    /// This registry's cache directory (e.g. `/var/cache/chilled/npm`).
    pub cache_dir: PathBuf,
    /// Metadata cache entry Time-to-Live.
    pub cache_ttl: Duration,
    /// Age-gating window; a zero duration disables filtering.
    pub cooldown: Duration,
    /// Registry-normalized package names exempt from age-gating.
    pub overrides: Arc<HashSet<String>>,
    /// Also refuse to *download* artifacts newer than the cooldown.
    pub restrict_downloads: bool,
    /// External URL of this registry's mount on the proxy (with trailing slash).
    pub proxy_url: Url,
    /// Cap on a metadata document fetched from upstream (index/packument/simple
    /// JSON/maven-metadata.xml). Over it, the fetch fails with 507.
    pub max_metadata_size: usize,
    /// Cap on an artifact fetched from upstream (crate/tarball/wheel/jar).
    ///
    /// Bodies are read into memory before being cached and served, so this is
    /// also the per-request memory ceiling — raising it far past the default
    /// trades a clean 507 for memory pressure under concurrency.
    pub max_artifact_size: usize,
}

impl RegistrySettings {
    /// The age-gating cutoff (unix seconds) for a package, or `None` when it is
    /// served unfiltered. `name` must already be normalized per registry rules.
    pub fn cutoff_for(&self, name: &str) -> Option<u64> {
        if self.overrides.contains(name) {
            return None;
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        cooldown::cutoff_from(now, self.cooldown)
    }

    /// The cooldown window (seconds) a package is served under right now, or
    /// `None` when it is served unfiltered.
    pub fn serve_window(&self, name: &str) -> Option<u64> {
        self.cutoff_for(name).map(|_| self.cooldown.as_secs())
    }

    /// The ETag marker a filtered body is served under right now, or `None`
    /// when the package is served unfiltered. The bucket component makes a
    /// client's cached copy stale once versions age past the cutoff.
    pub fn serve_marker(&self, name: &str) -> Option<Marker> {
        self.cutoff_for(name).map(|cutoff| Marker {
            window: self.cooldown.as_secs(),
            bucket: cutoff / MEMO_BUCKET_SECS,
        })
    }
}

/// Normalizes a requested log level to a known value, defaulting to `info`.
pub fn normalize_log_level(level: Option<String>) -> String {
    match level.as_deref().map(str::trim).map(str::to_ascii_lowercase) {
        Some(l)
            if matches!(
                l.as_str(),
                "error" | "warn" | "info" | "debug" | "trace" | "off"
            ) =>
        {
            l
        }
        _ => "info".to_string(),
    }
}

/// Parses a byte size: a plain byte count, or a number with a unit suffix
/// (`k`, `m`, `g`, case-insensitive, optionally spelled `KB`/`KiB`/…).
///
/// Units are powers of 1024 in every spelling — `1MB` and `1MiB` both mean
/// 1048576. Ops-facing sizes are meant in binary units, and honoring the
/// SI/IEC distinction here would make `--max-artifact-size 512MB` quietly
/// smaller than the 512 MiB default it is meant to raise.
pub fn parse_size(s: &str) -> Result<usize, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size".to_string());
    }

    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if split == 0 {
        return Err(format!("invalid size value in '{s}' (expected a number)"));
    }
    let (digits, suffix) = s.split_at(split);

    // `b`/`ib` is decoration on the unit letter: k, kb, and kib all mean 1024.
    let suffix = suffix.trim().to_ascii_lowercase();
    let unit = suffix
        .strip_suffix("ib")
        .or_else(|| suffix.strip_suffix('b'))
        .unwrap_or(&suffix);
    let mult: usize = match unit {
        "" => 1,
        "k" => 1024,
        "m" => 1024 * 1024,
        "g" => 1024 * 1024 * 1024,
        _ => {
            return Err(format!(
                "invalid size unit '{suffix}' in '{s}' (use k, m, or g)"
            ))
        }
    };

    let value: usize = digits
        .parse()
        .map_err(|_| format!("invalid size value in '{s}'"))?;
    value
        .checked_mul(mult)
        .ok_or_else(|| format!("size '{s}' is too large"))
}

/// Parses a comma/whitespace-separated package list into a lower-cased set.
/// Registries with stronger normalization rules normalize again on lookup.
pub fn parse_overrides(list: &str) -> HashSet<String> {
    list.split([',', ' ', '\t', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}
