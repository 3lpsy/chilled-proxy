//! The bootstrap admin user created at startup.

use log::info;
use sea_orm::{ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use super::entity::user;
use crate::authn::password;
use crate::time::now;

/// Creates the bootstrap user if it does not exist. Never overwrites an
/// existing user's password: restarts must not clobber a UI-made change.
pub async fn bootstrap_admin(
    db: &DatabaseConnection,
    username: &str,
    password_plain: &str,
) -> Result<(), String> {
    let existing = user::Entity::find()
        .filter(user::Column::Username.eq(username))
        .one(db)
        .await
        .map_err(|e| format!("bootstrap user lookup failed: {e}"))?;
    if existing.is_some() {
        info!("ui: bootstrap user '{username}' already exists, leaving it unchanged");
        return Ok(());
    }
    let ts = now();
    let row = user::ActiveModel {
        username: ActiveValue::Set(username.to_owned()),
        auth_source: ActiveValue::Set("builtin".to_owned()),
        password_hash: ActiveValue::Set(Some(password::hash(password_plain)?)),
        created_at: ActiveValue::Set(ts),
        updated_at: ActiveValue::Set(ts),
        ..Default::default()
    };
    user::Entity::insert(row)
        .exec(db)
        .await
        .map_err(|e| format!("bootstrap user creation failed: {e}"))?;
    info!("ui: created bootstrap user '{username}'");
    Ok(())
}
