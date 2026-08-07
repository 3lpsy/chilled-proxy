//! Initial schema: users, sessions, artifacts, snapshot runs, mount configs.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .col(pk_auto(Users::Id))
                    .col(string_uniq(Users::Username))
                    .col(string(Users::AuthSource))
                    .col(string_null(Users::PasswordHash))
                    .col(big_integer(Users::CreatedAt))
                    .col(big_integer(Users::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Sessions::Table)
                    .col(pk_auto(Sessions::Id))
                    .col(string_uniq(Sessions::TokenHash))
                    .col(big_integer(Sessions::UserId))
                    .col(big_integer(Sessions::CreatedAt))
                    .col(big_integer(Sessions::ExpiresAt))
                    .foreign_key(
                        ForeignKey::create()
                            .from(Sessions::Table, Sessions::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Artifacts::Table)
                    .col(pk_auto(Artifacts::Id))
                    .col(string(Artifacts::Mount))
                    .col(string(Artifacts::Kind))
                    .col(string(Artifacts::Name))
                    .col(string(Artifacts::Version))
                    .col(big_integer(Artifacts::SizeBytes))
                    .col(big_integer(Artifacts::CachedAt))
                    .col(big_integer(Artifacts::FirstSeenAt))
                    .col(big_integer(Artifacts::LastSeenRunId))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_artifacts_identity")
                    .table(Artifacts::Table)
                    .col(Artifacts::Mount)
                    .col(Artifacts::Name)
                    .col(Artifacts::Version)
                    .unique()
                    .to_owned(),
            )
            .await?;
        for (name, col) in [
            ("idx_artifacts_name", Artifacts::Name),
            ("idx_artifacts_cached_at", Artifacts::CachedAt),
            ("idx_artifacts_size", Artifacts::SizeBytes),
        ] {
            manager
                .create_index(
                    Index::create()
                        .name(name)
                        .table(Artifacts::Table)
                        .col(col)
                        .to_owned(),
                )
                .await?;
        }

        manager
            .create_table(
                Table::create()
                    .table(SnapshotRuns::Table)
                    .col(pk_auto(SnapshotRuns::Id))
                    .col(big_integer(SnapshotRuns::StartedAt))
                    .col(big_integer_null(SnapshotRuns::FinishedAt))
                    .col(big_integer(SnapshotRuns::ArtifactCount))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(MountConfigs::Table)
                    .col(string(MountConfigs::Mount).primary_key())
                    .col(string(MountConfigs::Kind))
                    .col(string(MountConfigs::Path))
                    .col(string(MountConfigs::Upstream))
                    .col(string_null(MountConfigs::Secondary))
                    .col(string(MountConfigs::ProxyUrl))
                    .col(big_integer(MountConfigs::CooldownSecs))
                    .col(big_integer(MountConfigs::CacheTtlSecs))
                    .col(boolean(MountConfigs::RestrictDownloads))
                    .col(boolean(MountConfigs::AuthBasic))
                    .col(string(MountConfigs::AuthHeaderNames))
                    .col(big_integer(MountConfigs::UpdatedAt))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MountConfigs::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(SnapshotRuns::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Artifacts::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Sessions::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Username,
    AuthSource,
    PasswordHash,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    Id,
    TokenHash,
    UserId,
    CreatedAt,
    ExpiresAt,
}

#[derive(DeriveIden)]
enum Artifacts {
    Table,
    Id,
    Mount,
    Kind,
    Name,
    Version,
    SizeBytes,
    CachedAt,
    FirstSeenAt,
    LastSeenRunId,
}

#[derive(DeriveIden)]
enum SnapshotRuns {
    Table,
    Id,
    StartedAt,
    FinishedAt,
    ArtifactCount,
}

#[derive(DeriveIden)]
enum MountConfigs {
    Table,
    Mount,
    Kind,
    Path,
    Upstream,
    Secondary,
    ProxyUrl,
    CooldownSecs,
    CacheTtlSecs,
    RestrictDownloads,
    AuthBasic,
    AuthHeaderNames,
    UpdatedAt,
}
