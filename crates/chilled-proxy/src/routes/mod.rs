//! Top-level HTTP request handlers, one module per route.

pub(crate) mod healthz;
pub(crate) mod home;
pub(crate) mod metrics;

pub(crate) use healthz::handle_healthz;
pub(crate) use home::handle_home;
pub(crate) use metrics::handle_metrics;

use std::sync::Arc;

use chilled_core::registry::RegistryProxy;

/// One mounted registry instance. The name lives here rather than on the proxy
/// because a registry can be mounted more than once, each mount with its own
/// upstream, cache directory, and `/metrics` key.
pub(crate) struct MountedRegistry {
    /// Instance name — the `/metrics` key and cache subdirectory.
    pub(crate) name: String,
    /// The proxy serving this mount.
    pub(crate) proxy: Arc<dyn RegistryProxy>,
}

/// Shared top-level state: the mounted registries (cheap to clone).
#[derive(Clone)]
pub(crate) struct TopState {
    pub(crate) registries: Arc<Vec<MountedRegistry>>,
}
