//! Snapshot run info and the manual refresh trigger.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chilled_wire::SnapshotInfo;
use sea_orm::{EntityTrait, QueryOrder};

use super::api_error;
use crate::db::entity::snapshot_run;
use crate::state::UiState;

/// `GET /api/snapshots/latest` — the most recent finished run, falling back
/// to the newest row (the run in flight) only when none has finished yet.
pub(crate) async fn handle_latest(State(state): State<UiState>) -> Response {
    use sea_orm::{ColumnTrait, QueryFilter};
    let finished = snapshot_run::Entity::find()
        .filter(snapshot_run::Column::FinishedAt.is_not_null())
        .order_by_desc(snapshot_run::Column::Id)
        .one(&state.db)
        .await;
    let run = match finished {
        Ok(Some(run)) => Ok(Some(run)),
        Ok(None) => {
            snapshot_run::Entity::find()
                .order_by_desc(snapshot_run::Column::Id)
                .one(&state.db)
                .await
        }
        err => err,
    };
    match run {
        Ok(Some(run)) => Json(info(&run)).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "no snapshot has run yet"),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "database error"),
    }
}

/// `POST /api/snapshots/refresh` — queue a full snapshot now.
pub(crate) async fn handle_refresh(State(state): State<UiState>) -> Response {
    let _ = state.refresh.send(None);
    StatusCode::ACCEPTED.into_response()
}

pub(crate) fn info(run: &snapshot_run::Model) -> SnapshotInfo {
    SnapshotInfo {
        id: run.id,
        started_at: run.started_at,
        finished_at: run.finished_at,
        artifact_count: run.artifact_count,
    }
}
