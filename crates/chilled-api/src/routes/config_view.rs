//! `GET /api/config` — the whole-server view-only configuration report.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chilled_wire::{ServerConfig, UiConfigView};

use super::registries::mount_config;
use crate::state::UiState;

pub(crate) async fn handle_config(State(state): State<UiState>) -> Response {
    let mut mounts = Vec::with_capacity(state.mounts.len());
    for view in &state.mounts {
        mounts.push(mount_config(&state, view).await);
    }
    Json(ServerConfig {
        version: state.version.clone(),
        listen: state.server.listen.clone(),
        log_level: state.server.log_level.clone(),
        metrics_enabled: state.server.metrics_enabled,
        disabled: state.server.disabled.clone(),
        ui: UiConfigView {
            auth_mode: state.config.auth_mode,
            public_readonly: state.config.public_readonly,
            cache_update_interval_secs: state.config.cache_update_interval.as_secs(),
            db_path: state.config.db_path.to_string_lossy().into_owned(),
            trust_first_user_signup: state.config.trust_first_user_signup,
            session_ttl_secs: state.config.session_ttl.as_secs(),
        },
        mounts,
    })
    .into_response()
}
