use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DiscoveredSchema::Table)
                    .add_column(ColumnDef::new(DiscoveredSchema::SchemaAlias).text().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DiscoveredSchema::Table)
                    .drop_column(DiscoveredSchema::SchemaAlias)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum DiscoveredSchema {
    Table,
    SchemaAlias,
}
