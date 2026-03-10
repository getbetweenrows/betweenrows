use arrow_pg::datatypes::df::encode_dataframe;
use async_trait::async_trait;
use chrono::Utc;
use datafusion::common::ScalarValue;
use datafusion::logical_expr::registry::FunctionRegistry;
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
use std::collections::{HashMap, HashSet};
use std::ops::{ControlFlow, Not};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::QueryHook;
use crate::entity::{
    data_source, discovered_schema, policy, policy_assignment, policy_obligation, query_audit_log,
};
use crate::policy_match::{ColumnAccessDef, ColumnMaskDef, RowFilterDef, matches_schema_table};

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
/// IS NULL, IS NOT NULL, NOT, BETWEEN, LIKE, IN LIST, CAST, and scalar functions.
///
/// Pass `Some(ctx)` as `registry` to enable full scalar function lookup (required for
/// column mask expressions). Pass `None` for row filter expressions where only
/// COALESCE is supported.
fn sql_ast_to_df_expr(
    expr: &SqlExpr,
    var_values: &[(String, String)],
    registry: Option<&dyn FunctionRegistry>,
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
            let l = sql_ast_to_df_expr(left, var_values, registry)?;
            let r = sql_ast_to_df_expr(right, var_values, registry)?;
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
        SqlExpr::IsNull(inner) => Ok(sql_ast_to_df_expr(inner, var_values, registry)?.is_null()),
        SqlExpr::IsNotNull(inner) => {
            Ok(sql_ast_to_df_expr(inner, var_values, registry)?.is_not_null())
        }
        SqlExpr::Nested(inner) => sql_ast_to_df_expr(inner, var_values, registry),
        SqlExpr::UnaryOp { op, expr } => {
            use datafusion::sql::sqlparser::ast::UnaryOperator;
            let inner = sql_ast_to_df_expr(expr, var_values, registry)?;
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
            let e = sql_ast_to_df_expr(expr, var_values, registry)?;
            let lo = sql_ast_to_df_expr(low, var_values, registry)?;
            let hi = sql_ast_to_df_expr(high, var_values, registry)?;
            let between = e.clone().gt_eq(lo).and(e.lt_eq(hi));
            Ok(if *negated { between.not() } else { between })
        }
        SqlExpr::Like {
            negated,
            expr,
            pattern,
            ..
        } => {
            let col_expr = sql_ast_to_df_expr(expr, var_values, registry)?;
            let pat_expr = sql_ast_to_df_expr(pattern, var_values, registry)?;
            let like_expr = col_expr.like(pat_expr);
            Ok(if *negated { like_expr.not() } else { like_expr })
        }
        SqlExpr::InList {
            expr,
            list,
            negated,
        } => {
            let col_expr = sql_ast_to_df_expr(expr, var_values, registry)?;
            let list_exprs: Vec<_> = list
                .iter()
                .map(|e| sql_ast_to_df_expr(e, var_values, registry))
                .collect::<datafusion::error::Result<_>>()?;
            Ok(col_expr.in_list(list_exprs, *negated))
        }
        SqlExpr::Function(f) => {
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
                .join(".");

            let args = match &f.args {
                FunctionArguments::List(list) => list
                    .args
                    .iter()
                    .map(|arg| match arg {
                        FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                            sql_ast_to_df_expr(e, var_values, registry)
                        }
                        other => Err(datafusion::error::DataFusionError::Plan(format!(
                            "Unsupported function arg: {other:?}"
                        ))),
                    })
                    .collect::<datafusion::error::Result<Vec<_>>>()?,
                FunctionArguments::None => vec![],
                other => {
                    return Err(datafusion::error::DataFusionError::Plan(format!(
                        "Unsupported function arguments in mask/filter expression: {other:?}"
                    )));
                }
            };

            if let Some(reg) = registry {
                // Full function lookup via registry — supports all built-in and user-defined UDFs.
                let func_name_lower = func_name.to_lowercase();
                let udf = reg.udf(&func_name_lower).map_err(|_| {
                    datafusion::error::DataFusionError::Plan(format!(
                        "Unknown function '{func_name}' in mask expression"
                    ))
                })?;
                Ok(udf.call(args))
            } else {
                // Filter expressions: only COALESCE supported.
                match func_name.to_uppercase().as_str() {
                    "COALESCE" => Ok(datafusion::functions::expr_fn::coalesce(args)),
                    other => Err(datafusion::error::DataFusionError::Plan(format!(
                        "Function '{other}' in filter expressions is not supported. \
                         For complex expressions, use column masks instead."
                    ))),
                }
            }
        }
        SqlExpr::Cast {
            expr, data_type, ..
        } => {
            use datafusion::arrow::datatypes::DataType as ArrowType;
            use datafusion::sql::sqlparser::ast::DataType as SqlDataType;
            let inner = sql_ast_to_df_expr(expr, var_values, registry)?;
            let arrow_type = match data_type {
                SqlDataType::Varchar(_)
                | SqlDataType::Text
                | SqlDataType::Char(_)
                | SqlDataType::String(_) => ArrowType::Utf8,
                SqlDataType::SmallInt(_) => ArrowType::Int16,
                SqlDataType::Integer(_) | SqlDataType::Int(_) => ArrowType::Int32,
                SqlDataType::BigInt(_) => ArrowType::Int64,
                SqlDataType::Float(_) | SqlDataType::Float4 | SqlDataType::Real => {
                    ArrowType::Float32
                }
                SqlDataType::Double(_)
                | SqlDataType::DoublePrecision
                | SqlDataType::Float8
                | SqlDataType::Float64 => ArrowType::Float64,
                SqlDataType::Boolean => ArrowType::Boolean,
                other => {
                    return Err(datafusion::error::DataFusionError::Plan(format!(
                        "Unsupported CAST target type in mask/filter expression: {other:?}"
                    )));
                }
            };
            Ok(datafusion::logical_expr::cast(inner, arrow_type))
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

    sql_ast_to_df_expr(&sql_expr, &var_values, None)
}

/// Parse a column mask expression into a DataFusion Expr.
///
/// Supports all scalar functions registered in the session context (RIGHT, LEFT,
/// UPPER, LOWER, CONCAT, COALESCE, etc.), string concatenation (`||`), literals,
/// and column references. Template variables like `{user.tenant}` are substituted
/// as string literals — never interpolated as raw SQL.
fn parse_mask_expr(
    ctx: &SessionContext,
    column: &str,
    mask_template: &str,
    vars: &UserVars,
) -> datafusion::error::Result<datafusion::logical_expr::Expr> {
    let (mangled, var_values) = mangle_vars(mask_template, vars);
    let dialect = GenericDialect {};
    let mut parser = Parser::new(&dialect).try_with_sql(&mangled).map_err(|e| {
        datafusion::error::DataFusionError::Plan(format!(
            "Failed to parse mask expression for column '{column}': {e}"
        ))
    })?;
    let sql_expr = parser.parse_expr().map_err(|e| {
        datafusion::error::DataFusionError::Plan(format!(
            "Failed to parse mask expression for column '{column}': {e}"
        ))
    })?;
    sql_ast_to_df_expr(&sql_expr, &var_values, Some(ctx))
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

// ---------- policy error ----------

/// Errors that can occur during policy obligation application.
#[derive(Debug)]
enum PolicyError {
    /// A deny-effect policy matched the query — reject with SQLSTATE 42501.
    DeniedByPolicy { policy_name: String },
    /// All columns were denied — nothing left to project (SQLSTATE 42501).
    AllColumnsDenied { columns: Vec<String> },
    /// Plan rewriting (filter injection or projection build) failed.
    PlanTransformation(datafusion::error::DataFusionError),
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::DeniedByPolicy { policy_name } => {
                write!(f, "Access denied by policy '{policy_name}'")
            }
            PolicyError::AllColumnsDenied { columns } => {
                write!(
                    f,
                    "Access denied: column{} {} restricted by policy",
                    if columns.len() == 1 { "" } else { "s" },
                    columns.join(", ")
                )
            }
            PolicyError::PlanTransformation(e) => write!(f, "Plan transformation error: {e}"),
        }
    }
}

impl PolicyError {
    fn into_pgwire_error(self) -> PgWireError {
        match self {
            PolicyError::DeniedByPolicy { policy_name } => {
                PgWireError::UserError(Box::new(ErrorInfo::new(
                    "ERROR".to_owned(),
                    "42501".to_owned(),
                    format!("Access denied by policy '{policy_name}'"),
                )))
            }
            PolicyError::AllColumnsDenied { columns } => {
                PgWireError::UserError(Box::new(ErrorInfo::new(
                    "ERROR".to_string(),
                    "42501".to_string(),
                    format!(
                        "Access denied: column{} {} restricted by policy",
                        if columns.len() == 1 { "" } else { "s" },
                        columns.join(", ")
                    ),
                )))
            }
            PolicyError::PlanTransformation(e) => PgWireError::ApiError(Box::new(e)),
        }
    }
}

// ---------- obligation effects pipeline ----------

/// Collected effects from all policies — separates "what to apply" from "how to apply it".
struct ObligationEffects {
    /// Combined row filter per (df_schema, table): AND within a policy, AND across policies.
    row_filters: HashMap<(String, String), datafusion::logical_expr::Expr>,
    /// Column mask expressions keyed by column name. First (highest priority) wins.
    column_masks: HashMap<String, datafusion::logical_expr::Expr>,
    /// Exact column names to completely remove from the projection (O(1) HashSet lookup).
    column_denies: HashSet<String>,
    /// Glob patterns for column deny (contain `*`). Checked at projection time via matches_pattern.
    /// Kept separate to avoid O(P×C) cost on every query when no globs are in use.
    column_glob_patterns: Vec<String>,
    /// Tables that have at least one matching permit obligation.
    tables_with_permit: HashSet<(String, String)>,
    /// If set, a deny-effect row_filter matched the query — must reject before executing.
    denied_by_policy: Option<String>,
}

impl ObligationEffects {
    /// Collect all obligation effects from the session's policies.
    fn collect(
        session: &SessionDataClone,
        user_tables: &[(String, String)],
        user_vars: &UserVars,
        session_context: &SessionContext,
    ) -> Self {
        let mut effects = ObligationEffects {
            row_filters: HashMap::new(),
            column_masks: HashMap::new(),
            column_denies: HashSet::new(),
            column_glob_patterns: Vec::new(),
            tables_with_permit: HashSet::new(),
            denied_by_policy: None,
        };

        // Check deny policies for row_filter obligations first (short-circuit on first match).
        'deny_check: for policy in &session.deny_policies {
            for obl in &policy.obligations {
                if let Ok(def) = serde_json::from_value::<RowFilterDef>(obl.definition.clone()) {
                    for (df_schema, table) in user_tables {
                        if matches_schema_table(
                            &def.schema,
                            &def.table,
                            df_schema,
                            table,
                            &session.df_to_upstream,
                        ) {
                            effects.denied_by_policy = Some(policy.name.clone());
                            break 'deny_check;
                        }
                    }
                }
            }
        }

        // Collect permit policy effects.
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
                            for (df_schema, table) in user_tables {
                                if matches_schema_table(
                                    &def.schema,
                                    &def.table,
                                    df_schema,
                                    table,
                                    &session.df_to_upstream,
                                ) {
                                    let key = (df_schema.clone(), table.clone());
                                    effects.tables_with_permit.insert(key.clone());

                                    match parse_filter_expr(&def.filter_expression, user_vars) {
                                        Ok(filter) => {
                                            // AND within the same policy
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
                            for (df_schema, table) in user_tables {
                                if matches_schema_table(
                                    &def.schema,
                                    &def.table,
                                    df_schema,
                                    table,
                                    &session.df_to_upstream,
                                ) {
                                    effects
                                        .tables_with_permit
                                        .insert((df_schema.clone(), table.clone()));

                                    // First (highest priority) mask wins.
                                    if !effects.column_masks.contains_key(&def.column) {
                                        match parse_mask_expr(
                                            session_context,
                                            &def.column,
                                            &def.mask_expression,
                                            user_vars,
                                        ) {
                                            Ok(mask) => {
                                                effects
                                                    .column_masks
                                                    .insert(def.column.clone(), mask);
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
                            for (df_schema, table) in user_tables {
                                if matches_schema_table(
                                    &def.schema,
                                    &def.table,
                                    df_schema,
                                    table,
                                    &session.df_to_upstream,
                                ) {
                                    effects
                                        .tables_with_permit
                                        .insert((df_schema.clone(), table.clone()));
                                    if def.action == "deny" {
                                        for c in &def.columns {
                                            if c.contains('*') {
                                                effects.column_glob_patterns.push(c.clone());
                                            } else {
                                                effects.column_denies.insert(c.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            // AND this policy's per-table filters into the global row_filters map.
            // Each permit policy adds a restriction; users see the intersection of all policies.
            for (key, filter) in policy_table_filters {
                let entry = effects.row_filters.entry(key).or_insert_with(|| lit(true));
                *entry = entry.clone().and(filter);
            }
        }

        // Also apply column_access deny from deny-effect policies.
        for policy in &session.deny_policies {
            for obl in &policy.obligations {
                if obl.obligation_type == "column_access"
                    && let Ok(def) =
                        serde_json::from_value::<ColumnAccessDef>(obl.definition.clone())
                {
                    for (df_schema, table) in user_tables {
                        if matches_schema_table(
                            &def.schema,
                            &def.table,
                            df_schema,
                            table,
                            &session.df_to_upstream,
                        ) && def.action == "deny"
                        {
                            for c in &def.columns {
                                if c.contains('*') {
                                    effects.column_glob_patterns.push(c.clone());
                                } else {
                                    effects.column_denies.insert(c.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        effects
    }

    /// Return an error if a deny-effect row_filter matched the query.
    fn check_deny(&self) -> Result<(), PolicyError> {
        if let Some(name) = &self.denied_by_policy {
            Err(PolicyError::DeniedByPolicy {
                policy_name: name.clone(),
            })
        } else {
            Ok(())
        }
    }

    /// For `access_mode = "policy_required"`: inject `lit(false)` for tables with no permit.
    fn apply_access_mode(&mut self, access_mode: &str, user_tables: &[(String, String)]) {
        if access_mode == "policy_required" {
            for table_key in user_tables {
                if !self.tables_with_permit.contains(table_key) {
                    self.row_filters.insert(table_key.clone(), lit(false));
                }
            }
        }
    }

    /// Inject row filter `Filter` nodes below each matching `TableScan` via `transform_up`.
    fn apply_row_filters(&self, plan: LogicalPlan) -> Result<LogicalPlan, PolicyError> {
        if self.row_filters.is_empty() {
            return Ok(plan);
        }

        use datafusion::common::tree_node::{Transformed, TreeNode};

        let result = plan.transform_up(|node| {
            let LogicalPlan::TableScan(ref scan) = node else {
                return Ok(Transformed::no(node));
            };
            let df_schema = scan.table_name.schema().unwrap_or("").to_string();
            let table = scan.table_name.table().to_string();
            let key = (df_schema, table);

            if let Some(filter_expr) = self.row_filters.get(&key) {
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

        result
            .map(|t| t.data)
            .map_err(PolicyError::PlanTransformation)
    }

    /// Apply column masks and column denies as a top-level `Projection`.
    ///
    /// Denied columns take priority: a column that is both denied and masked is removed,
    /// never replaced with the mask expression.
    fn apply_projection(&self, plan: LogicalPlan) -> Result<LogicalPlan, PolicyError> {
        if self.column_masks.is_empty()
            && self.column_denies.is_empty()
            && self.column_glob_patterns.is_empty()
        {
            return Ok(plan);
        }

        let output_schema = plan.schema();
        let arrow_fields = output_schema.fields();

        let is_denied = |col_name: &str| -> bool {
            if self.column_denies.contains(col_name) {
                return true; // O(1) exact match
            }
            // Only iterate globs when they are actually present
            if !self.column_glob_patterns.is_empty() {
                return self
                    .column_glob_patterns
                    .iter()
                    .any(|p| crate::policy_match::matches_pattern(p, col_name));
            }
            false
        };

        let new_exprs: Vec<datafusion::logical_expr::Expr> = arrow_fields
            .iter()
            .filter_map(|field| {
                let col_name = field.name();
                // Deny takes priority over mask.
                if is_denied(col_name) {
                    return None;
                }
                if let Some(mask) = self.column_masks.get(col_name.as_str()) {
                    Some(mask.clone().alias(col_name))
                } else {
                    Some(col(col_name))
                }
            })
            .collect();

        if new_exprs.is_empty() {
            let denied: Vec<String> = arrow_fields
                .iter()
                .map(|f| f.name().clone())
                .filter(|n| is_denied(n))
                .collect();
            return Err(PolicyError::AllColumnsDenied { columns: denied });
        }

        LogicalPlanBuilder::from(plan)
            .project(new_exprs)
            .and_then(|b| b.build())
            .map_err(|e| {
                PolicyError::PlanTransformation(datafusion::error::DataFusionError::Plan(
                    e.to_string(),
                ))
            })
    }

    /// True if any row filters, column masks, or column denies were collected.
    fn has_effects(&self) -> bool {
        !self.row_filters.is_empty()
            || !self.column_masks.is_empty()
            || !self.column_denies.is_empty()
            || !self.column_glob_patterns.is_empty()
    }
}

/// Apply all policy obligations to a logical plan.
///
/// Returns `(modified_plan, had_effects)` where `had_effects` is true when any
/// row filter, column mask, or column deny was applied (used to decide whether to
/// mark the query as "policy-rewritten" in the audit log).
///
/// This function is the testable core extracted from `PolicyHook::handle_query`.
/// Tests construct a `SessionDataClone` and a `LogicalPlan` directly and call this.
async fn apply_obligations(
    session: &SessionDataClone,
    session_context: &SessionContext,
    logical_plan: LogicalPlan,
    user_vars: &UserVars,
) -> Result<(LogicalPlan, bool), PolicyError> {
    let user_tables = collect_user_tables(&logical_plan);

    let mut effects = ObligationEffects::collect(session, &user_tables, user_vars, session_context);

    effects.check_deny()?;
    effects.apply_access_mode(&session.access_mode, &user_tables);

    let had_effects = effects.has_effects();
    let plan = effects.apply_row_filters(logical_plan)?;
    let plan = effects.apply_projection(plan)?;

    Ok((plan, had_effects))
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

        // Apply all policy obligations (deny check, row filters, column masks/denies).
        let (final_plan, had_effects) =
            match apply_obligations(&session, session_context, logical_plan, &user_vars).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!(error = %e, "PolicyHook: obligation error");
                    return Some(Err(e.into_pgwire_error()));
                }
            };

        let rewritten_query = if had_effects {
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
    use datafusion::arrow::array::{Array, Int32Array, StringArray};
    use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
    use datafusion::arrow::record_batch::RecordBatch;
    use datafusion::catalog::default_table_source::DefaultTableSource;
    use datafusion::datasource::MemTable;
    use datafusion::datasource::empty::EmptyTable;
    use datafusion::logical_expr::LogicalPlanBuilder;
    use datafusion::prelude::SessionContext;
    use datafusion::sql::sqlparser::{dialect::PostgreSqlDialect, parser::Parser as SqlParser};
    use std::sync::Arc;

    // ---------- shared test helpers ----------

    fn make_session(
        permit_policies: Vec<ResolvedPolicy>,
        deny_policies: Vec<ResolvedPolicy>,
        access_mode: &str,
        df_to_upstream: HashMap<String, String>,
    ) -> SessionDataClone {
        SessionDataClone {
            permit_policies,
            deny_policies,
            access_mode: access_mode.to_string(),
            df_to_upstream,
            datasource_id: Uuid::nil(),
            datasource_name: "test_ds".to_string(),
        }
    }

    fn make_policy(
        name: &str,
        effect: &str,
        priority: i32,
        obligations: Vec<ResolvedObligation>,
    ) -> ResolvedPolicy {
        ResolvedPolicy {
            id: Uuid::now_v7(),
            name: name.to_string(),
            effect: effect.to_string(),
            version: 1,
            priority,
            obligations,
        }
    }

    fn make_row_filter_obl(schema: &str, table: &str, filter: &str) -> ResolvedObligation {
        ResolvedObligation {
            obligation_type: "row_filter".to_string(),
            definition: serde_json::json!({
                "schema": schema,
                "table": table,
                "filter_expression": filter,
            }),
        }
    }

    fn make_column_mask_obl(
        schema: &str,
        table: &str,
        column: &str,
        mask: &str,
    ) -> ResolvedObligation {
        ResolvedObligation {
            obligation_type: "column_mask".to_string(),
            definition: serde_json::json!({
                "schema": schema,
                "table": table,
                "column": column,
                "mask_expression": mask,
            }),
        }
    }

    fn make_column_deny_obl(schema: &str, table: &str, columns: &[&str]) -> ResolvedObligation {
        ResolvedObligation {
            obligation_type: "column_access".to_string(),
            definition: serde_json::json!({
                "schema": schema,
                "table": table,
                "columns": columns,
                "action": "deny",
            }),
        }
    }

    /// Build a scan plan over an `EmptyTable` (no real data; for plan-structure tests).
    fn build_scan_plan(schema_table: &str, columns: Vec<(&str, DataType)>) -> LogicalPlan {
        let fields: Vec<Field> = columns
            .into_iter()
            .map(|(name, dt)| Field::new(name, dt, true))
            .collect();
        let schema = Arc::new(Schema::new(fields));
        let table = Arc::new(EmptyTable::new(schema));
        let source = Arc::new(DefaultTableSource::new(table));
        LogicalPlanBuilder::scan(schema_table, source, None)
            .unwrap()
            .build()
            .unwrap()
    }

    fn default_vars() -> UserVars {
        UserVars {
            tenant: "acme".to_string(),
            username: "alice".to_string(),
            user_id: "00000000-0000-0000-0000-000000000001".to_string(),
        }
    }

    fn plan_display(plan: &LogicalPlan) -> String {
        format!("{}", plan.display_indent())
    }

    fn assert_plan_contains(plan: &LogicalPlan, expected: &str) {
        let display = plan_display(plan);
        assert!(
            display.contains(expected),
            "Plan does not contain '{expected}':\n{display}"
        );
    }

    // ---------- system-only detection ----------

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

    // ---------- parse_filter_expr ----------

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

    // ---------- collect_user_tables ----------

    #[test]
    fn test_collect_user_tables_skips_pg_catalog() {
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

    // ---------- Tier 1: plan-structure tests (apply_obligations with EmptyTable) ----------

    #[tokio::test]
    async fn test_row_filter_injected_below_table_scan() {
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_row_filter_obl("public", "orders", "status = 'active'")],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        assert_plan_contains(&result_plan, "Filter");
    }

    #[tokio::test]
    async fn test_row_filters_and_within_policy() {
        // Two row_filter obligations in the same policy → AND'd together.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![
                    make_row_filter_obl("public", "orders", "status = 'active'"),
                    make_row_filter_obl("public", "orders", "amount > 0"),
                ],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![
                ("id", DataType::Int32),
                ("status", DataType::Utf8),
                ("amount", DataType::Int64),
            ],
        );

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let display = plan_display(&result_plan);
        // Both filter expressions should appear
        assert!(display.contains("Filter"), "Expected Filter: {display}");
    }

    #[tokio::test]
    async fn test_row_filters_and_across_policies() {
        // Same table filtered by two permit policies → AND'd together (intersection).
        let session = make_session(
            vec![
                make_policy(
                    "p1",
                    "permit",
                    1,
                    vec![make_row_filter_obl("public", "orders", "org = 'acme'")],
                ),
                make_policy(
                    "p2",
                    "permit",
                    2,
                    vec![make_row_filter_obl("public", "orders", "org = 'globex'")],
                ),
            ],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("org", DataType::Utf8)],
        );

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        assert_plan_contains(&result_plan, "Filter");
        // AND semantics: both filter values appear in the expression (ANDed together).
        let display = plan_display(&result_plan);
        assert!(
            display.contains("acme") && display.contains("globex"),
            "Expected AND filter with both orgs: {display}"
        );
    }

    #[tokio::test]
    async fn test_column_deny_strips_column() {
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_column_deny_obl("public", "customers", &["ssn"])],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.customers",
            vec![
                ("id", DataType::Int32),
                ("name", DataType::Utf8),
                ("ssn", DataType::Utf8),
            ],
        );

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        // ssn should be stripped from the projection
        let display = plan_display(&result_plan);
        assert!(!display.contains("ssn"), "ssn should be denied: {display}");
        assert!(display.contains("name"), "name should remain: {display}");
    }

    #[tokio::test]
    async fn test_column_deny_all_columns_error() {
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_column_deny_obl("public", "customers", &["id", "name"])],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.customers",
            vec![("id", DataType::Int32), ("name", DataType::Utf8)],
        );

        let result = apply_obligations(&session, &ctx, plan, &default_vars()).await;
        assert!(
            matches!(result, Err(PolicyError::AllColumnsDenied { .. })),
            "Expected AllColumnsDenied: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_deny_policy_row_filter_rejects() {
        // Deny-effect policy with row_filter on matching table → short-circuit error.
        let session = make_session(
            vec![],
            vec![make_policy(
                "deny_p",
                "deny",
                1,
                vec![make_row_filter_obl("public", "orders", "1=1")],
            )],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan("public.orders", vec![("id", DataType::Int32)]);

        let result = apply_obligations(&session, &ctx, plan, &default_vars()).await;
        assert!(
            matches!(result, Err(PolicyError::DeniedByPolicy { .. })),
            "Expected DeniedByPolicy: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_deny_policy_row_filter_no_match() {
        // Deny-effect policy with row_filter on a DIFFERENT table → no error.
        let session = make_session(
            vec![],
            vec![make_policy(
                "deny_p",
                "deny",
                1,
                vec![make_row_filter_obl("public", "users", "1=1")],
            )],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        // Query is on "orders", deny is on "users" → should pass through
        let plan = build_scan_plan("public.orders", vec![("id", DataType::Int32)]);

        let (_, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();
        assert!(!had_effects, "No effects expected when deny doesn't match");
    }

    #[tokio::test]
    async fn test_policy_required_no_permit_false_filter() {
        // access_mode = "policy_required" with no permit → lit(false) injected.
        let session = make_session(vec![], vec![], "policy_required", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan("public.orders", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let display = plan_display(&result_plan);
        assert!(
            display.contains("false"),
            "Expected lit(false) filter: {display}"
        );
    }

    #[tokio::test]
    async fn test_policy_required_with_permit_normal() {
        // access_mode = "policy_required" with a permit → normal filter applied, not false.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_row_filter_obl("public", "orders", "id > 0")],
            )],
            vec![],
            "policy_required",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan("public.orders", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let display = plan_display(&result_plan);
        // Should contain the actual filter, not a blanket false
        assert!(display.contains("Filter"), "Expected Filter: {display}");
    }

    #[tokio::test]
    async fn test_wildcard_schema_matches_all() {
        // Obligation with schema: "*" matches any schema name.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_row_filter_obl("*", "orders", "id > 0")],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan("any_schema.orders", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        assert_plan_contains(&result_plan, "Filter");
    }

    #[tokio::test]
    async fn test_wildcard_table_matches_all() {
        // Obligation with table: "*" matches any table in the schema.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_row_filter_obl("public", "*", "id > 0")],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan("public.anything", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        assert_plan_contains(&result_plan, "Filter");
    }

    #[tokio::test]
    async fn test_schema_alias_resolved() {
        // df schema alias "sales" maps to upstream "public"; obligation targets "public".
        let mut df_to_upstream = HashMap::new();
        df_to_upstream.insert("sales".to_string(), "public".to_string());

        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_row_filter_obl("public", "orders", "id > 0")],
            )],
            vec![],
            "open",
            df_to_upstream,
        );
        let ctx = SessionContext::new();
        // Plan uses "sales" alias, which resolves to upstream "public"
        let plan = build_scan_plan("sales.orders", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        assert_plan_contains(&result_plan, "Filter");
    }

    #[tokio::test]
    async fn test_deny_overrides_mask() {
        // Column is both denied (via column_deny) and would be masked;
        // deny takes priority — column is removed, mask expression never applied.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![
                    make_column_deny_obl("public", "customers", &["ssn"]),
                    make_column_mask_obl("public", "customers", "ssn", "'***'"),
                ],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        // Register table so parse_mask_expr can resolve it
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, true),
            Field::new("ssn", DataType::Utf8, true),
        ]));
        let ctx = SessionContext::new();
        let empty = RecordBatch::new_empty(schema.clone());
        let table = MemTable::try_new(schema, vec![vec![empty]]).unwrap();
        ctx.register_table("customers", Arc::new(table)).unwrap();

        let plan = build_scan_plan(
            "public.customers",
            vec![("id", DataType::Int32), ("ssn", DataType::Utf8)],
        );

        let (result_plan, _) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        let display = plan_display(&result_plan);
        assert!(
            !display.contains("ssn"),
            "ssn should be denied (not masked): {display}"
        );
        assert!(
            !display.contains("***"),
            "mask expression must not appear when column is denied: {display}"
        );
    }

    #[tokio::test]
    async fn test_no_policies_no_effects() {
        // No policies at all → plan is returned unchanged.
        let session = make_session(vec![], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan("public.orders", vec![("id", DataType::Int32)]);

        let (_, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(!had_effects);
    }

    // ---------- Tier 2: execution tests (apply_obligations with MemTable + real data) ----------

    /// 5-row customers table: 3 acme, 2 globex. Columns: id, org_id, name, ssn, credit_card.
    async fn setup_customers_ctx() -> SessionContext {
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("org_id", DataType::Utf8, false),
            Field::new("name", DataType::Utf8, false),
            Field::new("ssn", DataType::Utf8, true),
            Field::new("credit_card", DataType::Utf8, true),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Int32Array::from(vec![1, 2, 3, 4, 5])),
                Arc::new(StringArray::from(vec![
                    "acme", "acme", "acme", "globex", "globex",
                ])),
                Arc::new(StringArray::from(vec![
                    "Alice", "Bob", "Charlie", "Dave", "Eve",
                ])),
                Arc::new(StringArray::from(vec![
                    "123-45-6789",
                    "234-56-7890",
                    "345-67-8901",
                    "456-78-9012",
                    "567-89-0123",
                ])),
                Arc::new(StringArray::from(vec![
                    "4111111111111111",
                    "4222222222222222",
                    "4333333333333333",
                    "4444444444444444",
                    "4555555555555555",
                ])),
            ],
        )
        .unwrap();
        let ctx = SessionContext::new();
        let table = MemTable::try_new(schema, vec![vec![batch]]).unwrap();
        ctx.register_table("customers", Arc::new(table)).unwrap();
        ctx
    }

    async fn exec_plan(ctx: &SessionContext, plan: LogicalPlan) -> Vec<RecordBatch> {
        ctx.execute_logical_plan(plan)
            .await
            .unwrap()
            .collect()
            .await
            .unwrap()
    }

    fn total_rows(batches: &[RecordBatch]) -> usize {
        batches.iter().map(|b| b.num_rows()).sum()
    }

    fn column_names(batches: &[RecordBatch]) -> Vec<String> {
        if batches.is_empty() {
            return vec![];
        }
        batches[0]
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect()
    }

    #[tokio::test]
    async fn test_exec_permit_row_filter() {
        // row_filter "org_id = 'acme'" → only 3 of 5 rows returned.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_row_filter_obl("*", "customers", "org_id = 'acme'")],
            )],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(total_rows(&batches), 3, "Only acme rows expected");
    }

    #[tokio::test]
    async fn test_exec_permit_column_deny() {
        // column_access deny on ssn → output has 4 columns (not 5), ssn absent.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_column_deny_obl("*", "customers", &["ssn"])],
            )],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(total_rows(&batches), 5);
        let cols = column_names(&batches);
        assert!(
            !cols.contains(&"ssn".to_string()),
            "ssn should not appear: {cols:?}"
        );
        assert_eq!(cols.len(), 4, "Expected 4 columns: {cols:?}");
    }

    #[tokio::test]
    async fn test_exec_deny_row_filter_rejects() {
        // Deny-effect policy with row_filter on matching table → error returned.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![],
            vec![make_policy(
                "deny_p",
                "deny",
                1,
                vec![make_row_filter_obl("*", "customers", "1=1")],
            )],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let result = apply_obligations(&session, &ctx, plan, &default_vars()).await;

        assert!(
            matches!(result, Err(PolicyError::DeniedByPolicy { .. })),
            "Expected DeniedByPolicy: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_exec_policy_required_no_permit_empty() {
        // policy_required + no permit → lit(false) filter → 0 rows returned.
        let ctx = setup_customers_ctx().await;
        let session = make_session(vec![], vec![], "policy_required", HashMap::new());

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(
            total_rows(&batches),
            0,
            "No rows expected with policy_required + no permit"
        );
    }

    #[tokio::test]
    async fn test_exec_policy_required_with_permit_normal() {
        // policy_required + permit with row_filter → filtered rows returned.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_row_filter_obl("*", "customers", "org_id = 'acme'")],
            )],
            vec![],
            "policy_required",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, _) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(total_rows(&batches), 3);
    }

    #[tokio::test]
    async fn test_exec_two_permits_row_filter_and() {
        // Policy A: org = 'acme', Policy B: org = 'globex' → AND → 0 rows (disjoint sets).
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![
                make_policy(
                    "p_acme",
                    "permit",
                    1,
                    vec![make_row_filter_obl("*", "customers", "org_id = 'acme'")],
                ),
                make_policy(
                    "p_globex",
                    "permit",
                    2,
                    vec![make_row_filter_obl("*", "customers", "org_id = 'globex'")],
                ),
            ],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, _) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(
            total_rows(&batches),
            0,
            "AND semantics: disjoint filters produce 0 rows"
        );
    }

    #[tokio::test]
    async fn test_exec_two_permits_row_filter_and_overlapping() {
        // Policy A: org_id = 'acme' (rows 1,2,3).
        // Policy B: name != 'Charlie' (rows 1,2,4,5).
        // AND intersection: acme rows where name != 'Charlie' → rows 1 (Alice), 2 (Bob) → 2 rows.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![
                make_policy(
                    "p_acme",
                    "permit",
                    1,
                    vec![make_row_filter_obl("*", "customers", "org_id = 'acme'")],
                ),
                make_policy(
                    "p_not_charlie",
                    "permit",
                    2,
                    vec![make_row_filter_obl("*", "customers", "name != 'Charlie'")],
                ),
            ],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, _) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(
            total_rows(&batches),
            2,
            "AND intersection: acme AND not-Charlie → Alice + Bob only"
        );
    }

    #[tokio::test]
    async fn test_exec_permit_column_mask() {
        // column_mask with a literal → SSN shows 'REDACTED' instead of actual value.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_column_mask_obl("*", "customers", "ssn", "'REDACTED'")],
            )],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(total_rows(&batches), 5);
        let cols = column_names(&batches);
        assert!(
            cols.contains(&"ssn".to_string()),
            "ssn should be present (masked, not denied): {cols:?}"
        );
        // Verify all SSN values are the mask value, not original data.
        let ssn_idx = batches[0].schema().index_of("ssn").unwrap();
        let ssn_array = batches[0]
            .column(ssn_idx)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        for i in 0..ssn_array.len() {
            let val = ssn_array.value(i);
            assert_eq!(val, "REDACTED", "SSN row {i} should be masked, got: {val}");
        }
    }

    #[tokio::test]
    async fn test_exec_column_mask_with_row_filter() {
        // row_filter "org_id = 'acme'" (3 rows) + column_mask on ssn → 3 rows with masked SSN.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![
                    make_row_filter_obl("*", "customers", "org_id = 'acme'"),
                    make_column_mask_obl("*", "customers", "ssn", "'***'"),
                ],
            )],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(total_rows(&batches), 3, "Only acme rows expected");
        let ssn_idx = batches[0].schema().index_of("ssn").unwrap();
        let ssn_array = batches[0]
            .column(ssn_idx)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        for i in 0..ssn_array.len() {
            assert_eq!(ssn_array.value(i), "***", "SSN row {i} should be masked");
        }
    }

    #[tokio::test]
    async fn test_exec_deny_all_columns_error() {
        // All columns denied → AllColumnsDenied error.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_column_deny_obl(
                    "*",
                    "customers",
                    &["id", "org_id", "name", "ssn", "credit_card"],
                )],
            )],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let result = apply_obligations(&session, &ctx, plan, &default_vars()).await;

        assert!(
            matches!(result, Err(PolicyError::AllColumnsDenied { .. })),
            "Expected AllColumnsDenied: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_exec_full_composition() {
        // Tenant isolation (row_filter) + column hiding (credit_card deny) in one session.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![make_policy(
                "tenant_policy",
                "permit",
                1,
                vec![
                    make_row_filter_obl("*", "customers", "org_id = 'acme'"),
                    make_column_deny_obl("*", "customers", &["credit_card"]),
                ],
            )],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let batches = exec_plan(&ctx, result_plan).await;
        // 3 acme rows
        assert_eq!(total_rows(&batches), 3);
        // credit_card column removed
        let cols = column_names(&batches);
        assert!(
            !cols.contains(&"credit_card".to_string()),
            "credit_card should be hidden: {cols:?}"
        );
        // Other columns present
        assert!(cols.contains(&"name".to_string()));
        assert!(cols.contains(&"ssn".to_string()));
    }

    #[tokio::test]
    async fn test_exec_deny_column_from_deny_policy() {
        // Deny-effect policy with column_access deny → column stripped.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![],
            vec![make_policy(
                "deny_p",
                "deny",
                1,
                vec![make_column_deny_obl("*", "customers", &["credit_card"])],
            )],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(total_rows(&batches), 5);
        let cols = column_names(&batches);
        assert!(
            !cols.contains(&"credit_card".to_string()),
            "credit_card should be denied by deny policy: {cols:?}"
        );
    }

    // ---------- apply_projection glob pattern tests ----------

    #[tokio::test]
    async fn test_apply_projection_suffix_glob() {
        // columns: ["*_at"] → strips created_at and updated_at, keeps others.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_column_deny_obl("public", "events", &["*_at"])],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.events",
            vec![
                ("id", DataType::Int32),
                ("name", DataType::Utf8),
                ("created_at", DataType::Utf8),
                ("updated_at", DataType::Utf8),
            ],
        );

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let schema = result_plan.schema();
        let col_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert!(col_names.contains(&"id"), "id should remain: {col_names:?}");
        assert!(
            col_names.contains(&"name"),
            "name should remain: {col_names:?}"
        );
        assert!(
            !col_names.contains(&"created_at"),
            "created_at should be denied: {col_names:?}"
        );
        assert!(
            !col_names.contains(&"updated_at"),
            "updated_at should be denied: {col_names:?}"
        );
    }

    #[tokio::test]
    async fn test_apply_projection_star_all_denied() {
        // columns: ["*"] → all columns denied → AllColumnsDenied error.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_column_deny_obl("public", "events", &["*"])],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.events",
            vec![("id", DataType::Int32), ("name", DataType::Utf8)],
        );

        let result = apply_obligations(&session, &ctx, plan, &default_vars()).await;
        assert!(
            matches!(result, Err(PolicyError::AllColumnsDenied { .. })),
            "Expected AllColumnsDenied: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_apply_projection_mask_vs_deny_priority() {
        // Column is both masked and denied via glob → deny wins (column removed).
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![
                    make_column_deny_obl("public", "events", &["secret_*"]),
                    make_column_mask_obl("public", "events", "secret_val", "'***'"),
                ],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.events",
            vec![("id", DataType::Int32), ("secret_val", DataType::Utf8)],
        );

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();

        assert!(had_effects);
        let schema = result_plan.schema();
        let col_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert!(col_names.contains(&"id"), "id should remain: {col_names:?}");
        assert!(
            !col_names.contains(&"secret_val"),
            "secret_val should be denied (not masked): {col_names:?}"
        );
    }

    #[tokio::test]
    async fn test_apply_projection_join_collision() {
        // KNOWN LIMITATION: deny ["id"] is name-based — strips "id" from both table_a and table_b.
        // This test documents the current behavior; a future fix will be visible as a test change.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                // Intended to only deny id on table_a, but column_denies is flat.
                vec![make_column_deny_obl("public", "table_a", &["id"])],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();

        // Build a plan that has two "id" columns (simulated via a single table with id + id2,
        // and we manually test that the flat HashSet approach strips ALL "id" columns).
        let effects = ObligationEffects {
            row_filters: HashMap::new(),
            column_masks: HashMap::new(),
            column_denies: {
                let mut s = HashSet::new();
                s.insert("id".to_string());
                s
            },
            column_glob_patterns: Vec::new(),
            tables_with_permit: HashSet::new(),
            denied_by_policy: None,
        };

        // A plan with two "id"-like columns (both would be denied since deny is name-based).
        let plan = build_scan_plan(
            "public.joined",
            vec![
                ("id", DataType::Int32),   // from table_a — should be denied
                ("id_b", DataType::Int32), // from table_b — should NOT be denied
                ("name", DataType::Utf8),
            ],
        );

        let result = effects.apply_projection(plan).unwrap();
        let schema = result.schema();
        let col_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();

        // Current behavior: only exact "id" is stripped (name-based, not table-qualified).
        assert!(!col_names.contains(&"id"), "id stripped: {col_names:?}");
        assert!(col_names.contains(&"id_b"), "id_b kept: {col_names:?}");
        assert!(col_names.contains(&"name"), "name kept: {col_names:?}");
        let _ = ctx; // silence unused warning
    }

    #[tokio::test]
    async fn test_apply_projection_exact_uses_set_path() {
        // Exact name deny with no glob — column_glob_patterns must be empty.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_column_deny_obl("public", "events", &["ssn"])],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.events",
            vec![
                ("id", DataType::Int32),
                ("ssn", DataType::Utf8),
                ("name", DataType::Utf8),
            ],
        );

        let user_tables = collect_user_tables(&plan);
        let vars = default_vars();
        let effects = ObligationEffects::collect(&session, &user_tables, &vars, &ctx);

        // No glob patterns — must use the O(1) HashSet path.
        assert!(
            effects.column_glob_patterns.is_empty(),
            "No glob patterns expected for exact deny: {:?}",
            effects.column_glob_patterns
        );
        assert!(
            effects.column_denies.contains("ssn"),
            "ssn must be in exact deny set: {:?}",
            effects.column_denies
        );

        let (result_plan, had_effects) = apply_obligations(&session, &ctx, plan, &default_vars())
            .await
            .unwrap();
        assert!(had_effects);
        let schema = result_plan.schema();
        let col_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert!(
            !col_names.contains(&"ssn"),
            "ssn should be denied: {col_names:?}"
        );
        assert!(col_names.contains(&"id"), "id should remain: {col_names:?}");
    }

    #[tokio::test]
    async fn test_exact_deny_no_glob_overhead() {
        // Deny ["ssn"] (no *) → column_glob_patterns must be empty after collection.
        let session = make_session(
            vec![make_policy(
                "p1",
                "permit",
                1,
                vec![make_column_deny_obl("public", "events", &["ssn"])],
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.events",
            vec![("id", DataType::Int32), ("ssn", DataType::Utf8)],
        );

        let user_tables = collect_user_tables(&plan);
        let vars = default_vars();
        let effects = ObligationEffects::collect(&session, &user_tables, &vars, &ctx);

        assert!(
            effects.column_glob_patterns.is_empty(),
            "No glob overhead for exact deny: {:?}",
            effects.column_glob_patterns
        );
    }
}
