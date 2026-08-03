//! Immutable crates-proxy configuration.

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use chilled_core::config::RegistrySettings;
use chilled_core::etag::Marker;
use url::Url;

/// crates.io proxy configuration (immutable after startup).
#[derive(Debug, Clone)]
pub struct Config {
    /// Upstream registry index URL.
    pub(crate) index_url: Url,
    /// Upstream crate download URL.
    pub(crate) upstream_url: Url,
    /// Common per-registry settings (cooldown, TTL, overrides, proxy URL, ...).
    pub(crate) settings: RegistrySettings,
    /// Registry index cache directory (`<cache_dir>/index`).
    pub(crate) index_dir: PathBuf,
    /// Crate files cache directory (`<cache_dir>/crates`).
    pub(crate) crates_dir: PathBuf,
}

impl Config {
    /// Builds a configuration, deriving the `index`/`crates` cache
    /// subdirectories from the settings' cache dir.
    #[must_use]
    pub fn new(index_url: Url, upstream_url: Url, settings: RegistrySettings) -> Self {
        let index_dir = settings.cache_dir.join("index");
        let crates_dir = settings.cache_dir.join("crates");
        Config {
            index_url,
            upstream_url,
            settings,
            index_dir,
            crates_dir,
        }
    }

    /// The age-gating cutoff for `name`, or `None` when served unfiltered.
    /// Override lookup is case-insensitive (crates.io names are).
    pub(crate) fn cutoff_for(&self, name: &str) -> Option<u64> {
        self.settings.cutoff_for(&name.to_ascii_lowercase())
    }

    /// The ETag marker `name` is served under, or `None` when unfiltered.
    pub(crate) fn serve_marker(&self, name: &str) -> Option<Marker> {
        self.settings.serve_marker(&name.to_ascii_lowercase())
    }
}
