// WARNING: ALTER TABLE DROP COLUMN is not idempotent. Do not interrupt this migration.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ProxyUser::Table)
                    .drop_column(ProxyUser::Tenant)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ProxyUser::Table)
                    .add_column(
                        ColumnDef::new(ProxyUser::Tenant)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
enum ProxyUser {
    Table,
    Tenant,
}
