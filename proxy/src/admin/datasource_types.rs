use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub enum FieldType {
    Text,
    Number,
    Select(Vec<&'static str>),
    TextArea,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub key: &'static str,
    pub label: &'static str,
    pub field_type: FieldType,
    pub required: bool,
    pub is_secret: bool,
    pub default_value: Option<&'static str>,
}

pub struct DataSourceTypeDef {
    pub ds_type: &'static str,
    pub label: &'static str,
    pub fields: Vec<FieldDef>,
}

static TYPE_DEFS: OnceLock<Vec<DataSourceTypeDef>> = OnceLock::new();

pub fn get_type_defs() -> &'static [DataSourceTypeDef] {
    TYPE_DEFS.get_or_init(|| {
        vec![DataSourceTypeDef {
            ds_type: "postgres",
            label: "PostgreSQL",
            fields: vec![
                FieldDef {
                    key: "host",
                    label: "Host",
                    field_type: FieldType::Text,
                    required: true,
                    is_secret: false,
                    default_value: None,
                },
                FieldDef {
                    key: "port",
                    label: "Port",
                    field_type: FieldType::Number,
                    required: true,
                    is_secret: false,
                    default_value: Some("5432"),
                },
                FieldDef {
                    key: "database",
                    label: "Database",
                    field_type: FieldType::Text,
                    required: true,
                    is_secret: false,
                    default_value: None,
                },
                FieldDef {
                    key: "username",
                    label: "Username",
                    field_type: FieldType::Text,
                    required: true,
                    is_secret: false,
                    default_value: None,
                },
                FieldDef {
                    key: "password",
                    label: "Password",
                    field_type: FieldType::Text,
                    required: true,
                    is_secret: true,
                    default_value: None,
                },
                FieldDef {
                    key: "sslmode",
                    label: "SSL Mode",
                    field_type: FieldType::Select(vec!["disable", "prefer", "require"]),
                    required: true,
                    is_secret: false,
                    default_value: Some("require"),
                },
            ],
        }]
    })
}

pub fn get_type_def(ds_type: &str) -> Option<&'static DataSourceTypeDef> {
    get_type_defs().iter().find(|d| d.ds_type == ds_type)
}

#[derive(Debug)]
pub enum ConfigError {
    UnknownType(String),
    MissingRequiredField(String),
    InvalidInput(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::UnknownType(t) => write!(f, "Unknown data source type: {t}"),
            ConfigError::MissingRequiredField(k) => write!(f, "Missing required field: {k}"),
            ConfigError::InvalidInput(msg) => write!(f, "Invalid input: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Split a flat config input into (non_secret_config, secret_config).
/// Validates required fields are present (with defaults applied).
pub fn split_config(
    ds_type: &str,
    config_input: serde_json::Value,
) -> Result<(serde_json::Value, serde_json::Value), ConfigError> {
    let type_def = get_type_def(ds_type)
        .ok_or_else(|| ConfigError::UnknownType(ds_type.to_string()))?;

    let input = config_input
        .as_object()
        .ok_or_else(|| ConfigError::InvalidInput("config must be a JSON object".to_string()))?;

    let mut config = serde_json::Map::new();
    let mut secure = serde_json::Map::new();

    for field in &type_def.fields {
        let value = input.get(field.key).cloned();

        let resolved = match value {
            Some(v) => v,
            None => {
                if let Some(default) = field.default_value {
                    match &field.field_type {
                        FieldType::Number => serde_json::Value::Number(
                            default
                                .parse::<i64>()
                                .map_err(|_| {
                                    ConfigError::InvalidInput(format!(
                                        "Default for '{}' is not a valid number",
                                        field.key
                                    ))
                                })?
                                .into(),
                        ),
                        _ => serde_json::Value::String(default.to_string()),
                    }
                } else if field.required {
                    return Err(ConfigError::MissingRequiredField(field.key.to_string()));
                } else {
                    continue;
                }
            }
        };

        if field.is_secret {
            secure.insert(field.key.to_string(), resolved);
        } else {
            config.insert(field.key.to_string(), resolved);
        }
    }

    Ok((
        serde_json::Value::Object(config),
        serde_json::Value::Object(secure),
    ))
}

/// Merge an update input with existing config + secure_config.
/// Preserves existing values for fields not provided in update_input.
/// For secret fields: empty string means "keep existing".
pub fn merge_config(
    ds_type: &str,
    existing_config: serde_json::Value,
    existing_secure: serde_json::Value,
    update_input: serde_json::Value,
) -> Result<(serde_json::Value, serde_json::Value), ConfigError> {
    let type_def = get_type_def(ds_type)
        .ok_or_else(|| ConfigError::UnknownType(ds_type.to_string()))?;

    let input = update_input
        .as_object()
        .ok_or_else(|| ConfigError::InvalidInput("config must be a JSON object".to_string()))?;

    let existing_cfg = existing_config.as_object().cloned().unwrap_or_default();
    let existing_sec = existing_secure.as_object().cloned().unwrap_or_default();

    let mut config = serde_json::Map::new();
    let mut secure = serde_json::Map::new();

    for field in &type_def.fields {
        if let Some(val) = input.get(field.key).cloned() {
            if field.is_secret {
                // Empty string = "keep existing"
                if val.as_str().map(|s| !s.is_empty()).unwrap_or(true) {
                    secure.insert(field.key.to_string(), val);
                } else if let Some(existing_val) = existing_sec.get(field.key).cloned() {
                    secure.insert(field.key.to_string(), existing_val);
                }
            } else {
                config.insert(field.key.to_string(), val);
            }
        } else {
            // Field not in update — preserve existing value
            if field.is_secret {
                if let Some(existing_val) = existing_sec.get(field.key).cloned() {
                    secure.insert(field.key.to_string(), existing_val);
                }
            } else if let Some(existing_val) = existing_cfg.get(field.key).cloned() {
                config.insert(field.key.to_string(), existing_val);
            }
        }
    }

    Ok((
        serde_json::Value::Object(config),
        serde_json::Value::Object(secure),
    ))
}

// ---------- API response types ----------

#[derive(Debug, Serialize)]
pub struct FieldDefResponse {
    pub key: String,
    pub label: String,
    pub field_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    pub required: bool,
    pub is_secret: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DataSourceTypeResponse {
    pub ds_type: String,
    pub label: String,
    pub fields: Vec<FieldDefResponse>,
}

impl From<&DataSourceTypeDef> for DataSourceTypeResponse {
    fn from(def: &DataSourceTypeDef) -> Self {
        Self {
            ds_type: def.ds_type.to_string(),
            label: def.label.to_string(),
            fields: def
                .fields
                .iter()
                .map(|f| FieldDefResponse {
                    key: f.key.to_string(),
                    label: f.label.to_string(),
                    field_type: match &f.field_type {
                        FieldType::Text => "text".to_string(),
                        FieldType::Number => "number".to_string(),
                        FieldType::Select(_) => "select".to_string(),
                        FieldType::TextArea => "textarea".to_string(),
                    },
                    options: match &f.field_type {
                        FieldType::Select(opts) => {
                            Some(opts.iter().map(|s| s.to_string()).collect())
                        }
                        _ => None,
                    },
                    required: f.required,
                    is_secret: f.is_secret,
                    default_value: f.default_value.map(|s| s.to_string()),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_type_def_postgres() {
        let def = get_type_def("postgres");
        assert!(def.is_some(), "Expected to find postgres type definition");
        let def = def.unwrap();
        assert_eq!(def.ds_type, "postgres");
        assert_eq!(def.label, "PostgreSQL");
        assert!(def.fields.len() >= 5, "Expected at least 5 fields");
    }

    #[test]
    fn test_get_type_def_unknown() {
        let def = get_type_def("mongodb");
        assert!(def.is_none(), "Expected None for unknown type");
    }

    #[test]
    fn test_postgres_has_password_as_secret() {
        let def = get_type_def("postgres").unwrap();
        let password = def.fields.iter().find(|f| f.key == "password");
        assert!(password.is_some(), "Expected password field");
        assert!(password.unwrap().is_secret, "Password field must be secret");
    }

    #[test]
    fn test_split_config_separates_secret_fields() {
        let input = serde_json::json!({
            "host": "db.example.com",
            "port": 5432,
            "database": "mydb",
            "username": "alice",
            "password": "s3cret",
            "sslmode": "require",
        });

        let (config, secure) = split_config("postgres", input).unwrap();

        assert_eq!(config["host"], "db.example.com");
        assert_eq!(config["port"], 5432);
        assert!(
            config.get("password").is_none(),
            "Password must NOT be in config"
        );
        assert_eq!(secure["password"], "s3cret");
    }

    #[test]
    fn test_split_config_missing_required_field() {
        let input = serde_json::json!({
            "host": "localhost",
            "port": 5432,
            "username": "alice",
            "password": "s3cret",
            "sslmode": "require",
        });

        let result = split_config("postgres", input);
        assert!(result.is_err(), "Expected error for missing required field");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("database"),
            "Expected error to mention 'database', got: {err}"
        );
    }

    #[test]
    fn test_split_config_uses_defaults() {
        let input = serde_json::json!({
            "host": "localhost",
            "database": "mydb",
            "username": "alice",
            "password": "s3cret",
        });

        let (config, _secure) = split_config("postgres", input).unwrap();

        assert_eq!(config["port"], 5432, "Expected default port 5432");
        assert_eq!(
            config["sslmode"], "require",
            "Expected default sslmode 'require'"
        );
    }

    #[test]
    fn test_split_config_unknown_type() {
        let result = split_config("mongodb", serde_json::json!({}));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::UnknownType(_)));
    }

    #[test]
    fn test_merge_config_preserves_existing_secrets() {
        let existing_config = serde_json::json!({
            "host": "old-host",
            "port": 5432,
            "database": "mydb",
            "username": "alice",
            "sslmode": "require",
        });
        let existing_secure = serde_json::json!({
            "password": "original-password",
        });
        let update_input = serde_json::json!({
            "host": "new-host",
        });

        let (config, secure) =
            merge_config("postgres", existing_config, existing_secure, update_input).unwrap();

        assert_eq!(config["host"], "new-host", "Host should be updated");
        assert_eq!(
            secure["password"], "original-password",
            "Password should be preserved"
        );
    }

    #[test]
    fn test_merge_config_replaces_secrets_when_provided() {
        let existing_config = serde_json::json!({
            "host": "old-host",
            "port": 5432,
            "database": "mydb",
            "username": "alice",
            "sslmode": "require",
        });
        let existing_secure = serde_json::json!({
            "password": "original-password",
        });
        let update_input = serde_json::json!({
            "password": "new-password",
        });

        let (_config, secure) =
            merge_config("postgres", existing_config, existing_secure, update_input).unwrap();

        assert_eq!(secure["password"], "new-password", "Password should be updated");
    }

    #[test]
    fn test_merge_config_empty_string_preserves_secret() {
        let existing_config = serde_json::json!({
            "host": "localhost",
            "port": 5432,
            "database": "mydb",
            "username": "alice",
            "sslmode": "require",
        });
        let existing_secure = serde_json::json!({
            "password": "original-password",
        });
        let update_input = serde_json::json!({
            "password": "",
        });

        let (_config, secure) =
            merge_config("postgres", existing_config, existing_secure, update_input).unwrap();

        assert_eq!(
            secure["password"], "original-password",
            "Empty password should preserve existing"
        );
    }
}
