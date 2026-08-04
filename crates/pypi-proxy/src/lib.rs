//! Caching PyPI simple-index proxy with upload-time age-gating (cooldown).
//!
//! Serves two route families relative to its mount prefix (`/pypi` in
//! chilled-proxy): `GET /simple/{project}/` (proxied, cached, age-gated
//! PEP 691/503 project indexes with proxied file URLs) and
//! `GET /files/{project}/{path}` (proxied, cached distribution downloads).

pub(crate) mod accept;
pub(crate) mod constants;
pub(crate) mod filter;
pub(crate) mod html;
pub(crate) mod model;
pub(crate) mod render;
pub(crate) mod routes;
pub(crate) mod state;
pub(crate) mod stats;
pub(crate) mod valid;

/// Built-in upstream size caps for this registry; the CLI uses them as the
/// defaults behind `--max-metadata-size` / `--max-artifact-size`.
pub use constants::{DEFAULT_MAX_ARTIFACT_SIZE, DEFAULT_MAX_METADATA_SIZE};

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use chilled_core::cache::{FilteredMemo, MetadataCache};
use chilled_core::config::RegistrySettings;
use chilled_core::etag::Marker;
use chilled_core::registry::{CacheStats, RegistryProxy};
use url::Url;

use crate::routes::handle_pypi;
use crate::state::AppState;

/// Default upstream PyPI simple-index URL.
pub const PYPI_SIMPLE_URL: &str = "https://pypi.org/simple/";

/// Default upstream PyPI file-hosting URL.
pub const PYPI_FILES_URL: &str = "https://files.pythonhosted.org/";

/// PyPI proxy configuration (immutable after startup).
#[derive(Debug, Clone)]
pub struct Config {
    /// Upstream simple-index URL.
    pub(crate) upstream_url: Url,
    /// Upstream file-hosting URL.
    pub(crate) files_url: Url,
    /// Common per-registry settings.
    pub(crate) settings: RegistrySettings,
    /// Simple-index cache directory (`<cache_dir>/simple`).
    pub(crate) simple_dir: PathBuf,
    /// Distribution file cache directory (`<cache_dir>/files`).
    pub(crate) files_dir: PathBuf,
}

impl Config {
    /// Builds a configuration, deriving the `simple`/`files` cache
    /// subdirectories from the settings' cache dir.
    #[must_use]
    pub fn new(upstream_url: Url, files_url: Url, mut settings: RegistrySettings) -> Self {
        let simple_dir = settings.cache_dir.join("simple");
        let files_dir = settings.cache_dir.join("files");
        // PEP 503-normalize override entries so `Foo.Bar` matches `foo-bar`.
        settings.overrides = Arc::new(
            settings
                .overrides
                .iter()
                .map(|name| valid::normalize(name))
                .collect(),
        );
        Config {
            upstream_url,
            files_url,
            settings,
            simple_dir,
            files_dir,
        }
    }

    /// The age-gating cutoff for a normalized project, or `None` if unfiltered.
    pub(crate) fn cutoff_for(&self, normalized: &str) -> Option<u64> {
        self.settings.cutoff_for(normalized)
    }

    /// The ETag marker a normalized project is served under, or `None`.
    pub(crate) fn serve_marker(&self, normalized: &str) -> Option<Marker> {
        self.settings.serve_marker(normalized)
    }
}

/// The PyPI registry proxy, mountable under a path prefix.
#[derive(Clone)]
pub struct PypiProxy {
    state: AppState,
}

impl PypiProxy {
    /// Builds the proxy from its config and a shared HTTP client.
    pub fn new(config: Config, client: reqwest::Client) -> Self {
        PypiProxy {
            state: AppState {
                config: Arc::new(config),
                client,
                memo: Arc::new(FilteredMemo::new()),
                metadata: Arc::new(MetadataCache::new()),
            },
        }
    }
}

impl RegistryProxy for PypiProxy {
    fn id(&self) -> &'static str {
        "pypi"
    }

    fn router(&self) -> Router {
        // A single fallback classifies the raw path itself (decode-once).
        Router::new()
            .fallback(handle_pypi)
            .with_state(self.state.clone())
    }

    fn cache_stats(&self) -> CacheStats {
        stats::cache_stats(&self.state.config.files_dir)
    }
}
