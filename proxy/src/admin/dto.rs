use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::proxy_user;

// ---------- user requests ----------

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub tenant: String,
    #[serde(default)]
    pub is_admin: bool,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub tenant: Option<String>,
    pub is_admin: Option<bool>,
    pub is_active: Option<bool>,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ListUsersQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub search: Option<String>,
}

// ---------- user responses ----------

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Serialize, Clone)]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub tenant: String,
    pub is_admin: bool,
    pub is_active: bool,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub last_login_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<proxy_user::Model> for UserResponse {
    fn from(m: proxy_user::Model) -> Self {
        Self {
            id: m.id,
            username: m.username,
            tenant: m.tenant,
            is_admin: m.is_admin,
            is_active: m.is_active,
            email: m.email,
            display_name: m.display_name,
            last_login_at: m.last_login_at,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

// ---------- data source requests ----------

#[derive(Debug, Deserialize)]
pub struct CreateDataSourceRequest {
    pub name: String,
    pub ds_type: String,
    /// Flat config object containing all fields (secret and non-secret).
    /// Backend splits them into config/secure_config using the type registry.
    pub config: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDataSourceRequest {
    pub name: Option<String>,
    pub is_active: Option<bool>,
    /// Flat config update — absent fields are preserved, empty-string secret fields kept as-is.
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ListDataSourcesQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetDataSourceUsersRequest {
    pub user_ids: Vec<Uuid>,
}

// ---------- data source responses ----------

#[derive(Debug, Serialize)]
pub struct DataSourceResponse {
    pub id: Uuid,
    pub name: String,
    pub ds_type: String,
    /// Non-secret config only (password/secrets are never returned).
    pub config: serde_json::Value,
    pub is_active: bool,
    pub last_sync_at: Option<NaiveDateTime>,
    pub last_sync_result: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Serialize)]
pub struct TestConnectionResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ---------- catalog discovery requests ----------

#[derive(Debug, Deserialize)]
pub struct DiscoverTablesRequest {
    pub schemas: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DiscoverColumnsRequest {
    /// Each item is {schema, table}
    pub tables: Vec<TableRef>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TableRef {
    pub schema: String,
    pub table: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveCatalogRequest {
    pub schemas: Vec<CatalogSchemaSelection>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CatalogSchemaSelection {
    pub schema_name: String,
    pub is_selected: bool,
    pub tables: Vec<CatalogTableSelection>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CatalogTableSelection {
    pub table_name: String,
    pub table_type: String,
    pub is_selected: bool,
}

// ---------- catalog discovery responses ----------

#[derive(Debug, Serialize)]
pub struct DiscoveredSchemaResponse {
    pub schema_name: String,
    pub is_already_selected: bool,
}

#[derive(Debug, Serialize)]
pub struct DiscoveredTableResponse {
    pub schema_name: String,
    pub table_name: String,
    pub table_type: String,
    pub is_already_selected: bool,
}

#[derive(Debug, Serialize)]
pub struct DiscoveredColumnResponse {
    pub schema_name: String,
    pub table_name: String,
    pub column_name: String,
    pub ordinal_position: i32,
    pub data_type: String,
    pub is_nullable: bool,
    pub column_default: Option<String>,
    pub arrow_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CatalogResponse {
    pub schemas: Vec<CatalogSchemaResponse>,
}

#[derive(Debug, Serialize)]
pub struct CatalogSchemaResponse {
    pub id: Uuid,
    pub schema_name: String,
    pub is_selected: bool,
    pub tables: Vec<CatalogTableResponse>,
}

#[derive(Debug, Serialize)]
pub struct CatalogTableResponse {
    pub id: Uuid,
    pub table_name: String,
    pub table_type: String,
    pub is_selected: bool,
    pub columns: Vec<CatalogColumnResponse>,
}

#[derive(Debug, Serialize)]
pub struct CatalogColumnResponse {
    pub id: Uuid,
    pub column_name: String,
    pub ordinal_position: i32,
    pub data_type: String,
    pub is_nullable: bool,
    pub column_default: Option<String>,
    pub arrow_type: Option<String>,
}

// ---------- discovery job responses ----------

#[derive(Debug, Serialize)]
pub struct SubmitDiscoveryResponse {
    pub job_id: String,
}

#[derive(Debug, Serialize)]
pub struct JobStatusResponse {
    pub job_id: String,
    pub action: String,
    pub status: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}
