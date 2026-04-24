// Makes column_anchor.relationship_id nullable so an anchor can alternatively
// carry actual_column_name (same-table alias) instead of a relationship_id
// (FK walk). Exactly one of the two is set — XOR is enforced at the API layer.
//
// On SQLite, ALTER COLUMN DROP NOT NULL is not supported natively, so this
// migration does the table-recreate dance:
//   1. CREATE TABLE column_anchor_new with the relaxed constraint
//   2. INSERT INTO column_anchor_new SELECT ... FROM column_anchor
//   3. DROP TABLE column_anchor
//   4. ALTER TABLE column_anchor_new RENAME TO column_anchor
//   5. Recreate idx_column_anchor_unique
//
// On PostgreSQL, the first step emits a single native ALTER COLUMN DROP NOT
// NULL. This migration intentionally performs multiple statements on SQLite;
// per `.claude/CLAUDE.md`, this is the unavoidable pattern for "make column
// nullable" on SQLite. If interrupted mid-way, dev DBs may need manual
// recovery (drop `column_anchor_new` and re-run).
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        match conn.get_database_backend() {
            DatabaseBackend::Sqlite => {
                // 1. New table with relaxed NOT NULL on relationship_id.
                conn.execute_unprepared(
                    r#"
                    CREATE TABLE column_anchor_new (
                        id BLOB NOT NULL PRIMARY KEY,
                        data_source_id BLOB NOT NULL,
                        child_table_id BLOB NOT NULL,
                        resolved_column_name TEXT NOT NULL,
                        relationship_id BLOB NULL,
                        designated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                        designated_by BLOB NOT NULL,
                        actual_column_name TEXT NULL,
                        FOREIGN KEY (data_source_id) REFERENCES data_source(id) ON DELETE CASCADE,
                        FOREIGN KEY (child_table_id) REFERENCES discovered_table(id) ON DELETE CASCADE,
                        FOREIGN KEY (relationship_id) REFERENCES table_relationship(id) ON DELETE RESTRICT
                    )
                    "#,
                )
                .await?;

                // 2. Copy data (actual_column_name defaults to NULL for all existing rows).
                conn.execute_unprepared(
                    r#"
                    INSERT INTO column_anchor_new
                        (id, data_source_id, child_table_id, resolved_column_name,
                         relationship_id, designated_at, designated_by, actual_column_name)
                    SELECT id, data_source_id, child_table_id, resolved_column_name,
                           relationship_id, designated_at, designated_by, actual_column_name
                    FROM column_anchor
                    "#,
                )
                .await?;

                // 3. Drop old table.
                conn.execute_unprepared("DROP TABLE column_anchor").await?;

                // 4. Rename new → old name.
                conn.execute_unprepared("ALTER TABLE column_anchor_new RENAME TO column_anchor")
                    .await?;

                // 5. Recreate the unique index that lived on the original table.
                conn.execute_unprepared(
                    "CREATE UNIQUE INDEX IF NOT EXISTS idx_column_anchor_unique \
                     ON column_anchor (data_source_id, child_table_id, resolved_column_name)",
                )
                .await?;
            }
            DatabaseBackend::Postgres => {
                conn.execute(Statement::from_string(
                    DatabaseBackend::Postgres,
                    "ALTER TABLE column_anchor ALTER COLUMN relationship_id DROP NOT NULL"
                        .to_string(),
                ))
                .await?;
            }
            DatabaseBackend::MySql => {
                // Not used in this project but kept for completeness.
                conn.execute(Statement::from_string(
                    DatabaseBackend::MySql,
                    "ALTER TABLE column_anchor MODIFY relationship_id BINARY(16) NULL".to_string(),
                ))
                .await?;
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Down is best-effort: making a nullable column NOT NULL requires that
        // no NULLs exist. We don't reverse the SQLite rebuild — a re-up of
        // this migration is idempotent on a PG/SQLite schema that already
        // matches the target shape.
        let conn = manager.get_connection();
        match conn.get_database_backend() {
            DatabaseBackend::Postgres => {
                conn.execute(Statement::from_string(
                    DatabaseBackend::Postgres,
                    "ALTER TABLE column_anchor ALTER COLUMN relationship_id SET NOT NULL"
                        .to_string(),
                ))
                .await?;
            }
            DatabaseBackend::Sqlite | DatabaseBackend::MySql => {
                // No-op; SQLite down would require another rebuild and we
                // don't support downgrading past this boundary.
            }
        }
        Ok(())
    }
}
