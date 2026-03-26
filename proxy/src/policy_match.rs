use std::collections::{HashMap, HashSet};

// ---------- policy type enum ----------

/// Strongly-typed policy type identifier. Encodes both the semantic intent
/// (permit/deny) and the kind of enforcement (row, column, table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyType {
    RowFilter,
    ColumnMask,
    ColumnAllow,
    ColumnDeny,
    TableDeny,
}

impl PolicyType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RowFilter => "row_filter",
            Self::ColumnMask => "column_mask",
            Self::ColumnAllow => "column_allow",
            Self::ColumnDeny => "column_deny",
            Self::TableDeny => "table_deny",
        }
    }

    /// Returns true for deny-type policies (ColumnDeny, TableDeny).
    pub fn is_deny(self) -> bool {
        matches!(self, Self::ColumnDeny | Self::TableDeny)
    }

    /// Returns true for policy types that affect catalog-level visibility
    /// (schema filtering at connect time). Decision functions on these types
    /// are evaluated at visibility time when `evaluate_context = "session"`.
    pub fn affects_visibility(self) -> bool {
        matches!(self, Self::ColumnAllow | Self::ColumnDeny | Self::TableDeny)
    }
}

impl std::fmt::Display for PolicyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for PolicyType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "row_filter" => Ok(Self::RowFilter),
            "column_mask" => Ok(Self::ColumnMask),
            "column_allow" => Ok(Self::ColumnAllow),
            "column_deny" => Ok(Self::ColumnDeny),
            "table_deny" => Ok(Self::TableDeny),
            other => Err(format!("Unknown policy_type: '{other}'")),
        }
    }
}

// ---------- resource targeting ----------

/// A resource targeting entry: which schemas, tables, and optionally columns a policy applies to.
///
/// Supports `"*"` and prefix/suffix globs (`"prefix*"`, `"*suffix"`) in any field.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TargetEntry {
    pub schemas: Vec<String>,
    pub tables: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
}

impl TargetEntry {
    /// Returns true if this entry matches the given (df_schema, table) pair.
    ///
    /// Resolves df_schema aliases via `df_to_upstream` before matching.
    pub fn matches_table(
        &self,
        df_schema: &str,
        table: &str,
        df_to_upstream: &HashMap<String, String>,
    ) -> bool {
        let upstream = df_to_upstream
            .get(df_schema)
            .map(|s| s.as_str())
            .unwrap_or(df_schema);
        self.schemas.iter().any(|sp| matches_pattern(sp, upstream))
            && self.tables.iter().any(|tp| matches_pattern(tp, table))
    }
}

// ---------- policy definitions ----------

/// Parsed definition for a `row_filter` policy.
#[derive(Debug, serde::Deserialize, Clone)]
pub struct RowFilterDef {
    pub filter_expression: String,
}

/// Parsed definition for a `column_mask` policy.
#[derive(Debug, serde::Deserialize, Clone)]
pub struct ColumnMaskDef {
    pub mask_expression: String,
}

// ---------- pattern matching ----------

/// Check whether a pattern matches a value.
///
/// Supports exact match, `"*"` (match-all), prefix glob `"prefix*"`, and suffix glob `"*suffix"`.
pub fn matches_pattern(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        true
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix) // prefix* glob
    } else if let Some(suffix) = pattern.strip_prefix('*') {
        value.ends_with(suffix) // *suffix glob
    } else {
        pattern == value
    }
}

/// Expand column patterns (including globs) against actual column names.
///
/// Returns the set of concrete column names matching any pattern.
pub fn expand_column_patterns(patterns: &[String], actual_columns: &[&str]) -> HashSet<String> {
    actual_columns
        .iter()
        .filter(|col| patterns.iter().any(|p| matches_pattern(p, col)))
        .map(|col| col.to_string())
        .collect()
}

/// Check whether a (schema, table) pattern matches a DataFusion table scan.
///
/// Supports `"*"` and prefix globs (e.g., `"raw_*"`) for either field.
/// `df_schema` is the DataFusion schema alias; `df_to_upstream` maps it to the upstream name.
pub fn matches_schema_table(
    res_schema: &str,
    res_table: &str,
    df_schema: &str,
    table: &str,
    df_to_upstream: &HashMap<String, String>,
) -> bool {
    if !matches_pattern(res_table, table) {
        return false;
    }
    let upstream_schema = df_to_upstream
        .get(df_schema)
        .map(|s| s.as_str())
        .unwrap_or(df_schema);
    matches_pattern(res_schema, upstream_schema)
}

/// Check whether a schema pattern matches a DataFusion schema alias.
pub fn matches_schema_only(
    res_schema: &str,
    df_schema: &str,
    df_to_upstream: &HashMap<String, String>,
) -> bool {
    let upstream = df_to_upstream
        .get(df_schema)
        .map(|s| s.as_str())
        .unwrap_or(df_schema);
    matches_pattern(res_schema, upstream)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- matches_pattern unit tests ---

    #[test]
    fn test_matches_pattern_wildcard_any() {
        assert!(matches_pattern("*", "hello"));
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "anything_at_all"));
    }

    #[test]
    fn test_matches_pattern_prefix_glob() {
        assert!(matches_pattern("user_*", "user_id"));
        assert!(matches_pattern("user_*", "user_name"));
        assert!(!matches_pattern("user_*", "id_user"));
        assert!(!matches_pattern("user_*", "email"));
    }

    #[test]
    fn test_matches_pattern_suffix_glob() {
        assert!(matches_pattern("*_date", "created_date"));
        assert!(matches_pattern("*_date", "updated_date"));
        assert!(!matches_pattern("*_date", "date_created"));
        assert!(!matches_pattern("*_date", "date"));
    }

    #[test]
    fn test_matches_pattern_exact() {
        assert!(matches_pattern("email", "email"));
        assert!(!matches_pattern("email", "emails"));
        assert!(!matches_pattern("email", "Email"));
    }

    #[test]
    fn test_matches_pattern_case_sensitive() {
        assert!(!matches_pattern("Email", "email"));
        assert!(matches_pattern("Email", "Email"));
    }

    #[test]
    fn test_matches_pattern_empty() {
        assert!(matches_pattern("", ""));
        assert!(matches_pattern("*", ""));
        assert!(!matches_pattern("", "nonempty"));
    }

    #[test]
    fn test_exact_match() {
        let map = HashMap::new();
        assert!(matches_schema_table(
            "public", "orders", "public", "orders", &map
        ));
    }

    #[test]
    fn test_wrong_table() {
        let map = HashMap::new();
        assert!(!matches_schema_table(
            "public", "orders", "public", "users", &map
        ));
    }

    #[test]
    fn test_schema_wildcard() {
        let map = HashMap::new();
        assert!(matches_schema_table(
            "*",
            "orders",
            "any_schema",
            "orders",
            &map
        ));
    }

    #[test]
    fn test_table_wildcard() {
        let map = HashMap::new();
        assert!(matches_schema_table(
            "public", "*", "public", "anything", &map
        ));
    }

    #[test]
    fn test_both_wildcards() {
        let map = HashMap::new();
        assert!(matches_schema_table("*", "*", "any", "anything", &map));
    }

    #[test]
    fn test_alias_resolved() {
        let mut map = HashMap::new();
        map.insert("sales".to_string(), "public".to_string());
        assert!(matches_schema_table(
            "public", "orders", "sales", "orders", &map
        ));
    }

    #[test]
    fn test_alias_no_match() {
        let mut map = HashMap::new();
        map.insert("sales".to_string(), "public".to_string());
        assert!(!matches_schema_table(
            "private", "orders", "sales", "orders", &map
        ));
    }

    #[test]
    fn test_wrong_schema_no_alias() {
        let map = HashMap::new();
        assert!(!matches_schema_table(
            "public", "orders", "private", "orders", &map
        ));
    }

    #[test]
    fn test_empty_schema_matches_itself() {
        let map = HashMap::new();
        assert!(matches_schema_table("", "orders", "", "orders", &map));
    }

    // --- glob prefix tests ---

    #[test]
    fn test_table_glob_prefix_match() {
        let map = HashMap::new();
        assert!(matches_schema_table(
            "public",
            "raw_*",
            "public",
            "raw_orders",
            &map
        ));
        assert!(matches_schema_table(
            "public",
            "raw_*",
            "public",
            "raw_customers",
            &map
        ));
    }

    #[test]
    fn test_table_glob_prefix_no_suffix() {
        let map = HashMap::new();
        assert!(!matches_schema_table(
            "public",
            "raw_*",
            "public",
            "orders_raw",
            &map
        ));
        assert!(!matches_schema_table(
            "public", "raw_*", "public", "orders", &map
        ));
    }

    #[test]
    fn test_schema_glob_prefix_match() {
        let map = HashMap::new();
        assert!(matches_schema_table(
            "analytics_*",
            "*",
            "analytics_dev",
            "reports",
            &map
        ));
        assert!(matches_schema_table(
            "analytics_*",
            "*",
            "analytics_prod",
            "reports",
            &map
        ));
    }

    #[test]
    fn test_schema_glob_no_match() {
        let map = HashMap::new();
        assert!(!matches_schema_table(
            "analytics_*",
            "*",
            "public",
            "orders",
            &map
        ));
        assert!(!matches_schema_table(
            "analytics_*",
            "*",
            "raw_analytics",
            "orders",
            &map
        ));
    }

    #[test]
    fn test_glob_both_schema_and_table() {
        let map = HashMap::new();
        assert!(matches_schema_table(
            "raw_*",
            "events_*",
            "raw_prod",
            "events_2024",
            &map
        ));
        assert!(!matches_schema_table(
            "raw_*", "events_*", "raw_prod", "orders", &map
        ));
        assert!(!matches_schema_table(
            "raw_*",
            "events_*",
            "public",
            "events_2024",
            &map
        ));
    }

    #[test]
    fn test_glob_backward_compat_exact() {
        let map = HashMap::new();
        assert!(matches_schema_table(
            "public", "orders", "public", "orders", &map
        ));
        assert!(!matches_schema_table(
            "public", "orders", "public", "users", &map
        ));
    }

    #[test]
    fn test_glob_backward_compat_wildcard() {
        let map = HashMap::new();
        assert!(matches_schema_table(
            "*",
            "*",
            "any_schema",
            "any_table",
            &map
        ));
        assert!(matches_schema_table(
            "*",
            "orders",
            "any_schema",
            "orders",
            &map
        ));
    }

    #[test]
    fn test_glob_empty_prefix() {
        let map = HashMap::new();
        assert!(matches_schema_table("*", "*", "", "", &map));
        assert!(matches_schema_table("*", "*", "x", "y", &map));
    }

    // --- matches_schema_only tests ---

    #[test]
    fn test_matches_schema_only_exact() {
        let map = HashMap::new();
        assert!(matches_schema_only("public", "public", &map));
        assert!(!matches_schema_only("public", "private", &map));
    }

    #[test]
    fn test_matches_schema_only_wildcard() {
        let map = HashMap::new();
        assert!(matches_schema_only("*", "any_schema", &map));
    }

    #[test]
    fn test_matches_schema_only_glob() {
        let map = HashMap::new();
        assert!(matches_schema_only("analytics_*", "analytics_dev", &map));
        assert!(matches_schema_only("analytics_*", "analytics_prod", &map));
        assert!(!matches_schema_only("analytics_*", "public", &map));
    }

    #[test]
    fn test_matches_schema_only_alias() {
        let mut map = HashMap::new();
        map.insert("ds_alias".to_string(), "public".to_string());
        assert!(matches_schema_only("public", "ds_alias", &map));
        assert!(!matches_schema_only("private", "ds_alias", &map));
    }

    // --- PolicyType tests ---

    #[test]
    fn test_policy_type_is_deny() {
        assert!(!PolicyType::RowFilter.is_deny());
        assert!(!PolicyType::ColumnMask.is_deny());
        assert!(!PolicyType::ColumnAllow.is_deny());
        assert!(PolicyType::ColumnDeny.is_deny());
        assert!(PolicyType::TableDeny.is_deny());
    }

    #[test]
    fn test_policy_type_affects_visibility() {
        assert!(!PolicyType::RowFilter.affects_visibility());
        assert!(!PolicyType::ColumnMask.affects_visibility());
        assert!(PolicyType::ColumnAllow.affects_visibility());
        assert!(PolicyType::ColumnDeny.affects_visibility());
        assert!(PolicyType::TableDeny.affects_visibility());
    }

    #[test]
    fn test_policy_type_as_str() {
        assert_eq!(PolicyType::RowFilter.as_str(), "row_filter");
        assert_eq!(PolicyType::ColumnMask.as_str(), "column_mask");
        assert_eq!(PolicyType::ColumnAllow.as_str(), "column_allow");
        assert_eq!(PolicyType::ColumnDeny.as_str(), "column_deny");
        assert_eq!(PolicyType::TableDeny.as_str(), "table_deny");
    }

    #[test]
    fn test_target_entry_matches_table() {
        let map = HashMap::new();
        let entry = TargetEntry {
            schemas: vec!["public".to_string()],
            tables: vec!["customers".to_string(), "employees".to_string()],
            columns: None,
        };
        assert!(entry.matches_table("public", "customers", &map));
        assert!(entry.matches_table("public", "employees", &map));
        assert!(!entry.matches_table("public", "orders", &map));
        assert!(!entry.matches_table("private", "customers", &map));
    }

    #[test]
    fn test_target_entry_matches_table_wildcard() {
        let map = HashMap::new();
        let entry = TargetEntry {
            schemas: vec!["*".to_string()],
            tables: vec!["*".to_string()],
            columns: None,
        };
        assert!(entry.matches_table("any_schema", "any_table", &map));
    }

    #[test]
    fn test_target_entry_matches_table_alias() {
        let mut map = HashMap::new();
        map.insert("sales".to_string(), "public".to_string());
        let entry = TargetEntry {
            schemas: vec!["public".to_string()],
            tables: vec!["orders".to_string()],
            columns: None,
        };
        assert!(entry.matches_table("sales", "orders", &map));
        assert!(!entry.matches_table("private", "orders", &map));
    }
}
