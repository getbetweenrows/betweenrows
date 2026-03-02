use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DiscoveredColumn::Table)
                    .add_column(
                        ColumnDef::new(DiscoveredColumn::IsSelected)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DiscoveredColumn::Table)
                    .drop_column(DiscoveredColumn::IsSelected)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum DiscoveredColumn {
    Table,
    IsSelected,
}
