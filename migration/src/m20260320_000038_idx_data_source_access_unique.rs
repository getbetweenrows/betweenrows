use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite doesn't support partial unique indexes, so use COALESCE to create a composite
        // unique key that prevents duplicate assignments of the same scope+target+datasource.
        // The nil UUID sentinel (00000000-...) is safe because all real IDs are UUID v7
        // (timestamp-based), which never produce the nil value.
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_dsa_scope \
             ON data_source_access( \
                 assignment_scope, \
                 COALESCE(user_id, '00000000-0000-0000-0000-000000000000'), \
                 COALESCE(role_id, '00000000-0000-0000-0000-000000000000'), \
                 data_source_id \
             )",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_dsa_scope")
            .await?;
        Ok(())
    }
}
