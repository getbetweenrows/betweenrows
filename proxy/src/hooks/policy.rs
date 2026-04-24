use arrow_pg::datatypes::df::encode_dataframe;
use async_trait::async_trait;
use chrono::Utc;
use datafusion::common::ScalarValue;
use datafusion::logical_expr::registry::FunctionRegistry;
use datafusion::logical_expr::{LogicalPlan, LogicalPlanBuilder, TableScan, col, lit};
use datafusion::prelude::SessionContext;
use datafusion::sql::sqlparser::ast::{
    BinaryOperator as SqlBinaryOp, Expr as SqlExpr, FunctionArg, FunctionArgExpr,
    FunctionArguments, Statement, TableFactor, Visit, Visitor,
};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;
use datafusion::sql::unparser::Unparser;
use pgwire::api::ClientInfo;
use pgwire::api::portal::Format;
use pgwire::api::results::Response;
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::{ControlFlow, Not};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::QueryHook;
use super::read_only::is_allowed_statement;
use crate::engine::BetweenRowsPostgresDialect;
use crate::entity::{
    column_anchor as column_anchor_entity, data_source, decision_function, discovered_column,
    discovered_schema, discovered_table, policy, query_audit_log,
    table_relationship as table_relationship_entity,
};
use crate::policy_match::{PolicyType, TargetEntry, expand_column_patterns};
use crate::resolution::graph::{self as resolution_graph, RelationshipEdge, RelationshipSnapshot};

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

/// A user attribute with its value and type (from attribute_definition).
#[derive(Clone, Debug)]
pub struct TypedAttribute {
    pub value: String,
    pub value_type: String, // "string", "integer", "boolean", "null"
}

/// Lightweight attribute definition metadata for default resolution at query time.
#[derive(Clone, Debug)]
pub struct AttrDefInfo {
    pub default_value: Option<String>,
    pub value_type: String,
}

/// Merge user's actual attributes with defaults from attribute definitions.
/// Missing attributes with a default_value get that value inserted.
/// Missing attributes whose default is NULL get a null sentinel (value_type="null").
pub fn resolve_user_attribute_defaults(
    user_attrs: &HashMap<String, TypedAttribute>,
    attr_defs: &HashMap<String, AttrDefInfo>,
) -> HashMap<String, TypedAttribute> {
    let mut result = user_attrs.clone();
    for (key, def) in attr_defs {
        if !result.contains_key(key) {
            match &def.default_value {
                Some(v) => {
                    result.insert(
                        key.clone(),
                        TypedAttribute {
                            value: v.clone(),
                            value_type: def.value_type.clone(),
                        },
                    );
                }
                None => {
                    result.insert(
                        key.clone(),
                        TypedAttribute {
                            value: String::new(),
                            value_type: "null".to_string(),
                        },
                    );
                }
            }
        }
    }
    result
}

#[derive(Clone)]
struct UserVars {
    username: String,
    user_id: String,
    attributes: HashMap<String, TypedAttribute>,
    /// All user-entity attribute definitions — used for default resolution when user lacks an attribute.
    attribute_defs: HashMap<String, AttrDefInfo>,
}

impl UserVars {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            // Built-in fields take priority — prevents attribute override attacks
            "user.username" => Some(&self.username),
            "user.id" => Some(&self.user_id),
            _ => {
                let attr_key = key.strip_prefix("user.")?;
                self.attributes.get(attr_key).map(|a| a.value.as_str())
            }
        }
    }

    #[cfg(test)]
    fn get_type(&self, key: &str) -> &str {
        match key {
            "user.username" | "user.id" => "string",
            _ => {
                if let Some(attr_key) = key.strip_prefix("user.") {
                    self.attributes
                        .get(attr_key)
                        .map(|a| a.value_type.as_str())
                        .unwrap_or("string")
                } else {
                    "string"
                }
            }
        }
    }
}

/// Regex for `{user.KEY}` patterns. Compiled once.
fn user_var_regex() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\{user\.(\w+)\}").unwrap())
}

/// Variable mapping entry carrying both value and type for typed literal production.
#[derive(Debug)]
struct VarMapping {
    placeholder: String,
    value: String,
    value_type: String,
}

/// Replace `{user.X}` placeholders with safe identifier placeholders.
/// Returns the mangled expression and typed mappings, or an error if an undefined attribute is referenced.
///
/// Uses `resolve_user_attribute_defaults` to merge user attributes with definition defaults
/// before substitution. Missing attributes with a default produce typed literals; missing
/// attributes with a NULL default produce SQL NULL; references to undefined attributes error.
fn mangle_vars(template: &str, vars: &UserVars) -> Result<(String, Vec<VarMapping>), String> {
    let mut result = template.to_string();
    let mut mappings = Vec::new();

    // Resolve attributes with defaults applied
    let resolved = if vars.attribute_defs.is_empty() {
        // Save-time validation mode: no defs loaded, use raw attributes only
        vars.attributes.clone()
    } else {
        resolve_user_attribute_defaults(&vars.attributes, &vars.attribute_defs)
    };

    // Built-in keys first (stable placeholders)
    for key in ["user.username", "user.id"] {
        let placeholder = format!("__br_{}__", key.replace('.', "_"));
        let needle = format!("{{{}}}", key);
        if result.contains(&needle) {
            let value = vars.get(key).unwrap_or("").to_string();
            result = result.replace(&needle, &placeholder);
            mappings.push(VarMapping {
                placeholder: placeholder.to_lowercase(),
                value,
                value_type: "string".to_string(),
            });
        }
    }

    // Dynamic attribute keys: scan for remaining {user.WORD} patterns
    let re = user_var_regex();
    let captures: Vec<(String, String)> = re
        .captures_iter(&result)
        .map(|cap| (cap[0].to_string(), cap[1].to_string()))
        .collect();
    for (needle, attr_key) in captures {
        // Look up resolved value (user's actual value, default, or null sentinel)
        let (value, value_type) = match resolved.get(&attr_key) {
            Some(ta) => (ta.value.clone(), ta.value_type.clone()),
            None => {
                if vars.attribute_defs.is_empty() {
                    // Save-time validation: fall back to empty string
                    (String::new(), "string".to_string())
                } else {
                    return Err(format!(
                        "Policy references undefined attribute '{}'",
                        attr_key
                    ));
                }
            }
        };

        if value_type == "list" {
            // List type: expand into multiple comma-separated placeholders
            let elements: Vec<String> = serde_json::from_str(&value).unwrap_or_default();
            if elements.is_empty() {
                // Empty list → single NULL placeholder
                let ph = format!("__br_user_{}_0__", attr_key);
                result = result.replace(&needle, &ph);
                mappings.push(VarMapping {
                    placeholder: ph.to_lowercase(),
                    value: String::new(),
                    value_type: "null".to_string(),
                });
            } else {
                let phs: Vec<String> = elements
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("__br_user_{}_{i}__", attr_key))
                    .collect();
                result = result.replace(&needle, &phs.join(", "));
                for (i, elem) in elements.iter().enumerate() {
                    mappings.push(VarMapping {
                        placeholder: format!("__br_user_{}_{i}__", attr_key).to_lowercase(),
                        value: elem.clone(),
                        value_type: "string".to_string(),
                    });
                }
            }
        } else if value_type == "null" {
            // Null default → SQL NULL literal
            let placeholder = format!("__br_user_{}__", attr_key);
            result = result.replace(&needle, &placeholder);
            mappings.push(VarMapping {
                placeholder: placeholder.to_lowercase(),
                value: String::new(),
                value_type: "null".to_string(),
            });
        } else {
            let placeholder = format!("__br_user_{}__", attr_key);
            result = result.replace(&needle, &placeholder);
            mappings.push(VarMapping {
                placeholder: placeholder.to_lowercase(),
                value,
                value_type,
            });
        }
    }

    Ok((result, mappings))
}

/// Produce a typed DataFusion literal from a string value and value_type.
fn typed_lit(value: &str, value_type: &str) -> datafusion::logical_expr::Expr {
    match value_type {
        "null" => lit(ScalarValue::Null),
        "integer" => {
            if let Ok(n) = value.parse::<i64>() {
                lit(ScalarValue::Int64(Some(n)))
            } else {
                lit(value) // fallback to string if parse fails
            }
        }
        "boolean" => {
            if let Ok(b) = value.parse::<bool>() {
                lit(ScalarValue::Boolean(Some(b)))
            } else {
                lit(value)
            }
        }
        _ => lit(value), // "string" and unknown types
    }
}

/// Convert a sqlparser AST expression to a DataFusion Expr.
///
/// This is a custom converter for expression fragments (filter/mask templates),
/// not full SQL statements. It handles a subset of SQL syntax:
///
/// **Supported:** Identifier, CompoundIdentifier, Value (number/string/bool/null),
/// BinaryOp (+, -, *, /, =, !=, <, >, <=, >=, AND, OR, ||), UnaryOp (NOT, -),
/// IsNull, IsNotNull, Nested (parentheses), Between, Like, InList, Cast,
/// Function (via registry), Case (CASE WHEN ... THEN ... ELSE ... END).
///
/// **Not yet supported (add as needed):**
/// - ILike (case-insensitive LIKE)
/// - IsTrue / IsFalse / IsNotTrue / IsNotFalse
/// - IsDistinctFrom / IsNotDistinctFrom
/// - InSubquery (subquery in IN clause)
/// - Extract (EXTRACT(field FROM expr))
/// - Substring, Trim, Overlay, Position (SQL string functions — use UDF registry instead)
/// - Exists / Subquery (correlated subqueries)
/// - Array, Struct, Map literals
/// - JsonAccess (-> / ->> operators)
/// - AtTimeZone
/// - TypedString (e.g., DATE '2024-01-01')
/// - Interval
///
/// Pass `Some(ctx)` as `registry` to enable full scalar function lookup (required for
/// column mask expressions). Pass `None` for row filter expressions where only
/// COALESCE is supported.
fn sql_ast_to_df_expr(
    expr: &SqlExpr,
    var_values: &[VarMapping],
    registry: Option<&dyn FunctionRegistry>,
) -> datafusion::error::Result<datafusion::logical_expr::Expr> {
    use datafusion::logical_expr::Expr;
    match expr {
        SqlExpr::Identifier(ident) => {
            let name_lc = ident.value.to_lowercase();
            if let Some(mapping) = var_values.iter().find(|m| m.placeholder == name_lc) {
                Ok(typed_lit(&mapping.value, &mapping.value_type))
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
        SqlExpr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => {
            // CASE [operand] WHEN cond THEN result [ELSE else_result] END
            let operand_expr = operand
                .as_ref()
                .map(|e| sql_ast_to_df_expr(e, var_values, registry))
                .transpose()?;
            let when_then: Vec<(Box<Expr>, Box<Expr>)> = conditions
                .iter()
                .map(|cw| {
                    let cond = sql_ast_to_df_expr(&cw.condition, var_values, registry)?;
                    let result = sql_ast_to_df_expr(&cw.result, var_values, registry)?;
                    // For simple CASE (with operand), the condition is compared via Eq
                    let when = if let Some(ref op) = operand_expr {
                        Box::new(op.clone().eq(cond))
                    } else {
                        Box::new(cond)
                    };
                    Ok((when, Box::new(result)))
                })
                .collect::<datafusion::error::Result<_>>()?;
            let else_expr = else_result
                .as_ref()
                .map(|e| sql_ast_to_df_expr(e, var_values, registry).map(Box::new))
                .transpose()?;
            Ok(Expr::Case(datafusion::logical_expr::Case {
                expr: None, // conditions already include the comparison
                when_then_expr: when_then,
                else_expr,
            }))
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

    let (mangled, var_values) =
        mangle_vars(template, vars).map_err(datafusion::error::DataFusionError::Plan)?;

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
    let (mangled, var_values) =
        mangle_vars(mask_template, vars).map_err(datafusion::error::DataFusionError::Plan)?;
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

// ---------- expression validation (used at policy save time) ----------

/// Validate that a filter_expression or mask_expression can be parsed successfully.
/// Called at policy create/update time to reject unsupported syntax early,
/// instead of failing silently at query time.
///
/// Uses dummy user variables so the parse succeeds regardless of actual user values.
/// The goal is to catch unsupported SQL syntax, not to evaluate the expression.
pub fn validate_expression(expression: &str, is_mask: bool) -> Result<(), String> {
    let dummy_vars = UserVars {
        username: "__validate__".to_string(),
        user_id: "__validate__".to_string(),
        attributes: HashMap::new(),
        attribute_defs: HashMap::new(), // empty → save-time validation skips default resolution
    };

    if is_mask {
        // Mask expressions need a function registry for UDF lookup.
        // Use a bare SessionContext which has all built-in functions registered.
        let ctx = SessionContext::new();
        parse_mask_expr(&ctx, "dummy_col", expression, &dummy_vars)
            .map(|_| ())
            .map_err(|e| format!("Invalid mask expression: {e}"))
    } else {
        parse_filter_expr(expression, &dummy_vars)
            .map(|_| ())
            .map_err(|e| format!("Invalid filter expression: {e}"))
    }
}

/// Parse a row-filter expression with dummy user vars and return the resulting
/// Expr. Used by admin-side dry-runs (e.g. anchor-coverage) that need to walk
/// the AST for column references without evaluating against a real user.
pub(crate) fn parse_filter_expr_for_admin(
    expression: &str,
) -> datafusion::error::Result<datafusion::logical_expr::Expr> {
    let dummy_vars = UserVars {
        username: "__admin__".to_string(),
        user_id: "__admin__".to_string(),
        attributes: HashMap::new(),
        attribute_defs: HashMap::new(),
    };
    parse_filter_expr(expression, &dummy_vars)
}

// ---------- resolved policy data structures ----------

#[derive(Clone)]
struct ResolvedDecisionFunction {
    #[allow(dead_code)]
    id: Uuid,
    decision_wasm: Option<Vec<u8>>,
    decision_config: Option<serde_json::Value>,
    evaluate_context: String,
    on_error: String,
    log_level: String,
    is_enabled: bool,
}

#[derive(Clone)]
struct ResolvedPolicy {
    id: Uuid,
    name: String,
    policy_type: PolicyType,
    version: i32,
    priority: i32,
    targets: Vec<TargetEntry>,
    /// Parsed definition JSON (filter_expression or mask_expression). Null for non-expression types.
    definition: Option<serde_json::Value>,
    /// Decision function (loaded from decision_function table via FK).
    decision_function: Option<ResolvedDecisionFunction>,
}

struct SessionData {
    permit_policies: Vec<ResolvedPolicy>,
    deny_policies: Vec<ResolvedPolicy>,
    access_mode: String,
    /// DataFusion schema alias → upstream schema name
    df_to_upstream: HashMap<String, String>,
    datasource_id: Uuid,
    datasource_name: String,
    /// Role names for the current user (resolved via role_resolver).
    roles: Vec<String>,
    /// User attributes with types (from attribute_definition).
    user_attributes: HashMap<String, TypedAttribute>,
    /// All user-entity attribute definitions for default resolution.
    attribute_defs: HashMap<String, AttrDefInfo>,
    /// Relationships + column anchors for this datasource, used by the
    /// row-filter rewriter to resolve columns that live on a parent table.
    /// Empty snapshot when no admin has configured anchors.
    relationship_snapshot: Arc<RelationshipSnapshot>,
    /// Cache of parent-table `LogicalPlan`s materialized for anchor
    /// resolution. Populated lazily on first query by
    /// `precompute_parent_scans` and reused across queries for this
    /// `SessionData`'s lifetime (60s, or until `invalidate_datasource`
    /// drops the entry). Keyed by `(df_schema, table)`. Shared with
    /// `SessionDataClone` via `Arc::clone` so query-side population
    /// benefits subsequent queries hitting the same cache entry.
    parent_scans_cache: Arc<tokio::sync::RwLock<HashMap<(String, String), LogicalPlan>>>,
    loaded_at: std::time::Instant,
}

const CACHE_TTL_SECS: u64 = 60;

// ---------- PolicyHook ----------

pub struct PolicyHook {
    db: DatabaseConnection,
    cache: Arc<RwLock<HashMap<(Uuid, String), SessionData>>>,
    /// Shared WASM runtime for evaluating decision functions at query time.
    wasm_runtime: Arc<crate::decision::wasm::WasmDecisionRuntime>,
}

impl PolicyHook {
    pub fn new(
        db: DatabaseConnection,
        wasm_runtime: Arc<crate::decision::wasm::WasmDecisionRuntime>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            cache: Arc::new(RwLock::new(HashMap::new())),
            wasm_runtime,
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

    /// Best-effort audit write for a statement that will be rejected by `ReadOnlyHook`.
    /// Skips silently if user context is missing or the session can't be loaded.
    async fn audit_write_rejected(&self, statement: &Statement, client: &(dyn ClientInfo + Sync)) {
        let metadata = client.metadata();
        let user_id_str = match metadata.get("user_id") {
            Some(s) => s.clone(),
            None => return,
        };
        let user_id = match Uuid::parse_str(&user_id_str) {
            Ok(id) => id,
            Err(_) => return,
        };
        let username = metadata.get("user").cloned().unwrap_or_default();
        let datasource = metadata.get("datasource").cloned().unwrap_or_default();
        let client_ip = Some(client.socket_addr().ip().to_string());
        let client_info = metadata.get("application_name").cloned();

        let session = match self.get_session(user_id, &datasource).await {
            Ok(s) => s,
            Err(_) => return,
        };

        let db = self.db.clone();
        let original_query = statement.to_string();

        tokio::spawn(async move {
            let now = Utc::now().naive_utc();
            let entry = query_audit_log::ActiveModel {
                id: sea_orm::Set(Uuid::now_v7()),
                user_id: sea_orm::Set(user_id),
                username: sea_orm::Set(username),
                data_source_id: sea_orm::Set(session.datasource_id),
                datasource_name: sea_orm::Set(session.datasource_name),
                original_query: sea_orm::Set(original_query),
                rewritten_query: sea_orm::Set(None),
                policies_applied: sea_orm::Set("[]".to_string()),
                execution_time_ms: sea_orm::Set(None),
                client_ip: sea_orm::Set(client_ip),
                client_info: sea_orm::Set(client_info),
                created_at: sea_orm::Set(now),
                status: sea_orm::Set("denied".to_string()),
                error_message: sea_orm::Set(Some("Only read-only queries are allowed".to_string())),
            };
            if let Err(e) = sea_orm::ActiveModelTrait::insert(entry, &db).await {
                tracing::error!(error = %e, "Failed to write audit log entry for rejected write");
            }
        });
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

        // Resolve role names for the user (for decision function context)
        let role_ids = crate::role_resolver::resolve_user_roles(&self.db, user_id).await?;
        let role_names: Vec<String> = if !role_ids.is_empty() {
            use crate::entity::role;
            role::Entity::find()
                .filter(role::Column::Id.is_in(role_ids))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|r| r.name)
                .collect()
        } else {
            vec![]
        };

        // Load user attributes (from proxy_user.attributes JSON column)
        let (user_attributes, attribute_defs) = self.load_user_attributes(user_id).await?;

        // Load policy assignments for this datasource+user (user-specific, role-based, or wildcard)
        let relevant_assignments =
            crate::role_resolver::resolve_effective_assignments(&self.db, user_id, ds.id).await?;

        let policy_ids: Vec<Uuid> = relevant_assignments
            .iter()
            .map(|a| a.policy_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // Build priority map: policy_id → min priority (already deduplicated by resolve_effective_assignments)
        let mut policy_priority: HashMap<Uuid, i32> = HashMap::new();
        for a in &relevant_assignments {
            let entry = policy_priority.entry(a.policy_id).or_insert(a.priority);
            if a.priority < *entry {
                *entry = a.priority;
            }
        }

        let relationship_snapshot = Arc::new(load_relationship_snapshot(&self.db, ds.id).await?);

        if policy_ids.is_empty() {
            return Ok(SessionData {
                permit_policies: vec![],
                deny_policies: vec![],
                access_mode: ds.access_mode.clone(),
                df_to_upstream,
                datasource_id: ds.id,
                datasource_name: ds.name.clone(),
                roles: role_names,
                user_attributes,
                attribute_defs,
                relationship_snapshot,
                parent_scans_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                loaded_at: std::time::Instant::now(),
            });
        }

        // Load policies (enabled only)
        let policies = policy::Entity::find()
            .filter(policy::Column::Id.is_in(policy_ids.clone()))
            .filter(policy::Column::IsEnabled.eq(true))
            .all(&self.db)
            .await?;

        // Batch-load decision functions referenced by these policies
        let df_ids: Vec<Uuid> = policies
            .iter()
            .filter_map(|p| p.decision_function_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let df_map: HashMap<Uuid, decision_function::Model> = if !df_ids.is_empty() {
            decision_function::Entity::find()
                .filter(decision_function::Column::Id.is_in(df_ids))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|df| (df.id, df))
                .collect()
        } else {
            HashMap::new()
        };

        let mut permit_policies = Vec::new();
        let mut deny_policies = Vec::new();

        for p in policies {
            let policy_type = match p.policy_type.parse::<PolicyType>() {
                Ok(pt) => pt,
                Err(e) => {
                    tracing::warn!(policy = %p.name, error = %e, "Skipping policy with unknown type");
                    continue;
                }
            };
            let targets: Vec<TargetEntry> = serde_json::from_str(&p.targets).unwrap_or_default();
            let definition: Option<serde_json::Value> = p
                .definition
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok());

            // Resolve decision function from FK
            let decision_function = p.decision_function_id.and_then(|df_id| {
                df_map.get(&df_id).map(|df| {
                    let decision_config: Option<serde_json::Value> = df
                        .decision_config
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok());
                    ResolvedDecisionFunction {
                        id: df.id,
                        decision_wasm: df.decision_wasm.clone(),
                        decision_config,
                        evaluate_context: df.evaluate_context.clone(),
                        on_error: df.on_error.clone(),
                        log_level: df.log_level.clone(),
                        is_enabled: df.is_enabled,
                    }
                })
            });

            let priority = policy_priority.get(&p.id).copied().unwrap_or(100);
            let resolved = ResolvedPolicy {
                id: p.id,
                name: p.name.clone(),
                policy_type,
                version: p.version,
                priority,
                targets,
                definition,
                decision_function,
            };
            if policy_type.is_deny() {
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
            roles: role_names,
            user_attributes,
            attribute_defs,
            relationship_snapshot,
            parent_scans_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            loaded_at: std::time::Instant::now(),
        })
    }

    /// Load user attributes from the proxy_user model and pair with types
    /// from attribute_definition. Also loads ALL user-entity attribute definitions
    /// for default resolution at query time via `resolve_user_attribute_defaults`.
    async fn load_user_attributes(
        &self,
        user_id: Uuid,
    ) -> Result<
        (
            HashMap<String, TypedAttribute>,
            HashMap<String, AttrDefInfo>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        use crate::entity::{attribute_definition, proxy_user};

        let user = proxy_user::Entity::find_by_id(user_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| format!("User {user_id} not found"))?;

        let raw_attrs = proxy_user::parse_attributes(&user.attributes);

        // Load ALL user-type attribute definitions (not just the user's keys)
        // so we have default_value info for attributes the user does NOT have.
        let defs = attribute_definition::Entity::find()
            .filter(attribute_definition::Column::EntityType.eq("user"))
            .all(&self.db)
            .await?;

        let def_map: HashMap<&str, &str> = defs
            .iter()
            .map(|d| (d.key.as_str(), d.value_type.as_str()))
            .collect();

        // Build attribute_defs map from all definitions
        let attr_defs: HashMap<String, AttrDefInfo> = defs
            .iter()
            .map(|d| {
                (
                    d.key.clone(),
                    AttrDefInfo {
                        default_value: d.default_value.clone(),
                        value_type: d.value_type.clone(),
                    },
                )
            })
            .collect();

        let mut result = HashMap::new();
        for (key, value) in raw_attrs {
            let value_type = def_map.get(key.as_str()).unwrap_or(&"string");
            // Convert serde_json::Value to string representation for TypedAttribute.
            // For list type, serialize back to JSON array string.
            // For scalar types, extract the raw string value.
            let str_value = match *value_type {
                "list" => serde_json::to_string(&value).unwrap_or_else(|_| "[]".to_string()),
                _ => match &value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                },
            };
            result.insert(
                key,
                TypedAttribute {
                    value: str_value,
                    value_type: value_type.to_string(),
                },
            );
        }
        Ok((result, attr_defs))
    }
}

/// Load all relationships, anchors, and per-table column lists for a datasource
/// into an in-memory snapshot. Called once per `load_session`; read-only on
/// the hot path afterward.
///
/// The snapshot keys schemas by their DataFusion alias (`schema_alias` when
/// set, else raw `schema_name`) because that's what `scan_policy_key`
/// produces during query rewriting. The alias is derived directly from
/// `discovered_schema` rows — no external mapping needed.
pub(crate) async fn load_relationship_snapshot(
    db: &DatabaseConnection,
    datasource_id: Uuid,
) -> Result<RelationshipSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    // Pull schemas + tables + columns scoped to this datasource so the
    // snapshot can expose `(df_schema, table)` keys directly.
    let schemas = discovered_schema::Entity::find()
        .filter(discovered_schema::Column::DataSourceId.eq(datasource_id))
        .all(db)
        .await?;
    let schema_id_to_df_alias: HashMap<Uuid, String> = schemas
        .iter()
        .map(|s| {
            let alias = s
                .schema_alias
                .as_deref()
                .unwrap_or(&s.schema_name)
                .to_string();
            (s.id, alias)
        })
        .collect();

    if schema_id_to_df_alias.is_empty() {
        return Ok(RelationshipSnapshot::default());
    }

    let schema_ids: Vec<Uuid> = schema_id_to_df_alias.keys().copied().collect();
    let tables = discovered_table::Entity::find()
        .filter(discovered_table::Column::DiscoveredSchemaId.is_in(schema_ids.clone()))
        .all(db)
        .await?;

    // Map table id → (df_schema, table_name) for easy lookup below.
    let mut table_id_to_key: HashMap<Uuid, (String, String)> = HashMap::new();
    for t in &tables {
        if let Some(df_schema) = schema_id_to_df_alias.get(&t.discovered_schema_id) {
            table_id_to_key.insert(t.id, (df_schema.clone(), t.table_name.clone()));
        }
    }

    let table_ids: Vec<Uuid> = table_id_to_key.keys().copied().collect();

    // Column lists per table for the rewriter's "is column on this table?"
    // test.
    let mut columns_by_table: HashMap<(String, String), HashSet<String>> = HashMap::new();
    if !table_ids.is_empty() {
        let cols = discovered_column::Entity::find()
            .filter(discovered_column::Column::DiscoveredTableId.is_in(table_ids.clone()))
            .all(db)
            .await?;
        for c in cols {
            if let Some(key) = table_id_to_key.get(&c.discovered_table_id) {
                columns_by_table
                    .entry(key.clone())
                    .or_default()
                    .insert(c.column_name);
            }
        }
    }

    // Relationships — both endpoints must be in this datasource (always true
    // since the schema cascade-scoped us here, but guard against orphans).
    let relationships_rows = table_relationship_entity::Entity::find()
        .filter(table_relationship_entity::Column::DataSourceId.eq(datasource_id))
        .all(db)
        .await?;
    let mut relationships: HashMap<Uuid, RelationshipEdge> = HashMap::new();
    for r in relationships_rows {
        let (child_schema, child_table) = match table_id_to_key.get(&r.child_table_id) {
            Some(v) => v.clone(),
            None => continue,
        };
        let (parent_schema, parent_table) = match table_id_to_key.get(&r.parent_table_id) {
            Some(v) => v.clone(),
            None => continue,
        };
        relationships.insert(
            r.id,
            RelationshipEdge {
                id: r.id,
                child_schema,
                child_table,
                child_column: r.child_column_name,
                parent_schema,
                parent_table,
                parent_column: r.parent_column_name,
            },
        );
    }

    // Column anchors: each row is exactly one of an FK-walk (relationship_id
    // set) or a same-table alias (actual_column_name set). XOR is enforced at
    // the API layer on create — here we defensively skip rows that violate it
    // so a corrupt DB can't destabilize the session build.
    let anchor_rows = column_anchor_entity::Entity::find()
        .filter(column_anchor_entity::Column::DataSourceId.eq(datasource_id))
        .all(db)
        .await?;
    let mut anchors: HashMap<(String, String, String), resolution_graph::AnchorShape> =
        HashMap::new();
    for a in anchor_rows {
        let (df_schema, table_name) = match table_id_to_key.get(&a.child_table_id) {
            Some(v) => v.clone(),
            None => continue,
        };
        let shape = match (a.relationship_id, a.actual_column_name.clone()) {
            (Some(rel_id), None) => {
                if !relationships.contains_key(&rel_id) {
                    continue;
                }
                resolution_graph::AnchorShape::Relationship(rel_id)
            }
            (None, Some(actual)) => resolution_graph::AnchorShape::Alias(actual),
            _ => {
                tracing::warn!(
                    anchor_id = %a.id,
                    has_relationship = a.relationship_id.is_some(),
                    has_actual_column = a.actual_column_name.is_some(),
                    "column_anchor row violates XOR invariant; skipping"
                );
                continue;
            }
        };
        anchors.insert((df_schema, table_name, a.resolved_column_name), shape);
    }

    Ok(RelationshipSnapshot {
        relationships,
        anchors,
        columns_by_table,
    })
}

// SessionData doesn't derive Clone, so we clone it manually.
type SessionDataRef = Box<SessionDataClone>;

struct SessionDataClone {
    permit_policies: Vec<ResolvedPolicy>,
    deny_policies: Vec<ResolvedPolicy>,
    access_mode: String,
    df_to_upstream: HashMap<String, String>,
    datasource_id: Uuid,
    datasource_name: String,
    roles: Vec<String>,
    user_attributes: HashMap<String, TypedAttribute>,
    attribute_defs: HashMap<String, AttrDefInfo>,
    relationship_snapshot: Arc<RelationshipSnapshot>,
    parent_scans_cache: Arc<tokio::sync::RwLock<HashMap<(String, String), LogicalPlan>>>,
}

fn clone_session_data(s: &SessionData) -> SessionDataRef {
    Box::new(SessionDataClone {
        permit_policies: s.permit_policies.clone(),
        deny_policies: s.deny_policies.clone(),
        access_mode: s.access_mode.clone(),
        df_to_upstream: s.df_to_upstream.clone(),
        datasource_id: s.datasource_id,
        datasource_name: s.datasource_name.clone(),
        roles: s.roles.clone(),
        user_attributes: s.user_attributes.clone(),
        attribute_defs: s.attribute_defs.clone(),
        relationship_snapshot: Arc::clone(&s.relationship_snapshot),
        parent_scans_cache: Arc::clone(&s.parent_scans_cache),
    })
}

/// Return the `(df_schema, table)` policy key for a `TableScan`.
///
/// SECURITY INVARIANT: `scan.table_name.schema()` is the parsed schema
/// segment — empty for bare references like `FROM orders`. Using `""` as the
/// policy key would let a user bypass any policy targeting `schemas:
/// ["public"]` simply by omitting the schema prefix (vector #71 in
/// `docs/security-vectors.md`). We fall back to the session's default schema,
/// which is the same value `create_session_context_from_catalog` configured
/// DataFusion with at connect time — so a bare reference resolves against
/// exactly that one schema and nothing else (`SET search_path` is blocked
/// upstream by `ReadOnlyHook`).
fn scan_policy_key(scan: &TableScan, default_schema: &str) -> (String, String) {
    let schema = scan
        .table_name
        .schema()
        .unwrap_or(default_schema)
        .to_string();
    (schema, scan.table_name.table().to_string())
}

/// Collect all user-table `(df_schema, table)` policy keys from a logical plan,
/// deduplicating consecutive repeats. System tables (`pg_catalog`,
/// `information_schema`, etc.) are filtered out.
fn collect_user_tables(plan: &LogicalPlan, default_schema: &str) -> Vec<(String, String)> {
    let mut tables = Vec::new();
    collect_tables_inner(plan, default_schema, &mut tables);
    tables.dedup();
    tables
}

fn collect_tables_inner(
    plan: &LogicalPlan,
    default_schema: &str,
    tables: &mut Vec<(String, String)>,
) {
    if let LogicalPlan::TableScan(scan) = plan {
        let (df_schema, table) = scan_policy_key(scan, default_schema);
        let is_system = SYSTEM_SCHEMAS.contains(&df_schema.as_str()) || table.starts_with("pg_");
        if !is_system {
            tables.push((df_schema, table));
        }
        return;
    }
    for input in plan.inputs() {
        collect_tables_inner(input, default_schema, tables);
    }
}

// ---------- query metadata extraction ----------

/// Extract query metadata from a logical plan for decision function evaluation.
///
/// `default_schema` is used as the fallback for bare table references (see
/// `scan_policy_key` and vector #71). `datasource_name` is the BR datasource
/// label attached to every `TableRef` so decision functions can match on the
/// full `(datasource, schema, table)` identity without parsing strings.
fn extract_query_metadata(
    plan: &LogicalPlan,
    default_schema: &str,
    datasource_name: &str,
) -> crate::decision::context::QueryMetadata {
    let mut tables = Vec::new();
    let mut join_count = 0usize;
    let mut has_aggregation = false;
    let mut has_subquery = false;
    let mut has_where = false;
    extract_metadata_inner(
        plan,
        default_schema,
        datasource_name,
        &mut tables,
        &mut join_count,
        &mut has_aggregation,
        &mut has_subquery,
        &mut has_where,
    );
    tables.dedup();

    // Columns from the top-level output schema
    let columns: Vec<String> = plan
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();

    crate::decision::context::QueryMetadata {
        tables,
        columns,
        join_count,
        has_aggregation,
        has_subquery,
        has_where,
        statement_type: "SELECT".to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_metadata_inner(
    plan: &LogicalPlan,
    default_schema: &str,
    datasource_name: &str,
    tables: &mut Vec<crate::decision::context::TableRef>,
    join_count: &mut usize,
    has_aggregation: &mut bool,
    has_subquery: &mut bool,
    has_where: &mut bool,
) {
    match plan {
        LogicalPlan::TableScan(scan) => {
            // Use `scan_policy_key` so bare references (`FROM orders`) are
            // reported to decision functions with the session's default
            // schema (`public.orders`) rather than an empty schema segment.
            // This keeps decision-function JS that inspects
            // `ctx.query.tables[*]` consistent regardless of how the user
            // qualified the reference.
            let (schema, table) = scan_policy_key(scan, default_schema);
            let is_system = SYSTEM_SCHEMAS.contains(&schema.as_str()) || table.starts_with("pg_");
            if !is_system {
                tables.push(crate::decision::context::TableRef {
                    datasource: datasource_name.to_string(),
                    schema,
                    table,
                });
            }
        }
        LogicalPlan::Join(_) => {
            *join_count += 1;
        }
        LogicalPlan::Aggregate(_) => {
            *has_aggregation = true;
        }
        LogicalPlan::Filter(_) => {
            *has_where = true;
        }
        LogicalPlan::SubqueryAlias(_) => {
            *has_subquery = true;
        }
        _ => {}
    }
    for input in plan.inputs() {
        extract_metadata_inner(
            input,
            default_schema,
            datasource_name,
            tables,
            join_count,
            has_aggregation,
            has_subquery,
            has_where,
        );
    }
}

// ---------- policy error ----------

/// Errors that can occur during policy application.
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

// ---------- policy effects pipeline ----------

/// Collected effects from all policies — separates "what to apply" from "how to apply it".
struct PolicyEffects {
    /// Session default schema, used as the fallback when `scan.table_name.schema()`
    /// is empty (bare references). Read from `session_context` at `collect` time;
    /// same value `create_session_context_from_catalog` configured DataFusion with
    /// at connect time. See vector #71.
    default_schema: String,
    /// Combined row filter per (df_schema, table): AND within a policy, AND across policies.
    row_filters: HashMap<(String, String), datafusion::logical_expr::Expr>,
    /// Raw column allow patterns per (df_schema, table). Populated by `column_allow` policies.
    /// Expanded against actual column names at TableScan injection time.
    column_allow_patterns: HashMap<(String, String), Vec<String>>,
    /// Raw column deny patterns per (df_schema, table). Expanded at TableScan injection time.
    column_deny_patterns: HashMap<(String, String), Vec<String>>,
    /// Column mask expressions keyed by (df_schema, table, column). First wins per column.
    column_masks: HashMap<(String, String, String), datafusion::logical_expr::Expr>,
    /// Tables that have at least one `column_allow` policy.
    /// `row_filter` and `column_mask` do NOT grant table access (zero-trust model).
    tables_with_permit: HashSet<(String, String)>,
    /// If set, a deny-type policy matched the query — must reject before executing.
    denied_by_policy: Option<String>,
    /// Decision function evaluation results, keyed by policy ID, for audit logging.
    decision_results: HashMap<Uuid, crate::decision::DecisionResult>,
}

/// Optional context for decision function evaluation at query time.
pub struct DecisionEvalContext<'a> {
    pub wasm_runtime: &'a crate::decision::wasm::WasmDecisionRuntime,
    pub decision_ctx: serde_json::Value,
}

/// Evaluate a policy's decision function. Returns `true` if the policy should fire.
///
/// - No decision function → always fires.
/// - `is_enabled = false` → always fires (gate disabled).
/// - `decision_wasm` is None → always fires (not compiled yet).
/// - Decision function returns `fire: true` → fires.
/// - Decision function returns `fire: false` → skip.
/// - Decision function error + `on_error = "deny"` → fires (fail-secure).
/// - Decision function error + `on_error = "skip"` → skip (fail-open).
/// - `evaluate_context = "query"` but no query context → fires (defensive fallback).
async fn evaluate_decision_fn(
    policy: &ResolvedPolicy,
    decision_eval: Option<&DecisionEvalContext<'_>>,
    decision_results: &mut HashMap<Uuid, crate::decision::DecisionResult>,
) -> bool {
    let df = match &policy.decision_function {
        Some(df) => df,
        None => return true, // No decision function → always fire
    };

    if !df.is_enabled {
        return true; // Gate disabled → always fire
    }

    let wasm_bytes = match &df.decision_wasm {
        Some(bytes) if !bytes.is_empty() => bytes,
        _ => return true, // Not compiled yet → always fire
    };

    let eval_ctx = match decision_eval {
        Some(ctx) => ctx,
        None => return true, // No eval context available (tests) → always fire
    };

    // Defensive fallback: if evaluate_context is "query" but query data is missing,
    // fire the policy (in practice, query context is always present at query time).
    // Visibility-level evaluation is handled separately by evaluate_visibility_decision_fn().
    if df.evaluate_context == "query" && eval_ctx.decision_ctx.get("query").is_none() {
        return true;
    }

    let config = df
        .decision_config
        .as_ref()
        .cloned()
        .unwrap_or(serde_json::json!({}));

    let fuel_limit = crate::decision::wasm::DEFAULT_FUEL_LIMIT;
    let log_level = df.log_level.clone();
    let log_level_outer = log_level.clone();
    let on_error = df.on_error.clone();
    let policy_name = policy.name.clone();
    let policy_id = policy.id;

    let spawn_result = eval_ctx
        .wasm_runtime
        .evaluate_bytes(
            wasm_bytes,
            &eval_ctx.decision_ctx,
            &config,
            fuel_limit,
            &log_level,
        )
        .await;

    match spawn_result {
        Ok(mut result) => {
            // Filter logs based on log_level
            if log_level_outer == "error" {
                // Keep only error-related logs (stderr), which in practice
                // means we keep all captured logs since they come from stderr
            } else if log_level_outer == "off" {
                result.logs.clear();
            }
            let fire = result.fire;
            decision_results.insert(policy_id, result);
            fire
        }
        Err(e) => {
            tracing::error!(
                policy = %policy_name,
                error = %e,
                "Decision function evaluation failed"
            );
            let fire = on_error == "deny";
            decision_results.insert(
                policy_id,
                crate::decision::DecisionResult {
                    fire,
                    logs: vec![],
                    fuel_consumed: 0,
                    time_us: 0,
                    error: Some(e.to_string()),
                },
            );
            fire
        }
    }
}

impl PolicyEffects {
    /// Collect all policy effects from the session's policies.
    ///
    /// If `decision_eval` is provided, policies with decision functions will be evaluated
    /// via WASM. If a decision function returns `fire: false`, the policy is skipped.
    /// If `decision_eval` is None, policies with decision functions are treated as if they
    /// always fire (backward-compatible behavior for tests).
    async fn collect(
        session: &SessionDataClone,
        user_tables: &[(String, String)],
        user_vars: &UserVars,
        session_context: &SessionContext,
        decision_eval: Option<&DecisionEvalContext<'_>>,
    ) -> Self {
        // Read the session's default schema from the SessionContext. This is
        // the same value `engine/mod.rs::create_session_context_from_catalog`
        // configured via `with_default_catalog_and_schema` at connect time,
        // and it's the single schema a bare reference like `FROM orders`
        // resolves against (SET search_path is blocked upstream).
        let default_schema = session_context
            .state()
            .config_options()
            .catalog
            .default_schema
            .clone();

        let mut effects = PolicyEffects {
            default_schema,
            row_filters: HashMap::new(),
            column_allow_patterns: HashMap::new(),
            column_deny_patterns: HashMap::new(),
            column_masks: HashMap::new(),
            tables_with_permit: HashSet::new(),
            denied_by_policy: None,
            decision_results: HashMap::new(),
        };

        // Check table_deny policies first (short-circuit on first match).
        'deny_check: for policy in &session.deny_policies {
            if policy.policy_type != PolicyType::TableDeny {
                continue;
            }
            // Evaluate decision function if present
            if !evaluate_decision_fn(policy, decision_eval, &mut effects.decision_results).await {
                continue;
            }
            for (df_schema, table) in user_tables {
                for entry in &policy.targets {
                    if entry.matches_table(df_schema, table, &session.df_to_upstream) {
                        effects.denied_by_policy = Some(policy.name.clone());
                        break 'deny_check;
                    }
                }
            }
        }

        // Collect column_deny patterns from deny policies (ColumnDeny).
        for policy in &session.deny_policies {
            if policy.policy_type != PolicyType::ColumnDeny {
                continue;
            }
            // Evaluate decision function if present
            if !evaluate_decision_fn(policy, decision_eval, &mut effects.decision_results).await {
                continue;
            }
            for (df_schema, table) in user_tables {
                for entry in &policy.targets {
                    if entry.matches_table(df_schema, table, &session.df_to_upstream) {
                        let key = (df_schema.clone(), table.clone());
                        if let Some(cols) = &entry.columns {
                            effects
                                .column_deny_patterns
                                .entry(key)
                                .or_default()
                                .extend(cols.iter().cloned());
                        }
                    }
                }
            }
        }

        // Collect permit policy effects.
        for policy in &session.permit_policies {
            // Evaluate decision function if present
            if !evaluate_decision_fn(policy, decision_eval, &mut effects.decision_results).await {
                continue;
            }

            let mut policy_table_filters: HashMap<
                (String, String),
                datafusion::logical_expr::Expr,
            > = HashMap::new();

            match policy.policy_type {
                PolicyType::RowFilter => {
                    let filter_expr = policy
                        .definition
                        .as_ref()
                        .and_then(|d| d.get("filter_expression"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if filter_expr.is_empty() {
                        continue;
                    }
                    for (df_schema, table) in user_tables {
                        for entry in &policy.targets {
                            if entry.matches_table(df_schema, table, &session.df_to_upstream) {
                                let key = (df_schema.clone(), table.clone());
                                // row_filter does NOT grant table access (zero-trust model).
                                match parse_filter_expr(filter_expr, user_vars) {
                                    Ok(filter) => {
                                        // AND within the same policy, then ANDed globally.
                                        let e = policy_table_filters
                                            .entry(key)
                                            .or_insert_with(|| lit(true));
                                        *e = e.clone().and(filter);
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            error = %e,
                                            policy = %policy.name,
                                            "Failed to parse row_filter expression"
                                        );
                                    }
                                }
                                break; // one resource entry match is sufficient per table
                            }
                        }
                    }
                }
                PolicyType::ColumnMask => {
                    let mask_expr = policy
                        .definition
                        .as_ref()
                        .and_then(|d| d.get("mask_expression"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if mask_expr.is_empty() {
                        continue;
                    }
                    for (df_schema, table) in user_tables {
                        for entry in &policy.targets {
                            if entry.matches_table(df_schema, table, &session.df_to_upstream) {
                                // column_mask does NOT grant table access (zero-trust model).
                                let columns = entry.columns.as_deref().unwrap_or_default();
                                for col in columns {
                                    let triple = (df_schema.clone(), table.clone(), col.clone());
                                    // First (highest priority) mask wins.
                                    if let std::collections::hash_map::Entry::Vacant(e) =
                                        effects.column_masks.entry(triple)
                                    {
                                        match parse_mask_expr(
                                            session_context,
                                            col,
                                            mask_expr,
                                            user_vars,
                                        ) {
                                            Ok(mask) => {
                                                e.insert(mask);
                                            }
                                            Err(err) => {
                                                tracing::error!(
                                                    error = %err,
                                                    policy = %policy.name,
                                                    column = %col,
                                                    "Failed to parse column_mask expression"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                PolicyType::ColumnAllow => {
                    // column_allow grants table access and restricts visible columns.
                    for (df_schema, table) in user_tables {
                        for entry in &policy.targets {
                            if entry.matches_table(df_schema, table, &session.df_to_upstream) {
                                let key = (df_schema.clone(), table.clone());
                                effects.tables_with_permit.insert(key.clone());
                                if let Some(cols) = &entry.columns {
                                    effects
                                        .column_allow_patterns
                                        .entry(key)
                                        .or_default()
                                        .extend(cols.iter().cloned());
                                }
                            }
                        }
                    }
                }
                // ColumnDeny and TableDeny are handled in the deny_policies loop above.
                PolicyType::ColumnDeny | PolicyType::TableDeny => {}
            }

            // AND this policy's per-table filters into the global row_filters map.
            for (key, filter) in policy_table_filters {
                let entry = effects.row_filters.entry(key).or_insert_with(|| lit(true));
                *entry = entry.clone().and(filter);
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
    ///
    /// Row filters are scoped to their source table, so they can safely reference columns
    /// that are later stripped by the top-level projection (e.g. `tenant_id` filters).
    ///
    /// When a filter references a column that isn't on the target table, the rewriter
    /// consults the admin-designated `column_anchor` registered for
    /// `(child_table, resolved_column)`. Two anchor shapes are handled:
    ///   - **FK walk** (`AnchorShape::Relationship`): walks the `table_relationship`
    ///     chain up to a parent that carries the column, replacing the `TableScan`
    ///     subtree with `Project([target.*], Filter(rewritten, InnerJoin(target,
    ///     parent_chain)))`. Parent scans are pre-planned in `apply_policies`
    ///     (since `transform_up` is synchronous).
    ///   - **Same-table alias** (`AnchorShape::Alias`): rewrites the filter
    ///     expression's column reference (`tenant_id` → `org_id`) in place — no
    ///     join, no parent scan.
    ///
    /// On resolution failure the filter becomes `lit(false)` (deny-wins) and a
    /// structured `column_resolution_unresolved` warn log is emitted.
    fn apply_row_filters(
        &self,
        plan: LogicalPlan,
        snapshot: &RelationshipSnapshot,
        parent_scans: &HashMap<(String, String), LogicalPlan>,
        deny_wins: &HashSet<(String, String)>,
    ) -> Result<LogicalPlan, PolicyError> {
        if self.row_filters.is_empty() {
            return Ok(plan);
        }

        use datafusion::common::tree_node::{Transformed, TreeNode};

        let result = plan.transform_up(|node| {
            let LogicalPlan::TableScan(ref scan) = node else {
                return Ok(Transformed::no(node));
            };
            let key = scan_policy_key(scan, &self.default_schema);

            let Some(filter_expr) = self.row_filters.get(&key) else {
                return Ok(Transformed::no(node));
            };

            // Resolution failed earlier for this key — substitute deny-wins.
            if deny_wins.contains(&key) {
                let plan_denied = LogicalPlanBuilder::from(node)
                    .filter(lit(false))
                    .and_then(|b| b.build())
                    .map_err(|e| datafusion::error::DataFusionError::Plan(e.to_string()))?;
                return Ok(Transformed::yes(plan_denied));
            }

            tracing::debug!(table = %scan.table_name, "PolicyHook: applying row filter");

            // Attempt the fast path (no missing columns). If column resolution
            // rewrites the plan, use the rewritten plan instead of wrapping
            // the raw scan.
            let resolution = resolution_graph::build_column_resolution_plan(
                &key.0,
                &key.1,
                node.clone(),
                filter_expr,
                snapshot,
                parent_scans,
            );
            match resolution {
                Ok(Some(r)) => Ok(Transformed::yes(r.plan)),
                Ok(None) => {
                    // Fast path: the resolver determined no anchor traversal
                    // is needed. Before wrapping the scan with the raw filter,
                    // verify every unqualified column in the filter expression
                    // actually exists on the scan's output schema. Without
                    // this check, a filter referencing a column missing from
                    // the scan would hit a DataFusion "field not found" error
                    // at plan-validation time instead of surfacing deny-wins —
                    // the 5th failure mode listed under vector 73 Defense.
                    let scan_column_names: HashSet<String> = scan
                        .projected_schema
                        .fields()
                        .iter()
                        .map(|f| f.name().to_string())
                        .collect();
                    let referenced = resolution_graph::expr_column_names(filter_expr)
                        .map_err(|e| datafusion::error::DataFusionError::Plan(e.to_string()))?;
                    if let Some(missing) = referenced
                        .into_iter()
                        .find(|name| !scan_column_names.contains(name))
                    {
                        tracing::warn!(
                            reason = "column_resolution_unresolved",
                            table = %scan.table_name,
                            column = %missing,
                            "Row filter references a column not present on the scan and not \
                             resolvable via any anchor; substituting deny-wins"
                        );
                        let plan_denied = LogicalPlanBuilder::from(node)
                            .filter(lit(false))
                            .and_then(|b| b.build())
                            .map_err(|e| datafusion::error::DataFusionError::Plan(e.to_string()))?;
                        return Ok(Transformed::yes(plan_denied));
                    }

                    let plan_with_filter = LogicalPlanBuilder::from(node)
                        .filter(filter_expr.clone())
                        .and_then(|b| b.build())
                        .map_err(|e| datafusion::error::DataFusionError::Plan(e.to_string()))?;
                    Ok(Transformed::yes(plan_with_filter))
                }
                Err(e) => {
                    tracing::warn!(
                        reason = "column_resolution_unresolved",
                        table = %scan.table_name,
                        error = %e,
                        "Row filter column resolution failed; substituting deny-wins"
                    );
                    let plan_denied = LogicalPlanBuilder::from(node)
                        .filter(lit(false))
                        .and_then(|b| b.build())
                        .map_err(|e| datafusion::error::DataFusionError::Plan(e.to_string()))?;
                    Ok(Transformed::yes(plan_denied))
                }
            }
        });

        result
            .map(|t| t.data)
            .map_err(PolicyError::PlanTransformation)
    }

    /// Apply column masks at the `TableScan` level via `transform_up`.
    ///
    /// For each `TableScan` that has masked columns, injects a `Projection` above the
    /// scan that replaces the masked column with the mask expression (aliased to the
    /// original column name). This ensures masks are applied at the source before
    /// `SubqueryAlias` or CTE nodes can change the DFSchema qualifier, which would
    /// cause the top-level `apply_projection_qualified` to miss the match.
    ///
    /// **Architectural invariant:** All column-level policies (deny, mask, and any future
    /// types) MUST be enforced at the `TableScan` level via `transform_up` to prevent
    /// CTE/subquery alias bypass. Top-level projection is defense-in-depth only.
    fn apply_column_mask_at_scan(&self, plan: LogicalPlan) -> Result<LogicalPlan, PolicyError> {
        if self.column_masks.is_empty() {
            return Ok(plan);
        }

        use datafusion::common::tree_node::{Transformed, TreeNode};
        use datafusion::logical_expr::Expr;

        let masks = &self.column_masks;
        let deny_patterns = &self.column_deny_patterns;

        let result = plan.transform_up(|node| {
            let LogicalPlan::TableScan(ref scan) = node else {
                return Ok(Transformed::no(node));
            };
            let (df_schema, table) = scan_policy_key(scan, &self.default_schema);

            // Check if any column in this table has a mask
            let has_masks = masks.keys().any(|(s, t, _)| s == &df_schema && t == &table);
            if !has_masks {
                return Ok(Transformed::no(node));
            }

            tracing::debug!(
                table = %scan.table_name,
                "PolicyHook: applying column mask at scan level"
            );

            // Build projection: pass through all columns, replacing masked ones.
            // Use alias_qualified to preserve the table qualifier on masked columns,
            // so downstream nodes (CTEs, subqueries) can still resolve them.
            // Skip mask if the column is also denied (deny beats mask).
            let schema = node.schema();
            let key = (df_schema.clone(), table.clone());
            let all_cols: Vec<&str> = schema.iter().map(|(_, f)| f.name().as_str()).collect();
            let denied_cols: HashSet<String> = deny_patterns
                .get(&key)
                .map_or_else(HashSet::new, |pats| expand_column_patterns(pats, &all_cols));

            // Pre-collect masks for this table to avoid per-column String allocations.
            let table_masks: HashMap<&str, &Expr> = masks
                .iter()
                .filter(|((s, t, _), _)| s == &df_schema && t == &table)
                .map(|((_, _, c), e)| (c.as_str(), e))
                .collect();

            let mut exprs: Vec<Expr> = Vec::new();
            let mut any_masked = false;
            for (qualifier, field) in schema.iter() {
                let col_name = field.name().as_str();
                let is_denied = denied_cols.contains(col_name);
                if !is_denied && let Some(mask_expr) = table_masks.get(col_name) {
                    exprs.push(
                        (*mask_expr)
                            .clone()
                            .alias_qualified(qualifier.cloned(), col_name),
                    );
                    any_masked = true;
                    continue;
                }
                let col_expr = match qualifier {
                    Some(tref) => Expr::Column(datafusion::common::Column::new(
                        Some(tref.clone()),
                        col_name,
                    )),
                    None => col(col_name),
                };
                exprs.push(col_expr);
            }

            // If no columns were actually masked (all overridden by deny), skip
            // the Projection entirely to keep the plan clean.
            if !any_masked {
                tracing::debug!(
                    table = %scan.table_name,
                    "PolicyHook: all masked columns overridden by deny — skipping mask Projection"
                );
                return Ok(Transformed::no(node));
            }

            let plan_with_mask = LogicalPlanBuilder::from(node)
                .project(exprs)
                .and_then(|b| b.build())
                .map_err(|e| datafusion::error::DataFusionError::Plan(e.to_string()))?;
            Ok(Transformed::yes(plan_with_mask))
        });

        result
            .map(|t| t.data)
            .map_err(PolicyError::PlanTransformation)
    }

    /// Apply column allow/deny/mask policies as a single top-level `Projection`.
    ///
    /// Uses the DFSchema's qualifier information (which table each field originated from)
    /// to scope allow/deny patterns to their source table. This eliminates JOIN column
    /// collisions: denying `email` on `customers` does not affect `orders.email`.
    ///
    /// The Projection sits above any existing SQL-planner Projections, so it does not
    /// interfere with column resolution inside those lower nodes.
    ///
    /// Deny takes priority over mask: a column that is both denied and masked is removed.
    fn apply_projection_qualified(&self, plan: LogicalPlan) -> Result<LogicalPlan, PolicyError> {
        use datafusion::logical_expr::Expr;

        // Note: column_masks are applied at scan level (apply_column_mask_at_scan)
        // and cleared before this function runs. Only allow/deny need checking here.
        if self.column_allow_patterns.is_empty() && self.column_deny_patterns.is_empty() {
            return Ok(plan);
        }

        let output_schema = plan.schema();
        let mut new_exprs: Vec<Expr> = Vec::new();

        for (qualifier, field) in output_schema.iter() {
            let col_name = field.name();

            // Resolve the DFSchema field qualifier to a `(df_schema, table)`
            // policy key. For bare references the qualifier's schema segment
            // is empty; fall back to the session's default schema — same
            // invariant `scan_policy_key` relies on. See vector #71.
            let (df_schema, table) = match qualifier {
                Some(tref) => (
                    tref.schema().unwrap_or(&self.default_schema).to_string(),
                    tref.table().to_string(),
                ),
                None => (String::new(), String::new()),
            };
            let key = (df_schema.clone(), table.clone());

            // Allow check: if allow patterns exist for this table, column must be in the set.
            if let Some(allow_pats) = self.column_allow_patterns.get(&key) {
                let actual = [col_name.as_str()];
                if expand_column_patterns(allow_pats, &actual).is_empty() {
                    continue; // not in allow list → invisible
                }
            }

            // Deny check: column matches a deny pattern → skip (deny beats mask).
            if let Some(deny_pats) = self.column_deny_patterns.get(&key) {
                let actual = [col_name.as_str()];
                if !expand_column_patterns(deny_pats, &actual).is_empty() {
                    continue; // denied
                }
            }

            // Mask check: replace column expr with mask expression.
            let triple = (df_schema, table, col_name.clone());
            if let Some(mask) = self.column_masks.get(&triple) {
                new_exprs.push(mask.clone().alias(col_name));
            } else {
                // Use a qualifier-aware column reference to avoid JOIN ambiguity.
                let col_expr = match qualifier {
                    Some(tref) => Expr::Column(datafusion::common::Column::new(
                        Some(tref.clone()),
                        col_name,
                    )),
                    None => col(col_name),
                };
                new_exprs.push(col_expr);
            }
        }

        if new_exprs.is_empty() {
            let denied: Vec<String> = output_schema
                .field_names()
                .into_iter()
                .map(|n| n.to_string())
                .collect();
            return Err(PolicyError::AllColumnsDenied { columns: denied });
        }

        tracing::debug!(
            visible = new_exprs.len(),
            "PolicyHook: applying column projection"
        );

        LogicalPlanBuilder::from(plan)
            .project(new_exprs)
            .and_then(|b| b.build())
            .map_err(|e| {
                PolicyError::PlanTransformation(datafusion::error::DataFusionError::Plan(
                    e.to_string(),
                ))
            })
    }

    /// True if any row filters, column patterns, or column masks were collected.
    fn has_effects(&self) -> bool {
        !self.row_filters.is_empty()
            || !self.column_allow_patterns.is_empty()
            || !self.column_deny_patterns.is_empty()
            || !self.column_masks.is_empty()
    }
}

/// Apply all policy effects to a logical plan.
///
/// Returns `(modified_plan, had_effects)` where `had_effects` is true when any
/// row filter, column mask, or column deny was applied (used to decide whether to
/// mark the query as "policy-rewritten" in the audit log).
///
/// This function is the testable core extracted from `PolicyHook::handle_query`.
/// Tests construct a `SessionDataClone` and a `LogicalPlan` directly and call this.
///
/// **Enforcement order:**
/// 1. `apply_column_mask_at_scan` — mask `Projection` injected above each `TableScan` (scan level)
/// 2. `apply_row_filters` — `Filter` nodes injected below each `TableScan` (scan level)
/// 3. `apply_projection_qualified` — top-level `Projection` for allow/deny (defense-in-depth for deny)
///
/// Masks run before filters so that `transform_up` places `Filter` between `TableScan` and
/// the mask `Projection`. This ensures row filters evaluate against raw (unmasked) data.
///
/// Scan-level enforcement is required because `SubqueryAlias` and CTE nodes can change
/// the DFSchema qualifier, causing top-level-only matching to miss.
/// Precompute a `LogicalPlan` for every parent table that the row-filter
/// rewriter will need during resolution. Returns
/// `(parent_scans, deny_wins_keys)`: the scans map is consumed by the
/// synchronous `apply_row_filters`; `deny_wins_keys` collects row-filter
/// keys for which resolution errored early (missing anchor, cycle, depth)
/// so the rewriter can substitute `lit(false)` without re-running
/// resolution on the hot path.
async fn precompute_parent_scans(
    session_context: &SessionContext,
    row_filters: &HashMap<(String, String), datafusion::logical_expr::Expr>,
    snapshot: &RelationshipSnapshot,
    cache: &tokio::sync::RwLock<HashMap<(String, String), LogicalPlan>>,
) -> (
    HashMap<(String, String), LogicalPlan>,
    HashSet<(String, String)>,
) {
    let mut scans: HashMap<(String, String), LogicalPlan> = HashMap::new();
    let mut deny_wins: HashSet<(String, String)> = HashSet::new();

    if snapshot.is_empty() || row_filters.is_empty() {
        return (scans, deny_wins);
    }

    // Collect every parent (schema, table) pair touched by any filter.
    // `BTreeSet` pins iteration order so `missing` below, and the subsequent
    // planning calls, are deterministic across runs — important for stable
    // EXPLAIN output and reproducible behavior behind a load balancer.
    let mut parents_needed: BTreeSet<(String, String)> = BTreeSet::new();
    for (key, filter_expr) in row_filters {
        match snapshot.parents_needed_for(&key.0, &key.1, filter_expr) {
            Ok(set) => {
                for p in set {
                    parents_needed.insert(p);
                }
            }
            Err(e) => {
                tracing::warn!(
                    reason = "column_resolution_unresolved",
                    schema = %key.0,
                    table = %key.1,
                    error = %e,
                    "Row filter references a column that cannot be resolved; deny-wins"
                );
                deny_wins.insert(key.clone());
            }
        }
    }

    // Serve from cache where possible; only go out to the session catalog for
    // parents we haven't planned yet in this `SessionData`'s lifetime. The
    // cache lives as long as the `SessionData` entry (60s or until
    // `invalidate_datasource` drops it), so a steady-state session with a
    // stable set of row filters pays the planning cost exactly once.
    let missing: Vec<(String, String)> = {
        let cache_guard = cache.read().await;
        for p in &parents_needed {
            if let Some(plan) = cache_guard.get(p) {
                scans.insert(p.clone(), plan.clone());
            }
        }
        parents_needed
            .iter()
            .filter(|p| !scans.contains_key(*p))
            .cloned()
            .collect()
    };

    if missing.is_empty() {
        return (scans, deny_wins);
    }

    // Build LogicalPlans for cache misses via the session's virtual catalog.
    // Errors here produce a log line but not deny-wins: the downstream
    // `MissingParentScan` error from the resolver is what ultimately surfaces
    // deny-wins, keeping the "what triggered deny-wins" semantics in one place.
    let mut new_plans: HashMap<(String, String), LogicalPlan> = HashMap::new();
    for (schema, table) in missing {
        match session_context
            .table(datafusion::sql::TableReference::partial(
                schema.clone(),
                table.clone(),
            ))
            .await
        {
            Ok(df) => {
                let plan = df.into_unoptimized_plan();
                new_plans.insert((schema, table), plan);
            }
            Err(e) => {
                tracing::warn!(
                    reason = "column_resolution_unresolved",
                    failure = "parent_scan_planning",
                    schema = %schema,
                    table = %table,
                    error = %e,
                    "Failed to plan parent table for anchor resolution"
                );
            }
        }
    }

    // Publish newly built plans to the shared cache. `or_insert_with` is a
    // no-op for entries another concurrent query already populated, so
    // duplicate work (two queries racing on the same missing parent) wastes
    // planning time but not memory or correctness.
    if !new_plans.is_empty() {
        let mut cache_guard = cache.write().await;
        for (k, plan) in &new_plans {
            cache_guard.entry(k.clone()).or_insert_with(|| plan.clone());
        }
    }

    scans.extend(new_plans);
    (scans, deny_wins)
}

async fn apply_policies(
    session: &SessionDataClone,
    session_context: &SessionContext,
    logical_plan: LogicalPlan,
    user_vars: &UserVars,
    decision_eval: Option<&DecisionEvalContext<'_>>,
) -> Result<
    (
        LogicalPlan,
        bool,
        HashMap<Uuid, crate::decision::DecisionResult>,
    ),
    PolicyError,
> {
    // Read the session's default schema once — same value used in
    // `PolicyEffects::collect` and passed to the scan walker. This is
    // the single schema a bare reference resolves against.
    let default_schema = session_context
        .state()
        .config_options()
        .catalog
        .default_schema
        .clone();

    let user_tables = collect_user_tables(&logical_plan, &default_schema);

    let mut effects = PolicyEffects::collect(
        session,
        &user_tables,
        user_vars,
        session_context,
        decision_eval,
    )
    .await;

    effects.check_deny()?;
    effects.apply_access_mode(&session.access_mode, &user_tables);

    let had_effects = effects.has_effects();

    // Pre-plan parent scans needed for column resolution. We do this here
    // (not inside `apply_row_filters`, which must stay synchronous to run
    // within `transform_up`) so the rewriter has ready-to-use `LogicalPlan`s.
    let snapshot = Arc::clone(&session.relationship_snapshot);
    let (parent_scans, deny_wins) = precompute_parent_scans(
        session_context,
        &effects.row_filters,
        &snapshot,
        &session.parent_scans_cache,
    )
    .await;

    // Masks must be applied before row filters so that row filters evaluate
    // against raw (unmasked) data. With transform_up, mask runs first and
    // inserts Projection above TableScan; then row filter inserts Filter
    // directly above the same TableScan (below the mask Projection).
    // Result: TableScan → Filter(raw) → Projection(mask) — correct.
    let plan = effects.apply_column_mask_at_scan(logical_plan)?;
    let plan = effects.apply_row_filters(plan, &snapshot, &parent_scans, &deny_wins)?;
    // Clear masks after scan-level application to prevent double-masking in
    // apply_projection_qualified (the scan-level mask is the primary enforcement;
    // the top-level projection is defense-in-depth for allow/deny only).
    effects.column_masks.clear();
    let plan = effects.apply_projection_qualified(plan)?;

    Ok((plan, had_effects, effects.decision_results))
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
            // Write statements (INSERT, UPDATE, DELETE, DROP, SET, …) will be rejected
            // by ReadOnlyHook. Audit them here before passing through, so the denied
            // attempt is on the record.
            if !is_allowed_statement(statement) {
                self.audit_write_rejected(statement, client).await;
            }
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
        let username = metadata.get("user").cloned().unwrap_or_default();
        let datasource = metadata.get("datasource").cloned().unwrap_or_default();
        let client_ip = Some(client.socket_addr().ip().to_string());
        let client_info = metadata.get("application_name").cloned();

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

        let user_vars = UserVars {
            username: username.clone(),
            user_id: user_id.to_string(),
            attributes: session.user_attributes.clone(),
            attribute_defs: session.attribute_defs.clone(),
        };

        let query_start = std::time::Instant::now();
        let original_query = statement.to_string();

        // --- labeled block: returns (result, status, error_message, rewritten_query, decision_results) ---
        // This single block captures all outcome paths so the audit write is in one place.
        let (result, audit_status, audit_error, audit_rewritten, decision_results): (
            PgWireResult<Response>,
            &'static str,
            Option<String>,
            Option<String>,
            HashMap<Uuid, crate::decision::DecisionResult>,
        ) = 'query: {
            // Build logical plan
            let df_stmt =
                datafusion::sql::parser::Statement::Statement(Box::new(statement.clone()));
            let logical_plan = match session_context.state().statement_to_plan(df_stmt).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(error = %e, "PolicyHook: failed to build plan");
                    let msg = e.to_string();
                    break 'query (
                        Err(PgWireError::ApiError(Box::new(e))),
                        "error",
                        Some(msg),
                        None,
                        HashMap::new(),
                    );
                }
            };

            // Build decision evaluation context with session + query metadata.
            // Use resolve_user_attribute_defaults to include defaults for missing attrs.
            let resolved_attrs =
                resolve_user_attribute_defaults(&session.user_attributes, &session.attribute_defs);
            let json_attrs: HashMap<String, serde_json::Value> = resolved_attrs
                .iter()
                .map(|(k, ta)| {
                    let v = match ta.value_type.as_str() {
                        "null" => serde_json::Value::Null,
                        "list" => serde_json::from_str::<Vec<String>>(&ta.value)
                            .map(|arr| serde_json::json!(arr))
                            .unwrap_or_else(|_| serde_json::json!(&ta.value)),
                        "integer" => ta
                            .value
                            .parse::<i64>()
                            .map(|n| serde_json::json!(n))
                            .unwrap_or_else(|_| serde_json::json!(&ta.value)),
                        "boolean" => ta
                            .value
                            .parse::<bool>()
                            .map(|b| serde_json::json!(b))
                            .unwrap_or_else(|_| serde_json::json!(&ta.value)),
                        _ => serde_json::json!(&ta.value),
                    };
                    (k.clone(), v)
                })
                .collect();
            let session_info = crate::decision::context::SessionInfo {
                user_id,
                username: username.clone(),
                roles: session.roles.clone(),
                datasource_name: session.datasource_name.clone(),
                access_mode: session.access_mode.clone(),
                attributes: json_attrs,
            };
            // Read the session's default schema for the metadata extraction,
            // so bare references appear as `public.orders` (not `orders`) in
            // `ctx.query.tables`. Same value `apply_policies` reads below.
            let default_schema = session_context
                .state()
                .config_options()
                .catalog
                .default_schema
                .clone();
            let query_meta =
                extract_query_metadata(&logical_plan, &default_schema, &session.datasource_name);
            let decision_ctx =
                crate::decision::context::build_query_context(&session_info, &query_meta);
            let decision_eval = DecisionEvalContext {
                wasm_runtime: &self.wasm_runtime,
                decision_ctx,
            };

            let (final_plan, had_effects, decision_results) = match apply_policies(
                &session,
                session_context,
                logical_plan,
                &user_vars,
                Some(&decision_eval),
            )
            .await
            {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!(error = %e, "PolicyHook: policy error");
                    let (status, msg) = match &e {
                        PolicyError::DeniedByPolicy { policy_name } => {
                            ("denied", format!("Access denied by policy '{policy_name}'"))
                        }
                        PolicyError::AllColumnsDenied { columns } => (
                            "denied",
                            format!(
                                "Column{} {} restricted by policy",
                                if columns.len() == 1 { "" } else { "s" },
                                columns.join(", ")
                            ),
                        ),
                        PolicyError::PlanTransformation(inner) => ("error", inner.to_string()),
                    };
                    break 'query (
                        Err(e.into_pgwire_error()),
                        status,
                        Some(msg),
                        None,
                        HashMap::new(),
                    );
                }
            };

            // Unparse the rewritten plan back to SQL when policy effects were applied.
            let rewritten_query = if had_effects {
                let unparser = Unparser::new(&BetweenRowsPostgresDialect);
                match unparser.plan_to_sql(&final_plan) {
                    Ok(sql) => Some(sql.to_string()),
                    Err(_) => Some(format!("/* plan-to-sql failed */ {original_query}")),
                }
            } else {
                None
            };

            // Execute the plan.
            let df = match session_context.execute_logical_plan(final_plan).await {
                Ok(df) => df,
                Err(e) => {
                    tracing::error!(error = %e, "PolicyHook: execution failed");
                    let msg = e.to_string();
                    break 'query (
                        Err(PgWireError::ApiError(Box::new(e))),
                        "error",
                        Some(msg),
                        rewritten_query,
                        decision_results,
                    );
                }
            };

            // Encode the DataFrame into a pgwire response (this is where rows are pulled).
            let response = match encode_dataframe(df, &Format::UnifiedText, None).await {
                Ok(qr) => Response::Query(qr),
                Err(e) => {
                    tracing::error!(error = %e, "PolicyHook: encoding error");
                    let msg = e.to_string();
                    break 'query (
                        Err(e),
                        "error",
                        Some(msg),
                        rewritten_query,
                        decision_results,
                    );
                }
            };

            (
                Ok(response),
                "success",
                None,
                rewritten_query,
                decision_results,
            )
        };

        // Duration measured after the labeled block — covers planning + execution + encoding.
        let elapsed_ms = query_start.elapsed().as_millis() as i64;

        // Async audit log — runs on all paths (success, error, denied).
        // Include both permit and deny policies, plus decision function results.
        let policies_applied: Vec<serde_json::Value> = session
            .permit_policies
            .iter()
            .chain(session.deny_policies.iter())
            .map(|p| {
                let mut entry = serde_json::json!({
                    "policy_id": p.id.to_string(),
                    "version": p.version,
                    "name": p.name,
                });
                if let Some(dr) = decision_results.get(&p.id) {
                    entry["decision"] = serde_json::json!({
                        "result": {
                            "fire": dr.fire,
                            "fuel_consumed": dr.fuel_consumed,
                            "time_us": dr.time_us,
                        },
                        "logs": dr.logs,
                        "error": dr.error,
                    });
                }
                entry
            })
            .collect();

        let db = self.db.clone();
        let audit_user_id = user_id;
        let audit_username = username;
        let audit_ds_id = session.datasource_id;
        let audit_ds_name = session.datasource_name.clone();
        let audit_orig_q = original_query;
        let audit_policies = serde_json::to_string(&policies_applied).unwrap_or_default();
        let audit_ip = client_ip;
        let audit_info = client_info;
        let audit_status_owned = audit_status.to_string();

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
                status: sea_orm::Set(audit_status_owned),
                error_message: sea_orm::Set(audit_error),
            };
            if let Err(e) = sea_orm::ActiveModelTrait::insert(entry, &db).await {
                tracing::error!(error = %e, "Failed to write audit log entry");
            }
        });

        Some(result)
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
            roles: vec![],
            user_attributes: HashMap::new(),
            attribute_defs: HashMap::new(),
            relationship_snapshot: Arc::new(RelationshipSnapshot::default()),
            parent_scans_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    fn make_row_filter_policy(
        name: &str,
        priority: i32,
        schema: &str,
        table: &str,
        filter: &str,
    ) -> ResolvedPolicy {
        ResolvedPolicy {
            id: Uuid::now_v7(),
            name: name.to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority,
            targets: vec![TargetEntry {
                schemas: vec![schema.to_string()],
                tables: vec![table.to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": filter})),
            decision_function: None,
        }
    }

    fn make_column_mask_policy(
        name: &str,
        priority: i32,
        schema: &str,
        table: &str,
        column: &str,
        mask: &str,
    ) -> ResolvedPolicy {
        ResolvedPolicy {
            id: Uuid::now_v7(),
            name: name.to_string(),
            policy_type: PolicyType::ColumnMask,
            version: 1,
            priority,
            targets: vec![TargetEntry {
                schemas: vec![schema.to_string()],
                tables: vec![table.to_string()],
                columns: Some(vec![column.to_string()]),
            }],
            definition: Some(serde_json::json!({"mask_expression": mask})),
            decision_function: None,
        }
    }

    fn make_column_allow_policy(
        name: &str,
        priority: i32,
        schema: &str,
        table: &str,
        columns: &[&str],
    ) -> ResolvedPolicy {
        ResolvedPolicy {
            id: Uuid::now_v7(),
            name: name.to_string(),
            policy_type: PolicyType::ColumnAllow,
            version: 1,
            priority,
            targets: vec![TargetEntry {
                schemas: vec![schema.to_string()],
                tables: vec![table.to_string()],
                columns: Some(columns.iter().map(|c| c.to_string()).collect()),
            }],
            definition: None,
            decision_function: None,
        }
    }

    fn make_column_deny_policy(
        name: &str,
        priority: i32,
        schema: &str,
        table: &str,
        columns: &[&str],
    ) -> ResolvedPolicy {
        ResolvedPolicy {
            id: Uuid::now_v7(),
            name: name.to_string(),
            policy_type: PolicyType::ColumnDeny,
            version: 1,
            priority,
            targets: vec![TargetEntry {
                schemas: vec![schema.to_string()],
                tables: vec![table.to_string()],
                columns: Some(columns.iter().map(|c| c.to_string()).collect()),
            }],
            definition: None,
            decision_function: None,
        }
    }

    fn make_table_deny_policy(
        name: &str,
        priority: i32,
        schema: &str,
        table: &str,
    ) -> ResolvedPolicy {
        ResolvedPolicy {
            id: Uuid::now_v7(),
            name: name.to_string(),
            policy_type: PolicyType::TableDeny,
            version: 1,
            priority,
            targets: vec![TargetEntry {
                schemas: vec![schema.to_string()],
                tables: vec![table.to_string()],
                columns: None,
            }],
            definition: None,
            decision_function: None,
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
        let mut attrs = HashMap::new();
        attrs.insert(
            "tenant".to_string(),
            TypedAttribute {
                value: "acme".to_string(),
                value_type: "string".to_string(),
            },
        );
        UserVars {
            username: "alice".to_string(),
            user_id: "00000000-0000-0000-0000-000000000001".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
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
        let mut attrs = HashMap::new();
        attrs.insert(
            "tenant".to_string(),
            TypedAttribute {
                value: "acme".to_string(),
                value_type: "string".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "test-id".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
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
            username: "u".to_string(),
            user_id: "i".to_string(),
            attributes: HashMap::new(),
            attribute_defs: HashMap::new(),
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
        let mut attrs = HashMap::new();
        attrs.insert(
            "tenant".to_string(),
            TypedAttribute {
                value: "my-tenant".to_string(),
                value_type: "string".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid-1".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        let (mangled, mappings) =
            mangle_vars("org = {user.tenant} AND user = {user.username}", &vars).unwrap();
        assert!(!mangled.contains("{user.tenant}"));
        assert!(!mangled.contains("{user.username}"));
        assert_eq!(mappings.len(), 2);
    }

    #[test]
    fn test_parse_filter_and() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "tenant".to_string(),
            TypedAttribute {
                value: "acme".to_string(),
                value_type: "string".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
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

        let tables = collect_user_tables(&plan, "public");
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

        let tables = collect_user_tables(&plan, "public");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0], ("public".to_string(), "orders".to_string()));
    }

    #[test]
    fn test_collect_user_tables_bare_reference_uses_default_schema() {
        // Security invariant for vector #71: bare references must inherit the
        // session's default schema, NOT empty string. Otherwise a policy
        // targeting `schemas: ["public"]` would silently skip bare queries
        // like `SELECT * FROM orders`.
        let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int32, false)]));
        let table = Arc::new(EmptyTable::new(schema));
        let source = Arc::new(DefaultTableSource::new(table));

        let plan = LogicalPlanBuilder::scan("orders", source, None)
            .unwrap()
            .build()
            .unwrap();

        let tables = collect_user_tables(&plan, "public");
        assert_eq!(tables, vec![("public".to_string(), "orders".to_string())]);
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

        let tables = collect_user_tables(&plan, "public");
        assert!(
            tables.is_empty(),
            "information_schema should be excluded: {tables:?}"
        );
    }

    // ---------- Tier 1: plan-structure tests (apply_policies with EmptyTable) ----------

    #[tokio::test]
    async fn test_row_filter_injected_below_table_scan() {
        let session = make_session(
            vec![make_row_filter_policy(
                "p1",
                1,
                "public",
                "orders",
                "status = 'active'",
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

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        assert_plan_contains(&result_plan, "Filter");
    }

    #[tokio::test]
    async fn test_row_filters_and_within_policy() {
        // Two row_filter policies on the same table → AND'd together.
        let session = make_session(
            vec![
                make_row_filter_policy("p1", 1, "public", "orders", "status = 'active'"),
                make_row_filter_policy("p1_b", 1, "public", "orders", "amount > 0"),
            ],
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

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
                make_row_filter_policy("p1", 1, "public", "orders", "org = 'acme'"),
                make_row_filter_policy("p2", 2, "public", "orders", "org = 'globex'"),
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

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
            vec![],
            vec![make_column_deny_policy(
                "p1",
                1,
                "public",
                "customers",
                &["ssn"],
            )],
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

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
            vec![],
            vec![make_column_deny_policy(
                "p1",
                1,
                "public",
                "customers",
                &["id", "name"],
            )],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.customers",
            vec![("id", DataType::Int32), ("name", DataType::Utf8)],
        );

        let result = apply_policies(&session, &ctx, plan, &default_vars(), None).await;
        assert!(
            matches!(result, Err(PolicyError::AllColumnsDenied { .. })),
            "Expected AllColumnsDenied: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_deny_policy_row_filter_rejects() {
        // table_deny on matching table → short-circuit error.
        let session = make_session(
            vec![],
            vec![make_table_deny_policy("deny_p", 1, "public", "orders")],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan("public.orders", vec![("id", DataType::Int32)]);

        let result = apply_policies(&session, &ctx, plan, &default_vars(), None).await;
        assert!(
            matches!(result, Err(PolicyError::DeniedByPolicy { .. })),
            "Expected DeniedByPolicy: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_deny_policy_row_filter_no_match() {
        // table_deny on a DIFFERENT table → no error.
        let session = make_session(
            vec![],
            vec![make_table_deny_policy("deny_p", 1, "public", "users")],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        // Query is on "orders", deny is on "users" → should pass through
        let plan = build_scan_plan("public.orders", vec![("id", DataType::Int32)]);

        let (_, had_effects, _) = apply_policies(&session, &ctx, plan, &default_vars(), None)
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

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
        // access_mode = "policy_required" with row_filter only → lit(false) injected.
        // row_filter does not grant table access (zero-trust model); only column_access
        // "allow" grants access. Without it the table is blocked.
        let session = make_session(
            vec![make_row_filter_policy(
                "p1", 1, "public", "orders", "id > 0",
            )],
            vec![],
            "policy_required",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan("public.orders", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        let display = plan_display(&result_plan);
        // lit(false) injected because row_filter alone doesn't grant access
        assert!(
            display.contains("false"),
            "Expected lit(false) filter (row_filter alone does not grant access): {display}"
        );
    }

    #[tokio::test]
    async fn test_wildcard_schema_matches_all() {
        // Policy with schema: "*" matches any schema name.
        let session = make_session(
            vec![make_row_filter_policy("p1", 1, "*", "orders", "id > 0")],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan("any_schema.orders", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        assert_plan_contains(&result_plan, "Filter");
    }

    #[tokio::test]
    async fn test_wildcard_table_matches_all() {
        // Policy with table: "*" matches any table in the schema.
        let session = make_session(
            vec![make_row_filter_policy("p1", 1, "public", "*", "id > 0")],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan("public.anything", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        assert_plan_contains(&result_plan, "Filter");
    }

    #[tokio::test]
    async fn test_schema_alias_resolved() {
        // df schema alias "sales" maps to upstream "public"; policy targets "public".
        let mut df_to_upstream = HashMap::new();
        df_to_upstream.insert("sales".to_string(), "public".to_string());

        let session = make_session(
            vec![make_row_filter_policy(
                "p1", 1, "public", "orders", "id > 0",
            )],
            vec![],
            "open",
            df_to_upstream,
        );
        let ctx = SessionContext::new();
        // Plan uses "sales" alias, which resolves to upstream "public"
        let plan = build_scan_plan("sales.orders", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        assert_plan_contains(&result_plan, "Filter");
    }

    #[tokio::test]
    async fn test_deny_overrides_mask() {
        // Column is both denied (via deny policy) and would be masked (via permit policy);
        // deny takes priority — column is removed, mask expression never applied.
        let session = make_session(
            vec![make_column_mask_policy(
                "p1",
                1,
                "public",
                "customers",
                "ssn",
                "'***'",
            )],
            vec![make_column_deny_policy(
                "deny_p",
                2,
                "public",
                "customers",
                &["ssn"],
            )],
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

        let (result_plan, _, _) = apply_policies(&session, &ctx, plan, &default_vars(), None)
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

        let (_, had_effects, _) = apply_policies(&session, &ctx, plan, &default_vars(), None)
            .await
            .unwrap();

        assert!(!had_effects);
    }

    // ---------- Tier 2: execution tests (apply_policies with MemTable + real data) ----------

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
            vec![make_row_filter_policy(
                "p1",
                1,
                "*",
                "customers",
                "org_id = 'acme'",
            )],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
            vec![],
            vec![make_column_deny_policy("p1", 1, "*", "customers", &["ssn"])],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
        // table_deny on matching table → error returned.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![],
            vec![make_table_deny_policy("deny_p", 1, "*", "customers")],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let result = apply_policies(&session, &ctx, plan, &default_vars(), None).await;

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
        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
        // policy_required + permit with row_filter only → 0 rows (zero-trust: row_filter alone
        // doesn't grant access; only column_access "allow" does).
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![make_row_filter_policy(
                "p1",
                1,
                "*",
                "customers",
                "org_id = 'acme'",
            )],
            vec![],
            "policy_required",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, _, _) = apply_policies(&session, &ctx, plan, &default_vars(), None)
            .await
            .unwrap();

        let batches = exec_plan(&ctx, result_plan).await;
        assert_eq!(
            total_rows(&batches),
            0,
            "row_filter alone does not grant access in policy_required mode"
        );
    }

    #[tokio::test]
    async fn test_exec_two_permits_row_filter_and() {
        // Policy A: org = 'acme', Policy B: org = 'globex' → AND → 0 rows (disjoint sets).
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![
                make_row_filter_policy("p_acme", 1, "*", "customers", "org_id = 'acme'"),
                make_row_filter_policy("p_globex", 2, "*", "customers", "org_id = 'globex'"),
            ],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, _, _) = apply_policies(&session, &ctx, plan, &default_vars(), None)
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
                make_row_filter_policy("p_acme", 1, "*", "customers", "org_id = 'acme'"),
                make_row_filter_policy("p_not_charlie", 2, "*", "customers", "name != 'Charlie'"),
            ],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, _, _) = apply_policies(&session, &ctx, plan, &default_vars(), None)
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
            vec![make_column_mask_policy(
                "p1",
                1,
                "*",
                "customers",
                "ssn",
                "'REDACTED'",
            )],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
            vec![
                make_row_filter_policy("p1", 1, "*", "customers", "org_id = 'acme'"),
                make_column_mask_policy("p1_mask", 1, "*", "customers", "ssn", "'***'"),
            ],
            vec![],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
        // All columns denied by deny policy → AllColumnsDenied error.
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![],
            vec![make_column_deny_policy(
                "p1",
                1,
                "*",
                "customers",
                &["id", "org_id", "name", "ssn", "credit_card"],
            )],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let result = apply_policies(&session, &ctx, plan, &default_vars(), None).await;

        assert!(
            matches!(result, Err(PolicyError::AllColumnsDenied { .. })),
            "Expected AllColumnsDenied: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_exec_full_composition() {
        // Tenant isolation (row_filter via permit) + column hiding (credit_card deny via deny).
        let ctx = setup_customers_ctx().await;
        let session = make_session(
            vec![make_row_filter_policy(
                "tenant_policy",
                1,
                "*",
                "customers",
                "org_id = 'acme'",
            )],
            vec![make_column_deny_policy(
                "deny_cc",
                2,
                "*",
                "customers",
                &["credit_card"],
            )],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
            vec![make_column_deny_policy(
                "deny_p",
                1,
                "*",
                "customers",
                &["credit_card"],
            )],
            "open",
            HashMap::new(),
        );

        let base_plan = ctx.sql("SELECT * FROM customers").await.unwrap();
        let plan = base_plan.logical_plan().clone();
        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
            vec![],
            vec![make_column_deny_policy(
                "p1",
                1,
                "public",
                "events",
                &["*_at"],
            )],
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

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
            vec![],
            vec![make_column_deny_policy("p1", 1, "public", "events", &["*"])],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.events",
            vec![("id", DataType::Int32), ("name", DataType::Utf8)],
        );

        let result = apply_policies(&session, &ctx, plan, &default_vars(), None).await;
        assert!(
            matches!(result, Err(PolicyError::AllColumnsDenied { .. })),
            "Expected AllColumnsDenied: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_apply_projection_mask_vs_deny_priority() {
        // Column is both masked (permit) and denied via glob (deny) → deny wins (column removed).
        let session = make_session(
            vec![make_column_mask_policy(
                "p1",
                1,
                "public",
                "events",
                "secret_val",
                "'***'",
            )],
            vec![make_column_deny_policy(
                "deny_p",
                2,
                "public",
                "events",
                &["secret_*"],
            )],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.events",
            vec![("id", DataType::Int32), ("secret_val", DataType::Utf8)],
        );

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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

    /// Build a cross-join plan over two EmptyTables — for JOIN collision tests.
    fn build_join_plan(
        table_a: &str,
        cols_a: Vec<(&str, DataType)>,
        table_b: &str,
        cols_b: Vec<(&str, DataType)>,
    ) -> LogicalPlan {
        let fields_a: Vec<Field> = cols_a
            .into_iter()
            .map(|(n, dt)| Field::new(n, dt, true))
            .collect();
        let schema_a = Arc::new(Schema::new(fields_a));
        let source_a = Arc::new(DefaultTableSource::new(Arc::new(EmptyTable::new(schema_a))));
        let plan_a = LogicalPlanBuilder::scan(table_a, source_a, None)
            .unwrap()
            .build()
            .unwrap();

        let fields_b: Vec<Field> = cols_b
            .into_iter()
            .map(|(n, dt)| Field::new(n, dt, true))
            .collect();
        let schema_b = Arc::new(Schema::new(fields_b));
        let source_b = Arc::new(DefaultTableSource::new(Arc::new(EmptyTable::new(schema_b))));
        let plan_b = LogicalPlanBuilder::scan(table_b, source_b, None)
            .unwrap()
            .build()
            .unwrap();

        LogicalPlanBuilder::from(plan_a)
            .cross_join(plan_b)
            .unwrap()
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn test_apply_projection_join_collision() {
        // FIX: per-TableScan injection scopes column deny to its source table.
        // Denying "email" on customers must NOT strip "email" from orders.
        let session = make_session(
            vec![],
            vec![make_column_deny_policy(
                "p1",
                1,
                "public",
                "customers",
                &["email"],
            )],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();

        let plan = build_join_plan(
            "public.customers",
            vec![
                ("id", DataType::Int32),
                ("name", DataType::Utf8),
                ("email", DataType::Utf8),
            ],
            "public.orders",
            vec![
                ("id", DataType::Int32),
                ("email", DataType::Utf8),
                ("amount", DataType::Int32),
            ],
        );

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        let schema = result_plan.schema();
        let col_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();

        // customers.email denied → only 1 "email" remaining (from orders)
        let email_count = col_names.iter().filter(|&&n| n == "email").count();
        assert_eq!(
            email_count, 1,
            "Only orders.email should survive: {col_names:?}"
        );
        assert!(
            col_names.contains(&"name"),
            "customers.name remains: {col_names:?}"
        );
        assert!(
            col_names.contains(&"amount"),
            "orders.amount remains: {col_names:?}"
        );
    }

    #[tokio::test]
    async fn test_apply_projection_exact_uses_set_path() {
        // Exact name deny — "ssn" is denied, other columns survive.
        let session = make_session(
            vec![],
            vec![make_column_deny_policy(
                "p1",
                1,
                "public",
                "events",
                &["ssn"],
            )],
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

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
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
        // Deny ["ssn"] (no *) → stored as raw pattern in column_deny_patterns; ssn denied.
        let session = make_session(
            vec![],
            vec![make_column_deny_policy(
                "p1",
                1,
                "public",
                "events",
                &["ssn"],
            )],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.events",
            vec![("id", DataType::Int32), ("ssn", DataType::Utf8)],
        );

        let tables = collect_user_tables(&plan, "public");
        let vars = default_vars();
        let effects = PolicyEffects::collect(&session, &tables, &vars, &ctx, None).await;

        // Pattern is stored as-is; expansion happens at injection time.
        let key = ("public".to_string(), "events".to_string());
        let patterns = effects.column_deny_patterns.get(&key);
        assert!(
            patterns.is_some_and(|p| p.contains(&"ssn".to_string())),
            "ssn pattern must be in column_deny_patterns: {:?}",
            effects.column_deny_patterns
        );
    }

    // ---------- zero-trust column model tests ----------

    #[tokio::test]
    async fn test_column_access_allow_whitelist() {
        // column_access "allow" [name, email] → only those two columns visible.
        let session = make_session(
            vec![make_column_allow_policy(
                "p1",
                1,
                "public",
                "customers",
                &["name", "email"],
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
                ("email", DataType::Utf8),
                ("ssn", DataType::Utf8),
            ],
        );

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        let schema = result_plan.schema();
        let col_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(
            col_names,
            vec!["name", "email"],
            "only allowed columns: {col_names:?}"
        );
    }

    #[tokio::test]
    async fn test_row_filter_alone_zero_columns() {
        // row_filter only in policy_required → lit(false) (no column_access allow = not in
        // tables_with_permit). Table is blocked, not AllColumnsDenied.
        let session = make_session(
            vec![make_row_filter_policy(
                "p1",
                1,
                "public",
                "customers",
                "org_id = 'acme'",
            )],
            vec![],
            "policy_required",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.customers",
            vec![("id", DataType::Int32), ("name", DataType::Utf8)],
        );

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        let display = plan_display(&result_plan);
        assert!(
            display.contains("false"),
            "Expected lit(false) — row_filter alone does not grant access: {display}"
        );
    }

    #[tokio::test]
    async fn test_column_access_allow_and_row_filter() {
        // column_access "allow" + row_filter → only allowed columns, with row filter applied.
        let session = make_session(
            vec![
                make_column_allow_policy("p1", 1, "public", "customers", &["id", "name"]),
                make_row_filter_policy("p1_rf", 1, "public", "customers", "id > 0"),
            ],
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

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        let display = plan_display(&result_plan);
        // Both a Filter and a Projection must be present
        assert!(
            display.contains("Filter"),
            "Expected Filter node: {display}"
        );
        let schema = result_plan.schema();
        let col_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(
            col_names,
            vec!["id", "name"],
            "only allowed columns: {col_names:?}"
        );
    }

    #[tokio::test]
    async fn test_column_access_deny_no_table_permit() {
        // A deny policy with column_access does NOT grant table access.
        // In policy_required mode, without a permit policy the table stays blocked (lit(false)).
        let session = make_session(
            vec![],
            vec![make_column_deny_policy(
                "deny_p",
                1,
                "public",
                "customers",
                &["ssn"],
            )],
            "policy_required",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.customers",
            vec![("id", DataType::Int32), ("ssn", DataType::Utf8)],
        );

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        let display = plan_display(&result_plan);
        assert!(
            display.contains("false"),
            "Expected lit(false) — deny policy alone does not grant table access: {display}"
        );
    }

    #[tokio::test]
    async fn test_join_targeted_deny() {
        // Deny email on customers only — orders.email must survive.
        // This is the core JOIN collision regression test.
        let session = make_session(
            vec![],
            vec![make_column_deny_policy(
                "p1",
                1,
                "public",
                "customers",
                &["email"],
            )],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();

        let plan = build_join_plan(
            "public.customers",
            vec![("id", DataType::Int32), ("email", DataType::Utf8)],
            "public.orders",
            vec![
                ("id", DataType::Int32),
                ("email", DataType::Utf8),
                ("total", DataType::Int32),
            ],
        );

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        let schema = result_plan.schema();
        let col_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();

        // customers.email denied, orders.email survives → 1 "email" in output
        let email_count = col_names.iter().filter(|&&n| n == "email").count();
        assert_eq!(email_count, 1, "orders.email must survive: {col_names:?}");
        assert!(
            col_names.contains(&"total"),
            "orders.total remains: {col_names:?}"
        );
    }

    // ---------- decision function tests ----------

    fn javy_available() -> bool {
        std::process::Command::new("javy")
            .arg("--version")
            .output()
            .is_ok()
    }

    fn make_decision_function(
        wasm_bytes: Vec<u8>,
        config: Option<serde_json::Value>,
        evaluate_context: &str,
        on_error: &str,
        log_level: &str,
        is_enabled: bool,
    ) -> ResolvedDecisionFunction {
        ResolvedDecisionFunction {
            id: Uuid::now_v7(),
            decision_wasm: Some(wasm_bytes),
            decision_config: config,
            evaluate_context: evaluate_context.to_string(),
            on_error: on_error.to_string(),
            log_level: log_level.to_string(),
            is_enabled,
        }
    }

    /// Compile JS source to dynamic-mode WASM bytecode via javy CLI (blocking, for tests).
    fn compile_js_sync(js_source: &str) -> Vec<u8> {
        let tmp_dir = std::env::temp_dir();
        let unique = Uuid::now_v7();
        let input_path = tmp_dir.join(format!("br_test_{unique}.js"));
        let output_path = tmp_dir.join(format!("br_test_{unique}.wasm"));
        let plugin_path = crate::decision::wasm::plugin_file_path();

        let wrapped = format!(
            r#"var evaluate = (function() {{
    "use strict";
    {js_source}
    if (typeof evaluate !== 'function') {{
        throw new Error('Decision function must define an evaluate(ctx, config) function');
    }}
    return evaluate;
}})();

function __br_readStdin() {{
    const chunks = [];
    let total = 0;
    while (true) {{
        const buf = new Uint8Array(4096);
        const n = Javy.IO.readSync(0, buf);
        if (n === 0) break;
        chunks.push(buf.subarray(0, n));
        total += n;
    }}
    const all = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) {{ all.set(c, off); off += c.length; }}
    return all;
}}

const input = JSON.parse(new TextDecoder().decode(__br_readStdin()));
const result = evaluate(input.ctx, input.config);
if (typeof result !== 'object' || result === null || typeof result.fire !== 'boolean') {{
    throw new Error('Decision function must return {{ fire: boolean }}');
}}
Javy.IO.writeSync(1, new TextEncoder().encode(JSON.stringify(result)));
"#
        );

        std::fs::write(&input_path, &wrapped).unwrap();
        let javy = crate::decision::wasm::javy_cli_path();
        let output = std::process::Command::new(javy)
            .arg("build")
            .arg("-C")
            .arg("dynamic")
            .arg("-C")
            .arg(format!("plugin={}", plugin_path.display()))
            .arg("-o")
            .arg(&output_path)
            .arg(&input_path)
            .output()
            .unwrap();
        let _ = std::fs::remove_file(&input_path);
        assert!(
            output.status.success(),
            "javy build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let wasm = std::fs::read(&output_path).unwrap();
        let _ = std::fs::remove_file(&output_path);
        wasm
    }

    fn shared_wasm_runtime() -> &'static crate::decision::wasm::WasmDecisionRuntime {
        use std::sync::OnceLock;
        static RUNTIME: OnceLock<crate::decision::wasm::WasmDecisionRuntime> = OnceLock::new();
        RUNTIME.get_or_init(|| crate::decision::wasm::WasmDecisionRuntime::new().unwrap())
    }

    #[tokio::test]
    async fn decision_fn_fire_true_applies_policy() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        let wasm = compile_js_sync(r#"function evaluate(ctx, config) { return { fire: true }; }"#);
        let df = make_decision_function(wasm, None, "session", "deny", "off", true);

        let policy = ResolvedPolicy {
            id: Uuid::now_v7(),
            name: "filter_with_df_true".to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": "status = 'active'"})),
            decision_function: Some(df),
        };

        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        let decision_ctx = serde_json::json!({"session": {"user": {"username": "alice"}}});
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        let (result_plan, had_effects, decision_results) =
            apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval))
                .await
                .unwrap();

        assert!(had_effects, "Policy with fire:true should apply effects");
        let display = plan_display(&result_plan);
        assert!(
            display.contains("Filter"),
            "Row filter should be applied: {display}"
        );
        assert!(!decision_results.is_empty(), "Should have decision results");
        let dr = decision_results.values().next().unwrap();
        assert!(dr.fire);
    }

    #[tokio::test]
    async fn decision_fn_fire_false_skips_policy() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        let wasm = compile_js_sync(r#"function evaluate(ctx, config) { return { fire: false }; }"#);
        let df = make_decision_function(wasm, None, "session", "deny", "off", true);

        let policy = ResolvedPolicy {
            id: Uuid::now_v7(),
            name: "filter_with_df_false".to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": "status = 'active'"})),
            decision_function: Some(df),
        };

        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        let decision_ctx = serde_json::json!({"session": {"user": {"username": "alice"}}});
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        let (result_plan, had_effects, decision_results) =
            apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval))
                .await
                .unwrap();

        assert!(
            !had_effects,
            "Policy with fire:false should not apply effects"
        );
        let display = plan_display(&result_plan);
        assert!(
            !display.contains("Filter"),
            "No filter should be applied: {display}"
        );
        let dr = decision_results.values().next().unwrap();
        assert!(!dr.fire);
    }

    #[tokio::test]
    async fn no_decision_fn_always_fires() {
        // Policy without decision function → effect always applied (backward compat)
        let policy = make_row_filter_policy("no_df", 1, "public", "orders", "status = 'active'");
        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        // Pass None for decision_eval — backward compatibility
        let (_, had_effects, _) = apply_policies(&session, &ctx, plan, &default_vars(), None)
            .await
            .unwrap();
        assert!(had_effects, "Policy without decision fn should always fire");
    }

    #[tokio::test]
    async fn decision_fn_disabled_always_fires() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        let wasm = compile_js_sync(r#"function evaluate(ctx, config) { return { fire: false }; }"#);
        // is_enabled = false → function gate disabled, policy fires regardless
        let df = make_decision_function(wasm, None, "session", "deny", "off", false);

        let policy = ResolvedPolicy {
            id: Uuid::now_v7(),
            name: "disabled_df".to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": "status = 'active'"})),
            decision_function: Some(df),
        };

        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        let decision_ctx = serde_json::json!({"session": {"user": {"username": "alice"}}});
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        let (_, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval))
                .await
                .unwrap();
        assert!(
            had_effects,
            "Disabled decision fn should not gate the policy"
        );
    }

    #[tokio::test]
    async fn decision_fn_on_error_deny_fires() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        // Function that will throw a runtime error
        let wasm =
            compile_js_sync(r#"function evaluate(ctx, config) { throw new Error("boom"); }"#);
        let df = make_decision_function(wasm, None, "session", "deny", "off", true);

        let policy = ResolvedPolicy {
            id: Uuid::now_v7(),
            name: "error_deny".to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": "status = 'active'"})),
            decision_function: Some(df),
        };

        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        let decision_ctx = serde_json::json!({"session": {"user": {"username": "alice"}}});
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        let (_, had_effects, decision_results) =
            apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval))
                .await
                .unwrap();
        assert!(had_effects, "on_error=deny should fire the policy");
        let dr = decision_results.values().next().unwrap();
        assert!(dr.fire, "on_error=deny: fire should be true");
        assert!(dr.error.is_some(), "Should have error message");
    }

    #[tokio::test]
    async fn decision_fn_on_error_skip_does_not_fire() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        let wasm =
            compile_js_sync(r#"function evaluate(ctx, config) { throw new Error("boom"); }"#);
        let df = make_decision_function(wasm, None, "session", "skip", "off", true);

        let policy = ResolvedPolicy {
            id: Uuid::now_v7(),
            name: "error_skip".to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": "status = 'active'"})),
            decision_function: Some(df),
        };

        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        let decision_ctx = serde_json::json!({"session": {"user": {"username": "alice"}}});
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        let (_, had_effects, decision_results) =
            apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval))
                .await
                .unwrap();
        assert!(!had_effects, "on_error=skip should not fire the policy");
        let dr = decision_results.values().next().unwrap();
        assert!(!dr.fire, "on_error=skip: fire should be false");
        assert!(dr.error.is_some(), "Should have error message");
    }

    #[tokio::test]
    async fn decision_fn_query_context_available() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        // Function that checks query context is present
        let wasm = compile_js_sync(
            r#"function evaluate(ctx, config) {
                return {
                    fire: ctx.query !== undefined
                        && ctx.query.tables.length > 0
                        && typeof ctx.query.join_count === 'number'
                };
            }"#,
        );
        let df = make_decision_function(wasm, None, "query", "deny", "off", true);

        let policy = ResolvedPolicy {
            id: Uuid::now_v7(),
            name: "query_ctx_check".to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": "status = 'active'"})),
            decision_function: Some(df),
        };

        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        // Build query context (has query.tables, query.join_count, etc.)
        let decision_ctx = crate::decision::context::build_query_context(
            &crate::decision::context::SessionInfo {
                user_id: Uuid::nil(),
                username: "alice".to_string(),
                roles: vec!["analyst".to_string()],
                datasource_name: "test_ds".to_string(),
                access_mode: "open".to_string(),
                attributes: HashMap::new(),
            },
            &crate::decision::context::QueryMetadata {
                tables: vec![crate::decision::context::TableRef {
                    datasource: "test_ds".to_string(),
                    schema: "public".to_string(),
                    table: "orders".to_string(),
                }],
                columns: vec!["id".to_string(), "status".to_string()],
                join_count: 0,
                has_aggregation: false,
                has_subquery: false,
                has_where: false,
                statement_type: "SELECT".to_string(),
            },
        );
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        let (_, had_effects, decision_results) =
            apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval))
                .await
                .unwrap();
        assert!(
            had_effects,
            "Query-mode fn should fire when query context is available"
        );
        let dr = decision_results.values().next().unwrap();
        assert!(
            dr.fire,
            "Function should see query context and return fire:true"
        );
    }

    #[tokio::test]
    async fn decision_fn_session_context_no_query() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        // Session-mode function checks that query is NOT present
        let wasm = compile_js_sync(
            r#"function evaluate(ctx, config) {
                return { fire: ctx.session !== undefined && ctx.session.user.username === 'alice' };
            }"#,
        );
        let df = make_decision_function(wasm, None, "session", "deny", "off", true);

        let policy = ResolvedPolicy {
            id: Uuid::now_v7(),
            name: "session_ctx_check".to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": "status = 'active'"})),
            decision_function: Some(df),
        };

        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        // Build session-only context (no query field)
        let decision_ctx = crate::decision::context::build_session_context(
            &crate::decision::context::SessionInfo {
                user_id: Uuid::nil(),
                username: "alice".to_string(),
                roles: vec![],
                datasource_name: "test_ds".to_string(),
                access_mode: "open".to_string(),
                attributes: HashMap::new(),
            },
        );
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        let (_, had_effects, decision_results) =
            apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval))
                .await
                .unwrap();
        assert!(
            had_effects,
            "Session-mode fn should fire with session context"
        );
        let dr = decision_results.values().next().unwrap();
        assert!(dr.fire);
    }

    #[tokio::test]
    async fn decision_fn_with_config() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        let wasm = compile_js_sync(
            r#"function evaluate(ctx, config) {
                return { fire: config.threshold <= 10 };
            }"#,
        );
        let df = make_decision_function(
            wasm,
            Some(serde_json::json!({"threshold": 5})),
            "session",
            "deny",
            "off",
            true,
        );

        let policy = ResolvedPolicy {
            id: Uuid::now_v7(),
            name: "config_test".to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": "status = 'active'"})),
            decision_function: Some(df),
        };

        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        let decision_ctx = serde_json::json!({"session": {"user": {"username": "alice"}}});
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        let (_, had_effects, decision_results) =
            apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval))
                .await
                .unwrap();
        assert!(had_effects, "Config threshold=5 <= 10, should fire");
        let dr = decision_results.values().next().unwrap();
        assert!(dr.fire);
    }

    #[tokio::test]
    async fn decision_fn_on_deny_policy() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        // Decision fn on a table_deny policy returning fire:false → deny skipped
        let wasm = compile_js_sync(r#"function evaluate(ctx, config) { return { fire: false }; }"#);
        let df = make_decision_function(wasm, None, "session", "deny", "off", true);

        let policy = ResolvedPolicy {
            id: Uuid::now_v7(),
            name: "conditional_deny".to_string(),
            policy_type: PolicyType::TableDeny,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: None,
            decision_function: Some(df),
        };

        let session = make_session(vec![], vec![policy], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        let decision_ctx = serde_json::json!({"session": {"user": {"username": "alice"}}});
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        // Should NOT be denied because decision fn returns fire:false
        let result = apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval)).await;
        assert!(result.is_ok(), "table_deny with fire:false should not deny");
    }

    #[tokio::test]
    async fn decision_results_populated() {
        if !javy_available() {
            eprintln!("Skipping: javy CLI not available");
            return;
        }

        let wasm = compile_js_sync(r#"function evaluate(ctx, config) { return { fire: true }; }"#);
        let df = make_decision_function(wasm, None, "session", "deny", "off", true);

        let policy_id = Uuid::now_v7();
        let policy = ResolvedPolicy {
            id: policy_id,
            name: "results_check".to_string(),
            policy_type: PolicyType::RowFilter,
            version: 1,
            priority: 1,
            targets: vec![TargetEntry {
                schemas: vec!["public".to_string()],
                tables: vec!["orders".to_string()],
                columns: None,
            }],
            definition: Some(serde_json::json!({"filter_expression": "status = 'active'"})),
            decision_function: Some(df),
        };

        let session = make_session(vec![policy], vec![], "open", HashMap::new());
        let ctx = SessionContext::new();
        let plan = build_scan_plan(
            "public.orders",
            vec![("id", DataType::Int32), ("status", DataType::Utf8)],
        );

        let wasm_runtime = shared_wasm_runtime();
        let decision_ctx = serde_json::json!({"session": {"user": {"username": "alice"}}});
        let eval = DecisionEvalContext {
            wasm_runtime,
            decision_ctx,
        };

        let (_, _, decision_results) =
            apply_policies(&session, &ctx, plan, &default_vars(), Some(&eval))
                .await
                .unwrap();

        assert!(
            decision_results.contains_key(&policy_id),
            "decision_results should contain the policy ID"
        );
        let dr = &decision_results[&policy_id];
        assert!(dr.fire);
        assert!(dr.fuel_consumed > 0, "Should have consumed some fuel");
        assert!(dr.time_us > 0, "Should have nonzero execution time");
        assert!(dr.error.is_none());
    }

    // ---------- ABAC: mangle_vars with custom attributes ----------

    #[test]
    fn test_mangle_vars_custom_attribute() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "region".to_string(),
            TypedAttribute {
                value: "us-east".to_string(),
                value_type: "string".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        let (mangled, mappings) = mangle_vars("region = {user.region}", &vars).unwrap();
        assert!(
            !mangled.contains("{user.region}"),
            "placeholder should be replaced"
        );
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].value, "us-east");
        assert_eq!(mappings[0].value_type, "string");
    }

    #[test]
    fn test_mangle_vars_mixed_builtin_and_custom() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "tenant".to_string(),
            TypedAttribute {
                value: "acme".to_string(),
                value_type: "string".to_string(),
            },
        );
        attrs.insert(
            "dept".to_string(),
            TypedAttribute {
                value: "eng".to_string(),
                value_type: "string".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        let (mangled, mappings) =
            mangle_vars("tenant = {user.tenant} AND dept = {user.dept}", &vars).unwrap();
        assert!(!mangled.contains("{user.tenant}"));
        assert!(!mangled.contains("{user.dept}"));
        assert_eq!(mappings.len(), 2);
    }

    #[test]
    fn test_mangle_vars_unknown_attribute_empty() {
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: HashMap::new(),
            attribute_defs: HashMap::new(),
        };
        let (_, mappings) = mangle_vars("x = {user.nonexistent}", &vars).unwrap();
        assert_eq!(mappings.len(), 1);
        assert_eq!(
            mappings[0].value, "",
            "unknown attribute should produce empty string"
        );
    }

    // ---------- ABAC: missing attribute default resolution ----------

    #[test]
    fn test_mangle_vars_missing_attr_with_default() {
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: HashMap::new(), // user has NO attributes
            attribute_defs: HashMap::from([(
                "tenant".to_string(),
                AttrDefInfo {
                    default_value: Some("acme".to_string()),
                    value_type: "string".to_string(),
                },
            )]),
        };
        let (_, mappings) = mangle_vars("tenant = {user.tenant}", &vars).unwrap();
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].value, "acme", "should use default_value");
        assert_eq!(mappings[0].value_type, "string");
    }

    #[test]
    fn test_mangle_vars_missing_attr_null_default() {
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: HashMap::new(),
            attribute_defs: HashMap::from([(
                "tenant".to_string(),
                AttrDefInfo {
                    default_value: None, // null default
                    value_type: "string".to_string(),
                },
            )]),
        };
        let (_, mappings) = mangle_vars("tenant = {user.tenant}", &vars).unwrap();
        assert_eq!(mappings.len(), 1);
        assert_eq!(
            mappings[0].value_type, "null",
            "null default should produce NULL literal"
        );
    }

    #[test]
    fn test_mangle_vars_missing_attr_no_definition() {
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: HashMap::new(),
            attribute_defs: HashMap::from([(
                "other_key".to_string(),
                AttrDefInfo {
                    default_value: None,
                    value_type: "string".to_string(),
                },
            )]), // defs exist but NOT for "nonexistent"
        };
        let result = mangle_vars("x = {user.nonexistent}", &vars);
        assert!(result.is_err(), "should error for undefined attribute");
        assert!(
            result.unwrap_err().contains("undefined attribute"),
            "error should mention undefined attribute"
        );
    }

    #[test]
    fn test_mangle_vars_missing_attr_default_integer() {
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: HashMap::new(),
            attribute_defs: HashMap::from([(
                "clearance".to_string(),
                AttrDefInfo {
                    default_value: Some("0".to_string()),
                    value_type: "integer".to_string(),
                },
            )]),
        };
        let (_, mappings) = mangle_vars("level >= {user.clearance}", &vars).unwrap();
        assert_eq!(mappings[0].value, "0");
        assert_eq!(mappings[0].value_type, "integer");
    }

    #[test]
    fn test_mangle_vars_missing_attr_default_list() {
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: HashMap::new(),
            attribute_defs: HashMap::from([(
                "departments".to_string(),
                AttrDefInfo {
                    default_value: Some(r#"["eng"]"#.to_string()),
                    value_type: "list".to_string(),
                },
            )]),
        };
        let (mangled, mappings) = mangle_vars("dept IN {user.departments}", &vars).unwrap();
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].value, "eng");
        assert!(mangled.contains("__br_user_departments_0__"));
    }

    #[test]
    fn test_mangle_vars_present_attr_ignores_default() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "tenant".to_string(),
            TypedAttribute {
                value: "real_value".to_string(),
                value_type: "string".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::from([(
                "tenant".to_string(),
                AttrDefInfo {
                    default_value: Some("default_value".to_string()),
                    value_type: "string".to_string(),
                },
            )]),
        };
        let (_, mappings) = mangle_vars("tenant = {user.tenant}", &vars).unwrap();
        assert_eq!(
            mappings[0].value, "real_value",
            "actual attribute should take priority over default"
        );
    }

    #[test]
    fn test_resolve_user_attribute_defaults() {
        let user_attrs = HashMap::from([(
            "tenant".to_string(),
            TypedAttribute {
                value: "acme".to_string(),
                value_type: "string".to_string(),
            },
        )]);
        let attr_defs = HashMap::from([
            (
                "tenant".to_string(),
                AttrDefInfo {
                    default_value: Some("default_tenant".to_string()),
                    value_type: "string".to_string(),
                },
            ),
            (
                "clearance".to_string(),
                AttrDefInfo {
                    default_value: Some("0".to_string()),
                    value_type: "integer".to_string(),
                },
            ),
            (
                "region".to_string(),
                AttrDefInfo {
                    default_value: None,
                    value_type: "string".to_string(),
                },
            ),
        ]);
        let resolved = resolve_user_attribute_defaults(&user_attrs, &attr_defs);
        // User's actual value wins
        assert_eq!(resolved["tenant"].value, "acme");
        // Missing + default → uses default
        assert_eq!(resolved["clearance"].value, "0");
        assert_eq!(resolved["clearance"].value_type, "integer");
        // Missing + NULL default → null sentinel
        assert_eq!(resolved["region"].value_type, "null");
    }

    // ---------- ABAC: list attribute expansion ----------

    #[test]
    fn test_mangle_vars_list_attribute() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "departments".to_string(),
            TypedAttribute {
                value: r#"["eng","sec"]"#.to_string(),
                value_type: "list".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        let (mangled, mappings) = mangle_vars("dept IN {user.departments}", &vars).unwrap();
        assert_eq!(mappings.len(), 2);
        assert!(mangled.contains("__br_user_departments_0__"));
        assert!(mangled.contains("__br_user_departments_1__"));
        assert_eq!(mappings[0].value, "eng");
        assert_eq!(mappings[0].value_type, "string");
        assert_eq!(mappings[1].value, "sec");
        assert_eq!(mappings[1].value_type, "string");
    }

    #[test]
    fn test_mangle_vars_list_empty() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "departments".to_string(),
            TypedAttribute {
                value: "[]".to_string(),
                value_type: "list".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        let (mangled, mappings) = mangle_vars("dept IN {user.departments}", &vars).unwrap();
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].value_type, "null");
        assert!(mangled.contains("__br_user_departments_0__"));
    }

    #[test]
    fn test_mangle_vars_list_single_element() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "regions".to_string(),
            TypedAttribute {
                value: r#"["us-east"]"#.to_string(),
                value_type: "list".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        let (mangled, mappings) = mangle_vars("region IN {user.regions}", &vars).unwrap();
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].value, "us-east");
        assert_eq!(mappings[0].value_type, "string");
        assert!(mangled.contains("__br_user_regions_0__"));
    }

    #[test]
    fn test_typed_lit_null() {
        let expr = typed_lit("", "null");
        match expr {
            datafusion::logical_expr::Expr::Literal(sv, _) => {
                assert!(sv.is_null(), "null sentinel should produce NULL literal");
            }
            _ => panic!("expected Literal, got {expr:?}"),
        }
    }

    #[test]
    fn test_parse_filter_list_in_clause() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "departments".to_string(),
            TypedAttribute {
                value: r#"["eng","sec"]"#.to_string(),
                value_type: "list".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        let expr = parse_filter_expr("department IN ({user.departments})", &vars);
        assert!(expr.is_ok(), "list IN clause should parse: {expr:?}");
        let expr = expr.unwrap();
        let display = format!("{expr}");
        assert!(
            display.contains("department IN"),
            "should contain IN expression: {display}"
        );
    }

    #[test]
    fn test_parse_filter_list_empty_in_clause() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "departments".to_string(),
            TypedAttribute {
                value: "[]".to_string(),
                value_type: "list".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        let expr = parse_filter_expr("department IN ({user.departments})", &vars);
        assert!(expr.is_ok(), "empty list IN clause should parse: {expr:?}");
    }

    // ---------- ABAC: UserVars::get priority ----------

    #[test]
    fn test_user_vars_tenant_from_attributes() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "tenant".to_string(),
            TypedAttribute {
                value: "acme".to_string(),
                value_type: "string".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        // tenant is now a custom attribute, resolved from attributes map
        assert_eq!(vars.get("user.tenant"), Some("acme"));
    }

    #[test]
    fn test_user_vars_custom_attribute_fallback() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "region".to_string(),
            TypedAttribute {
                value: "eu-west".to_string(),
                value_type: "string".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        assert_eq!(vars.get("user.region"), Some("eu-west"));
        assert_eq!(vars.get("user.unknown"), None);
    }

    // ---------- ABAC: typed_lit ----------

    #[test]
    fn test_typed_lit_string() {
        let expr = typed_lit("hello", "string");
        let debug = format!("{expr:?}");
        assert!(
            debug.contains("Utf8"),
            "string should produce Utf8 literal: {debug}"
        );
    }

    #[test]
    fn test_typed_lit_integer() {
        let expr = typed_lit("42", "integer");
        let debug = format!("{expr:?}");
        assert!(
            debug.contains("Int64"),
            "integer should produce Int64 literal: {debug}"
        );
    }

    #[test]
    fn test_typed_lit_boolean() {
        let expr = typed_lit("true", "boolean");
        let debug = format!("{expr:?}");
        assert!(
            debug.contains("Boolean"),
            "boolean should produce Boolean literal: {debug}"
        );
    }

    #[test]
    fn test_typed_lit_integer_fallback() {
        // Invalid integer falls back to string
        let expr = typed_lit("abc", "integer");
        let debug = format!("{expr:?}");
        assert!(
            debug.contains("Utf8"),
            "bad integer should fallback to Utf8: {debug}"
        );
    }

    // ---------- ABAC: parse_filter_expr with integer attribute ----------

    #[test]
    fn test_parse_filter_integer_comparison() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "clearance".to_string(),
            TypedAttribute {
                value: "3".to_string(),
                value_type: "integer".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "uid".to_string(),
            attributes: attrs,
            attribute_defs: HashMap::new(),
        };
        let expr = parse_filter_expr("level >= {user.clearance}", &vars).unwrap();
        let debug = format!("{expr:?}");
        assert!(
            debug.contains("Int64(3)"),
            "clearance should be Int64(3) not a string: {debug}"
        );
    }

    // ---------- ABAC: CASE WHEN in expression parser ----------

    #[test]
    fn test_parse_case_when_expression() {
        let vars = default_vars();
        let expr = parse_filter_expr(
            "CASE WHEN status = 'active' THEN true ELSE false END",
            &vars,
        )
        .unwrap();
        let debug = format!("{expr:?}");
        assert!(
            debug.contains("Case"),
            "Should produce a Case expression: {debug}"
        );
    }

    // ---------- Security: builtin field override defense (Vector 67) ----------

    #[test]
    fn test_user_vars_builtin_priority_over_attributes() {
        let mut attributes = HashMap::new();
        attributes.insert(
            "tenant".to_string(),
            TypedAttribute {
                value: "from_attribute".to_string(),
                value_type: "string".to_string(),
            },
        );
        attributes.insert(
            "username".to_string(),
            TypedAttribute {
                value: "evil_user".to_string(),
                value_type: "string".to_string(),
            },
        );
        attributes.insert(
            "id".to_string(),
            TypedAttribute {
                value: "evil_id".to_string(),
                value_type: "string".to_string(),
            },
        );

        let vars = UserVars {
            username: "real_user".to_string(),
            user_id: "real_id".to_string(),
            attributes,
            attribute_defs: HashMap::new(),
        };

        // Built-in username and id must always win over attribute overrides
        assert_eq!(vars.get("user.username"), Some("real_user"));
        assert_eq!(vars.get("user.id"), Some("real_id"));

        // tenant is now a regular attribute (no longer built-in)
        assert_eq!(vars.get("user.tenant"), Some("from_attribute"));

        // Type should return the builtin type for username/id
        assert_eq!(vars.get_type("user.username"), "string");
        assert_eq!(vars.get_type("user.id"), "string");
    }

    #[test]
    fn test_filter_expr_uses_tenant_attribute() {
        let mut attributes = HashMap::new();
        attributes.insert(
            "tenant".to_string(),
            TypedAttribute {
                value: "acme".to_string(),
                value_type: "string".to_string(),
            },
        );
        let vars = UserVars {
            username: "alice".to_string(),
            user_id: "test-id".to_string(),
            attributes,
            attribute_defs: HashMap::new(),
        };
        let expr = parse_filter_expr("organization_id = {user.tenant}", &vars).unwrap();
        let debug = format!("{expr:?}");
        assert!(
            debug.contains("acme"),
            "Should use tenant from attributes: {debug}"
        );
    }

    #[tokio::test]
    async fn row_filter_deny_wins_when_column_not_on_scan_and_no_anchor() {
        // Vector 73 Defense, 5th failure mode: a filter referencing a column
        // that isn't on the scan and can't be resolved via any anchor must
        // surface deny-wins (Filter(lit(false))), not a DataFusion plan
        // error. Without this guard, the raw `Filter(expr)` would hit
        // "field 'org' not found" at plan validation.
        let session = make_session(
            vec![make_row_filter_policy(
                "p1",
                1,
                "public",
                "orders",
                "org = 'acme'",
            )],
            vec![],
            "open",
            HashMap::new(),
        );
        let ctx = SessionContext::new();
        // Scan has only "id" — the filter references "org" which isn't here
        // and has no anchor defined (empty relationship_snapshot).
        let plan = build_scan_plan("public.orders", vec![("id", DataType::Int32)]);

        let (result_plan, had_effects, _) =
            apply_policies(&session, &ctx, plan, &default_vars(), None)
                .await
                .unwrap();

        assert!(had_effects);
        let display = plan_display(&result_plan);
        assert!(
            display.contains("Boolean(false)"),
            "Expected deny-wins Filter(Boolean(false)), got: {display}"
        );
    }

    #[tokio::test]
    async fn precompute_parent_scans_serves_from_session_cache() {
        // Verify the H4 cache path: when a prior call populated
        // `parent_scans_cache` for a given (schema, table), a subsequent
        // precompute call with an empty SessionContext — which can't plan
        // any table — still returns the cached plan instead of failing.
        // This proves the cache is read before hitting the catalog.
        use crate::resolution::graph::{AnchorShape, RelationshipEdge, RelationshipSnapshot};
        use datafusion::prelude::{col, lit};

        let edge_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, b"payments-orders-h4");
        let mut relationships = HashMap::new();
        relationships.insert(
            edge_id,
            RelationshipEdge {
                id: edge_id,
                child_schema: "public".into(),
                child_table: "payments".into(),
                child_column: "order_id".into(),
                parent_schema: "public".into(),
                parent_table: "orders".into(),
                parent_column: "id".into(),
            },
        );
        let mut anchors = HashMap::new();
        anchors.insert(
            ("public".into(), "payments".into(), "org".into()),
            AnchorShape::Relationship(edge_id),
        );
        let mut columns_by_table: HashMap<(String, String), HashSet<String>> = HashMap::new();
        columns_by_table.insert(
            ("public".into(), "payments".into()),
            ["id", "order_id"].iter().map(|s| s.to_string()).collect(),
        );
        columns_by_table.insert(
            ("public".into(), "orders".into()),
            ["id", "org"].iter().map(|s| s.to_string()).collect(),
        );
        let snapshot = RelationshipSnapshot {
            relationships,
            anchors,
            columns_by_table,
        };

        // Build a placeholder "orders" plan and seed the cache with it.
        let orders_schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, true),
            Field::new("org", DataType::Utf8, true),
        ]));
        let orders_table = Arc::new(EmptyTable::new(orders_schema));
        let orders_source = Arc::new(DefaultTableSource::new(orders_table));
        let orders_plan = LogicalPlanBuilder::scan("public.orders", orders_source, None)
            .unwrap()
            .build()
            .unwrap();

        let cache = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        cache
            .write()
            .await
            .insert(("public".into(), "orders".into()), orders_plan.clone());

        let mut row_filters = HashMap::new();
        row_filters.insert(
            ("public".into(), "payments".into()),
            col("org").eq(lit("acme")),
        );

        // Empty session — it has no catalogs/tables. If the cache is
        // bypassed, the `session_context.table()` call below would fail
        // and `scans` would be empty.
        let session_context = SessionContext::new();

        let (scans, deny_wins) =
            precompute_parent_scans(&session_context, &row_filters, &snapshot, &cache).await;

        assert!(deny_wins.is_empty(), "unexpected deny_wins: {deny_wins:?}");
        assert_eq!(scans.len(), 1, "cached parent plan should be returned");
        assert!(
            scans.contains_key(&("public".to_string(), "orders".to_string())),
            "orders plan should be served from cache"
        );
    }
}
