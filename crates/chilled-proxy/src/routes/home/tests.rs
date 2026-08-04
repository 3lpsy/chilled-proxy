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

/// A mounted registry with `name` as its instance name.
fn mounted(name: &str) -> super::super::MountedRegistry {
    super::super::MountedRegistry {
        name: name.to_owned(),
        proxy: std::sync::Arc::new(Fake("npm")),
    }
}

#[test]
fn home_lists_registries() {
    let state = TopState {
        registries: std::sync::Arc::new(vec![mounted("crates"), mounted("npm")]),
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
        registries: std::sync::Arc::new(vec![mounted("maven"), mounted("gradle-plugins")]),
    };
    assert_eq!(
        home_json(&state),
        r#"{"status":"running","registries":["maven","gradle-plugins"]}"#
    );
}

#[test]
fn home_with_no_registries() {
    let state = TopState {
        registries: std::sync::Arc::new(vec![]),
    };
    assert_eq!(home_json(&state), r#"{"status":"running","registries":[]}"#);
}
