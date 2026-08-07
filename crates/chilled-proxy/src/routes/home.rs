//! `GET /` — liveness status plus the enabled registry mounts.

use axum::{extract::State, response::Response};
use chilled_core::http::json_response;

use super::TopState;

/// Handles `GET /`: liveness plus the list of mounted registries.
pub(crate) async fn handle_home(State(state): State<TopState>) -> Response {
    json_response(200, home_json(&state))
}

/// Builds the home JSON document.
pub(super) fn home_json(state: &TopState) -> String {
    let ids: Vec<String> = state
        .registries
        .iter()
        .map(|r| format!(r#""{}""#, r.name))
        .collect();
    format!(r#"{{"status":"running","registries":[{}]}}"#, ids.join(","))
}
