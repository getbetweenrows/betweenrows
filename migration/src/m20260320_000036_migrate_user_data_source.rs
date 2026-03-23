use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "INSERT OR IGNORE INTO data_source_access (id, user_id, role_id, data_source_id, assignment_scope, created_at) \
             SELECT id, user_id, NULL, data_source_id, 'user', created_at FROM user_data_source",
        )
        .await?;
        Ok(())
    }

    /// NOTE: This rollback deletes ALL user-scoped rows from `data_source_access`,
    /// not just the ones originally migrated from `user_data_source`. Any user-scoped
    /// rows created after migration will also be deleted. This is acceptable because
    /// rolling back this far implies a full reset of the RBAC feature.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DELETE FROM data_source_access WHERE assignment_scope = 'user'")
            .await?;
        Ok(())
    }
}
