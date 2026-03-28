use sea_orm::entity::prelude::*;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "attribute_definition")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub key: String,
    pub entity_type: String,
    pub display_name: String,
    pub value_type: String,
    pub default_value: Option<String>,
    pub allowed_values: Option<String>,
    pub description: Option<String>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Parse a JSON array string into a Vec of allowed values.
pub fn parse_allowed_values(json_str: &str) -> Vec<String> {
    serde_json::from_str(json_str).unwrap_or_default()
}

/// Validate a value against a value_type. Returns Ok(()) or Err with a message.
pub fn validate_value(value: &str, value_type: &str) -> Result<(), String> {
    match value_type {
        "string" => {
            if value.len() > 1024 {
                return Err("value exceeds 1024 character limit".to_string());
            }
            Ok(())
        }
        "integer" => value
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| format!("'{value}' is not a valid integer")),
        "boolean" => match value {
            "true" | "false" => Ok(()),
            _ => Err(format!(
                "'{value}' is not a valid boolean (must be 'true' or 'false')"
            )),
        },
        other => Err(format!("unknown value_type '{other}'")),
    }
}

/// Build a lookup map from attribute definitions keyed by attribute key.
pub fn definitions_by_key(defs: &[Model]) -> HashMap<&str, &Model> {
    defs.iter().map(|d| (d.key.as_str(), d)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_string_ok() {
        assert!(validate_value("hello", "string").is_ok());
        assert!(validate_value("", "string").is_ok());
    }

    #[test]
    fn validate_string_too_long() {
        let long = "x".repeat(1025);
        assert!(validate_value(&long, "string").is_err());
    }

    #[test]
    fn validate_integer_ok() {
        assert!(validate_value("42", "integer").is_ok());
        assert!(validate_value("-1", "integer").is_ok());
        assert!(validate_value("0", "integer").is_ok());
    }

    #[test]
    fn validate_integer_bad() {
        assert!(validate_value("abc", "integer").is_err());
        assert!(validate_value("3.14", "integer").is_err());
        assert!(validate_value("", "integer").is_err());
    }

    #[test]
    fn validate_boolean_ok() {
        assert!(validate_value("true", "boolean").is_ok());
        assert!(validate_value("false", "boolean").is_ok());
    }

    #[test]
    fn validate_boolean_bad() {
        assert!(validate_value("yes", "boolean").is_err());
        assert!(validate_value("1", "boolean").is_err());
        assert!(validate_value("True", "boolean").is_err());
    }

    #[test]
    fn validate_unknown_type() {
        assert!(validate_value("x", "float").is_err());
    }

    #[test]
    fn parse_allowed_values_ok() {
        let vals = parse_allowed_values(r#"["a","b","c"]"#);
        assert_eq!(vals, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_allowed_values_invalid_json() {
        let vals = parse_allowed_values("not json");
        assert!(vals.is_empty());
    }

    #[test]
    fn parse_allowed_values_empty() {
        let vals = parse_allowed_values("[]");
        assert!(vals.is_empty());
    }
}
