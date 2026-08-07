use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait};
use sea_orm_migration::MigratorTrait;

use super::entity::{session, user};
use super::{bootstrap_admin, migration};

async fn memory_db() -> DatabaseConnection {
    let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    db
}

#[tokio::test]
async fn bootstrap_is_idempotent_and_keeps_passwords() {
    let db = memory_db().await;
    bootstrap_admin(&db, "admin", "first").await.unwrap();
    let hash1 = user::Entity::find()
        .one(&db)
        .await
        .unwrap()
        .unwrap()
        .password_hash
        .unwrap();
    bootstrap_admin(&db, "admin", "second").await.unwrap();
    let users = user::Entity::find().all(&db).await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].password_hash.as_deref(), Some(hash1.as_str()));
}

#[tokio::test]
async fn deleting_a_user_cascades_to_sessions() {
    let db = memory_db().await;
    bootstrap_admin(&db, "admin", "pw").await.unwrap();
    let uid = user::Entity::find().one(&db).await.unwrap().unwrap().id;
    session::Entity::insert(session::ActiveModel {
        token_hash: ActiveValue::Set("h".into()),
        user_id: ActiveValue::Set(uid),
        created_at: ActiveValue::Set(1),
        expires_at: ActiveValue::Set(2),
        ..Default::default()
    })
    .exec(&db)
    .await
    .unwrap();
    user::Entity::delete_by_id(uid).exec(&db).await.unwrap();
    assert!(session::Entity::find().one(&db).await.unwrap().is_none());
}
