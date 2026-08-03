//! Caching npm registry proxy with packument age-gating (cooldown).
//!
//! Serves the npm registry protocol relative to its mount prefix (`/npm` in
//! chilled-proxy): packuments, per-version docs, and tarball downloads.
//! Tarball bytes are never modified — only packument metadata is filtered,
//! and its tarball URLs are rewritten to point back at this proxy.

pub(crate) mod constants;
pub(crate) mod filter;
pub(crate) mod http;
pub(crate) mod model;
pub(crate) mod routes;
pub(crate) mod state;
pub(crate) mod stats;
pub(crate) mod valid;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use chilled_core::cache::{FilteredMemo, MetadataCache};
use chilled_core::config::RegistrySettings;
use chilled_core::etag::Marker;
use chilled_core::registry::{CacheStats, RegistryProxy};
use url::Url;

use crate::routes::handle_npm;
use crate::state::AppState;

/// Default upstream npm registry URL.
pub const NPM_REGISTRY_URL: &str = "https://registry.npmjs.org/";

/// npm proxy configuration (immutable after startup).
#[derive(Debug, Clone)]
pub struct Config {
    /// Upstream registry URL.
    pub(crate) upstream_url: Url,
    /// Common per-registry settings.
    pub(crate) settings: RegistrySettings,
    /// Pristine packument cache directory (`<cache_dir>/packuments`).
    pub(crate) packuments_dir: PathBuf,
    /// Tarball cache directory (`<cache_dir>/tarballs`).
    pub(crate) tarballs_dir: PathBuf,
}

impl Config {
    /// Builds a configuration, deriving the `packuments`/`tarballs` cache
    /// subdirectories from the settings' cache dir.
    #[must_use]
    pub fn new(upstream_url: Url, settings: RegistrySettings) -> Self {
        let packuments_dir = settings.cache_dir.join("packuments");
        let tarballs_dir = settings.cache_dir.join("tarballs");
        Config {
            upstream_url,
            settings,
            packuments_dir,
            tarballs_dir,
        }
    }

    /// The age-gating cutoff for `name`, or `None` when served unfiltered.
    /// npm names are lowercase; legacy uppercase is lowercased for the lookup.
    pub(crate) fn cutoff_for(&self, name: &str) -> Option<u64> {
        self.settings.cutoff_for(&name.to_ascii_lowercase())
    }

    /// The ETag marker `name` is served under, or `None` when unfiltered.
    pub(crate) fn serve_marker(&self, name: &str) -> Option<Marker> {
        self.settings.serve_marker(&name.to_ascii_lowercase())
    }
}

/// The npm registry proxy, mountable under a path prefix.
#[derive(Clone)]
pub struct NpmProxy {
    state: AppState,
}

impl NpmProxy {
    /// Builds the proxy from its config and a shared HTTP client.
    pub fn new(config: Config, client: reqwest::Client) -> Self {
        NpmProxy {
            state: AppState {
                config: Arc::new(config),
                client,
                memo: Arc::new(FilteredMemo::new()),
                metadata: Arc::new(MetadataCache::new()),
            },
        }
    }
}

impl RegistryProxy for NpmProxy {
    fn id(&self) -> &'static str {
        "npm"
    }

    fn router(&self) -> Router {
        // One fallback handler: npm paths need raw-URI classification.
        Router::new()
            .fallback(handle_npm)
            .with_state(self.state.clone())
    }

    fn cache_stats(&self) -> CacheStats {
        stats::cache_stats(&self.state.config.tarballs_dir)
    }
}
