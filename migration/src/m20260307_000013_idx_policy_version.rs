use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_policy_version_policy_id_version")
                    .table(PolicyVersion::Table)
                    .col(PolicyVersion::PolicyId)
                    .col(PolicyVersion::Version)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_policy_version_policy_id_version")
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum PolicyVersion {
    Table,
    PolicyId,
    Version,
}
