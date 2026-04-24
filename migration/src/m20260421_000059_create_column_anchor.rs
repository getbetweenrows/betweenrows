use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ColumnAnchor::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ColumnAnchor::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ColumnAnchor::DataSourceId).uuid().not_null())
                    .col(ColumnDef::new(ColumnAnchor::ChildTableId).uuid().not_null())
                    .col(
                        ColumnDef::new(ColumnAnchor::ResolvedColumnName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ColumnAnchor::RelationshipId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ColumnAnchor::DesignatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(ColumnAnchor::DesignatedBy).uuid().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(ColumnAnchor::Table, ColumnAnchor::DataSourceId)
                            .to(DataSource::Table, DataSource::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ColumnAnchor::Table, ColumnAnchor::ChildTableId)
                            .to(DiscoveredTable::Table, DiscoveredTable::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ColumnAnchor::Table, ColumnAnchor::RelationshipId)
                            .to(TableRelationship::Table, TableRelationship::Id)
                            .on_delete(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ColumnAnchor::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum ColumnAnchor {
    Table,
    Id,
    DataSourceId,
    ChildTableId,
    ResolvedColumnName,
    RelationshipId,
    DesignatedAt,
    DesignatedBy,
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

#[derive(Iden)]
enum TableRelationship {
    Table,
    Id,
}
