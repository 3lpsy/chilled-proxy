//! Caching Maven repository proxy with metadata age-gating (cooldown).
//!
//! Serves a single wildcard route relative to its mount prefix (`/maven` in
//! chilled-proxy), classified by path shape: artifact-level `maven-metadata.xml`
//! (proxied, cached, age-gated, with checksums generated over the filtered
//! bytes) and artifact downloads (proxied, cached verbatim). Artifact bytes are
//! never modified — only metadata is filtered.

pub(crate) mod checksum;
pub(crate) mod constants;
pub(crate) mod coords;
pub(crate) mod filter;
pub(crate) mod model;
pub(crate) mod probe;
pub(crate) mod routes;
pub(crate) mod sidecar;
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

use crate::coords::MavenCoords;
use crate::routes::handle_maven;
use crate::state::AppState;

/// Default upstream Maven Central repository URL.
pub const MAVEN_CENTRAL_URL: &str = "https://repo.maven.apache.org/maven2/";

/// Maven proxy configuration (immutable after startup).
#[derive(Debug, Clone)]
pub struct Config {
    /// Upstream repository URL.
    pub(crate) upstream_url: Url,
    /// Common per-registry settings.
    pub(crate) settings: RegistrySettings,
    /// Repository cache directory (`<cache_dir>/repo`).
    pub(crate) repo_dir: PathBuf,
}

impl Config {
    /// Builds a configuration, deriving the `repo` cache subdirectory from the
    /// settings' cache dir.
    #[must_use]
    pub fn new(upstream_url: Url, settings: RegistrySettings) -> Self {
        let repo_dir = settings.cache_dir.join("repo");
        Config {
            upstream_url,
            settings,
            repo_dir,
        }
    }

    /// The age-gating cutoff for `coords`, or `None` when served unfiltered.
    pub(crate) fn cutoff_for(&self, coords: &MavenCoords) -> Option<u64> {
        self.settings.cutoff_for(&coords.override_key())
    }

    /// The ETag marker `coords` is served under, or `None` when unfiltered.
    pub(crate) fn serve_marker(&self, coords: &MavenCoords) -> Option<Marker> {
        self.settings.serve_marker(&coords.override_key())
    }
}

/// The Maven repository proxy, mountable under a path prefix.
#[derive(Clone)]
pub struct MavenProxy {
    state: AppState,
}

impl MavenProxy {
    /// Builds the proxy from its config and a shared HTTP client.
    pub fn new(config: Config, client: reqwest::Client) -> Self {
        MavenProxy {
            state: AppState {
                config: Arc::new(config),
                client,
                memo: Arc::new(FilteredMemo::new()),
                metadata: Arc::new(MetadataCache::new()),
            },
        }
    }
}

impl RegistryProxy for MavenProxy {
    fn id(&self) -> &'static str {
        "maven"
    }

    fn router(&self) -> Router {
        Router::new()
            .fallback(handle_maven)
            .with_state(self.state.clone())
    }

    fn cache_stats(&self) -> CacheStats {
        stats::cache_stats(&self.state.config.repo_dir)
    }
}
