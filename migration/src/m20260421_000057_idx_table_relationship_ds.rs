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
                    .name("idx_table_relationship_ds")
                    .table(TableRelationship::Table)
                    .col(TableRelationship::DataSourceId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_table_relationship_ds")
                    .table(TableRelationship::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum TableRelationship {
    Table,
    DataSourceId,
}
