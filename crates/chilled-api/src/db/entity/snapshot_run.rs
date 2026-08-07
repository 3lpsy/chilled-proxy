//! `snapshot_runs` — one row per cache-state snapshot pass.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "snapshot_runs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub started_at: i64,
    /// NULL while the run is still scanning.
    pub finished_at: Option<i64>,
    pub artifact_count: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
