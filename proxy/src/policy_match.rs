use std::collections::HashMap;

/// Check whether an obligation's (schema, table) pattern matches a DataFusion table scan.
///
/// Supports `"*"` as a wildcard for either field.
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
    if obl_table != "*" && obl_table != table {
        return false;
    }
    if obl_schema == "*" {
        return true;
    }
    let upstream_schema = df_to_upstream
        .get(df_schema)
        .map(|s| s.as_str())
        .unwrap_or(df_schema);
    obl_schema == upstream_schema
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
}
