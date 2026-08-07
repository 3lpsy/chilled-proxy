//! /api route assembly, in three auth tiers.

use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chilled_wire::ApiError;

use super::{
    artifacts, config_view, logs, meta, purge, registries, session, setup, snapshots, users,
};
use crate::authn;
use crate::state::UiState;

/// A JSON error response: `{"error": "..."}` with the given status.
pub(crate) fn api_error(status: StatusCode, msg: &str) -> Response {
    (status, Json(ApiError { error: msg.into() })).into_response()
}

/// Builds the /api router. Tier guards use `route_layer`; the identity
/// middleware is layered over the merged whole in [`crate::ui_router`].
pub(crate) fn api_router(state: &UiState) -> Router<UiState> {
    let public = Router::new()
        .route("/api/meta", get(meta::handle_meta))
        .route(
            "/api/session",
            post(session::handle_login).delete(session::handle_logout),
        )
        .route("/api/setup/first-user", post(setup::handle_first_user));

    let readonly =
        readonly_routes().route_layer(from_fn_with_state(state.clone(), authn::require_read));

    let mutating = mutating_routes().route_layer(axum::middleware::from_fn(authn::require_auth));

    // Unknown /api paths must 404 here: with a root-mounted registry the app
    // fallback belongs to the registry, which would proxy them upstream.
    public
        .merge(readonly)
        .merge(mutating)
        .route("/api", axum::routing::any(unknown_api))
        .route("/api/{*rest}", axum::routing::any(unknown_api))
}

async fn unknown_api() -> Response {
    api_error(StatusCode::NOT_FOUND, "no such endpoint")
}

/// State endpoints: authenticated, or open under `--ui-public-readonly-enabled`.
fn readonly_routes() -> Router<UiState> {
    Router::new()
        .route("/api/registries", get(registries::handle_list))
        .route("/api/registries/{name}", get(registries::handle_one))
        .route("/api/artifacts", get(artifacts::handle_list))
        .route("/api/config", get(config_view::handle_config))
        .route("/api/snapshots/latest", get(snapshots::handle_latest))
}

/// Everything else: authenticated always — user enumeration and logs stay
/// gated even in public-readonly mode.
fn mutating_routes() -> Router<UiState> {
    Router::new()
        .route("/api/snapshots/refresh", post(snapshots::handle_refresh))
        .route(
            "/api/artifacts/{id}",
            axum::routing::delete(purge::handle_delete),
        )
        .route("/api/artifacts/{id}/repull", post(purge::handle_repull))
        .route("/api/registries/{name}/clear", post(purge::handle_clear))
        .route(
            "/api/registries/{name}/refresh",
            post(registries::handle_refresh),
        )
        .route(
            "/api/users",
            get(users::handle_list).post(users::handle_create),
        )
        .route(
            "/api/users/{id}",
            axum::routing::delete(users::handle_delete),
        )
        .route(
            "/api/users/me",
            get(users::handle_me).patch(users::handle_update_me),
        )
        .route("/api/logs", get(logs::handle_logs))
}
