//! Cache deletion and re-pull: per-artifact and whole-mount.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;

use super::api_error;
use crate::db::entity::artifact;
use crate::state::{MountOps, UiState};

/// The artifact row and its mount's ops, or a ready error response.
async fn lookup(state: &UiState, id: i64) -> Result<(artifact::Model, MountOps), Box<Response>> {
    let row = artifact::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|_| {
            Box::new(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database error",
            ))
        })?
        .ok_or_else(|| Box::new(api_error(StatusCode::NOT_FOUND, "no such artifact")))?;
    let ops = state
        .ops(&row.mount)
        .cloned()
        .ok_or_else(|| Box::new(api_error(StatusCode::NOT_FOUND, "mount no longer exists")))?;
    Ok((row, ops))
}

/// Deletes the artifact's files off-runtime; returns the re-fetch paths.
async fn purge_files(row: &artifact::Model, ops: &MountOps) -> Vec<String> {
    let purge = ops.purge_artifact.clone();
    let (name, version) = (row.name.clone(), row.version.clone());
    tokio::task::spawn_blocking(move || purge(&name, &version))
        .await
        .unwrap_or_default()
}

/// `DELETE /api/artifacts/{id}` — remove one artifact from the cache.
pub(crate) async fn handle_delete(State(state): State<UiState>, Path(id): Path<i64>) -> Response {
    let (row, ops) = match lookup(&state, id).await {
        Ok(found) => found,
        Err(response) => return *response,
    };
    let removed = purge_files(&row, &ops).await.len();
    // Drop the row immediately so the table updates without waiting a tick;
    // the queued scoped run re-syncs everything else.
    let _ = artifact::Entity::delete_by_id(id).exec(&state.db).await;
    let _ = state.refresh.send(Some(row.mount));
    Json(json!({ "removed_files": removed })).into_response()
}

/// `POST /api/artifacts/{id}/repull` — delete, then fetch fresh through the
/// mount's own routes (which re-caches on the way).
pub(crate) async fn handle_repull(State(state): State<UiState>, Path(id): Path<i64>) -> Response {
    let (row, ops) = match lookup(&state, id).await {
        Ok(found) => found,
        Err(response) => return *response,
    };
    let paths = purge_files(&row, &ops).await;
    let mut refetched = 0usize;
    let mut failed = 0usize;
    for path in paths {
        if (ops.repull)(path).await {
            refetched += 1;
        } else {
            failed += 1;
        }
    }
    if refetched == 0 {
        // Nothing came back (gone upstream, or nothing was deleted); the row
        // no longer describes a cached file.
        let _ = artifact::Entity::delete_by_id(id).exec(&state.db).await;
    }
    let _ = state.refresh.send(Some(row.mount));
    Json(json!({ "refetched": refetched, "failed": failed })).into_response()
}

/// `POST /api/registries/{name}/clear` — delete every cached artifact of one
/// mount (metadata and index caches stay warm).
pub(crate) async fn handle_clear(
    State(state): State<UiState>,
    Path(name): Path<String>,
) -> Response {
    let Some(ops) = state.ops(&name).cloned() else {
        return api_error(StatusCode::NOT_FOUND, "no such mount");
    };
    let purge = ops.purge_all.clone();
    let _ = tokio::task::spawn_blocking(move || purge()).await;
    let _ = artifact::Entity::delete_many()
        .filter(artifact::Column::Mount.eq(&name))
        .exec(&state.db)
        .await;
    let _ = state.refresh.send(Some(name));
    StatusCode::ACCEPTED.into_response()
}
