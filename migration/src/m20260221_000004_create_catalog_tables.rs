use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // discovered_schema
        manager
            .create_table(
                Table::create()
                    .table(DiscoveredSchema::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscoveredSchema::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredSchema::DataSourceId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredSchema::SchemaName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredSchema::IsSelected)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(DiscoveredSchema::DiscoveredAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(DiscoveredSchema::Table, DiscoveredSchema::DataSourceId)
                            .to(DataSource::Table, DataSource::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .unique()
                    .name("uq_discovered_schema_ds_name")
                    .table(DiscoveredSchema::Table)
                    .col(DiscoveredSchema::DataSourceId)
                    .col(DiscoveredSchema::SchemaName)
                    .to_owned(),
            )
            .await?;

        // discovered_table
        manager
            .create_table(
                Table::create()
                    .table(DiscoveredTable::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscoveredTable::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredTable::DiscoveredSchemaId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredTable::TableName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredTable::TableType)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredTable::IsSelected)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(DiscoveredTable::DiscoveredAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(DiscoveredTable::Table, DiscoveredTable::DiscoveredSchemaId)
                            .to(DiscoveredSchema::Table, DiscoveredSchema::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .unique()
                    .name("uq_discovered_table_schema_name")
                    .table(DiscoveredTable::Table)
                    .col(DiscoveredTable::DiscoveredSchemaId)
                    .col(DiscoveredTable::TableName)
                    .to_owned(),
            )
            .await?;

        // discovered_column
        manager
            .create_table(
                Table::create()
                    .table(DiscoveredColumn::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscoveredColumn::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredColumn::DiscoveredTableId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredColumn::ColumnName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredColumn::OrdinalPosition)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredColumn::DataType)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoveredColumn::IsNullable)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(DiscoveredColumn::ColumnDefault)
                            .text()
                            .null(),
                    )
                    .col(ColumnDef::new(DiscoveredColumn::ArrowType).string().null())
                    .col(
                        ColumnDef::new(DiscoveredColumn::DiscoveredAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(DiscoveredColumn::Table, DiscoveredColumn::DiscoveredTableId)
                            .to(DiscoveredTable::Table, DiscoveredTable::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .unique()
                    .name("uq_discovered_column_table_name")
                    .table(DiscoveredColumn::Table)
                    .col(DiscoveredColumn::DiscoveredTableId)
                    .col(DiscoveredColumn::ColumnName)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DiscoveredColumn::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DiscoveredTable::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DiscoveredSchema::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum DataSource {
    Table,
    Id,
}

#[derive(Iden)]
enum DiscoveredSchema {
    Table,
    Id,
    DataSourceId,
    SchemaName,
    IsSelected,
    DiscoveredAt,
}

#[derive(Iden)]
enum DiscoveredTable {
    Table,
    Id,
    DiscoveredSchemaId,
    TableName,
    TableType,
    IsSelected,
    DiscoveredAt,
}

#[derive(Iden)]
enum DiscoveredColumn {
    Table,
    Id,
    DiscoveredTableId,
    ColumnName,
    OrdinalPosition,
    DataType,
    IsNullable,
    ColumnDefault,
    ArrowType,
    DiscoveredAt,
}
