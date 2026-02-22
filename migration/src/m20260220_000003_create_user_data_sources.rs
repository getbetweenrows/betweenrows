use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                    .col(
                        ColumnDef::new(UserDataSource::UserId)
                            .uuid()
                            .not_null(),
                    )
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
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_uds_user")
                            .from(UserDataSource::Table, UserDataSource::UserId)
                            .to(ProxyUser::Table, ProxyUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_uds_datasource")
                            .from(UserDataSource::Table, UserDataSource::DataSourceId)
                            .to(DataSource::Table, DataSource::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_uds_unique")
                    .table(UserDataSource::Table)
                    .col(UserDataSource::UserId)
                    .col(UserDataSource::DataSourceId)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(UserDataSource::Table).to_owned())
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

#[derive(Iden)]
enum ProxyUser {
    Table,
    Id,
}

#[derive(Iden)]
enum DataSource {
    Table,
    Id,
}
