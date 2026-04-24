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
                    .unique()
                    .name("idx_column_anchor_unique")
                    .table(ColumnAnchor::Table)
                    .col(ColumnAnchor::DataSourceId)
                    .col(ColumnAnchor::ChildTableId)
                    .col(ColumnAnchor::ResolvedColumnName)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_column_anchor_unique")
                    .table(ColumnAnchor::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum ColumnAnchor {
    Table,
    DataSourceId,
    ChildTableId,
    ResolvedColumnName,
}
