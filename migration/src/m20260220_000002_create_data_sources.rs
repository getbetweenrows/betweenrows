use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DataSource::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DataSource::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DataSource::Name)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(DataSource::DsType).string().not_null())
                    .col(
                        ColumnDef::new(DataSource::Config)
                            .text()
                            .not_null()
                            .default("{}"),
                    )
                    .col(
                        ColumnDef::new(DataSource::SecureConfig)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(DataSource::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(ColumnDef::new(DataSource::LastSyncAt).timestamp().null())
                    .col(ColumnDef::new(DataSource::LastSyncResult).text().null())
                    .col(
                        ColumnDef::new(DataSource::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(DataSource::UpdatedAt)
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
            .drop_table(Table::drop().table(DataSource::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum DataSource {
    Table,
    Id,
    Name,
    DsType,
    Config,
    SecureConfig,
    IsActive,
    LastSyncAt,
    LastSyncResult,
    CreatedAt,
    UpdatedAt,
}
