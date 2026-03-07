use arrow_pg::datatypes::df::encode_dataframe;
use async_trait::async_trait;
use chrono::Utc;
use datafusion::common::ScalarValue;
use datafusion::logical_expr::{LogicalPlan, LogicalPlanBuilder, col, lit};
use datafusion::prelude::SessionContext;
use datafusion::sql::sqlparser::ast::{
    BinaryOperator as SqlBinaryOp, Expr as SqlExpr, FunctionArg, FunctionArgExpr,
    FunctionArguments, Statement, TableFactor, Visit, Visitor,
};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;
use pgwire::api::ClientInfo;
use pgwire::api::portal::Format;
use pgwire::api::results::Response;
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::ops::{ControlFlow, Not};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::QueryHook;
use crate::entity::{
    data_source, discovered_schema, policy, policy_assignment, policy_obligation, query_audit_log,
};

// ---------- system schema detection ----------

const SYSTEM_SCHEMAS: &[&str] = &["pg_catalog", "information_schema", "pg_toast"];

struct SystemTableVisitor {
    has_user_table: bool,
}

impl Visitor for SystemTableVisitor {
    type Break = ();

    fn pre_visit_table_factor(&mut self, table_factor: &TableFactor) -> ControlFlow<Self::Break> {
        if let TableFactor::Table { name, .. } = table_factor {
            use datafusion::sql::sqlparser::ast::ObjectNamePart;
            let is_system = if name.0.len() >= 2 {
                if let ObjectNamePart::Identifier(schema_ident) = &name.0[0] {
                    let schema = schema_ident.value.to_lowercase();
                    SYSTEM_SCHEMAS.contains(&schema.as_str())
                } else {
                    false
                }
            } else {
                false
            };
            if !is_system {
                self.has_user_table = true;
            }
        }
        ControlFlow::Continue(())
    }
}

pub fn is_system_only_statement(statement: &Statement) -> bool {
    let mut visitor = SystemTableVisitor {
        has_user_table: false,
    };
    let _ = statement.visit(&mut visitor);
    !visitor.has_user_table
}

// ---------- obligation definitions ----------

#[derive(Deserialize, Clone)]
struct RowFilterDef {
    schema: String,
    table: String,
    filter_expression: String,
}

#[derive(Deserialize, Clone)]
struct ColumnMaskDef {
    schema: String,
    table: String,
    column: String,
    mask_expression: String,
}

#[derive(Deserialize, Clone)]
struct ColumnAccessDef {
    schema: String,
    table: String,
    columns: Vec<String>,
    action: String, // "deny"
}

// ---------- user variables ----------

#[derive(Clone)]
struct UserVars {
    tenant: String,
    username: String,
    user_id: String,
}

impl UserVars {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "user.tenant" => Some(&self.tenant),
            "user.username" => Some(&self.username),
            "user.id" => Some(&self.user_id),
            _ => None,
        }
    }
}

/// Replace `{user.X}` placeholders with safe identifier placeholders.
/// Returns the mangled expression and mappings (placeholder_lowercase → actual_value).
fn mangle_vars(template: &str, vars: &UserVars) -> (String, Vec<(String, String)>) {
    let mut result = template.to_string();
    let mut mappings = Vec::new();

    for key in ["user.tenant", "user.username", "user.id"] {
        let placeholder = format!("__br_{}__", key.replace('.', "_"));
        let needle = format!("{{{}}}", key);
        if result.contains(&needle) {
            let value = vars.get(key).unwrap_or("").to_string();
            result = result.replace(&needle, &placeholder);
            mappings.push((placeholder.to_lowercase(), value));
        }
    }

    (result, mappings)
}

/// Convert a sqlparser AST expression to a DataFusion Expr.
/// Handles: identifiers (column refs or placeholder vars), literals, binary ops,
/// IS NULL, IS NOT NULL, NOT, BETWEEN, LIKE, IN LIST.
fn sql_ast_to_df_expr(
    expr: &SqlExpr,
    var_values: &[(String, String)],
) -> datafusion::error::Result<datafusion::logical_expr::Expr> {
    use datafusion::logical_expr::Expr;
    match expr {
        SqlExpr::Identifier(ident) => {
            let name_lc = ident.value.to_lowercase();
            if let Some((_, val)) = var_values.iter().find(|(p, _)| p == &name_lc) {
                Ok(lit(val.as_str()))
            } else {
                Ok(col(&ident.value))
            }
        }
        SqlExpr::CompoundIdentifier(parts) => {
            // Parts are Vec<Ident> in newer sqlparser
            let name = parts
                .iter()
                .map(|i| i.value.as_str())
                .collect::<Vec<_>>()
                .join(".");
            Ok(col(name))
        }
        SqlExpr::Value(v) => {
            // In newer sqlparser, Value is wrapped in ValueWithSpan: access .value
            match &v.value {
                datafusion::sql::sqlparser::ast::Value::Number(n, _) => {
                    if let Ok(i) = n.parse::<i64>() {
                        Ok(lit(i))
                    } else {
                        Ok(lit(n.parse::<f64>().unwrap_or(0.0)))
                    }
                }
                datafusion::sql::sqlparser::ast::Value::SingleQuotedString(s)
                | datafusion::sql::sqlparser::ast::Value::DoubleQuotedString(s) => {
                    Ok(lit(s.as_str()))
                }
                datafusion::sql::sqlparser::ast::Value::Boolean(b) => Ok(lit(*b)),
                datafusion::sql::sqlparser::ast::Value::Null => Ok(lit(ScalarValue::Null)),
                other => Err(datafusion::error::DataFusionError::Plan(format!(
                    "Unsupported value in filter expression: {other:?}"
                ))),
            }
        }
        SqlExpr::BinaryOp { left, op, right } => {
            let l = sql_ast_to_df_expr(left, var_values)?;
            let r = sql_ast_to_df_expr(right, var_values)?;
            match op {
                SqlBinaryOp::Eq => Ok(l.eq(r)),
                SqlBinaryOp::NotEq => Ok(l.not_eq(r)),
                SqlBinaryOp::Lt => Ok(l.lt(r)),
                SqlBinaryOp::Gt => Ok(l.gt(r)),
                SqlBinaryOp::LtEq => Ok(l.lt_eq(r)),
                SqlBinaryOp::GtEq => Ok(l.gt_eq(r)),
                SqlBinaryOp::And => Ok(l.and(r)),
                SqlBinaryOp::Or => Ok(l.or(r)),
                SqlBinaryOp::StringConcat => {
                    Ok(Expr::BinaryExpr(datafusion::logical_expr::BinaryExpr {
                        left: Box::new(l),
                        op: datafusion::logical_expr::Operator::StringConcat,
                        right: Box::new(r),
                    }))
                }
                other => Err(datafusion::error::DataFusionError::Plan(format!(
                    "Unsupported operator in filter expression: {other:?}"
                ))),
            }
        }
        SqlExpr::IsNull(inner) => Ok(sql_ast_to_df_expr(inner, var_values)?.is_null()),
        SqlExpr::IsNotNull(inner) => Ok(sql_ast_to_df_expr(inner, var_values)?.is_not_null()),
        SqlExpr::Nested(inner) => sql_ast_to_df_expr(inner, var_values),
        SqlExpr::UnaryOp { op, expr } => {
            use datafusion::sql::sqlparser::ast::UnaryOperator;
            let inner = sql_ast_to_df_expr(expr, var_values)?;
            match op {
                UnaryOperator::Not => Ok(inner.not()),
                UnaryOperator::Minus => Ok(Expr::Negative(Box::new(inner))),
                other => Err(datafusion::error::DataFusionError::Plan(format!(
                    "Unsupported unary op: {other:?}"
                ))),
            }
        }
        SqlExpr::Between {
            expr,
            negated,
            low,
            high,
        } => {
            let e = sql_ast_to_df_expr(expr, var_values)?;
            let lo = sql_ast_to_df_expr(low, var_values)?;
            let hi = sql_ast_to_df_expr(high, var_values)?;
            let between = e.clone().gt_eq(lo).and(e.lt_eq(hi));
            Ok(if *negated { between.not() } else { between })
        }
        SqlExpr::Like {
            negated,
            expr,
            pattern,
            ..
        } => {
            let col_expr = sql_ast_to_df_expr(expr, var_values)?;
            let pat_expr = sql_ast_to_df_expr(pattern, var_values)?;
            let like_expr = col_expr.like(pat_expr);
            Ok(if *negated { like_expr.not() } else { like_expr })
        }
        SqlExpr::InList {
            expr,
            list,
            negated,
        } => {
            let col_expr = sql_ast_to_df_expr(expr, var_values)?;
            let list_exprs: Vec<_> = list
                .iter()
                .map(|e| sql_ast_to_df_expr(e, var_values))
                .collect::<datafusion::error::Result<_>>()?;
            Ok(col_expr.in_list(list_exprs, *negated))
        }
        SqlExpr::Function(f) => {
            // Only support COALESCE for filter expressions
            let func_name = f
                .name
                .0
                .iter()
                .filter_map(|p| {
                    use datafusion::sql::sqlparser::ast::ObjectNamePart;
                    if let ObjectNamePart::Identifier(i) = p {
                        Some(i.value.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(".")
                .to_uppercase();

            let args = match &f.args {
                FunctionArguments::List(list) => list
                    .args
                    .iter()
                    .map(|arg| match arg {
                        FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                            sql_ast_to_df_expr(e, var_values)
                        }
                        other => Err(datafusion::error::DataFusionError::Plan(format!(
                            "Unsupported function arg: {other:?}"
                        ))),
                    })
                    .collect::<datafusion::error::Result<Vec<_>>>()?,
                _ => vec![],
            };

            match func_name.as_str() {
                "COALESCE" => Ok(datafusion::functions::expr_fn::coalesce(args)),
                other => Err(datafusion::error::DataFusionError::Plan(format!(
                    "Function '{other}' in filter expressions is not supported. \
                     For complex expressions, use column masks instead."
                ))),
            }
        }
        other => Err(datafusion::error::DataFusionError::Plan(format!(
            "Unsupported expression type in filter: {other:?}"
        ))),
    }
}

/// Parse a filter expression template into a DataFusion Expr.
/// Template variables like {user.tenant} are substituted as literals.
fn parse_filter_expr(
    template: &str,
    vars: &UserVars,
) -> datafusion::error::Result<datafusion::logical_expr::Expr> {
    let trimmed = template.trim();
    if trimmed == "1=1" || trimmed == "true" {
        return Ok(lit(true));
    }
    if trimmed == "1=0" || trimmed == "false" {
        return Ok(lit(false));
    }

    let (mangled, var_values) = mangle_vars(template, vars);

    let dialect = GenericDialect {};
    let mut parser = Parser::new(&dialect).try_with_sql(&mangled).map_err(|e| {
        datafusion::error::DataFusionError::Plan(format!(
            "Failed to parse filter expression '{mangled}': {e}"
        ))
    })?;
    let sql_expr = parser.parse_expr().map_err(|e| {
        datafusion::error::DataFusionError::Plan(format!(
            "Failed to parse filter expression '{mangled}': {e}"
        ))
    })?;

    sql_ast_to_df_expr(&sql_expr, &var_values)
}

/// Parse a column mask expression using ctx.sql() for full SQL support.
async fn parse_mask_expr(
    ctx: &SessionContext,
    df_schema: &str,
    table: &str,
    column: &str,
    mask_template: &str,
    vars: &UserVars,
) -> datafusion::error::Result<datafusion::logical_expr::Expr> {
    // Substitute user variables (SQL-escape to prevent injection)
    let mut mask_sql = mask_template.to_string();
    for key in ["user.tenant", "user.username", "user.id"] {
        let needle = format!("{{{}}}", key);
        if mask_sql.contains(&needle) {
            let val = vars.get(key).unwrap_or("").replace('\'', "''");
            mask_sql = mask_sql.replace(&needle, &format!("'{}'", val));
        }
    }

    let sql = format!(
        "SELECT {} AS {} FROM {}.{}",
        mask_sql, column, df_schema, table
    );

    let plan = ctx.sql(&sql).await?.logical_plan().clone();

    if let LogicalPlan::Projection(proj) = plan
        && let Some(first_expr) = proj.expr.first()
    {
        return Ok(first_expr.clone());
    }

    Err(datafusion::error::DataFusionError::Plan(format!(
        "Could not parse mask expression for column '{column}': {mask_template}"
    )))
}

// ---------- resolved policy data structures ----------

#[derive(Clone)]
struct ResolvedObligation {
    obligation_type: String,
    definition: serde_json::Value,
}

#[derive(Clone)]
struct ResolvedPolicy {
    id: Uuid,
    name: String,
    #[allow(dead_code)]
    effect: String,
    version: i32,
    priority: i32,
    obligations: Vec<ResolvedObligation>,
}

struct SessionData {
    permit_policies: Vec<ResolvedPolicy>,
    deny_policies: Vec<ResolvedPolicy>,
    access_mode: String,
    /// DataFusion schema alias → upstream schema name
    df_to_upstream: HashMap<String, String>,
    datasource_id: Uuid,
    datasource_name: String,
    loaded_at: std::time::Instant,
}

const CACHE_TTL_SECS: u64 = 60;

// ---------- PolicyHook ----------

pub struct PolicyHook {
    db: DatabaseConnection,
    cache: Arc<RwLock<HashMap<(Uuid, String), SessionData>>>,
}

impl PolicyHook {
    pub fn new(db: DatabaseConnection) -> Arc<Self> {
        Arc::new(Self {
            db,
            cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn invalidate_datasource(&self, datasource_name: &str) {
        let mut cache = self.cache.write().await;
        cache.retain(|k, _| k.1 != datasource_name);
        tracing::debug!(datasource = %datasource_name, "PolicyHook: cache invalidated");
    }

    pub async fn invalidate_user(&self, user_id: Uuid) {
        let mut cache = self.cache.write().await;
        cache.retain(|k, _| k.0 != user_id);
    }

    async fn get_session(
        &self,
        user_id: Uuid,
        datasource_name: &str,
    ) -> Result<SessionDataRef, Box<dyn std::error::Error + Send + Sync>> {
        // Try read lock first
        {
            let cache = self.cache.read().await;
            if let Some(s) = cache.get(&(user_id, datasource_name.to_string()))
                && s.loaded_at.elapsed().as_secs() < CACHE_TTL_SECS
            {
                return Ok(clone_session_data(s));
            }
        }

        // Load and cache
        let mut cache = self.cache.write().await;
        let key = (user_id, datasource_name.to_string());

        // Re-check after acquiring write lock
        if let Some(s) = cache.get(&key)
            && s.loaded_at.elapsed().as_secs() < CACHE_TTL_SECS
        {
            return Ok(clone_session_data(s));
        }

        let session = self.load_session(user_id, datasource_name).await?;
        let cloned = clone_session_data(&session);
        cache.insert(key, session);
        Ok(cloned)
    }

    async fn load_session(
        &self,
        user_id: Uuid,
        datasource_name: &str,
    ) -> Result<SessionData, Box<dyn std::error::Error + Send + Sync>> {
        // Load datasource
        let ds = data_source::Entity::find()
            .filter(data_source::Column::Name.eq(datasource_name))
            .one(&self.db)
            .await?
            .ok_or_else(|| format!("Datasource '{datasource_name}' not found"))?;

        // Load schema alias mapping
        let schemas = discovered_schema::Entity::find()
            .filter(discovered_schema::Column::DataSourceId.eq(ds.id))
            .all(&self.db)
            .await?;

        let mut df_to_upstream: HashMap<String, String> = HashMap::new();
        for s in &schemas {
            let alias = s
                .schema_alias
                .as_deref()
                .unwrap_or(&s.schema_name)
                .to_string();
            df_to_upstream.insert(alias, s.schema_name.clone());
        }

        // Load policy assignments for this datasource+user (user-specific OR wildcard)
        let assignments = policy_assignment::Entity::find()
            .filter(policy_assignment::Column::DataSourceId.eq(ds.id))
            .all(&self.db)
            .await?;

        let relevant_assignments: Vec<_> = assignments
            .into_iter()
            .filter(|a| a.user_id.is_none() || a.user_id == Some(user_id))
            .collect();

        let policy_ids: Vec<Uuid> = relevant_assignments
            .iter()
            .map(|a| a.policy_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // Build priority map: policy_id → min priority (user-specific beats wildcard)
        let mut policy_priority: HashMap<Uuid, i32> = HashMap::new();
        for a in &relevant_assignments {
            let entry = policy_priority.entry(a.policy_id).or_insert(a.priority);
            if a.priority < *entry {
                *entry = a.priority;
            }
        }

        if policy_ids.is_empty() {
            return Ok(SessionData {
                permit_policies: vec![],
                deny_policies: vec![],
                access_mode: ds.access_mode.clone(),
                df_to_upstream,
                datasource_id: ds.id,
                datasource_name: ds.name.clone(),
                loaded_at: std::time::Instant::now(),
            });
        }

        // Load policies (enabled only)
        let policies = policy::Entity::find()
            .filter(policy::Column::Id.is_in(policy_ids.clone()))
            .filter(policy::Column::IsEnabled.eq(true))
            .all(&self.db)
            .await?;

        // Load obligations
        let obligations = policy_obligation::Entity::find()
            .filter(policy_obligation::Column::PolicyId.is_in(policy_ids))
            .all(&self.db)
            .await?;

        let mut obligations_by_policy: HashMap<Uuid, Vec<ResolvedObligation>> = HashMap::new();
        for obl in obligations {
            let def: serde_json::Value =
                serde_json::from_str(&obl.definition).unwrap_or(serde_json::Value::Null);
            obligations_by_policy
                .entry(obl.policy_id)
                .or_default()
                .push(ResolvedObligation {
                    obligation_type: obl.obligation_type,
                    definition: def,
                });
        }

        let mut permit_policies = Vec::new();
        let mut deny_policies = Vec::new();

        for p in policies {
            let priority = policy_priority.get(&p.id).copied().unwrap_or(100);
            let resolved = ResolvedPolicy {
                id: p.id,
                name: p.name.clone(),
                effect: p.effect.clone(),
                version: p.version,
                priority,
                obligations: obligations_by_policy.remove(&p.id).unwrap_or_default(),
            };
            if p.effect == "deny" {
                deny_policies.push(resolved);
            } else {
                permit_policies.push(resolved);
            }
        }

        permit_policies.sort_by_key(|p| p.priority);
        deny_policies.sort_by_key(|p| p.priority);

        Ok(SessionData {
            permit_policies,
            deny_policies,
            access_mode: ds.access_mode.clone(),
            df_to_upstream,
            datasource_id: ds.id,
            datasource_name: ds.name.clone(),
            loaded_at: std::time::Instant::now(),
        })
    }
}

// SessionData doesn't derive Clone, so we clone it manually
type SessionDataRef = Box<SessionDataClone>;

struct SessionDataClone {
    permit_policies: Vec<ResolvedPolicy>,
    deny_policies: Vec<ResolvedPolicy>,
    access_mode: String,
    df_to_upstream: HashMap<String, String>,
    datasource_id: Uuid,
    datasource_name: String,
}

fn clone_session_data(s: &SessionData) -> SessionDataRef {
    Box::new(SessionDataClone {
        permit_policies: s.permit_policies.clone(),
        deny_policies: s.deny_policies.clone(),
        access_mode: s.access_mode.clone(),
        df_to_upstream: s.df_to_upstream.clone(),
        datasource_id: s.datasource_id,
        datasource_name: s.datasource_name.clone(),
    })
}

/// Check if an obligation's schema/table matches a DataFusion table scan.
fn matches_schema_table(
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

/// Collect all user-table (df_schema, table) pairs from a logical plan.
fn collect_user_tables(plan: &LogicalPlan) -> Vec<(String, String)> {
    let mut tables = Vec::new();
    collect_tables_inner(plan, &mut tables);
    tables.dedup();
    tables
}

fn collect_tables_inner(plan: &LogicalPlan, tables: &mut Vec<(String, String)>) {
    if let LogicalPlan::TableScan(scan) = plan {
        let df_schema = scan.table_name.schema().unwrap_or("").to_string();
        let table = scan.table_name.table().to_string();
        let is_system = SYSTEM_SCHEMAS.contains(&df_schema.as_str())
            || table.starts_with("pg_")
            || df_schema == "information_schema";
        if !is_system {
            tables.push((df_schema, table));
        }
        return;
    }
    for input in plan.inputs() {
        collect_tables_inner(input, tables);
    }
}

#[async_trait]
impl QueryHook for PolicyHook {
    async fn handle_query(
        &self,
        statement: &Statement,
        session_context: &SessionContext,
        client: &(dyn ClientInfo + Sync),
    ) -> Option<PgWireResult<Response>> {
        if !matches!(statement, Statement::Query(_)) {
            return None;
        }
        if is_system_only_statement(statement) {
            return None;
        }

        let metadata = client.metadata();
        let user_id_str = metadata.get("user_id").cloned()?;
        let user_id = match Uuid::parse_str(&user_id_str) {
            Ok(id) => id,
            Err(_) => {
                return Some(Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                    "ERROR".to_owned(),
                    "28000".to_owned(),
                    "Invalid user_id in connection metadata".to_owned(),
                )))));
            }
        };
        let tenant = metadata.get("tenant").cloned().unwrap_or_default();
        let username = metadata.get("user").cloned().unwrap_or_default();
        let datasource = metadata.get("datasource").cloned().unwrap_or_default();
        let client_ip = Some(client.socket_addr().ip().to_string());
        let client_info = metadata.get("application_name").cloned();

        let user_vars = UserVars {
            tenant,
            username: username.clone(),
            user_id: user_id.to_string(),
        };

        // Load session data
        let session = match self.get_session(user_id, &datasource).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "PolicyHook: failed to load session");
                return Some(Err(PgWireError::ApiError(Box::new(std::io::Error::other(
                    e.to_string(),
                )))));
            }
        };

        let query_start = std::time::Instant::now();
        let original_query = statement.to_string();

        // Build logical plan
        let df_stmt = datafusion::sql::parser::Statement::Statement(Box::new(statement.clone()));
        let logical_plan = match session_context.state().statement_to_plan(df_stmt).await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "PolicyHook: failed to build plan");
                return Some(Err(PgWireError::ApiError(Box::new(e))));
            }
        };

        let user_tables = collect_user_tables(&logical_plan);

        // Check deny policies
        for policy in &session.deny_policies {
            for obl in &policy.obligations {
                if let Ok(def) = serde_json::from_value::<RowFilterDef>(obl.definition.clone()) {
                    for (df_schema, table) in &user_tables {
                        if matches_schema_table(
                            &def.schema,
                            &def.table,
                            df_schema,
                            table,
                            &session.df_to_upstream,
                        ) {
                            return Some(Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                                "ERROR".to_owned(),
                                "42501".to_owned(),
                                format!("Access denied by policy '{}'", policy.name),
                            )))));
                        }
                    }
                }
            }
        }

        // Build obligation maps
        let mut row_filters: HashMap<(String, String), datafusion::logical_expr::Expr> =
            HashMap::new();
        let mut column_masks: HashMap<String, datafusion::logical_expr::Expr> = HashMap::new();
        let mut column_denies: HashSet<String> = HashSet::new();
        let mut tables_with_permit: HashSet<(String, String)> = HashSet::new();

        for policy in &session.permit_policies {
            let mut policy_table_filters: HashMap<
                (String, String),
                datafusion::logical_expr::Expr,
            > = HashMap::new();

            for obl in &policy.obligations {
                match obl.obligation_type.as_str() {
                    "row_filter" => {
                        if let Ok(def) =
                            serde_json::from_value::<RowFilterDef>(obl.definition.clone())
                        {
                            for (df_schema, table) in &user_tables {
                                if matches_schema_table(
                                    &def.schema,
                                    &def.table,
                                    df_schema,
                                    table,
                                    &session.df_to_upstream,
                                ) {
                                    let key = (df_schema.clone(), table.clone());
                                    tables_with_permit.insert(key.clone());

                                    match parse_filter_expr(&def.filter_expression, &user_vars) {
                                        Ok(filter) => {
                                            // AND within same policy
                                            let entry = policy_table_filters
                                                .entry(key)
                                                .or_insert_with(|| lit(true));
                                            *entry = entry.clone().and(filter);
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                error = %e,
                                                policy = %policy.name,
                                                "Failed to parse row_filter"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "column_mask" => {
                        if let Ok(def) =
                            serde_json::from_value::<ColumnMaskDef>(obl.definition.clone())
                        {
                            for (df_schema, table) in &user_tables {
                                if matches_schema_table(
                                    &def.schema,
                                    &def.table,
                                    df_schema,
                                    table,
                                    &session.df_to_upstream,
                                ) {
                                    tables_with_permit.insert((df_schema.clone(), table.clone()));

                                    // First (highest priority) mask wins
                                    if !column_masks.contains_key(&def.column) {
                                        match parse_mask_expr(
                                            session_context,
                                            df_schema,
                                            table,
                                            &def.column,
                                            &def.mask_expression,
                                            &user_vars,
                                        )
                                        .await
                                        {
                                            Ok(mask) => {
                                                column_masks.insert(def.column.clone(), mask);
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    error = %e,
                                                    policy = %policy.name,
                                                    column = %def.column,
                                                    "Failed to parse column_mask"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "column_access" => {
                        if let Ok(def) =
                            serde_json::from_value::<ColumnAccessDef>(obl.definition.clone())
                        {
                            for (df_schema, table) in &user_tables {
                                if matches_schema_table(
                                    &def.schema,
                                    &def.table,
                                    df_schema,
                                    table,
                                    &session.df_to_upstream,
                                ) {
                                    tables_with_permit.insert((df_schema.clone(), table.clone()));
                                    if def.action == "deny" {
                                        for c in &def.columns {
                                            column_denies.insert(c.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            // OR this policy's filters with existing combined filters
            for (key, filter) in policy_table_filters {
                let entry = row_filters.entry(key).or_insert_with(|| lit(false));
                *entry = entry.clone().or(filter);
            }
        }

        // access_mode == "policy_required": tables with no permit → false filter
        if session.access_mode == "policy_required" {
            for table_key in &user_tables {
                if !tables_with_permit.contains(table_key) {
                    row_filters.insert(table_key.clone(), lit(false));
                }
            }
        }

        // Apply row filters via transform_up
        let modified_plan = {
            use datafusion::common::tree_node::{Transformed, TreeNode};

            let result = logical_plan.transform_up(|node| {
                let LogicalPlan::TableScan(ref scan) = node else {
                    return Ok(Transformed::no(node));
                };
                let df_schema = scan.table_name.schema().unwrap_or("").to_string();
                let table = scan.table_name.table().to_string();
                let key = (df_schema, table);

                if let Some(filter_expr) = row_filters.get(&key) {
                    tracing::debug!(table = %scan.table_name, "PolicyHook: applying row filter");
                    let plan_with_filter = LogicalPlanBuilder::from(node)
                        .filter(filter_expr.clone())
                        .and_then(|b| b.build())
                        .map_err(|e| datafusion::error::DataFusionError::Plan(e.to_string()))?;
                    Ok(Transformed::yes(plan_with_filter))
                } else {
                    Ok(Transformed::no(node))
                }
            });

            match result {
                Ok(t) => t.data,
                Err(e) => {
                    tracing::error!(error = %e, "PolicyHook: transform_up failed");
                    return Some(Err(PgWireError::ApiError(Box::new(e))));
                }
            }
        };

        // Apply column masks and access control as a top-level Projection
        let final_plan = if column_masks.is_empty() && column_denies.is_empty() {
            modified_plan
        } else {
            let output_schema = modified_plan.schema();
            // Use Arrow fields (no qualifier info, but sufficient for P0)
            let arrow_fields = output_schema.fields();
            let new_exprs: Vec<datafusion::logical_expr::Expr> = arrow_fields
                .iter()
                .filter_map(|field| {
                    let col_name = field.name();
                    if column_denies.contains(col_name.as_str()) {
                        return None;
                    }
                    if let Some(mask) = column_masks.get(col_name.as_str()) {
                        Some(mask.clone().alias(col_name))
                    } else {
                        Some(col(col_name))
                    }
                })
                .collect();

            match LogicalPlanBuilder::from(modified_plan)
                .project(new_exprs)
                .and_then(|b| b.build())
            {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(error = %e, "PolicyHook: projection failed");
                    return Some(Err(PgWireError::ApiError(Box::new(e))));
                }
            }
        };

        let rewritten_query =
            if !row_filters.is_empty() || !column_masks.is_empty() || !column_denies.is_empty() {
                Some(format!("/* policy-rewritten */ {original_query}"))
            } else {
                None
            };

        // Execute
        let df = match session_context.execute_logical_plan(final_plan).await {
            Ok(df) => df,
            Err(e) => {
                tracing::error!(error = %e, "PolicyHook: execution failed");
                return Some(Err(PgWireError::ApiError(Box::new(e))));
            }
        };

        let elapsed_ms = query_start.elapsed().as_millis() as i64;

        let response = match encode_dataframe(df, &Format::UnifiedText, None).await {
            Ok(qr) => Response::Query(qr),
            Err(e) => {
                tracing::error!(error = %e, "PolicyHook: encoding error");
                return Some(Err(e));
            }
        };

        // Async audit log
        let policies_applied: Vec<serde_json::Value> = session
            .permit_policies
            .iter()
            .map(|p| {
                serde_json::json!({
                    "policy_id": p.id.to_string(),
                    "version": p.version,
                    "name": p.name,
                })
            })
            .collect();

        let db = self.db.clone();
        let audit_user_id = user_id;
        let audit_username = username;
        let audit_ds_id = session.datasource_id;
        let audit_ds_name = session.datasource_name.clone();
        let audit_orig_q = original_query;
        let audit_rewritten = rewritten_query;
        let audit_policies = serde_json::to_string(&policies_applied).unwrap_or_default();
        let audit_ip = client_ip;
        let audit_info = client_info;

        tokio::spawn(async move {
            let now = Utc::now().naive_utc();
            let entry = query_audit_log::ActiveModel {
                id: sea_orm::Set(Uuid::now_v7()),
                user_id: sea_orm::Set(audit_user_id),
                username: sea_orm::Set(audit_username),
                data_source_id: sea_orm::Set(audit_ds_id),
                datasource_name: sea_orm::Set(audit_ds_name),
                original_query: sea_orm::Set(audit_orig_q),
                rewritten_query: sea_orm::Set(audit_rewritten),
                policies_applied: sea_orm::Set(audit_policies),
                execution_time_ms: sea_orm::Set(Some(elapsed_ms)),
                client_ip: sea_orm::Set(audit_ip),
                client_info: sea_orm::Set(audit_info),
                created_at: sea_orm::Set(now),
            };
            if let Err(e) = sea_orm::ActiveModelTrait::insert(entry, &db).await {
                tracing::error!(error = %e, "Failed to write audit log entry");
            }
        });

        Some(Ok(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::sql::sqlparser::{dialect::PostgreSqlDialect, parser::Parser as SqlParser};

    fn parse_statement(sql: &str) -> Statement {
        let mut statements =
            SqlParser::parse_sql(&PostgreSqlDialect {}, sql).expect("Failed to parse SQL");
        assert_eq!(statements.len(), 1);
        crate::engine::rewrite::rewrite_statement(&mut statements[0]);
        statements.remove(0)
    }

    #[test]
    fn test_system_only_pg_catalog() {
        let stmt = parse_statement("SELECT * FROM pg_catalog.pg_class");
        assert!(is_system_only_statement(&stmt));
    }

    #[test]
    fn test_system_only_information_schema() {
        let stmt = parse_statement("SELECT * FROM information_schema.tables");
        assert!(is_system_only_statement(&stmt));
    }

    #[test]
    fn test_user_table_not_system_only() {
        let stmt = parse_statement("SELECT * FROM users");
        assert!(!is_system_only_statement(&stmt));
    }

    #[test]
    fn test_select_no_from_is_system_only() {
        let stmt = parse_statement("SELECT 1");
        assert!(is_system_only_statement(&stmt));
    }

    #[test]
    fn test_parse_filter_simple_eq() {
        let vars = UserVars {
            tenant: "acme".to_string(),
            username: "alice".to_string(),
            user_id: "test-id".to_string(),
        };
        let expr = parse_filter_expr("organization_id = {user.tenant}", &vars).unwrap();
        let expr_str = format!("{expr:?}");
        assert!(
            expr_str.contains("acme"),
            "Expected tenant value in expr: {expr_str}"
        );
    }

    #[test]
    fn test_parse_filter_always_true() {
        let vars = UserVars {
            tenant: "any".to_string(),
            username: "u".to_string(),
            user_id: "i".to_string(),
        };
        let expr = parse_filter_expr("1=1", &vars).unwrap();
        let expr_str = format!("{expr:?}");
        assert!(
            expr_str.contains("true") || expr_str.contains("Boolean"),
            "{expr_str}"
        );
    }

    #[test]
    fn test_mangle_vars() {
        let vars = UserVars {
            tenant: "my-tenant".to_string(),
            username: "alice".to_string(),
            user_id: "uid-1".to_string(),
        };
        let (mangled, mappings) =
            mangle_vars("org = {user.tenant} AND user = {user.username}", &vars);
        assert!(!mangled.contains("{user.tenant}"));
        assert!(!mangled.contains("{user.username}"));
        assert_eq!(mappings.len(), 2);
    }

    #[test]
    fn test_parse_filter_and() {
        let vars = UserVars {
            tenant: "acme".to_string(),
            username: "alice".to_string(),
            user_id: "uid".to_string(),
        };
        let expr = parse_filter_expr(
            "organization_id = {user.tenant} AND is_active = true",
            &vars,
        )
        .unwrap();
        let expr_str = format!("{expr:?}");
        assert!(expr_str.contains("acme"));
        assert!(expr_str.contains("true") || expr_str.contains("is_active"));
    }

    // ---------- matches_schema_table ----------

    #[test]
    fn test_matches_schema_table_exact() {
        let map = std::collections::HashMap::new();
        assert!(matches_schema_table(
            "public", "orders", "public", "orders", &map
        ));
    }

    #[test]
    fn test_matches_schema_table_wrong_table() {
        let map = std::collections::HashMap::new();
        assert!(!matches_schema_table(
            "public", "orders", "public", "users", &map
        ));
    }

    #[test]
    fn test_matches_schema_table_schema_wildcard() {
        let map = std::collections::HashMap::new();
        assert!(matches_schema_table(
            "*",
            "orders",
            "any_schema",
            "orders",
            &map
        ));
    }

    #[test]
    fn test_matches_schema_table_table_wildcard() {
        let map = std::collections::HashMap::new();
        assert!(matches_schema_table(
            "public", "*", "public", "anything", &map
        ));
    }

    #[test]
    fn test_matches_schema_table_both_wildcards() {
        let map = std::collections::HashMap::new();
        assert!(matches_schema_table("*", "*", "any", "anything", &map));
    }

    #[test]
    fn test_matches_schema_table_alias_resolved() {
        // df_schema "sales" is an alias for upstream "public"
        let mut map = std::collections::HashMap::new();
        map.insert("sales".to_string(), "public".to_string());
        // obligation targets upstream schema "public"
        assert!(matches_schema_table(
            "public", "orders", "sales", "orders", &map
        ));
    }

    #[test]
    fn test_matches_schema_table_alias_no_match() {
        let mut map = std::collections::HashMap::new();
        map.insert("sales".to_string(), "public".to_string());
        // obligation targets "private", df_schema "sales" resolves to "public" — no match
        assert!(!matches_schema_table(
            "private", "orders", "sales", "orders", &map
        ));
    }

    // ---------- collect_user_tables ----------

    #[test]
    fn test_collect_user_tables_skips_pg_catalog() {
        use datafusion::arrow::datatypes::{DataType, Field, Schema};
        use datafusion::catalog::default_table_source::DefaultTableSource;
        use datafusion::datasource::empty::EmptyTable;
        use datafusion::logical_expr::LogicalPlanBuilder;
        use std::sync::Arc;

        let schema = Arc::new(Schema::new(vec![Field::new("oid", DataType::Int32, false)]));
        let table = Arc::new(EmptyTable::new(schema));
        let source = Arc::new(DefaultTableSource::new(table));

        let plan = LogicalPlanBuilder::scan("pg_catalog.pg_class", source, None)
            .unwrap()
            .build()
            .unwrap();

        let tables = collect_user_tables(&plan);
        assert!(
            tables.is_empty(),
            "pg_catalog tables should be excluded: {tables:?}"
        );
    }

    #[test]
    fn test_collect_user_tables_includes_user_table() {
        use datafusion::arrow::datatypes::{DataType, Field, Schema};
        use datafusion::catalog::default_table_source::DefaultTableSource;
        use datafusion::datasource::empty::EmptyTable;
        use datafusion::logical_expr::LogicalPlanBuilder;
        use std::sync::Arc;

        let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int32, false)]));
        let table = Arc::new(EmptyTable::new(schema));
        let source = Arc::new(DefaultTableSource::new(table));

        let plan = LogicalPlanBuilder::scan("public.orders", source, None)
            .unwrap()
            .build()
            .unwrap();

        let tables = collect_user_tables(&plan);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].1, "orders");
    }

    #[test]
    fn test_collect_user_tables_skips_information_schema() {
        use datafusion::arrow::datatypes::{DataType, Field, Schema};
        use datafusion::catalog::default_table_source::DefaultTableSource;
        use datafusion::datasource::empty::EmptyTable;
        use datafusion::logical_expr::LogicalPlanBuilder;
        use std::sync::Arc;

        let schema = Arc::new(Schema::new(vec![Field::new(
            "table_name",
            DataType::Utf8,
            false,
        )]));
        let table = Arc::new(EmptyTable::new(schema));
        let source = Arc::new(DefaultTableSource::new(table));

        let plan = LogicalPlanBuilder::scan("information_schema.tables", source, None)
            .unwrap()
            .build()
            .unwrap();

        let tables = collect_user_tables(&plan);
        assert!(
            tables.is_empty(),
            "information_schema should be excluded: {tables:?}"
        );
    }
}
