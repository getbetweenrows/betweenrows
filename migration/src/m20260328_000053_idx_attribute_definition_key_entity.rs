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
                    .name("idx_attribute_definition_key_entity")
                    .table(AttributeDefinition::Table)
                    .col(AttributeDefinition::Key)
                    .col(AttributeDefinition::EntityType)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_attribute_definition_key_entity")
                    .table(AttributeDefinition::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum AttributeDefinition {
    Table,
    Key,
    EntityType,
}
