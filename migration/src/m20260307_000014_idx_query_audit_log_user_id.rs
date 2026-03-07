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
                    .name("idx_query_audit_log_user_id")
                    .table(QueryAuditLog::Table)
                    .col(QueryAuditLog::UserId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(Index::drop().name("idx_query_audit_log_user_id").to_owned())
            .await
    }
}

#[derive(Iden)]
enum QueryAuditLog {
    Table,
    UserId,
}
