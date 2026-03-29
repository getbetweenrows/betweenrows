use std::sync::LazyLock;

use chrono::NaiveDateTime;
use sea_orm::{Iden, Iterable};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::entity::proxy_user;
use crate::policy_match::{PolicyType, TargetEntry};

/// Deserialize `Option<Option<T>>` with 3-state semantics:
/// - absent → `None` (no change) — handled by `#[serde(default)]`
/// - `null` → `Some(None)` (clear)
/// - value → `Some(Some(value))` (set)
///
/// Must pair with `#[serde(default)]` so absent fields become `None`.
/// When this function is called, the field IS present in JSON, so we
/// always wrap in `Some(...)`.
fn deserialize_optional_nullable<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    // Field is present → wrap in Some. null → Some(None), value → Some(Some(v))
    let inner: Option<T> = Option::deserialize(deserializer)?;
    Ok(Some(inner))
}

/// Alias for backward compat with existing call sites.
fn deserialize_optional_nullable_value<'de, D>(
    deserializer: D,
) -> Result<Option<Option<serde_json::Value>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_nullable(deserializer)
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
    /// Full-replace semantics: absent = don't touch, {} = clear all,
    /// {"key": "val"} = replace with exactly this.
    /// Values may be strings for scalar types or arrays of strings for list type.
    pub attributes: Option<std::collections::HashMap<String, serde_json::Value>>,
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
    pub attributes: std::collections::HashMap<String, serde_json::Value>,
    pub last_login_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<proxy_user::Model> for UserResponse {
    fn from(m: proxy_user::Model) -> Self {
        let attributes = proxy_user::parse_attributes(&m.attributes);
        Self {
            id: m.id,
            username: m.username,
            tenant: m.tenant,
            is_admin: m.is_admin,
            is_active: m.is_active,
            email: m.email,
            display_name: m.display_name,
            attributes,
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

// ---------- attribute definition requests/responses ----------

#[derive(Debug, Deserialize)]
pub struct CreateAttributeDefinitionRequest {
    pub key: String,
    pub entity_type: String,
    pub display_name: String,
    pub value_type: String,
    pub default_value: Option<String>,
    pub allowed_values: Option<Vec<String>>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAttributeDefinitionRequest {
    pub display_name: Option<String>,
    pub value_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    pub default_value: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    pub allowed_values: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    pub description: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ListAttributeDefinitionsQuery {
    pub entity_type: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteAttributeDefinitionQuery {
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Serialize)]
pub struct AttributeDefinitionResponse {
    pub id: Uuid,
    pub key: String,
    pub entity_type: String,
    pub display_name: String,
    pub value_type: String,
    pub default_value: Option<String>,
    pub allowed_values: Option<Vec<String>>,
    pub description: Option<String>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<crate::entity::attribute_definition::Model> for AttributeDefinitionResponse {
    fn from(m: crate::entity::attribute_definition::Model) -> Self {
        let allowed_values: Option<Vec<String>> = m
            .allowed_values
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        Self {
            id: m.id,
            key: m.key,
            entity_type: m.entity_type,
            display_name: m.display_name,
            value_type: m.value_type,
            default_value: m.default_value,
            allowed_values,
            description: m.description,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

/// Validate attribute definition key: 1-64 chars, starts with letter, [a-zA-Z0-9_].
pub fn validate_attribute_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > 64 {
        return Err("key must be 1-64 characters".to_string());
    }
    let first = key.chars().next().unwrap();
    if !first.is_ascii_alphabetic() {
        return Err("key must start with a letter".to_string());
    }
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("key must contain only letters, digits, and underscores".to_string());
    }
    Ok(())
}

/// Additional reserved names that aren't ORM columns but should still be blocked.
/// Virtual context fields and confusable aliases.
const EXTRA_RESERVED_USER_KEYS: &[&str] = &["roles", "user_id"];

/// Reserved attribute keys for user entities — ORM columns + extra reserved names.
/// Computed once on first access via `LazyLock`.
///
/// We derive from the ORM layer (`proxy_user::Column`) to automatically catch DB-level
/// names, plus `EXTRA_RESERVED_USER_KEYS` for virtual fields (`roles`) and aliases
/// (`user_id`). This intentionally over-reserves (e.g. `password_hash`, `created_at`)
/// which is harmless. A more precise approach would derive from the DTO layer
/// (`UserResponse` fields + decision context fields), but risks missing DB-level names
/// if DTO fields are aliased differently from columns. Revisit if we add a formal
/// user identity schema that unifies both layers.
static RESERVED_USER_ATTRIBUTE_KEYS: LazyLock<Vec<String>> = LazyLock::new(|| {
    let mut keys: Vec<String> = proxy_user::Column::iter()
        .map(|c| {
            let mut buf = String::new();
            c.unquoted(&mut buf);
            buf
        })
        .collect();
    keys.extend(EXTRA_RESERVED_USER_KEYS.iter().map(|s| String::from(*s)));
    keys
});
const VALID_ENTITY_TYPES: &[&str] = &["user", "table", "column"];
const VALID_VALUE_TYPES: &[&str] = &["string", "integer", "boolean", "list"];

pub fn validate_attribute_definition(
    key: &str,
    entity_type: &str,
    value_type: &str,
    default_value: Option<&str>,
    allowed_values: Option<&[String]>,
) -> Result<(), String> {
    validate_attribute_key(key)?;

    if !VALID_ENTITY_TYPES.contains(&entity_type) {
        return Err(format!(
            "entity_type must be one of: {}",
            VALID_ENTITY_TYPES.join(", ")
        ));
    }

    if entity_type == "user" && RESERVED_USER_ATTRIBUTE_KEYS.iter().any(|k| k == key) {
        return Err(format!(
            "'{key}' is a reserved attribute key for user entities"
        ));
    }

    if !VALID_VALUE_TYPES.contains(&value_type) {
        return Err(format!(
            "value_type must be one of: {}",
            VALID_VALUE_TYPES.join(", ")
        ));
    }

    if let Some(dv) = default_value {
        crate::entity::attribute_definition::validate_value(dv, value_type)
            .map_err(|e| format!("invalid default_value: {e}"))?;
    }

    if let Some(avs) = allowed_values {
        // For list type, allowed_values constrains the individual elements (strings),
        // not the list value itself.
        let element_type = if value_type == "list" {
            "string"
        } else {
            value_type
        };
        for av in avs {
            crate::entity::attribute_definition::validate_value(av, element_type)
                .map_err(|e| format!("invalid allowed_value '{av}': {e}"))?;
        }
        if value_type != "list" {
            if let Some(dv) = default_value
                && !avs.iter().any(|v| v == dv)
            {
                return Err("default_value must be in allowed_values".to_string());
            }
        } else if let Some(dv) = default_value {
            // For list type, each element of the default value must be in allowed_values.
            let default_elems: Vec<String> = serde_json::from_str(dv)
                .map_err(|_| "default_value for list must be a JSON array of strings")?;
            for elem in &default_elems {
                if !avs.iter().any(|v| v == elem) {
                    return Err(format!(
                        "default_value element '{elem}' is not in allowed_values"
                    ));
                }
            }
        }
    }

    Ok(())
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
                Some(v) if v.is_string() => {
                    let expr = v.as_str().unwrap();
                    crate::hooks::policy::validate_expression(expr, false)?;
                    Ok(())
                }
                Some(_) => Err("row_filter: 'filter_expression' must be a string".to_string()),
                None => Err("row_filter: missing required field 'filter_expression'".to_string()),
            }
        }
        PolicyType::ColumnMask => {
            let def = definition
                .as_ref()
                .ok_or("column_mask policy requires a 'definition' with 'mask_expression'")?;
            match def.get("mask_expression") {
                Some(v) if v.is_string() => {
                    let expr = v.as_str().unwrap();
                    crate::hooks::policy::validate_expression(expr, true)?;
                    Ok(())
                }
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

// ---------- expression validation ----------

#[derive(Debug, Deserialize)]
pub struct ValidateExpressionRequest {
    pub expression: String,
    pub is_mask: bool,
}

#[derive(Debug, Serialize)]
pub struct ValidateExpressionResponse {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- validate_attribute_key ----------

    #[test]
    fn key_valid() {
        assert!(validate_attribute_key("region").is_ok());
        assert!(validate_attribute_key("clearance_level").is_ok());
        assert!(validate_attribute_key("a").is_ok());
        assert!(validate_attribute_key("dept123").is_ok());
    }

    #[test]
    fn key_empty() {
        assert!(validate_attribute_key("").is_err());
    }

    #[test]
    fn key_too_long() {
        let long = "a".repeat(65);
        assert!(validate_attribute_key(&long).is_err());
    }

    #[test]
    fn key_starts_with_digit() {
        assert!(validate_attribute_key("1region").is_err());
    }

    #[test]
    fn key_special_chars() {
        assert!(validate_attribute_key("my-key").is_err());
        assert!(validate_attribute_key("my.key").is_err());
        assert!(validate_attribute_key("my key").is_err());
    }

    // ---------- validate_attribute_definition ----------

    #[test]
    fn valid_definition() {
        assert!(validate_attribute_definition("region", "user", "string", None, None).is_ok());
        assert!(validate_attribute_definition("level", "user", "integer", Some("3"), None).is_ok());
        assert!(
            validate_attribute_definition("active", "column", "boolean", Some("true"), None)
                .is_ok()
        );
    }

    #[test]
    fn reserved_key_for_user() {
        // Built-in context fields
        assert!(validate_attribute_definition("tenant", "user", "string", None, None).is_err());
        assert!(validate_attribute_definition("username", "user", "string", None, None).is_err());
        assert!(validate_attribute_definition("id", "user", "string", None, None).is_err());
        assert!(validate_attribute_definition("user_id", "user", "string", None, None).is_err());
        assert!(validate_attribute_definition("roles", "user", "string", None, None).is_err());
        // ORM columns are also reserved
        assert!(validate_attribute_definition("is_admin", "user", "string", None, None).is_err());
        assert!(
            validate_attribute_definition("password_hash", "user", "string", None, None).is_err()
        );
    }

    #[test]
    fn reserved_key_ok_for_non_user() {
        // "tenant" is only reserved for entity_type="user"
        assert!(validate_attribute_definition("tenant", "table", "string", None, None).is_ok());
    }

    #[test]
    fn invalid_entity_type() {
        assert!(validate_attribute_definition("key", "database", "string", None, None).is_err());
    }

    #[test]
    fn invalid_value_type() {
        assert!(validate_attribute_definition("key", "user", "float", None, None).is_err());
    }

    #[test]
    fn default_value_type_mismatch() {
        assert!(
            validate_attribute_definition("key", "user", "integer", Some("abc"), None).is_err()
        );
        assert!(
            validate_attribute_definition("key", "user", "boolean", Some("yes"), None).is_err()
        );
    }

    #[test]
    fn allowed_values_type_mismatch() {
        let av = vec!["abc".to_string()];
        assert!(validate_attribute_definition("key", "user", "integer", None, Some(&av)).is_err());
    }

    #[test]
    fn default_not_in_allowed_values() {
        let av = vec!["a".to_string(), "b".to_string()];
        assert!(
            validate_attribute_definition("key", "user", "string", Some("c"), Some(&av)).is_err()
        );
    }

    #[test]
    fn default_in_allowed_values() {
        let av = vec!["a".to_string(), "b".to_string()];
        assert!(
            validate_attribute_definition("key", "user", "string", Some("a"), Some(&av)).is_ok()
        );
    }

    // ---------- UpdateAttributeDefinitionRequest 3-state deserialization ----------

    #[test]
    fn attr_update_absent_fields() {
        let json = r#"{}"#;
        let req: UpdateAttributeDefinitionRequest = serde_json::from_str(json).unwrap();
        assert!(req.default_value.is_none());
        assert!(req.allowed_values.is_none());
        assert!(req.description.is_none());
    }

    #[test]
    fn attr_update_null_clears() {
        let json = r#"{"default_value": null, "allowed_values": null, "description": null}"#;
        let req: UpdateAttributeDefinitionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.default_value, Some(None));
        assert_eq!(req.allowed_values, Some(None));
        assert_eq!(req.description, Some(None));
    }

    #[test]
    fn attr_update_value_sets() {
        let json =
            r#"{"default_value": "foo", "allowed_values": ["a","b"], "description": "desc"}"#;
        let req: UpdateAttributeDefinitionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.default_value, Some(Some("foo".to_string())));
        assert_eq!(
            req.allowed_values,
            Some(Some(vec!["a".to_string(), "b".to_string()]))
        );
        assert_eq!(req.description, Some(Some("desc".to_string())));
    }

    // ---------- validate_expression (save-time) ----------

    #[test]
    fn validate_filter_expression_ok() {
        assert!(crate::hooks::policy::validate_expression("tenant = {user.tenant}", false).is_ok());
        assert!(crate::hooks::policy::validate_expression("level >= 3", false).is_ok());
        assert!(crate::hooks::policy::validate_expression("1=1", false).is_ok());
    }

    #[test]
    fn validate_filter_expression_bad_syntax() {
        assert!(
            crate::hooks::policy::validate_expression("EXTRACT(HOUR FROM col)", false).is_err()
        );
    }

    #[test]
    fn validate_mask_expression_ok() {
        assert!(crate::hooks::policy::validate_expression("'REDACTED'", true).is_ok());
        assert!(
            crate::hooks::policy::validate_expression("'***-**-' || RIGHT(ssn, 4)", true).is_ok()
        );
    }

    #[test]
    fn validate_mask_case_when_ok() {
        assert!(
            crate::hooks::policy::validate_expression(
                "CASE WHEN {user.clearance} >= 3 THEN salary ELSE 0 END",
                true,
            )
            .is_ok()
        );
    }

    #[test]
    fn validate_filter_list_attribute_in_ok() {
        // List-type attributes like {user.departments} are treated as scalar during validation
        // (dummy_vars has empty attributes). This is safe because IN (scalar) is valid SQL.
        assert!(
            crate::hooks::policy::validate_expression("department IN ({user.departments})", false,)
                .is_ok()
        );
        assert!(
            crate::hooks::policy::validate_expression(
                "department NOT IN ({user.departments})",
                false,
            )
            .is_ok()
        );
    }
}
