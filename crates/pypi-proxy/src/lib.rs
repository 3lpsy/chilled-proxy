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

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
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

/// Upstream endpoints for a PyPI-style registry. Named fields, because both
/// are URLs and a silent swap would be invisible to the type system.
#[derive(Debug, Clone)]
pub struct Upstreams {
    /// Simple-index URL.
    pub simple: Url,
    /// Default file-hosting URL.
    pub files: Url,
}

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
    /// Hosts this mount may fetch distribution files from.
    ///
    /// An index names each file's host itself, and one index can spread its
    /// files across several (PyTorch links `torch` at its own CDN, its
    /// dependencies at PyPI's, and some wheels relatively). Resolving the host
    /// from the document rather than from config is what makes those mounts
    /// work — but it also means a hostile upstream could name *any* host, so
    /// the resolved host must appear here or the download is refused.
    pub(crate) file_hosts: HashSet<String>,
}

impl Config {
    /// Builds a configuration, deriving the `simple`/`files` cache
    /// subdirectories from the settings' cache dir. `extra_file_hosts` names
    /// additional hosts this mount's index may serve files from, plainly
    /// declared by the operator.
    #[must_use]
    pub fn new(
        upstreams: Upstreams,
        mut settings: RegistrySettings,
        extra_file_hosts: &[String],
    ) -> Self {
        let Upstreams {
            simple: upstream_url,
            files: files_url,
        } = upstreams;
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
        // The index's own host covers relative links; the files host covers the
        // ordinary single-host case. Both are already operator-chosen.
        let mut file_hosts = HashSet::new();
        for url in [&upstream_url, &files_url] {
            if let Some(host) = url.host_str() {
                file_hosts.insert(host.to_ascii_lowercase());
            }
        }
        file_hosts.extend(
            extra_file_hosts
                .iter()
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty()),
        );

        Config {
            upstream_url,
            files_url,
            settings,
            simple_dir,
            files_dir,
            file_hosts,
        }
    }

    /// Whether this mount may fetch a distribution file from `url`.
    pub(crate) fn allows_file_host(&self, url: &Url) -> bool {
        url.host_str()
            .is_some_and(|host| self.file_hosts.contains(&host.to_ascii_lowercase()))
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
            state: AppState::new(config, client),
        }
    }
}

impl RegistryProxy for PypiProxy {
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
