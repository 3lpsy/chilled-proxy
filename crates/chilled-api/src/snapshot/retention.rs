//! Bookkeeping retention, run at the end of each snapshot pass.

use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, ExprTrait, QueryFilter};

use crate::db::entity::{mount_config, session, snapshot_run};
use crate::time::now;

/// How many finished snapshot runs the history keeps.
const KEEP_RUNS: i64 = 50;

/// Bounds the bookkeeping tables: old runs, runs that never finished (earlier
/// failures), expired sessions, and config rows for since-removed mounts.
pub(super) async fn retention_sweep<C: ConnectionTrait>(
    txn: &C,
    run_id: i64,
    views: &[crate::mount_view::MountView],
) -> Result<(), sea_orm::DbErr> {
    snapshot_run::Entity::delete_many()
        .filter(
            sea_orm::Condition::any()
                .add(snapshot_run::Column::Id.lte(run_id - KEEP_RUNS))
                .add(
                    snapshot_run::Column::Id
                        .lt(run_id)
                        .and(snapshot_run::Column::FinishedAt.is_null()),
                ),
        )
        .exec(txn)
        .await?;
    session::Entity::delete_many()
        .filter(session::Column::ExpiresAt.lte(now()))
        .exec(txn)
        .await?;
    let current: Vec<String> = views.iter().map(|v| v.name.clone()).collect();
    mount_config::Entity::delete_many()
        .filter(mount_config::Column::Mount.is_not_in(current))
        .exec(txn)
        .await?;
    Ok(())
}
