//! `GET /metrics` — cached-artifact report per registry (only routed when enabled).

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
            .map(|r| (r.name.clone(), r.proxy.cache_stats()))
            .collect::<Vec<_>>()
    })
    .await;

    match scan {
        Ok(stats) => json_response(200, metrics_json(&stats)),
        Err(_) => error_response(500),
    }
}

/// Builds the metrics JSON document from per-mount cache stats.
fn metrics_json<S: AsRef<str>>(stats: &[(S, CacheStats)]) -> String {
    let registries: Vec<String> = stats
        .iter()
        .map(|(name, s)| {
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
                json_escape(name.as_ref()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chilled_core::registry::CachedArtifact;

    #[test]
    fn metrics_json_shape() {
        let stats = vec![
            (
                "crates",
                CacheStats {
                    artifacts: vec![CachedArtifact {
                        name: "serde".into(),
                        version: "1.0.0".into(),
                        cached_at: 42,
                    }],
                },
            ),
            ("npm", CacheStats::default()),
        ];
        let json: serde_json::Value = serde_json::from_str(&metrics_json(&stats)).unwrap();
        assert_eq!(json["service"], "chilled-proxy");
        assert_eq!(json["registries"]["crates"]["cached_count"], 1);
        assert_eq!(
            json["registries"]["crates"]["artifacts"][0]["name"],
            "serde"
        );
        assert_eq!(json["registries"]["npm"]["cached_count"], 0);
    }

    #[test]
    fn metrics_json_escapes_names() {
        // npm names can contain `@`/`/`; anything unexpected must stay valid JSON.
        let stats = vec![(
            "npm",
            CacheStats {
                artifacts: vec![CachedArtifact {
                    name: "@scope/pkg\"x".into(),
                    version: "1.0.0".into(),
                    cached_at: 1,
                }],
            },
        )];
        let parsed: serde_json::Value = serde_json::from_str(&metrics_json(&stats)).unwrap();
        assert_eq!(
            parsed["registries"]["npm"]["artifacts"][0]["name"],
            "@scope/pkg\"x"
        );
    }
}
