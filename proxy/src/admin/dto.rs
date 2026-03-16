use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::proxy_user;
use crate::policy_match::{PolicyType, TargetEntry};

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
    /// "open" or "policy_required" (default "policy_required")
    #[serde(default = "default_access_mode")]
    pub access_mode: String,
}

fn default_access_mode() -> String {
    "policy_required".to_string()
}

pub fn validate_access_mode(mode: &str) -> bool {
    matches!(mode, "open" | "policy_required")
}

/// Username: 3–50 chars, starts with a letter, only [a-zA-Z0-9_.-]
pub fn validate_username(name: &str) -> Result<(), &'static str> {
    if name.len() < 3 || name.len() > 50 {
        return Err("Username must be between 3 and 50 characters");
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return Err("Username must start with a letter"),
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        return Err("Username may only contain letters, digits, underscores, dots, and hyphens");
    }
    Ok(())
}

/// Datasource name: 1–64 chars, starts with a letter, only [a-zA-Z0-9_-]
/// No spaces — the name is used as a DataFusion catalog identifier.
pub fn validate_datasource_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 64 {
        return Err("Datasource name must be between 1 and 64 characters");
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return Err("Datasource name must start with a letter"),
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) {
        return Err("Datasource name may only contain letters, digits, underscores, and hyphens");
    }
    Ok(())
}

/// Policy name: 1–100 chars, no leading/trailing whitespace,
/// only [a-zA-Z0-9 _\-.:()'"]
pub fn validate_policy_name(name: &str) -> Result<(), &'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 100 {
        return Err("Policy name must be between 1 and 100 characters");
    }
    if trimmed != name {
        return Err("Policy name must not have leading or trailing whitespace");
    }
    if !name.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, ' ' | '_' | '-' | '.' | ':' | '(' | ')' | '\'' | '"')
    }) {
        return Err("Policy name contains invalid characters");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct UpdateDataSourceRequest {
    pub name: Option<String>,
    pub is_active: Option<bool>,
    /// Flat config update — absent fields are preserved, empty-string secret fields kept as-is.
    pub config: Option<serde_json::Value>,
    pub access_mode: Option<String>,
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
    pub access_mode: String,
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
    /// User-provided alias exposed in DataFusion as the schema's search-path name.
    /// Empty string is treated as "no alias" (equivalent to `None`).
    #[serde(default)]
    pub schema_alias: Option<String>,
    pub is_selected: bool,
    pub tables: Vec<CatalogTableSelection>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CatalogTableSelection {
    pub table_name: String,
    pub table_type: String,
    pub is_selected: bool,
    pub columns: Option<Vec<CatalogColumnSelection>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CatalogColumnSelection {
    pub column_name: String,
    pub is_selected: bool,
}

// ---------- catalog discovery responses ----------

#[derive(Debug, Serialize)]
pub struct DiscoveredSchemaResponse {
    pub schema_name: String,
    /// Stored alias for this schema, if one was saved during a previous discovery run.
    pub schema_alias: Option<String>,
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
    pub is_already_selected: bool,
}

#[derive(Debug, Serialize)]
pub struct CatalogResponse {
    pub schemas: Vec<CatalogSchemaResponse>,
}

#[derive(Debug, Serialize)]
pub struct CatalogSchemaResponse {
    pub id: Uuid,
    pub schema_name: String,
    /// Alias under which this schema is exposed in DataFusion and to end users.
    /// `None` means the raw `schema_name` is used directly.
    pub schema_alias: Option<String>,
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
    pub is_selected: bool,
}

// ---------- policy requests ----------

/// Validate a policy's `definition` JSON for a given `policy_type`.
///
/// - `row_filter`: requires `filter_expression` (string)
/// - `column_mask`: requires `mask_expression` (string)
/// - Others: definition must be absent or null
pub fn validate_definition(
    policy_type: PolicyType,
    definition: &Option<serde_json::Value>,
) -> Result<(), String> {
    match policy_type {
        PolicyType::RowFilter => {
            let def = definition
                .as_ref()
                .ok_or("row_filter policy requires a 'definition' with 'filter_expression'")?;
            match def.get("filter_expression") {
                Some(v) if v.is_string() => Ok(()),
                Some(_) => Err("row_filter: 'filter_expression' must be a string".to_string()),
                None => Err("row_filter: missing required field 'filter_expression'".to_string()),
            }
        }
        PolicyType::ColumnMask => {
            let def = definition
                .as_ref()
                .ok_or("column_mask policy requires a 'definition' with 'mask_expression'")?;
            match def.get("mask_expression") {
                Some(v) if v.is_string() => Ok(()),
                Some(_) => Err("column_mask: 'mask_expression' must be a string".to_string()),
                None => Err("column_mask: missing required field 'mask_expression'".to_string()),
            }
        }
        PolicyType::ColumnAllow | PolicyType::ColumnDeny | PolicyType::TableDeny => Ok(()),
    }
}

/// Validate the `targets` array for a given `policy_type`.
///
/// - All types require at least one resource entry.
/// - `column_mask`, `column_allow`, `column_deny`: each entry must have non-empty `columns`.
/// - `row_filter`, `table_deny`: `columns` must be absent.
pub fn validate_targets(policy_type: PolicyType, targets: &[TargetEntry]) -> Result<(), String> {
    if targets.is_empty() {
        return Err("'targets' must not be empty".to_string());
    }
    for (i, entry) in targets.iter().enumerate() {
        if entry.schemas.is_empty() {
            return Err(format!("targets[{i}]: 'schemas' must not be empty"));
        }
        if entry.tables.is_empty() {
            return Err(format!("targets[{i}]: 'tables' must not be empty"));
        }
        match policy_type {
            PolicyType::ColumnMask | PolicyType::ColumnAllow | PolicyType::ColumnDeny => {
                match &entry.columns {
                    None => {
                        return Err(format!(
                            "targets[{i}]: '{policy_type}' requires non-empty 'columns'"
                        ));
                    }
                    Some(cols) if cols.is_empty() => {
                        return Err(format!(
                            "targets[{i}]: '{policy_type}' requires non-empty 'columns'"
                        ));
                    }
                    _ => {}
                }
            }
            PolicyType::RowFilter | PolicyType::TableDeny => {
                if entry.columns.is_some() {
                    return Err(format!(
                        "targets[{i}]: '{policy_type}' must not have 'columns'"
                    ));
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub policy_type: PolicyType,
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub is_enabled: bool,
    pub targets: Vec<TargetEntry>,
    pub definition: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct UpdatePolicyRequest {
    pub name: Option<String>,
    pub policy_type: Option<PolicyType>,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
    pub targets: Option<Vec<TargetEntry>>,
    pub definition: Option<serde_json::Value>,
    /// Optimistic concurrency: client must send the current version
    pub version: i32,
}

#[derive(Debug, Deserialize)]
pub struct ListPoliciesQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AssignPolicyRequest {
    pub policy_id: uuid::Uuid,
    pub user_id: Option<uuid::Uuid>, // None = all users
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_priority() -> i32 {
    100
}

// ---------- policy responses ----------

#[derive(Debug, Serialize)]
pub struct PolicyResponse {
    pub id: uuid::Uuid,
    pub name: String,
    pub description: Option<String>,
    pub policy_type: String,
    pub targets: serde_json::Value,
    pub definition: Option<serde_json::Value>,
    pub is_enabled: bool,
    pub version: i32,
    pub assignment_count: usize,
    pub created_by: uuid::Uuid,
    pub updated_by: uuid::Uuid,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignments: Option<Vec<PolicyAssignmentResponse>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct PolicyAssignmentResponse {
    pub id: uuid::Uuid,
    pub policy_id: uuid::Uuid,
    pub policy_name: String,
    pub data_source_id: uuid::Uuid,
    pub datasource_name: String,
    pub user_id: Option<uuid::Uuid>,
    pub username: Option<String>, // None = all users
    pub priority: i32,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

// ---------- audit log requests/responses ----------

#[derive(Debug, Deserialize)]
pub struct ListAuditLogQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub user_id: Option<uuid::Uuid>,
    pub datasource_id: Option<uuid::Uuid>,
    pub from: Option<String>, // ISO datetime string
    pub to: Option<String>,
    pub status: Option<String>, // "success" | "error" | "denied"
}

#[derive(Debug, Serialize)]
pub struct AuditLogResponse {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub username: String,
    pub data_source_id: uuid::Uuid,
    pub datasource_name: String,
    pub original_query: String,
    pub rewritten_query: Option<String>,
    pub policies_applied: serde_json::Value,
    pub execution_time_ms: Option<i64>,
    pub client_ip: Option<String>,
    pub client_info: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub status: String,
    pub error_message: Option<String>,
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
