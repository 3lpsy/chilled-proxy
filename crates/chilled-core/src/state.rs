//! Shared per-registry application state handed to every request handler.

use std::sync::Arc;

use crate::cache::{FilteredMemo, MetadataCache};

/// Shared application state passed to every request handler (cheap to clone),
/// generic over a registry's config and metadata cache entry types.
pub struct AppState<C, E: Clone> {
    /// Immutable proxy configuration.
    pub config: Arc<C>,
    /// Shared connection-pooling HTTP client.
    pub client: reqwest::Client,
    /// Memoized filtered/rewritten metadata bodies.
    pub memo: Arc<FilteredMemo>,
    /// In-memory metadata cache (etag / mtime).
    pub metadata: Arc<MetadataCache<E>>,
}

// Derived `Clone` would demand `C: Clone`, which `Arc<C>` does not need.
impl<C, E: Clone> Clone for AppState<C, E> {
    fn clone(&self) -> Self {
        AppState {
            config: self.config.clone(),
            client: self.client.clone(),
            memo: self.memo.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

impl<C, E: Clone> AppState<C, E> {
    /// Builds fresh state around a registry config and a shared HTTP client.
    pub fn new(config: C, client: reqwest::Client) -> Self {
        AppState {
            config: Arc::new(config),
            client,
            memo: Arc::new(FilteredMemo::new()),
            metadata: Arc::new(MetadataCache::new()),
        }
    }
}
