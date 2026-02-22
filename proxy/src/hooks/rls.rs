use async_trait::async_trait;
use datafusion::sql::sqlparser::ast::{ObjectNamePart, Statement, TableFactor, Visit, Visitor};
use datafusion::prelude::SessionContext;
use datafusion::logical_expr::LogicalPlan;
use pgwire::api::ClientInfo;
use pgwire::api::results::{Response, QueryResponse};
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};
use futures::stream::StreamExt;
use std::ops::ControlFlow;
use crate::arrow_conversion::{build_field_info, encode_batch_optimized};

use super::QueryHook;

/// System catalog schema names — tables in these schemas bypass RLS.
const SYSTEM_SCHEMAS: &[&str] = &["pg_catalog", "information_schema", "pg_toast"];

/// AST visitor that detects whether any user (non-system) tables are referenced.
struct SystemTableVisitor {
    has_user_table: bool,
}

impl Visitor for SystemTableVisitor {
    type Break = ();

    fn pre_visit_table_factor(
        &mut self,
        table_factor: &TableFactor,
    ) -> ControlFlow<Self::Break> {
        if let TableFactor::Table { name, .. } = table_factor {
            // A table is "system" only if it carries an explicit system-schema qualifier.
            // After sql_rewrite::rewrite_statement runs, bare `pg_class` becomes
            // `pg_catalog.pg_class`, so qualifying is reliable.
            let is_system = if name.0.len() >= 2 {
                if let ObjectNamePart::Identifier(schema_ident) = &name.0[0] {
                    let schema = schema_ident.value.to_lowercase();
                    SYSTEM_SCHEMAS.contains(&schema.as_str())
                } else {
                    false
                }
            } else {
                // Unqualified single-part name → user table.
                false
            };

            if !is_system {
                self.has_user_table = true;
            }
        }
        ControlFlow::Continue(())
    }
}


/// Returns `true` if the statement references only system catalog tables
/// (or has no table references at all). Uses AST inspection — immune to
/// bypass via string literals such as `WHERE name = 'pg_catalog'`.
fn is_system_only_statement(statement: &Statement) -> bool {
    let mut visitor = SystemTableVisitor { has_user_table: false };
    let _ = statement.visit(&mut visitor);
    !visitor.has_user_table
}

/// RLSHook implements Row-Level Security by injecting tenant filters.
/// The tenant is read from client metadata (set during authentication).
pub struct RLSHook;

impl RLSHook {
    pub fn new() -> Self {
        Self
    }

    /// Apply tenant filter to LogicalPlan.
    /// Uses DataFusion's TreeNode API to inject a Filter below every user TableScan.
    fn apply_tenant_filter(&self, plan: LogicalPlan, tenant: &str) -> datafusion::error::Result<LogicalPlan> {
        use datafusion::common::tree_node::{Transformed, TreeNode};
        use datafusion::logical_expr::{col, lit, LogicalPlanBuilder};

        let tenant = tenant.to_owned();
        let transformed = plan.transform_up(|node| {
            let LogicalPlan::TableScan(ref scan) = node else {
                return Ok(Transformed::no(node));
            };
            let table_name = scan.table_name.to_string();
            if table_name.contains("information_schema")
                || table_name.contains("pg_catalog")
                || table_name.starts_with("pg_")
            {
                tracing::debug!(table = %table_name, "Skipping RLS for system table");
                return Ok(Transformed::no(node));
            }

            tracing::debug!(table = %table_name, "Applying RLS tenant filter");

            let filter_expr = col("tenant").eq(lit(&tenant));
            let plan_with_filter = LogicalPlanBuilder::from(node)
                .filter(filter_expr)?
                .build()?;
            Ok(Transformed::yes(plan_with_filter))
        })?;
        Ok(transformed.data)
    }
}

#[async_trait]
impl QueryHook for RLSHook {
    async fn handle_query(
        &self,
        statement: &Statement,
        session_context: &SessionContext,
        client: &(dyn ClientInfo + Sync),
    ) -> Option<PgWireResult<Response>> {
        // Only handle SELECT statements
        if !matches!(statement, Statement::Query(_)) {
            return None;
        }

        // AST-level check: skip queries that reference only system catalogs
        // (or have no table references). Immune to string-injection bypass.
        if is_system_only_statement(statement) {
            return None;
        }

        // Read tenant from client metadata (set during authentication).
        // Return a proper PG error if the connection has no tenant context.
        let tenant = match client.metadata().get("tenant").cloned() {
            Some(t) => t,
            None => {
                return Some(Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                    "ERROR".to_owned(),
                    "28000".to_owned(),
                    "No tenant context available — connection may not be properly authenticated"
                        .to_owned(),
                )))));
            }
        };

        tracing::debug!(tenant = %tenant, "RLS hook processing query");

        // Convert AST directly to LogicalPlan
        let df_statement = datafusion::sql::parser::Statement::Statement(Box::new(statement.clone()));
        let logical_plan = match session_context.state().statement_to_plan(df_statement).await {
            Ok(plan) => plan,
            Err(e) => {
                tracing::error!(error = %e, "Failed to create logical plan");
                return Some(Err(PgWireError::ApiError(Box::new(e))));
            }
        };

        // Apply tenant filter transformation
        let filtered_plan = match self.apply_tenant_filter(logical_plan, &tenant) {
            Ok(plan) => plan,
            Err(e) => {
                tracing::error!(error = %e, "Failed to apply RLS filter");
                return Some(Err(PgWireError::ApiError(Box::new(e))));
            }
        };

        // Execute the transformed plan with streaming
        let query_start = std::time::Instant::now();

        let df = match session_context.execute_logical_plan(filtered_plan).await {
            Ok(df) => df,
            Err(e) => {
                tracing::error!(error = %e, "Failed to execute filtered plan");
                return Some(Err(PgWireError::ApiError(Box::new(e))));
            }
        };

        let stream_start = std::time::Instant::now();
        let mut stream = match df.execute_stream().await {
            Ok(stream) => stream,
            Err(e) => {
                tracing::error!(error = %e, "Failed to create stream");
                return Some(Err(PgWireError::ApiError(Box::new(e))));
            }
        };
        tracing::debug!(elapsed = ?stream_start.elapsed(), "RLS stream setup");

        let schema = stream.schema();
        let fields = build_field_info(&schema);
        let fields_for_stream = fields.clone();

        let mut total_rows: usize = 0;
        let mut batch_count: usize = 0;

        let encoded_stream = async_stream::stream! {
            while let Some(batch_result) = stream.next().await {
                match batch_result {
                    Ok(batch) => {
                        total_rows += batch.num_rows();
                        batch_count += 1;

                        match encode_batch_optimized(batch, fields_for_stream.clone()) {
                            Ok(rows) => {
                                for row in rows {
                                    yield Ok(row);
                                }
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "RLS batch encoding error");
                                yield Err(e);
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "RLS stream batch error");
                        yield Err(PgWireError::ApiError(Box::new(e)));
                        return;
                    }
                }
            }
            tracing::info!(batches = batch_count, rows = total_rows, elapsed = ?query_start.elapsed(), "RLS query completed");
        };

        Some(Ok(Response::Query(QueryResponse::new(fields, encoded_stream))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::logical_expr::{LogicalPlanBuilder, col};
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use datafusion::catalog::default_table_source::DefaultTableSource;
    use datafusion::datasource::empty::EmptyTable;
    use datafusion::sql::sqlparser::{dialect::PostgreSqlDialect, parser::Parser};
    use std::sync::Arc;

    fn parse_statement(sql: &str) -> Statement {
        let mut statements = Parser::parse_sql(&PostgreSqlDialect {}, sql)
            .expect("Failed to parse SQL");
        assert_eq!(statements.len(), 1);
        crate::sql_rewrite::rewrite_statement(&mut statements[0]);
        statements.remove(0)
    }

    #[test]
    fn test_is_system_only_statement_pg_catalog() {
        let stmt = parse_statement("SELECT * FROM pg_catalog.pg_class");
        assert!(
            is_system_only_statement(&stmt),
            "Expected pg_catalog query to be detected as system-only"
        );
    }

    #[test]
    fn test_is_system_only_statement_information_schema() {
        let stmt = parse_statement("SELECT * FROM information_schema.tables");
        assert!(
            is_system_only_statement(&stmt),
            "Expected information_schema query to be detected as system-only"
        );
    }

    #[test]
    fn test_is_system_only_statement_user_table() {
        let stmt = parse_statement("SELECT * FROM users");
        assert!(
            !is_system_only_statement(&stmt),
            "Expected user table query to NOT be detected as system-only"
        );
    }

    #[test]
    fn test_is_system_only_statement_pg_type_qualified() {
        let stmt = parse_statement("SELECT * FROM pg_type");
        assert!(
            is_system_only_statement(&stmt),
            "Expected pg_type (rewritten to pg_catalog.pg_type) to be detected as system-only"
        );
    }

    #[test]
    fn test_is_system_only_statement_no_from() {
        let stmt = parse_statement("SELECT 1");
        assert!(
            is_system_only_statement(&stmt),
            "Expected SELECT without FROM to be treated as system-only (no user tables)"
        );
    }

    /// RLS bypass attempt: string literal 'pg_catalog' in WHERE clause must NOT bypass RLS.
    #[test]
    fn test_is_system_only_statement_rls_bypass_attempt() {
        let stmt = parse_statement("SELECT * FROM users WHERE name = 'pg_catalog'");
        assert!(
            !is_system_only_statement(&stmt),
            "Expected RLS bypass attempt to NOT be detected as system-only"
        );
    }

    #[test]
    fn test_apply_tenant_filter_on_table_scan() {
        let hook = RLSHook::new();

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("tenant", DataType::Utf8, false),
        ]));

        let table = Arc::new(EmptyTable::new(schema));
        let table_source = Arc::new(DefaultTableSource::new(table));

        let plan = LogicalPlanBuilder::scan("users", table_source, None)
            .expect("Failed to build scan")
            .build()
            .expect("Failed to build plan");

        let filtered = hook.apply_tenant_filter(plan, "foo")
            .expect("Failed to apply tenant filter");

        let plan_str = format!("{:?}", filtered);
        assert!(plan_str.contains("Filter") && plan_str.contains("tenant"),
                "Expected filter to be injected, got: {}", plan_str);
    }

    #[test]
    fn test_apply_tenant_filter_skips_system_table() {
        let hook = RLSHook::new();

        let schema = Arc::new(Schema::new(vec![
            Field::new("relname", DataType::Utf8, false),
        ]));

        let table = Arc::new(EmptyTable::new(schema));
        let table_source = Arc::new(DefaultTableSource::new(table));

        let plan = LogicalPlanBuilder::scan("pg_catalog.pg_class", table_source, None)
            .expect("Failed to build scan")
            .build()
            .expect("Failed to build plan");

        let filtered = hook.apply_tenant_filter(plan.clone(), "foo")
            .expect("Failed to apply tenant filter");

        let original_str = format!("{:?}", plan);
        let filtered_str = format!("{:?}", filtered);
        assert_eq!(original_str, filtered_str,
                "Expected system table scan to remain unchanged");
    }

    #[test]
    fn test_apply_tenant_filter_recursive_projection() {
        let hook = RLSHook::new();

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("tenant", DataType::Utf8, false),
        ]));

        let table = Arc::new(EmptyTable::new(schema));
        let table_source = Arc::new(DefaultTableSource::new(table));

        let plan = LogicalPlanBuilder::scan("users", table_source, None)
            .expect("Failed to build scan")
            .project(vec![col("id"), col("name")])
            .expect("Failed to add projection")
            .build()
            .expect("Failed to build plan");

        let filtered = hook.apply_tenant_filter(plan, "foo")
            .expect("Failed to apply tenant filter");

        let plan_str = format!("{:?}", filtered);
        assert!(plan_str.contains("Projection"),
                "Expected projection to be preserved, got: {}", plan_str);
        assert!(plan_str.contains("Filter"),
                "Expected filter to be injected below projection, got: {}", plan_str);
    }

    #[test]
    fn test_apply_tenant_filter_aggregate() {
        let hook = RLSHook::new();

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("tenant", DataType::Utf8, false),
        ]));

        let table = Arc::new(EmptyTable::new(schema));
        let table_source = Arc::new(DefaultTableSource::new(table));

        let plan = LogicalPlanBuilder::scan("performer", table_source, None)
            .expect("Failed to build scan")
            .aggregate(vec![] as Vec<datafusion::logical_expr::Expr>, vec![datafusion::functions_aggregate::expr_fn::count(col("id"))])
            .expect("Failed to add aggregate")
            .build()
            .expect("Failed to build plan");

        let filtered = hook.apply_tenant_filter(plan, "foo")
            .expect("Failed to apply tenant filter");

        let plan_str = format!("{:?}", filtered);
        assert!(plan_str.contains("Aggregate"),
                "Expected aggregate to be preserved, got: {}", plan_str);
        assert!(plan_str.contains("Filter"),
                "Expected filter to be injected below aggregate, got: {}", plan_str);
    }

    #[test]
    fn test_apply_tenant_filter_recursive_sort() {
        let hook = RLSHook::new();

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("tenant", DataType::Utf8, false),
        ]));

        let table = Arc::new(EmptyTable::new(schema));
        let table_source = Arc::new(DefaultTableSource::new(table));

        let plan = LogicalPlanBuilder::scan("data", table_source, None)
            .expect("Failed to build scan")
            .sort(vec![col("id").sort(true, false)])
            .expect("Failed to add sort")
            .build()
            .expect("Failed to build plan");

        let filtered = hook.apply_tenant_filter(plan, "test")
            .expect("Failed to apply tenant filter");

        let plan_str = format!("{:?}", filtered);
        assert!(plan_str.contains("Sort"),
                "Expected sort to be preserved, got: {}", plan_str);
        assert!(plan_str.contains("Filter"),
                "Expected filter to be injected, got: {}", plan_str);
    }
}
