use std::collections::{HashMap, HashSet};

// ---------- obligation type enum ----------

/// Strongly-typed obligation type identifier.
///
/// Serialized as snake_case (e.g. `"row_filter"`, `"column_mask"`) in JSON.
/// Unknown values are rejected at deserialization time, which catches
/// typos and invalid obligation types before they reach the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationType {
    RowFilter,
    ColumnMask,
    ColumnAccess,
    ObjectAccess,
}

impl ObligationType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RowFilter => "row_filter",
            Self::ColumnMask => "column_mask",
            Self::ColumnAccess => "column_access",
            Self::ObjectAccess => "object_access",
        }
    }
}

impl std::fmt::Display for ObligationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------- obligation definitions ----------

/// Parsed definition for a `row_filter` obligation.
#[derive(Debug, serde::Deserialize, Clone)]
pub struct RowFilterDef {
    pub schema: String,
    pub table: String,
    pub filter_expression: String,
}

/// Parsed definition for a `column_mask` obligation.
#[derive(Debug, serde::Deserialize, Clone)]
pub struct ColumnMaskDef {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub mask_expression: String,
}

/// Parsed definition for a `column_access` obligation.
#[derive(Debug, serde::Deserialize, Clone)]
pub struct ColumnAccessDef {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
    pub action: String,
}

/// Parsed definition for an `object_access` obligation.
#[derive(Debug, serde::Deserialize, Clone)]
pub struct ObjectAccessDef {
    pub schema: String,
    pub table: Option<String>,
    pub action: String,
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
/// Consistent with `matches_schema_table` / `matches_schema_only` for pattern semantics.
/// Called by `PolicyHook` at `TableScan` time and by the engine at catalog-build time.
pub fn expand_column_patterns(patterns: &[String], actual_columns: &[&str]) -> HashSet<String> {
    actual_columns
        .iter()
        .filter(|col| patterns.iter().any(|p| matches_pattern(p, col)))
        .map(|col| col.to_string())
        .collect()
}

/// Check whether an obligation's (schema, table) pattern matches a DataFusion table scan.
///
/// Supports `"*"` and prefix globs (e.g., `"raw_*"`) for either field.
/// `df_schema` is the DataFusion schema alias; `df_to_upstream` maps it to the upstream name.
///
/// This is the single source of truth shared by `PolicyHook` and the engine's
/// `compute_user_visibility`. Any changes to matching semantics must be made here.
pub fn matches_schema_table(
    obl_schema: &str,
    obl_table: &str,
    df_schema: &str,
    table: &str,
    df_to_upstream: &HashMap<String, String>,
) -> bool {
    if !matches_pattern(obl_table, table) {
        return false;
    }
    let upstream_schema = df_to_upstream
        .get(df_schema)
        .map(|s| s.as_str())
        .unwrap_or(df_schema);
    matches_pattern(obl_schema, upstream_schema)
}

/// Check whether an obligation's schema pattern matches a DataFusion schema alias.
///
/// Used for schema-level `object_access` obligations that don't specify a table.
pub fn matches_schema_only(
    obl_schema: &str,
    df_schema: &str,
    df_to_upstream: &HashMap<String, String>,
) -> bool {
    let upstream = df_to_upstream
        .get(df_schema)
        .map(|s| s.as_str())
        .unwrap_or(df_schema);
    matches_pattern(obl_schema, upstream)
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
        assert!(!matches_pattern("user_*", "id_user")); // suffix doesn't match prefix glob
        assert!(!matches_pattern("user_*", "email"));
    }

    #[test]
    fn test_matches_pattern_suffix_glob() {
        assert!(matches_pattern("*_date", "created_date"));
        assert!(matches_pattern("*_date", "updated_date"));
        assert!(!matches_pattern("*_date", "date_created"));
        assert!(!matches_pattern("*_date", "date")); // exact "_date" not matching "*_date" against "date"
    }

    #[test]
    fn test_matches_pattern_exact() {
        assert!(matches_pattern("email", "email"));
        assert!(!matches_pattern("email", "emails"));
        assert!(!matches_pattern("email", "Email"));
    }

    #[test]
    fn test_matches_pattern_case_sensitive() {
        // Postgres folds identifiers to lowercase — patterns are case-sensitive by design.
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
        // Both must prefix-match
        assert!(matches_schema_table(
            "raw_*",
            "events_*",
            "raw_prod",
            "events_2024",
            &map
        ));
        // Schema matches but table doesn't
        assert!(!matches_schema_table(
            "raw_*", "events_*", "raw_prod", "orders", &map
        ));
        // Table matches but schema doesn't
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
        // strip_suffix('*') on "*" gives "" as prefix → starts_with("") is always true
        // But matches_pattern checks == "*" first, so this path hits the first branch anyway.
        // Test the strip_suffix("*") case with a non-"*" pattern that strips to empty prefix:
        // Actually "**" strips to "*" prefix. Let's just verify "*" matches everything.
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
}
