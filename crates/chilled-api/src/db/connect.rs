//! Opening and migrating the sqlite database.

use std::path::Path;
use std::time::Duration;

use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sea_orm::DatabaseConnection;
use sea_orm_migration::MigratorTrait;

use super::migration;

/// Opens (creating if missing) the sqlite database and applies migrations.
/// WAL keeps readers unblocked while the snapshot task writes.
pub async fn connect(db_path: &Path) -> Result<DatabaseConnection, String> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .map_err(|e| format!("cannot open UI database {}: {e}", db_path.display()))?;
    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);
    migration::Migrator::up(&db, None)
        .await
        .map_err(|e| format!("UI database migration failed: {e}"))?;
    Ok(db)
}
