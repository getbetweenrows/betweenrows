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
                    .name("idx_role_member_unique")
                    .table(RoleMember::Table)
                    .col(RoleMember::RoleId)
                    .col(RoleMember::UserId)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_role_member_unique")
                    .table(RoleMember::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum RoleMember {
    Table,
    RoleId,
    UserId,
}
