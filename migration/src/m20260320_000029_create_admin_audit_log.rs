use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AdminAuditLog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AdminAuditLog::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AdminAuditLog::ResourceType)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AdminAuditLog::ResourceId).uuid().not_null())
                    .col(ColumnDef::new(AdminAuditLog::Action).string().not_null())
                    .col(ColumnDef::new(AdminAuditLog::ActorId).uuid().not_null())
                    .col(ColumnDef::new(AdminAuditLog::Changes).text().null())
                    .col(
                        ColumnDef::new(AdminAuditLog::CreatedAt)
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
            .drop_table(Table::drop().table(AdminAuditLog::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum AdminAuditLog {
    Table,
    Id,
    ResourceType,
    ResourceId,
    Action,
    ActorId,
    Changes,
    CreatedAt,
}
