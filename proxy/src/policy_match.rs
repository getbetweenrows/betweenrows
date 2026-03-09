use std::collections::HashMap;

/// Check whether a pattern matches a value.
///
/// Supports exact match, `"*"` (match-all), and prefix glob `"prefix*"`.
fn matches_pattern(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        true
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
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
