// Foreign key: data_source_access.role_id -> role.id ON DELETE CASCADE.
//
// SQLite does not support ALTER TABLE ADD CONSTRAINT, so this migration is a
// no-op on SQLite. The FK is declared in the SeaORM entity
// (data_source_access::Relation::Role) for documentation/ORM purposes.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
