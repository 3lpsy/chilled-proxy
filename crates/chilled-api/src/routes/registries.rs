//! `GET /api/registries[/{name}]` — per-mount configuration plus cache totals.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chilled_wire::MountConfig;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect};

use super::api_error;
use crate::db::entity::{artifact, snapshot_run};
use crate::mount_view::MountView;
use crate::state::UiState;

pub(crate) async fn handle_list(State(state): State<UiState>) -> Response {
    let mut out = Vec::with_capacity(state.mounts.len());
    for view in &state.mounts {
        out.push(mount_config(&state, view).await);
    }
    Json(out).into_response()
}

pub(crate) async fn handle_one(State(state): State<UiState>, Path(name): Path<String>) -> Response {
    match state.mount(&name) {
        Some(view) => Json(mount_config(&state, view).await).into_response(),
        None => api_error(StatusCode::NOT_FOUND, "no such mount"),
    }
}

/// `POST /api/registries/{name}/refresh` — queue a snapshot of one mount.
/// Mutating tier: authenticated always, even in public-readonly mode.
pub(crate) async fn handle_refresh(
    State(state): State<UiState>,
    Path(name): Path<String>,
) -> Response {
    if state.mount(&name).is_none() {
        return api_error(StatusCode::NOT_FOUND, "no such mount");
    }
    let _ = state.refresh.send(Some(name));
    StatusCode::ACCEPTED.into_response()
}

/// Joins a mount view with its artifact totals and last snapshot time.
pub(crate) async fn mount_config(state: &UiState, view: &MountView) -> MountConfig {
    let totals = artifact::Entity::find()
        .filter(artifact::Column::Mount.eq(&view.name))
        .select_only()
        .column_as(Expr::col(artifact::Column::Id).count(), "cnt")
        .column_as(Expr::col(artifact::Column::SizeBytes).sum(), "total")
        .into_tuple::<(i64, Option<i64>)>()
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or((0, None));
    let last_snapshot_at = snapshot_run::Entity::find()
        .filter(snapshot_run::Column::FinishedAt.is_not_null())
        .order_by_desc(snapshot_run::Column::Id)
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .and_then(|run| run.finished_at);

    MountConfig {
        name: view.name.clone(),
        kind: view.kind.clone(),
        path: view.path.clone(),
        upstream: view.upstream.clone(),
        secondary: view.secondary.clone(),
        proxy_url: view.proxy_url.clone(),
        cooldown_secs: view.cooldown_secs,
        cache_ttl_secs: view.cache_ttl_secs,
        restrict_downloads: view.restrict_downloads,
        auth: view.auth.clone(),
        artifact_count: totals.0,
        total_size_bytes: totals.1.unwrap_or(0),
        last_snapshot_at,
    }
}
