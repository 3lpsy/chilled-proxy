//! `mount_configs` — the per-mount configuration snapshot, pre-redacted.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "mount_configs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub mount: String,
    pub kind: String,
    pub path: String,
    /// Already redacted by the binary before reaching this crate.
    pub upstream: String,
    pub secondary: Option<String>,
    pub proxy_url: String,
    pub cooldown_secs: i64,
    pub cache_ttl_secs: i64,
    pub restrict_downloads: bool,
    pub auth_basic: bool,
    /// JSON array of custom header names (never values).
    pub auth_header_names: String,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
