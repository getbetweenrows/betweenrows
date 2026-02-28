use async_trait::async_trait;
use datafusion::sql::sqlparser::ast::Statement;
use datafusion::prelude::SessionContext;
use pgwire::api::ClientInfo;
use pgwire::api::results::Response;
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};

use super::QueryHook;

/// Rejects any non-read SQL statement at the wire protocol level,
/// before DataFusion or RLS processing.
pub struct ReadOnlyHook;

impl ReadOnlyHook {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl QueryHook for ReadOnlyHook {
    async fn handle_query(
        &self,
        statement: &Statement,
        _session_context: &SessionContext,
        _client: &(dyn ClientInfo + Sync),
    ) -> Option<PgWireResult<Response>> {
        // SECURITY: This is an allowlist — any new Statement variant must be reviewed before adding here.
        let allowed = matches!(
            statement,
            Statement::Query(_)
                | Statement::ShowVariable { .. }
                | Statement::ExplainTable { .. }
                | Statement::Explain { .. }
                | Statement::ShowTables { .. }
                | Statement::ShowColumns { .. }
        );

        if allowed {
            None
        } else {
            let stmt_type = std::mem::discriminant(statement);
            tracing::warn!(statement_type = ?stmt_type, "Rejected non-read-only query");
            Some(Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                "ERROR".to_owned(),
                "25006".to_owned(),
                "only read-only queries are allowed".to_owned(),
            )))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::sql::sqlparser::{dialect::PostgreSqlDialect, parser::Parser};

    fn parse_statement(sql: &str) -> Statement {
        Parser::parse_sql(&PostgreSqlDialect {}, sql)
            .expect("failed to parse SQL")
            .into_iter()
            .next()
            .expect("no statements parsed")
    }

    fn is_allowed(sql: &str) -> bool {
        let stmt = parse_statement(sql);
        // SECURITY: This is an allowlist — any new Statement variant must be reviewed before adding here.
        matches!(
            stmt,
            Statement::Query(_)
                | Statement::ShowVariable { .. }
                | Statement::ExplainTable { .. }
                | Statement::Explain { .. }
                | Statement::ShowTables { .. }
                | Statement::ShowColumns { .. }
        )
    }

    #[test]
    fn select_is_allowed() {
        assert!(is_allowed("SELECT 1"));
    }

    #[test]
    fn insert_is_blocked() {
        assert!(!is_allowed("INSERT INTO t VALUES (1)"));
    }

    #[test]
    fn set_search_path_is_blocked() {
        assert!(!is_allowed("SET search_path = public"));
    }

    #[test]
    fn explain_select_is_allowed() {
        assert!(is_allowed("EXPLAIN SELECT 1"));
    }

    #[test]
    fn drop_table_is_blocked() {
        assert!(!is_allowed("DROP TABLE t"));
    }

    #[test]
    fn show_server_version_is_allowed() {
        assert!(is_allowed("SHOW server_version"));
    }
}
