use sea_orm::entity::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "query_audit_log")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub data_source_id: Uuid,
    /// Denormalized datasource name (survives rename)
    pub datasource_name: String,
    pub original_query: String,
    pub rewritten_query: Option<String>,
    /// JSON array of {policy_id, version, name}
    pub policies_applied: String,
    pub execution_time_ms: Option<i64>,
    pub client_ip: Option<String>,
    pub client_info: Option<String>,
    pub created_at: DateTime,
    /// "success" | "error" | "denied"
    pub status: String,
    pub error_message: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
