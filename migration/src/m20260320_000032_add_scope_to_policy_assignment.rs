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
                    .table(PolicyAssignment::Table)
                    .add_column(
                        ColumnDef::new(PolicyAssignment::AssignmentScope)
                            .string()
                            .not_null()
                            .default("user"),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(PolicyAssignment::Table)
                    .drop_column(PolicyAssignment::AssignmentScope)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum PolicyAssignment {
    Table,
    AssignmentScope,
}
