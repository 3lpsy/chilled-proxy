use std::sync::Arc;

use axum::http::header;
use axum::Router;
use chilled_core::registry::{CacheStats, CachedArtifact, RegistryProxy};

use super::home::home_json;
use super::metrics::metrics_json;
use super::{handle_healthz, MountedRegistry, TopState};

struct Fake;
impl RegistryProxy for Fake {
    fn router(&self) -> Router {
        Router::new()
    }
    fn cache_stats(&self) -> CacheStats {
        CacheStats::default()
    }
    fn purge_artifact(&self, _name: &str, _version: &str) -> Vec<String> {
        Vec::new()
    }
    fn purge_all(&self) {}
}

/// A mounted registry with `name` as its instance name.
fn mounted(name: &str) -> MountedRegistry {
    MountedRegistry {
        name: name.to_owned(),
        proxy: Arc::new(Fake),
    }
}

#[tokio::test]
async fn healthz_is_ok_text() {
    let resp = handle_healthz().await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()[header::CONTENT_TYPE],
        "text/plain; charset=utf-8"
    );
}

#[test]
fn home_lists_registries() {
    let state = TopState {
        registries: Arc::new(vec![mounted("crates"), mounted("npm")]),
    };
    assert_eq!(
        home_json(&state),
        r#"{"status":"running","registries":["crates","npm"]}"#
    );
}

#[test]
fn home_lists_each_mount_of_a_registry() {
    // Two mounts of the same registry are listed under their own names.
    let state = TopState {
        registries: Arc::new(vec![mounted("maven"), mounted("gradle-plugins")]),
    };
    assert_eq!(
        home_json(&state),
        r#"{"status":"running","registries":["maven","gradle-plugins"]}"#
    );
}

#[test]
fn home_with_no_registries() {
    let state = TopState {
        registries: Arc::new(vec![]),
    };
    assert_eq!(home_json(&state), r#"{"status":"running","registries":[]}"#);
}

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
                    size_bytes: 7,
                }],
                incomplete: false,
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
    assert_eq!(
        json["registries"]["crates"]["artifacts"][0]["size_bytes"],
        7
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
                size_bytes: 0,
            }],
            incomplete: false,
        },
    )];
    let parsed: serde_json::Value = serde_json::from_str(&metrics_json(&stats)).unwrap();
    assert_eq!(
        parsed["registries"]["npm"]["artifacts"][0]["name"],
        "@scope/pkg\"x"
    );
}
