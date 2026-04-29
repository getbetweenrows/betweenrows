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
    /// DEPRECATED — logically dropped 2026-04-26. Always written as `None`.
    /// The TCP peer address pgwire exposes is the Fly edge proxy, not the real
    /// client, so the value was misleading. Column kept for backward DB compat.
    /// Do NOT start writing it again without first parsing PROXY protocol v2 in
    /// the accept loop and gating it behind `BR_TRUST_PROXY_PROTOCOL`.
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
