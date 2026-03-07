use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(QueryAuditLog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(QueryAuditLog::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(QueryAuditLog::UserId).uuid().not_null())
                    .col(ColumnDef::new(QueryAuditLog::Username).string().not_null())
                    .col(
                        ColumnDef::new(QueryAuditLog::DataSourceId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QueryAuditLog::DatasourceName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(QueryAuditLog::OriginalQuery)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(QueryAuditLog::RewrittenQuery).text().null())
                    .col(
                        ColumnDef::new(QueryAuditLog::PoliciesApplied)
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .col(
                        ColumnDef::new(QueryAuditLog::ExecutionTimeMs)
                            .big_integer()
                            .null(),
                    )
                    .col(ColumnDef::new(QueryAuditLog::ClientIp).string().null())
                    .col(ColumnDef::new(QueryAuditLog::ClientInfo).string().null())
                    .col(
                        ColumnDef::new(QueryAuditLog::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(QueryAuditLog::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum QueryAuditLog {
    Table,
    Id,
    UserId,
    Username,
    DataSourceId,
    DatasourceName,
    OriginalQuery,
    RewrittenQuery,
    PoliciesApplied,
    ExecutionTimeMs,
    ClientIp,
    ClientInfo,
    CreatedAt,
}
