use super::*;
use axum::Router;
use chilled_core::registry::{CacheStats, RegistryProxy};

struct Fake(&'static str);
impl RegistryProxy for Fake {
    fn id(&self) -> &'static str {
        self.0
    }
    fn router(&self) -> Router {
        Router::new()
    }
    fn cache_stats(&self) -> CacheStats {
        CacheStats::default()
    }
}

#[test]
fn home_lists_registries() {
    let state = TopState {
        registries: std::sync::Arc::new(vec![
            std::sync::Arc::new(Fake("crates")),
            std::sync::Arc::new(Fake("npm")),
        ]),
    };
    assert_eq!(
        home_json(&state),
        r#"{"status":"running","registries":["crates","npm"]}"#
    );
}

#[test]
fn home_with_no_registries() {
    let state = TopState {
        registries: std::sync::Arc::new(vec![]),
    };
    assert_eq!(home_json(&state), r#"{"status":"running","registries":[]}"#);
}
