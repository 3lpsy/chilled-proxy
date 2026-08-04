//! Shared application state handed to every request handler.

use std::sync::Arc;

use chilled_core::cache::{FilteredMemo, MetadataCache};

use crate::model::MavenEntry;
use crate::Config;

/// Shared application state passed to every request handler (cheap to clone).
#[derive(Clone)]
pub(crate) struct AppState {
    /// Immutable proxy configuration.
    pub(crate) config: Arc<Config>,
    /// Shared connection-pooling HTTP client.
    pub(crate) client: reqwest::Client,
    /// Memoized filtered metadata bodies.
    pub(crate) memo: Arc<FilteredMemo>,
    /// In-memory metadata cache (etag / mtime), keyed by the artifact dir path.
    pub(crate) metadata: Arc<MetadataCache<MavenEntry>>,
}
