// Foreign key: data_source_access.user_id -> proxy_users.id ON DELETE CASCADE.
//
// SQLite does not support ALTER TABLE ADD CONSTRAINT, so this migration is a
// no-op on SQLite. The FK is declared in the SeaORM entity
// (data_source_access::Relation::User) for documentation/ORM purposes. On
// PostgreSQL production deploys the FK can be added manually if needed.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // No-op: SQLite cannot add FK constraints to existing tables.
        // The cascade behavior is enforced at the application layer
        // (role_handlers::delete_role cleans up data_source_access rows).
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
