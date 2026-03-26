// WARNING: ALTER TABLE ADD COLUMN is not idempotent. Do not interrupt this migration.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Policy::Table)
                    .add_column(ColumnDef::new(Policy::DecisionFunctionId).text().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Policy::Table)
                    .drop_column(Policy::DecisionFunctionId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum Policy {
    Table,
    DecisionFunctionId,
}
