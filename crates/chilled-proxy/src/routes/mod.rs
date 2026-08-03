//! Top-level HTTP request handlers, one module per route.

pub(crate) mod healthz;
pub(crate) mod home;
pub(crate) mod metrics;

pub(crate) use healthz::handle_healthz;
pub(crate) use home::handle_home;
pub(crate) use metrics::handle_metrics;

use std::sync::Arc;

use chilled_core::registry::RegistryProxy;

/// Shared top-level state: the mounted registries (cheap to clone).
#[derive(Clone)]
pub(crate) struct TopState {
    pub(crate) registries: Arc<Vec<Arc<dyn RegistryProxy>>>,
}
