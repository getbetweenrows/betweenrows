use sea_orm_migration::prelude::*;

/// Clear old static WASM blobs so they are recompiled in dynamic mode at startup.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("UPDATE decision_function SET decision_wasm = NULL")
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Cannot restore old WASM blobs — they'll be recompiled on next save.
        Ok(())
    }
}
