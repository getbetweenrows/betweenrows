use datafusion::sql::sqlparser::ast::*;
use std::ops::ControlFlow;

/// Rewrite a parsed statement to fix PostgreSQL compatibility issues.
/// Applies the same patterns as datafusion-postgres:
/// 1. Prepend `pg_catalog.` to unqualified `pg_*` table references
/// 2. Strip schema qualifier from function calls (e.g. `pg_catalog.func()` → `func()`)
pub fn rewrite_statement(statement: &mut Statement) {
    // Apply both rewrites via AST visitor
    let _ = statement.visit(&mut PgCompatVisitor);
}

struct PgCompatVisitor;

impl VisitorMut for PgCompatVisitor {
    type Break = ();

    fn pre_visit_table_factor(
        &mut self,
        table_factor: &mut TableFactor,
    ) -> ControlFlow<Self::Break> {
        // Prepend pg_catalog to unqualified pg_* table names
        // e.g. `FROM pg_class` → `FROM pg_catalog.pg_class`
        if let TableFactor::Table { name, args, .. } = table_factor {
            if args.is_none() && name.0.len() == 1 {
                if let ObjectNamePart::Identifier(ident) = &name.0[0] {
                    if ident.value.starts_with("pg_") {
                        *name = ObjectName(vec![
                            ObjectNamePart::Identifier(Ident::new("pg_catalog")),
                            name.0[0].clone(),
                        ]);
                    }
                }
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<Self::Break> {
        // Strip schema qualifier from function calls
        // e.g. `pg_catalog.pg_get_userbyid(x)` → `pg_get_userbyid(x)`
        if let Expr::Function(function) = expr {
            let name = &mut function.name;
            if name.0.len() > 1 {
                if let Some(last_ident) = name.0.last().cloned() {
                    *name = ObjectName(vec![last_ident]);
                }
            }
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::sql::sqlparser::{dialect::PostgreSqlDialect, parser::Parser};

    fn parse_and_rewrite(sql: &str) -> String {
        let mut statements = Parser::parse_sql(&PostgreSqlDialect {}, sql)
            .expect("Failed to parse SQL");
        assert_eq!(statements.len(), 1, "Expected single statement");
        rewrite_statement(&mut statements[0]);
        statements[0].to_string()
    }

    #[test]
    fn test_pg_table_gets_qualified() {
        let sql = "SELECT * FROM pg_class";
        let rewritten = parse_and_rewrite(sql);
        assert!(rewritten.contains("pg_catalog.pg_class"),
                "Expected pg_class to be qualified with pg_catalog, got: {}", rewritten);
    }

    #[test]
    fn test_non_pg_table_unchanged() {
        let sql = "SELECT * FROM users";
        let rewritten = parse_and_rewrite(sql);
        assert!(rewritten.contains("FROM users") && !rewritten.contains("pg_catalog"),
                "Expected users table to remain unqualified, got: {}", rewritten);
    }

    #[test]
    fn test_already_qualified_unchanged() {
        let sql = "SELECT * FROM pg_catalog.pg_class";
        let rewritten = parse_and_rewrite(sql);
        // Should still contain pg_catalog.pg_class (not double-qualified)
        assert!(rewritten.contains("pg_catalog.pg_class"),
                "Expected pg_catalog.pg_class to remain as-is, got: {}", rewritten);
    }

    #[test]
    fn test_function_schema_stripped() {
        let sql = "SELECT pg_catalog.pg_get_userbyid(1)";
        let rewritten = parse_and_rewrite(sql);
        assert!(rewritten.contains("pg_get_userbyid") && !rewritten.contains("pg_catalog.pg_get_userbyid"),
                "Expected function schema to be stripped, got: {}", rewritten);
    }

    #[test]
    fn test_unqualified_function_unchanged() {
        let sql = "SELECT count(*) FROM users";
        let rewritten = parse_and_rewrite(sql);
        assert!(rewritten.contains("count(*)"),
                "Expected count(*) to remain unchanged, got: {}", rewritten);
    }

    #[test]
    fn test_multiple_pg_tables() {
        let sql = "SELECT * FROM pg_class c JOIN pg_namespace n ON c.relnamespace = n.oid";
        let rewritten = parse_and_rewrite(sql);
        assert!(rewritten.contains("pg_catalog.pg_class"),
                "Expected pg_class to be qualified, got: {}", rewritten);
        assert!(rewritten.contains("pg_catalog.pg_namespace"),
                "Expected pg_namespace to be qualified, got: {}", rewritten);
    }
}
