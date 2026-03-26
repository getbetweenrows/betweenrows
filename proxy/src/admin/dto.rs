use chrono::NaiveDateTime;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::entity::proxy_user;
use crate::policy_match::{PolicyType, TargetEntry};

/// Deserialize `Option<Option<serde_json::Value>>` from JSON.
fn deserialize_optional_nullable_value<'de, D>(
    deserializer: D,
) -> Result<Option<Option<serde_json::Value>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<Option<serde_json::Value>> = Option::deserialize(deserializer)?;
    Ok(opt)
}

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

// ---------- decision function requests ----------

#[derive(Debug, Deserialize)]
pub struct ListDecisionFunctionsQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateDecisionFunctionRequest {
    pub name: String,
    pub description: Option<String>,
    #[serde(default = "default_language")]
    pub language: String,
    pub decision_fn: String,
    pub decision_config: Option<serde_json::Value>,
    pub evaluate_context: String,
    #[serde(default = "default_on_error")]
    pub on_error: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_language() -> String {
    "javascript".to_string()
}
fn default_on_error() -> String {
    "deny".to_string()
}
fn default_log_level() -> String {
    "off".to_string()
}

#[derive(Debug, Deserialize)]
pub struct UpdateDecisionFunctionRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub language: Option<String>,
    pub decision_fn: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable_value")]
    pub decision_config: Option<Option<serde_json::Value>>,
    pub evaluate_context: Option<String>,
    pub on_error: Option<String>,
    pub log_level: Option<String>,
    pub is_enabled: Option<bool>,
    /// Optimistic concurrency
    pub version: i32,
}

/// Validate decision function fields.
pub fn validate_decision_function_fields(
    language: &str,
    decision_fn: &str,
    evaluate_context: &str,
    on_error: &str,
    log_level: &str,
) -> Result<(), String> {
    if language != "javascript" {
        return Err(format!(
            "language must be 'javascript' (got '{language}'). Other languages are not yet supported."
        ));
    }
    if decision_fn.trim().is_empty() {
        return Err("decision_fn must not be empty".to_string());
    }
    if !matches!(evaluate_context, "session" | "query") {
        return Err(format!(
            "evaluate_context must be 'session' or 'query', got '{evaluate_context}'"
        ));
    }
    if !matches!(on_error, "deny" | "skip") {
        return Err(format!(
            "on_error must be 'deny' or 'skip', got '{on_error}'"
        ));
    }
    if !matches!(log_level, "off" | "error" | "info") {
        return Err(format!(
            "log_level must be 'off', 'error', or 'info', got '{log_level}'"
        ));
    }
    Ok(())
}

// ---------- decision function responses ----------

#[derive(Debug, Serialize)]
pub struct DecisionFunctionResponse {
    pub id: uuid::Uuid,
    pub name: String,
    pub description: Option<String>,
    pub language: String,
    pub decision_fn: String,
    pub decision_config: Option<serde_json::Value>,
    pub evaluate_context: String,
    pub on_error: String,
    pub log_level: String,
    pub is_enabled: bool,
    pub version: i32,
    pub policy_count: usize,
    pub created_by: uuid::Uuid,
    pub updated_by: uuid::Uuid,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

/// Summary embedded in PolicyResponse.
#[derive(Debug, Serialize, Clone)]
pub struct DecisionFunctionSummary {
    pub id: uuid::Uuid,
    pub name: String,
    pub is_enabled: bool,
    pub evaluate_context: String,
}

// ---------- policy requests ----------

#[derive(Debug, Deserialize)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub policy_type: PolicyType,
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub is_enabled: bool,
    pub targets: Vec<TargetEntry>,
    pub definition: Option<serde_json::Value>,
    /// Optional FK to an existing decision_function.
    pub decision_function_id: Option<uuid::Uuid>,
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
    /// 3-state nullable: absent=no change, null=detach, uuid=attach.
    #[serde(default, deserialize_with = "deserialize_optional_nullable_uuid")]
    pub decision_function_id: Option<Option<uuid::Uuid>>,
    /// Optimistic concurrency: client must send the current version
    pub version: i32,
}

/// Deserialize `Option<Option<Uuid>>` from JSON:
/// - absent → `None` (no change)
/// - `null` → `Some(None)` (detach)
/// - `"uuid"` → `Some(Some(uuid))` (attach)
fn deserialize_optional_nullable_uuid<'de, D>(
    deserializer: D,
) -> Result<Option<Option<uuid::Uuid>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<Option<uuid::Uuid>> = Option::deserialize(deserializer)?;
    Ok(opt)
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
    pub user_id: Option<uuid::Uuid>,
    pub role_id: Option<uuid::Uuid>,
    /// "user", "role", or "all". Inferred if not provided:
    /// role_id set → "role", user_id set → "user", both null → "all"
    pub scope: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_function_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_function: Option<DecisionFunctionSummary>,
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
    pub username: Option<String>,
    pub role_id: Option<uuid::Uuid>,
    pub role_name: Option<String>,
    pub assignment_scope: String,
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

// ---------- decision function test ----------

#[derive(Debug, Deserialize)]
pub struct TestDecisionFnRequest {
    /// JS function source code to test.
    pub decision_fn: String,
    /// Mock context JSON (the `ctx` parameter passed to the function).
    pub context: serde_json::Value,
    /// Config JSON (the `config` parameter passed to the function).
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct TestDecisionFnResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<crate::decision::DecisionResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
