use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TableRelationship::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TableRelationship::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TableRelationship::DataSourceId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TableRelationship::ChildTableId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TableRelationship::ChildColumnName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TableRelationship::ParentTableId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TableRelationship::ParentColumnName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TableRelationship::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(TableRelationship::CreatedBy).uuid().null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(TableRelationship::Table, TableRelationship::DataSourceId)
                            .to(DataSource::Table, DataSource::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TableRelationship::Table, TableRelationship::ChildTableId)
                            .to(DiscoveredTable::Table, DiscoveredTable::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TableRelationship::Table, TableRelationship::ParentTableId)
                            .to(DiscoveredTable::Table, DiscoveredTable::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TableRelationship::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum TableRelationship {
    Table,
    Id,
    DataSourceId,
    ChildTableId,
    ChildColumnName,
    ParentTableId,
    ParentColumnName,
    CreatedAt,
    CreatedBy,
}

#[derive(Iden)]
enum DataSource {
    Table,
    Id,
}

#[derive(Iden)]
enum DiscoveredTable {
    Table,
    Id,
}
