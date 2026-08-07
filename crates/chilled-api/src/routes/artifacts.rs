//! `GET /api/artifacts` — the paginated, searchable cached-artifacts table.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chilled_wire::{ArtifactPage, ArtifactRow};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::Deserialize;

use super::snapshots;
use crate::db::entity::{artifact, snapshot_run};
use crate::state::UiState;

/// Page-size ceiling; the default is deliberately modest.
const MAX_PER_PAGE: u64 = 500;
const DEFAULT_PER_PAGE: u64 = 50;

#[derive(Debug, Deserialize)]
pub(crate) struct Params {
    page: Option<u64>,
    per_page: Option<u64>,
    /// Case-insensitive substring match on the artifact name.
    q: Option<String>,
    mount: Option<String>,
    kind: Option<String>,
    sort: Option<String>,
    order: Option<String>,
}

pub(crate) async fn handle_list(
    State(state): State<UiState>,
    Query(params): Query<Params>,
) -> Response {
    let mut query = artifact::Entity::find();
    if let Some(mount) = params.mount.as_deref().filter(|m| !m.is_empty()) {
        query = query.filter(artifact::Column::Mount.eq(mount));
    }
    if let Some(kind) = params.kind.as_deref().filter(|k| !k.is_empty()) {
        query = query.filter(artifact::Column::Kind.eq(kind));
    }
    if let Some(q) = params.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        // Escape LIKE wildcards so a literal `%`/`_` searches literally.
        let escaped = q
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        query = query.filter(
            artifact::Column::Name
                .like(sea_orm::sea_query::LikeExpr::new(format!("%{escaped}%")).escape('\\')),
        );
    }

    let descending = params.order.as_deref() == Some("desc");
    let column = match params.sort.as_deref() {
        Some("version") => artifact::Column::Version,
        Some("size") => artifact::Column::SizeBytes,
        Some("cached_at") => artifact::Column::CachedAt,
        _ => artifact::Column::Name,
    };
    query = if descending {
        query.order_by_desc(column)
    } else {
        query.order_by_asc(column)
    };
    // Deterministic pagination even when the sort column ties.
    query = query.order_by_asc(artifact::Column::Id);

    let per_page = params
        .per_page
        .unwrap_or(DEFAULT_PER_PAGE)
        .clamp(1, MAX_PER_PAGE);
    let paginator = query.paginate(&state.db, per_page);
    let total = match paginator.num_items().await {
        Ok(total) => total,
        Err(_) => {
            return super::api_error(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "database error",
            )
        }
    };
    // Clamp to the real page range: an arbitrary u64 would overflow the
    // paginator's offset multiplication.
    let last_page = total.div_ceil(per_page).max(1);
    let page = params.page.unwrap_or(1).clamp(1, last_page);
    let rows = match paginator.fetch_page(page - 1).await {
        Ok(rows) => rows,
        Err(_) => {
            return super::api_error(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "database error",
            )
        }
    };

    let items = rows
        .into_iter()
        .map(|row| {
            let upstream = state
                .mount(&row.mount)
                .map(|m| m.upstream.clone())
                .unwrap_or_default();
            ArtifactRow {
                id: row.id,
                mount: row.mount,
                kind: row.kind,
                name: row.name,
                version: row.version,
                size_bytes: row.size_bytes,
                cached_at: row.cached_at,
                upstream,
            }
        })
        .collect();

    // The most recent *finished* run: an in-flight or failed run would report
    // itself as `finished_at: null` and read like perpetual progress.
    let snapshot = snapshot_run::Entity::find()
        .filter(snapshot_run::Column::FinishedAt.is_not_null())
        .order_by_desc(snapshot_run::Column::Id)
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|run| snapshots::info(&run));

    Json(ArtifactPage {
        items,
        page,
        per_page,
        total,
        snapshot,
    })
    .into_response()
}
