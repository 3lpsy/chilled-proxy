//! Caching crates.io proxy with sparse-index age-gating (cooldown).
//!
//! Serves two routes relative to its mount prefix (`/crates` in chilled-proxy):
//! `GET /index/{*path}` (proxied, cached, age-gated sparse index) and
//! `GET /api/v1/crates/{*path}` (proxied, cached crate downloads). Crate bytes
//! are never modified — only index metadata is filtered.

pub(crate) mod cache;
pub(crate) mod config;
pub(crate) mod constants;
pub(crate) mod filter;
pub(crate) mod http;
pub(crate) mod routes;
pub(crate) mod state;
pub(crate) mod stats;
pub(crate) mod valid;

/// Built-in upstream size caps for this registry; the CLI uses them as the
/// defaults behind `--max-metadata-size` / `--max-artifact-size`.
pub use constants::{DEFAULT_MAX_ARTIFACT_SIZE, DEFAULT_MAX_METADATA_SIZE};

use axum::{routing::get, Router};
use chilled_core::http::error_response;
use chilled_core::registry::{CacheStats, RegistryProxy};

use crate::routes::{handle_download, handle_index};
use crate::state::AppState;

pub use crate::config::{Config, Upstreams};
pub use crate::constants::{CRATES_IO_URL, INDEX_CRATES_IO_URL};

/// The crates.io registry proxy, mountable under a path prefix.
#[derive(Clone)]
pub struct CratesProxy {
    state: AppState,
}

impl CratesProxy {
    /// Builds the proxy from its config and a shared HTTP client.
    pub fn new(config: Config, client: reqwest::Client) -> Self {
        CratesProxy {
            state: AppState::new(config, client),
        }
    }
}

impl RegistryProxy for CratesProxy {
    fn router(&self) -> Router {
        Router::new()
            .route("/index/{*path}", get(handle_index))
            .route("/api/v1/crates/{*path}", get(handle_download))
            .fallback(|| async { error_response(404) })
            .with_state(self.state.clone())
    }

    fn cache_stats(&self) -> CacheStats {
        stats::cache_stats(&self.state.config.crates_dir)
    }
}
