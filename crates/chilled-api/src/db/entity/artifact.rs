//! `artifacts` — the current cached-artifact state, refreshed by snapshot runs.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "artifacts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub mount: String,
    pub kind: String,
    pub name: String,
    /// Version, or the cached file name where versions don't apply (PyPI).
    pub version: String,
    pub size_bytes: i64,
    pub cached_at: i64,
    pub first_seen_at: i64,
    /// The last snapshot run that saw this artifact; older rows are pruned.
    pub last_seen_run_id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
