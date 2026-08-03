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

/// Parses a comma/whitespace-separated package list into a lower-cased set.
/// Registries with stronger normalization rules normalize again on lookup.
pub fn parse_overrides(list: &str) -> HashSet<String> {
    list.split([',', ' ', '\t', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}
