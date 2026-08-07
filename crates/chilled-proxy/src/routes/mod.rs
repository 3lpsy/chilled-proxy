//! Top-level HTTP request handlers, one module per route.

#[cfg(test)]
mod tests;

mod healthz;
mod home;
mod metrics;
mod state;

pub(crate) use healthz::handle_healthz;
pub(crate) use home::handle_home;
pub(crate) use metrics::handle_metrics;
pub(crate) use state::{MountedRegistry, TopState};
