use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(UserDataSource::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Recreate the old table structure for rollback
        manager
            .create_table(
                Table::create()
                    .table(UserDataSource::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserDataSource::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(UserDataSource::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(UserDataSource::DataSourceId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserDataSource::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum UserDataSource {
    Table,
    Id,
    UserId,
    DataSourceId,
    CreatedAt,
}
