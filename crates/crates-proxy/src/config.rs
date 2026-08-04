//! Immutable crates-proxy configuration.

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::Duration;

    fn config(cooldown_secs: u64, overrides: &[&str]) -> Config {
        Config::new(
            Url::parse("https://index.crates.io/").unwrap(),
            Url::parse("https://crates.io/").unwrap(),
            RegistrySettings {
                cache_dir: PathBuf::from("/var/cache/chilled/crates"),
                cache_ttl: Duration::from_secs(3600),
                cooldown: Duration::from_secs(cooldown_secs),
                overrides: Arc::new(
                    overrides
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<HashSet<_>>(),
                ),
                restrict_downloads: false,
                proxy_url: Url::parse("http://localhost:3080/crates/").unwrap(),
                max_metadata_size: 0x400_0000,
                max_artifact_size: 0x2000_0000,
            },
        )
    }

    #[test]
    fn derives_cache_subdirs() {
        let c = config(0, &[]);
        assert_eq!(
            c.index_dir,
            PathBuf::from("/var/cache/chilled/crates/index")
        );
        assert_eq!(
            c.crates_dir,
            PathBuf::from("/var/cache/chilled/crates/crates")
        );
    }

    #[test]
    fn override_lookup_is_case_insensitive() {
        let c = config(86_400, &["serde"]);
        assert_eq!(c.cutoff_for("Serde"), None);
        assert_eq!(c.serve_marker("SERDE"), None);
        assert!(c.cutoff_for("tokio").is_some());
        assert_eq!(c.serve_marker("tokio").unwrap().window, 86_400);
    }
}
