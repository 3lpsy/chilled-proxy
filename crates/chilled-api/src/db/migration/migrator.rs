//! The migration registry.

use sea_orm_migration::{async_trait, MigrationTrait, MigratorTrait};

use super::m0001_initial;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m0001_initial::Migration)]
    }
}
