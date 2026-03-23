use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DataSourceAccess::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DataSourceAccess::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DataSourceAccess::UserId).uuid().null())
                    .col(ColumnDef::new(DataSourceAccess::RoleId).uuid().null())
                    .col(
                        ColumnDef::new(DataSourceAccess::DataSourceId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DataSourceAccess::AssignmentScope)
                            .string()
                            .not_null()
                            .default("user"),
                    )
                    .col(
                        ColumnDef::new(DataSourceAccess::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(DataSourceAccess::Table, DataSourceAccess::DataSourceId)
                            .to(DataSource::Table, DataSource::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DataSourceAccess::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum DataSourceAccess {
    Table,
    Id,
    UserId,
    RoleId,
    DataSourceId,
    AssignmentScope,
    CreatedAt,
}

#[derive(Iden)]
enum DataSource {
    Table,
    Id,
}
