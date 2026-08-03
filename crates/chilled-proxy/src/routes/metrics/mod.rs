//! `GET /metrics` — cached-artifact report per registry (only routed when enabled).

#[cfg(test)]
mod tests;

use axum::{extract::State, response::Response};
use chilled_core::http::{error_response, json_escape, json_response};
use chilled_core::registry::CacheStats;

use super::TopState;

/// Handles `GET /metrics`: scans every mounted registry's artifact cache.
/// Only routed when metrics are enabled, so reaching this means opt-in.
pub(crate) async fn handle_metrics(State(state): State<TopState>) -> Response {
    let registries = state.registries.clone();
    let scan = tokio::task::spawn_blocking(move || {
        registries
            .iter()
            .map(|r| (r.id(), r.cache_stats()))
            .collect::<Vec<_>>()
    })
    .await;

    match scan {
        Ok(stats) => json_response(200, metrics_json(&stats)),
        Err(_) => error_response(500),
    }
}

/// Builds the metrics JSON document from per-registry cache stats.
fn metrics_json(stats: &[(&str, CacheStats)]) -> String {
    let registries: Vec<String> = stats
        .iter()
        .map(|(id, s)| {
            let artifacts: Vec<String> = s
                .artifacts
                .iter()
                .map(|a| {
                    format!(
                        r#"{{"name":"{}","version":"{}","cached_at":{}}}"#,
                        json_escape(&a.name),
                        json_escape(&a.version),
                        a.cached_at
                    )
                })
                .collect();
            format!(
                r#""{}":{{"cached_count":{},"artifacts":[{}]}}"#,
                id,
                s.artifacts.len(),
                artifacts.join(",")
            )
        })
        .collect();

    format!(
        r#"{{"service":"chilled-proxy","registries":{{{}}}}}"#,
        registries.join(",")
    )
}
