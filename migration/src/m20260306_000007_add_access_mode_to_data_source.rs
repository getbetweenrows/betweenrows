use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DataSource::Table)
                    .add_column(
                        ColumnDef::new(DataSource::AccessMode)
                            .string()
                            .not_null()
                            .default("policy_required"),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite does not support DROP COLUMN — this migration is intentionally irreversible
        Ok(())
    }
}

#[derive(Iden)]
enum DataSource {
    Table,
    AccessMode,
}
