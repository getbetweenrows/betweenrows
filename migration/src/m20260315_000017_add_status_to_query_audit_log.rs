use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(QueryAuditLog::Table)
                    .add_column(
                        ColumnDef::new(QueryAuditLog::Status)
                            .text()
                            .not_null()
                            .default("success"),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(QueryAuditLog::Table)
                    .drop_column(QueryAuditLog::Status)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum QueryAuditLog {
    Table,
    Status,
}
