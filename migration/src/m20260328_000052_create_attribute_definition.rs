use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AttributeDefinition::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AttributeDefinition::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AttributeDefinition::Key).text().not_null())
                    .col(
                        ColumnDef::new(AttributeDefinition::EntityType)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AttributeDefinition::DisplayName)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AttributeDefinition::ValueType)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AttributeDefinition::DefaultValue)
                            .text()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(AttributeDefinition::AllowedValues)
                            .text()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(AttributeDefinition::Description)
                            .text()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(AttributeDefinition::CreatedBy)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AttributeDefinition::UpdatedBy)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AttributeDefinition::CreatedAt)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AttributeDefinition::UpdatedAt)
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AttributeDefinition::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum AttributeDefinition {
    Table,
    Id,
    Key,
    EntityType,
    DisplayName,
    ValueType,
    DefaultValue,
    AllowedValues,
    Description,
    CreatedBy,
    UpdatedBy,
    CreatedAt,
    UpdatedAt,
}
