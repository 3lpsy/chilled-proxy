//! `GET /` — liveness status plus the enabled registry mounts.

use axum::{extract::State, response::Response};
use chilled_core::http::json_response;

use super::TopState;

/// Handles `GET /`: liveness plus the list of mounted registries.
pub(crate) async fn handle_home(State(state): State<TopState>) -> Response {
    json_response(200, home_json(&state))
}

/// Builds the home JSON document.
fn home_json(state: &TopState) -> String {
    let ids: Vec<String> = state
        .registries
        .iter()
        .map(|r| format!(r#""{}""#, r.name))
        .collect();
    format!(r#"{{"status":"running","registries":[{}]}}"#, ids.join(","))
}

#[cfg(test)]
mod tests {
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
}
