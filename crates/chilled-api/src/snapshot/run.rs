//! A single snapshot pass: scan, upsert, prune, all in one transaction.

use log::warn;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, ExprTrait, QueryFilter,
    TransactionTrait,
};

use super::retention::retention_sweep;
use crate::db::entity::{artifact, mount_config, snapshot_run};
use crate::state::UiState;
use crate::time::now;

/// Rows per insert batch — sqlite's default variable limit is the constraint.
const CHUNK: usize = 500;

/// One full snapshot pass over every mount.
pub async fn run_once(state: &UiState) -> Result<usize, String> {
    run_scoped(state, None).await
}

/// A snapshot pass over a single mount; other mounts' rows are untouched.
pub async fn run_mount(state: &UiState, mount: &str) -> Result<usize, String> {
    run_scoped(state, Some(mount)).await
}

/// The snapshot pass, optionally scoped to one mount. A scoped run's row
/// count (and its `snapshot_runs.artifact_count`) covers only that mount.
pub(super) async fn run_scoped(state: &UiState, only: Option<&str>) -> Result<usize, String> {
    let started = now();
    let run = snapshot_run::Entity::insert(snapshot_run::ActiveModel {
        started_at: ActiveValue::Set(started),
        finished_at: ActiveValue::Set(None),
        artifact_count: ActiveValue::Set(0),
        ..Default::default()
    })
    .exec(&state.db)
    .await
    .map_err(|e| format!("snapshot run insert: {e}"))?;
    let run_id = run.last_insert_id;

    // The scans stat every cached file; keep them off the async runtime.
    let scanners: Vec<_> = state
        .mounts_ops
        .iter()
        .filter(|(name, _)| only.is_none_or(|m| m == name))
        .map(|(name, ops)| (name.clone(), ops.scan.clone()))
        .collect();
    let scanned = tokio::task::spawn_blocking(move || {
        scanners
            .iter()
            .map(|(name, scan)| (name.clone(), scan()))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| format!("snapshot scan panicked: {e}"))?;

    // A failed scan must not read as "cache emptied": its mount keeps its
    // rows and is left out of the prune below.
    let mut complete = Vec::new();
    let mut rows = Vec::new();
    for (mount, stats) in &scanned {
        if stats.incomplete {
            warn!("ui: cache scan for mount '{mount}' failed; keeping its previous snapshot");
            continue;
        }
        complete.push(mount.clone());
        let kind = state
            .mount(mount)
            .map(|m| m.kind.clone())
            .unwrap_or_default();
        for a in &stats.artifacts {
            rows.push(artifact::ActiveModel {
                mount: ActiveValue::Set(mount.clone()),
                kind: ActiveValue::Set(kind.clone()),
                name: ActiveValue::Set(a.name.clone()),
                version: ActiveValue::Set(a.version.clone()),
                size_bytes: ActiveValue::Set(a.size_bytes as i64),
                cached_at: ActiveValue::Set(a.cached_at as i64),
                first_seen_at: ActiveValue::Set(started),
                last_seen_run_id: ActiveValue::Set(run_id),
                ..Default::default()
            });
        }
    }
    let total = rows.len();

    let views = state.mounts.clone();
    // Unmounted-row cleanup must judge against every configured mount, not
    // just the scanned scope, or a scoped run would sweep its siblings.
    let all_mounts: Vec<String> = state.mounts_ops.iter().map(|(m, _)| m.clone()).collect();
    state
        .db
        .transaction::<_, (), sea_orm::DbErr>(move |txn| {
            Box::pin(async move {
                for chunk in rows.chunks(CHUNK) {
                    artifact::Entity::insert_many(chunk.to_vec())
                        .on_conflict(
                            OnConflict::columns([
                                artifact::Column::Mount,
                                artifact::Column::Name,
                                artifact::Column::Version,
                            ])
                            .update_columns([
                                artifact::Column::Kind,
                                artifact::Column::SizeBytes,
                                artifact::Column::CachedAt,
                                artifact::Column::LastSeenRunId,
                            ])
                            .to_owned(),
                        )
                        .exec_without_returning(txn)
                        .await?;
                }
                // Anything a *successful* scan no longer saw was evicted from
                // the cache; rows of unmounted names go with them.
                artifact::Entity::delete_many()
                    .filter(
                        sea_orm::Condition::any()
                            .add(
                                artifact::Column::LastSeenRunId
                                    .lt(run_id)
                                    .and(artifact::Column::Mount.is_in(complete)),
                            )
                            .add(artifact::Column::Mount.is_not_in(all_mounts)),
                    )
                    .exec(txn)
                    .await?;
                refresh_mount_configs(txn, &views).await?;
                snapshot_run::Entity::update(snapshot_run::ActiveModel {
                    id: ActiveValue::Set(run_id),
                    finished_at: ActiveValue::Set(Some(now())),
                    artifact_count: ActiveValue::Set(total as i64),
                    ..Default::default()
                })
                .exec(txn)
                .await?;
                retention_sweep(txn, run_id, &views).await?;
                Ok(())
            })
        })
        .await
        .map_err(|e| format!("snapshot write: {e}"))?;
    Ok(total)
}

/// Upserts the pre-redacted mount configuration snapshot.
async fn refresh_mount_configs<C: ConnectionTrait>(
    txn: &C,
    views: &[crate::mount_view::MountView],
) -> Result<(), sea_orm::DbErr> {
    for view in views {
        let names = serde_json::to_string(&view.auth.header_names).unwrap_or_else(|_| "[]".into());
        mount_config::Entity::insert(mount_config::ActiveModel {
            mount: ActiveValue::Set(view.name.clone()),
            kind: ActiveValue::Set(view.kind.clone()),
            path: ActiveValue::Set(view.path.clone()),
            upstream: ActiveValue::Set(view.upstream.clone()),
            secondary: ActiveValue::Set(view.secondary.clone()),
            proxy_url: ActiveValue::Set(view.proxy_url.clone()),
            cooldown_secs: ActiveValue::Set(view.cooldown_secs as i64),
            cache_ttl_secs: ActiveValue::Set(view.cache_ttl_secs as i64),
            restrict_downloads: ActiveValue::Set(view.restrict_downloads),
            auth_basic: ActiveValue::Set(view.auth.basic),
            auth_header_names: ActiveValue::Set(names),
            updated_at: ActiveValue::Set(now()),
        })
        .on_conflict(
            OnConflict::column(mount_config::Column::Mount)
                .update_columns([
                    mount_config::Column::Kind,
                    mount_config::Column::Path,
                    mount_config::Column::Upstream,
                    mount_config::Column::Secondary,
                    mount_config::Column::ProxyUrl,
                    mount_config::Column::CooldownSecs,
                    mount_config::Column::CacheTtlSecs,
                    mount_config::Column::RestrictDownloads,
                    mount_config::Column::AuthBasic,
                    mount_config::Column::AuthHeaderNames,
                    mount_config::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec(txn)
        .await?;
    }
    Ok(())
}
