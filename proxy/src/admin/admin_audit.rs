//! Admin audit log infrastructure.
//!
//! Append-only audit trail for all admin mutations. No UPDATE or DELETE endpoints
//! are exposed — this is a security invariant.
//!
//! # Usage
//!
//! Use `AuditedTxn` for all handlers that mutate entities. It wraps a
//! `DatabaseTransaction` and queues audit entries that are written atomically
//! on `commit()`. This makes the correct pattern (audit inside the transaction)
//! the only pattern — callers cannot forget or misplace audit calls.
//!
//! ```ignore
//! let mut txn = AuditedTxn::begin(&state.db).await?;
//! entity.insert(&*txn).await?;
//! txn.audit("resource", id, AuditAction::Create, actor_id, json!({...}));
//! txn.commit().await?;
//! ```

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbErr, Set,
    TransactionTrait,
};
use std::ops::Deref;
use uuid::Uuid;

use crate::entity::admin_audit_log;

/// Actions that can be recorded in the admin audit log.
#[derive(Debug, Clone, Copy)]
pub enum AuditAction {
    Create,
    Update,
    Delete,
    Deactivate,
    Reactivate,
    AddMember,
    RemoveMember,
    AddInheritance,
    RemoveInheritance,
    Assign,
    Unassign,
}

impl AuditAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Deactivate => "deactivate",
            Self::Reactivate => "reactivate",
            Self::AddMember => "add_member",
            Self::RemoveMember => "remove_member",
            Self::AddInheritance => "add_inheritance",
            Self::RemoveInheritance => "remove_inheritance",
            Self::Assign => "assign",
            Self::Unassign => "unassign",
        }
    }
}

/// A queued audit entry, written to the DB on `AuditedTxn::commit()`.
struct AuditEntry {
    resource_type: String,
    resource_id: Uuid,
    action: AuditAction,
    actor_id: Uuid,
    changes: serde_json::Value,
}

/// A transactional wrapper that queues audit entries and writes them atomically
/// on commit.
///
/// - `Deref<Target = DatabaseTransaction>` — pass `&*txn` to SeaORM operations.
/// - `audit()` queues entries (sync, cheap).
/// - `commit()` writes all entries then commits. Errors if no entries were queued
///   (prevents accidentally unaudited transactions — use a plain `DatabaseTransaction`
///   if you genuinely don't need audit).
/// - Dropping without `commit()` rolls back (SeaORM default).
pub struct AuditedTxn {
    txn: DatabaseTransaction,
    entries: Vec<AuditEntry>,
}

impl AuditedTxn {
    /// Begin a new audited transaction.
    pub async fn begin(db: &DatabaseConnection) -> Result<Self, DbErr> {
        let txn = db.begin().await?;
        Ok(Self {
            txn,
            entries: Vec::new(),
        })
    }

    /// Queue an audit entry. Written to DB on `commit()`.
    pub fn audit(
        &mut self,
        resource_type: &str,
        resource_id: Uuid,
        action: AuditAction,
        actor_id: Uuid,
        changes: serde_json::Value,
    ) {
        self.entries.push(AuditEntry {
            resource_type: resource_type.to_string(),
            resource_id,
            action,
            actor_id,
            changes,
        });
    }

    /// Write all queued audit entries, then commit the transaction.
    ///
    /// Returns `DbErr::Custom` if no audit entries were queued — this prevents
    /// accidental unaudited commits. Use a plain `DatabaseTransaction` if you
    /// don't need audit.
    pub async fn commit(self) -> Result<(), DbErr> {
        if self.entries.is_empty() {
            return Err(DbErr::Custom(
                "AuditedTxn::commit() called with no audit entries queued".to_string(),
            ));
        }

        for entry in &self.entries {
            audit_log(
                &self.txn,
                &entry.resource_type,
                entry.resource_id,
                entry.action,
                entry.actor_id,
                entry.changes.clone(),
            )
            .await?;
        }

        self.txn.commit().await
    }
}

impl Deref for AuditedTxn {
    type Target = DatabaseTransaction;
    fn deref(&self) -> &Self::Target {
        &self.txn
    }
}

/// Insert an admin audit log entry.
///
/// Low-level building block used internally by `AuditedTxn::commit()`.
/// Handlers should use `AuditedTxn` instead of calling this directly.
///
/// Convention: log on the owning entity.
/// - Role membership → resource_type = "role", resource_id = role.id
/// - Policy assignment → resource_type = "policy", resource_id = policy.id
/// - Inheritance → resource_type = "role", resource_id = child_role.id
pub(crate) async fn audit_log<C: ConnectionTrait>(
    db: &C,
    resource_type: &str,
    resource_id: Uuid,
    action: AuditAction,
    actor_id: Uuid,
    changes: serde_json::Value,
) -> Result<(), DbErr> {
    admin_audit_log::ActiveModel {
        id: Set(Uuid::now_v7()),
        resource_type: Set(resource_type.to_string()),
        resource_id: Set(resource_id),
        action: Set(action.as_str().to_string()),
        actor_id: Set(actor_id),
        changes: Set(Some(changes.to_string())),
        created_at: Set(Utc::now().naive_utc()),
    }
    .insert(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use migration::{Migrator, MigratorTrait};
    use sea_orm::{Database, EntityTrait};

    async fn setup() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        db
    }

    #[tokio::test]
    async fn audited_txn_commits_with_entries() {
        let db = setup().await;
        let mut txn = AuditedTxn::begin(&db).await.unwrap();
        txn.audit(
            "test",
            Uuid::now_v7(),
            AuditAction::Create,
            Uuid::now_v7(),
            serde_json::json!({"foo": "bar"}),
        );
        txn.commit().await.unwrap();

        let count = admin_audit_log::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .len();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn audited_txn_rejects_empty_commit() {
        let db = setup().await;
        let txn = AuditedTxn::begin(&db).await.unwrap();
        let result = txn.commit().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no audit entries"));
    }

    #[tokio::test]
    async fn audited_txn_rollback_on_drop() {
        let db = setup().await;

        let actor = Uuid::now_v7();
        {
            let mut txn = AuditedTxn::begin(&db).await.unwrap();
            txn.audit(
                "test",
                Uuid::now_v7(),
                AuditAction::Create,
                actor,
                serde_json::json!({}),
            );
            // dropped without commit
        }

        let count = admin_audit_log::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .len();
        assert_eq!(count, 0, "Dropped AuditedTxn should not write entries");
    }

    #[tokio::test]
    async fn audited_txn_deref_allows_entity_operations() {
        use crate::entity::role;

        let db = setup().await;
        let role_id = Uuid::now_v7();
        let now = chrono::Utc::now().naive_utc();

        let mut txn = AuditedTxn::begin(&db).await.unwrap();

        // Insert entity through the deref'd transaction
        role::ActiveModel {
            id: Set(role_id),
            name: Set("test-role".to_string()),
            description: Set(None),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&*txn)
        .await
        .unwrap();

        txn.audit(
            "role",
            role_id,
            AuditAction::Create,
            Uuid::now_v7(),
            serde_json::json!({"after": {"name": "test-role"}}),
        );

        txn.commit().await.unwrap();

        // Both the entity and audit entry should exist
        let role = role::Entity::find_by_id(role_id).one(&db).await.unwrap();
        assert!(role.is_some(), "Role should exist after commit");

        let audits = admin_audit_log::Entity::find().all(&db).await.unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].resource_id, role_id);
    }

    #[tokio::test]
    async fn audited_txn_drop_rolls_back_entity_and_audit() {
        use crate::entity::role;

        let db = setup().await;
        let role_id = Uuid::now_v7();
        let now = chrono::Utc::now().naive_utc();

        {
            let mut txn = AuditedTxn::begin(&db).await.unwrap();

            role::ActiveModel {
                id: Set(role_id),
                name: Set("ghost-role".to_string()),
                description: Set(None),
                is_active: Set(true),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&*txn)
            .await
            .unwrap();

            txn.audit(
                "role",
                role_id,
                AuditAction::Create,
                Uuid::now_v7(),
                serde_json::json!({}),
            );

            // dropped without commit
        }

        // Neither the entity nor audit entry should exist
        let role = role::Entity::find_by_id(role_id).one(&db).await.unwrap();
        assert!(role.is_none(), "Role should not exist after rollback");

        let audits = admin_audit_log::Entity::find().all(&db).await.unwrap();
        assert_eq!(
            audits.len(),
            0,
            "Audit entries should not exist after rollback"
        );
    }

    #[tokio::test]
    async fn audited_txn_multiple_entries() {
        let db = setup().await;
        let mut txn = AuditedTxn::begin(&db).await.unwrap();
        for i in 0..3 {
            txn.audit(
                "test",
                Uuid::now_v7(),
                AuditAction::Create,
                Uuid::now_v7(),
                serde_json::json!({"i": i}),
            );
        }
        txn.commit().await.unwrap();

        let count = admin_audit_log::Entity::find()
            .all(&db)
            .await
            .unwrap()
            .len();
        assert_eq!(count, 3);
    }
}
