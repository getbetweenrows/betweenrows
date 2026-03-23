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
                    .name("idx_admin_audit_created_at")
                    .table(AdminAuditLog::Table)
                    .col(AdminAuditLog::CreatedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_admin_audit_created_at")
                    .table(AdminAuditLog::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum AdminAuditLog {
    Table,
    CreatedAt,
}
