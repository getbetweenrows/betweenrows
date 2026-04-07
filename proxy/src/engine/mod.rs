pub mod rewrite;

use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit};
use datafusion::catalog::{CatalogProvider, SchemaProvider};
use datafusion::prelude::{SessionConfig, SessionContext};
use datafusion::sql::TableReference;
use datafusion::sql::sqlparser::ast as sql_ast;
use datafusion::sql::unparser::Unparser;
use datafusion::sql::unparser::dialect::{Dialect, IntervalStyle, PostgreSqlDialect};
use datafusion_expr::Expr;
use datafusion_pg_catalog::pg_catalog::{context::PgCatalogContextProvider, setup_pg_catalog};
use datafusion_table_providers::{
    UnsupportedTypeAction,
    postgres::DynPostgresConnectionPool,
    sql::{
        db_connection_pool::postgrespool::PostgresConnectionPool, sql_provider_datafusion::SqlTable,
    },
    util::secrets::to_secret_map,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;

use crate::entity::{
    data_source, decision_function, discovered_column, discovered_schema, discovered_table, policy,
    proxy_user, role,
};

// ---------- custom dialect for JSON pushdown ----------

/// PostgreSQL dialect extended to unparse `datafusion-functions-json` UDFs
/// back to native PG JSON operators (`->`, `->>`, `?`) so that filters are
/// pushed down to the upstream PostgreSQL server as native syntax.
pub(crate) struct BetweenRowsPostgresDialect;

// All methods below except `scalar_function_to_sql_overrides` mirror the
// implementations in `PostgreSqlDialect`. If that crate's defaults change,
// sync these accordingly.
impl Dialect for BetweenRowsPostgresDialect {
    fn identifier_quote_style(&self, _: &str) -> Option<char> {
        Some('"')
    }

    fn requires_derived_table_alias(&self) -> bool {
        true
    }

    fn supports_qualify(&self) -> bool {
        false
    }

    fn supports_empty_select_list(&self) -> bool {
        true
    }

    fn interval_style(&self) -> IntervalStyle {
        IntervalStyle::PostgresVerbose
    }

    fn float64_ast_dtype(&self) -> sql_ast::DataType {
        sql_ast::DataType::DoublePrecision
    }

    fn scalar_function_to_sql_overrides(
        &self,
        unparser: &Unparser,
        func_name: &str,
        args: &[Expr],
    ) -> datafusion::common::Result<Option<sql_ast::Expr>> {
        // `JsonFunctionRewriter` flattens nested calls to variadic form, so args are:
        //   [col, key0, key1, ...]
        // We chain binary ops left-to-right: col->key0->key1->...
        // For json_as_text / json_get_str: last hop uses ->>, earlier hops use ->
        // For json_get / json_get_json:    all hops use ->
        // For json_contains:               single ? operator (one key only)
        match func_name {
            "json_as_text" | "json_get_str" => {
                if args.len() < 2 {
                    return Ok(None);
                }
                let sql_args: Vec<sql_ast::Expr> = args
                    .iter()
                    .map(|a| unparser.expr_to_sql(a))
                    .collect::<datafusion::common::Result<_>>()?;
                let mut expr = sql_args[0].clone();
                for e in &sql_args[1..sql_args.len() - 1] {
                    expr = json_binary_op(expr, "->", e.clone());
                }
                expr = json_binary_op(expr, "->>", sql_args[sql_args.len() - 1].clone());
                Ok(Some(expr))
            }
            "json_get" | "json_get_json" => {
                if args.len() < 2 {
                    return Ok(None);
                }
                let sql_args: Vec<sql_ast::Expr> = args
                    .iter()
                    .map(|a| unparser.expr_to_sql(a))
                    .collect::<datafusion::common::Result<_>>()?;
                let mut expr = sql_args[0].clone();
                for e in &sql_args[1..] {
                    expr = json_binary_op(expr, "->", e.clone());
                }
                Ok(Some(expr))
            }
            "json_contains" => {
                if args.len() != 2 {
                    return Ok(None);
                }
                let left = unparser.expr_to_sql(&args[0])?;
                let right = unparser.expr_to_sql(&args[1])?;
                // `?` is PostgreSQL's key-exists operator. `BinaryOperator::Custom`
                // prints it verbatim. Note: some JDBC/ODBC drivers treat `?` as a
                // bind-parameter placeholder — this is only used in pushdown SQL sent
                // to upstream PG over tokio-postgres, which is unaffected.
                Ok(Some(json_binary_op(left, "?", right)))
            }
            _ => {
                // Other JSON UDFs (e.g. json_length, json_keys) are not mapped here;
                // they fall back to PostgreSqlDialect which serializes them as function
                // calls. PostgreSQL won't recognise those names, so pushdown is skipped
                // and DataFusion evaluates them in-process instead.
                PostgreSqlDialect {}.scalar_function_to_sql_overrides(unparser, func_name, args)
            }
        }
    }
}

/// Build a BinaryOp AST node for a JSON operator.
fn json_binary_op(left: sql_ast::Expr, op: &str, right: sql_ast::Expr) -> sql_ast::Expr {
    sql_ast::Expr::BinaryOp {
        left: Box::new(left),
        op: sql_ast::BinaryOperator::Custom(op.to_string()),
        right: Box::new(right),
    }
}

// ---------- helpers ----------

/// Minimal context provider for pg_catalog — uses default (empty) roles
#[derive(Clone, Debug)]
struct ProxyCatalogContext;

impl PgCatalogContextProvider for ProxyCatalogContext {}

/// Wraps a CatalogProvider to support registering extra schemas (e.g. pg_catalog).
struct ExtensibleCatalogProvider {
    inner: Arc<dyn CatalogProvider>,
    extra_schemas: RwLock<HashMap<String, Arc<dyn SchemaProvider>>>,
}

impl ExtensibleCatalogProvider {
    fn new(inner: impl CatalogProvider + 'static) -> Self {
        Self {
            inner: Arc::new(inner),
            extra_schemas: RwLock::new(HashMap::new()),
        }
    }
}

impl std::fmt::Debug for ExtensibleCatalogProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensibleCatalogProvider")
            .field("inner", &"<dyn CatalogProvider>")
            .field("extra_schemas", &self.extra_schemas)
            .finish()
    }
}

impl CatalogProvider for ExtensibleCatalogProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema_names(&self) -> Vec<String> {
        let mut names = self.inner.schema_names();
        names.extend(self.extra_schemas.read().unwrap().keys().cloned());
        names
    }

    fn schema(&self, name: &str) -> Option<Arc<dyn SchemaProvider>> {
        self.extra_schemas
            .read()
            .unwrap()
            .get(name)
            .cloned()
            .or_else(|| self.inner.schema(name))
    }

    fn register_schema(
        &self,
        name: &str,
        schema: Arc<dyn SchemaProvider>,
    ) -> datafusion::common::Result<Option<Arc<dyn SchemaProvider>>> {
        let mut schemas = self.extra_schemas.write().unwrap();
        Ok(schemas.insert(name.to_string(), schema))
    }
}

// ---------- data source config ----------

/// Resolved (decrypted) connection parameters for a data source.
#[derive(Debug, Clone)]
pub struct DataSourceConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub ssl_mode: String,
}

impl DataSourceConfig {
    /// Build config from a `data_source` model by decrypting secure_config with the master key.
    pub fn from_model(
        model: &data_source::Model,
        master_key: &[u8; 32],
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if model.ds_type != "postgres" {
            return Err(format!("Unsupported data source type: {}", model.ds_type).into());
        }

        let config: serde_json::Value =
            serde_json::from_str(&model.config).map_err(|e| format!("Invalid config JSON: {e}"))?;

        let secure: serde_json::Value = if model.secure_config.is_empty() {
            serde_json::json!({})
        } else {
            crate::crypto::decrypt_json(&model.secure_config, master_key)
                .map_err(|e| format!("Failed to decrypt secure_config: {e}"))?
        };

        Ok(Self {
            host: config["host"]
                .as_str()
                .ok_or("missing host in config")?
                .to_string(),
            port: config["port"].as_u64().ok_or("missing port in config")? as u16,
            database: config["database"]
                .as_str()
                .ok_or("missing database in config")?
                .to_string(),
            username: config["username"]
                .as_str()
                .ok_or("missing username in config")?
                .to_string(),
            password: secure["password"]
                .as_str()
                .ok_or("missing password in secure_config")?
                .to_string(),
            ssl_mode: config
                .get("sslmode")
                .and_then(|v| v.as_str())
                .unwrap_or("require")
                .to_string(),
        })
    }
}

// ---------- session context creation ----------

/// Build the connection parameter map (plain strings) for `PostgresConnectionPool`.
///
/// Key names must match what `datafusion-table-providers` expects:
/// - `"db"` (not `"dbname"`) — the database name
/// - `"pass"` (not `"password"`) — the password
/// - `"host"`, `"user"`, `"port"`, `"sslmode"` — as-is
pub fn build_postgres_params(cfg: &DataSourceConfig) -> HashMap<String, String> {
    HashMap::from([
        ("host".to_string(), cfg.host.clone()),
        ("user".to_string(), cfg.username.clone()),
        ("db".to_string(), cfg.database.clone()),
        ("pass".to_string(), cfg.password.clone()),
        ("port".to_string(), cfg.port.to_string()),
        ("sslmode".to_string(), cfg.ssl_mode.clone()),
    ])
}

// ---------- virtual schema layer ----------

/// A table in the virtual catalog with its pre-computed Arrow schema.
struct VirtualCatalogTable {
    table_name: String,
    arrow_schema: SchemaRef,
}

/// A schema in the virtual catalog, containing pre-selected tables.
struct VirtualCatalogSchema {
    /// The real upstream schema name, used in TableReference for SQL sent to upstream.
    schema_name: String,
    tables: HashMap<String, VirtualCatalogTable>,
}

/// Pre-loaded catalog data for a datasource. Cached per datasource name.
/// Contains raw schema/table/column metadata — shared across all connections to the same datasource.
struct CachedCatalog {
    datasource_id: Uuid,
    schemas: HashMap<String, VirtualCatalogSchema>,
    default_schema: String,
    access_mode: String,
}

/// Computed per-user visibility derived from policy assignments.
struct UserVisibility {
    /// None = no filtering needed (open mode, no denies)
    filter: Option<VisibilityFilter>,
}

struct VisibilityFilter {
    /// Tables the user can see: (df_alias, table_name). None = all tables visible (open mode with denies).
    visible_tables: Option<HashSet<(String, String)>>,
    /// Columns to hide: (df_alias, table_name, column_name).
    denied_columns: HashSet<(String, String, String)>,
    /// Entire schemas to hide (df_alias). Applied in both open and policy_required modes.
    denied_schemas: HashSet<String>,
    /// Individual tables to hide: (df_alias, table_name). Applied in both modes.
    denied_tables: HashSet<(String, String)>,
}

use crate::policy_match::{PolicyType, TargetEntry};

// ---------- lazy pool ----------

/// Lazily-initialized connection pool shared across SessionContext rebuilds.
///
/// The pool is not created until the first user-table query, so pg_catalog /
/// information_schema queries (e.g. TablePlus sidebar population) complete
/// instantly without an upstream connection.
struct LazyPool {
    pool: AsyncRwLock<Option<Arc<DynPostgresConnectionPool>>>,
    params: HashMap<String, String>,
}

impl std::fmt::Debug for LazyPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LazyPool { ... }")
    }
}

impl LazyPool {
    fn new(params: HashMap<String, String>) -> Self {
        Self {
            pool: AsyncRwLock::new(None),
            params,
        }
    }

    /// Return the shared pool, creating it on first call (one round-trip via bb8).
    async fn get(&self) -> Result<Arc<DynPostgresConnectionPool>, String> {
        // Fast path: already initialised
        {
            let guard = self.pool.read().await;
            if let Some(ref p) = *guard {
                return Ok(p.clone());
            }
        }

        // Slow path: create pool (serialised by write lock — double-check inside)
        let mut guard = self.pool.write().await;
        if let Some(ref p) = *guard {
            return Ok(p.clone());
        }

        tracing::debug!("Creating upstream pool (first user-table query)");
        let postgres_params = to_secret_map(self.params.clone());
        let new_pool = PostgresConnectionPool::new(postgres_params)
            .await
            .map_err(|e| format!("Failed to create Postgres pool: {e}"))?
            .with_unsupported_type_action(UnsupportedTypeAction::String);
        let new_pool: Arc<DynPostgresConnectionPool> = Arc::new(new_pool);
        *guard = Some(new_pool.clone());
        Ok(new_pool)
    }
}

/// Parse a stored arrow_type string back into an Arrow DataType.
/// Returns None for unsupported or unrecognized types.
#[cfg(test)]
pub(crate) fn parse_arrow_type_pub(s: &str) -> Option<DataType> {
    parse_arrow_type(s)
}

fn parse_arrow_type(s: &str) -> Option<DataType> {
    match s {
        "Int8" => Some(DataType::Int8),
        "Int16" => Some(DataType::Int16),
        "Int32" => Some(DataType::Int32),
        "Int64" => Some(DataType::Int64),
        "UInt32" => Some(DataType::UInt32),
        "Float32" => Some(DataType::Float32),
        "Float64" => Some(DataType::Float64),
        "Boolean" => Some(DataType::Boolean),
        "Utf8" => Some(DataType::Utf8),
        "Date32" => Some(DataType::Date32),
        "Binary" => Some(DataType::Binary),
        "Time64(Nanosecond)" => Some(DataType::Time64(TimeUnit::Nanosecond)),
        // Timestamps — Nanosecond (library-native) and Microsecond (backward compat)
        "Timestamp(Nanosecond,None)" => Some(DataType::Timestamp(TimeUnit::Nanosecond, None)),
        "Timestamp(Microsecond,None)" => Some(DataType::Timestamp(TimeUnit::Microsecond, None)),
        s if s.starts_with("Timestamp(Nanosecond,Some(") => Some(DataType::Timestamp(
            TimeUnit::Nanosecond,
            Some("UTC".into()),
        )),
        s if s.starts_with("Timestamp(Microsecond,Some(") => Some(DataType::Timestamp(
            TimeUnit::Microsecond,
            Some("UTC".into()),
        )),
        // Generic Decimal128(p,s) parser
        s if s.starts_with("Decimal128(") && s.ends_with(')') => {
            let inner = &s[11..s.len() - 1];
            let mut parts = inner.splitn(2, ',');
            let p: u8 = parts.next()?.trim().parse().ok()?;
            let scale: i8 = parts.next()?.trim().parse().ok()?;
            Some(DataType::Decimal128(p, scale))
        }
        _ => None,
    }
}

/// Serialize a DataType to the canonical DB storage string.
/// This is the exact inverse of `parse_arrow_type` — only types that
/// round-trip through parse_arrow_type are valid inputs.
pub fn arrow_type_to_string(dt: &DataType) -> String {
    match dt {
        DataType::Int8 => "Int8".to_string(),
        DataType::Int16 => "Int16".to_string(),
        DataType::Int32 => "Int32".to_string(),
        DataType::Int64 => "Int64".to_string(),
        DataType::UInt32 => "UInt32".to_string(),
        DataType::Float32 => "Float32".to_string(),
        DataType::Float64 => "Float64".to_string(),
        DataType::Boolean => "Boolean".to_string(),
        DataType::Utf8 => "Utf8".to_string(),
        DataType::Date32 => "Date32".to_string(),
        DataType::Binary => "Binary".to_string(),
        DataType::Time64(TimeUnit::Nanosecond) => "Time64(Nanosecond)".to_string(),
        DataType::Decimal128(p, s) => format!("Decimal128({p},{s})"),
        DataType::Timestamp(TimeUnit::Nanosecond, None) => "Timestamp(Nanosecond,None)".to_string(),
        DataType::Timestamp(TimeUnit::Nanosecond, Some(tz)) => {
            format!("Timestamp(Nanosecond,Some(\"{tz}\"))")
        }
        DataType::Timestamp(TimeUnit::Microsecond, None) => {
            "Timestamp(Microsecond,None)".to_string()
        }
        DataType::Timestamp(TimeUnit::Microsecond, Some(tz)) => {
            format!("Timestamp(Microsecond,Some(\"{tz}\"))")
        }
        other => format!("{other:?}"),
    }
}

/// Build an Arrow schema from stored discovered_column entities.
/// Columns with no recognized arrow_type are skipped (matches UnsupportedTypeAction::Warn).
/// Columns are ordered by ordinal_position.
pub fn build_arrow_schema(columns: &[discovered_column::Model]) -> SchemaRef {
    let mut sorted: Vec<(i32, Field)> = columns
        .iter()
        .filter_map(|col| {
            if !col.is_selected {
                return None;
            }
            let arrow_type_str = col.arrow_type.as_deref()?;
            let data_type = parse_arrow_type(arrow_type_str)?;
            Some((
                col.ordinal_position,
                Field::new(&col.column_name, data_type, col.is_nullable),
            ))
        })
        .collect();
    sorted.sort_by_key(|(pos, _)| *pos);
    let fields: Vec<Field> = sorted.into_iter().map(|(_, f)| f).collect();
    Arc::new(Schema::new(fields))
}

/// SchemaProvider backed by the local catalog — no live introspection on connect.
/// The upstream pool is created lazily on the first user-table `table()` call.
struct VirtualSchemaProvider {
    schema_name: String,
    /// table_name → Arrow schema
    tables: HashMap<String, SchemaRef>,
    pool: Arc<LazyPool>,
}

impl std::fmt::Debug for VirtualSchemaProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualSchemaProvider")
            .field("schema_name", &self.schema_name)
            .field("tables", &self.tables.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[async_trait::async_trait]
impl SchemaProvider for VirtualSchemaProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn table_names(&self) -> Vec<String> {
        self.tables.keys().cloned().collect()
    }

    async fn table(
        &self,
        name: &str,
    ) -> datafusion::error::Result<Option<Arc<dyn datafusion::datasource::TableProvider>>> {
        let arrow_schema = match self.tables.get(name) {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        // Initialise pool on first call (lazy — no upstream connection until needed)
        let pool = self.pool.get().await.map_err(|e| {
            datafusion::error::DataFusionError::External(Box::new(std::io::Error::other(e)))
        })?;

        let table = SqlTable::new_with_schema(
            "postgres",
            &pool,
            arrow_schema,
            TableReference::full("postgres", self.schema_name.as_str(), name),
        )
        .with_dialect(Arc::new(BetweenRowsPostgresDialect));

        Ok(Some(Arc::new(table)))
    }

    fn table_exist(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }
}

/// CatalogProvider backed by the local catalog.
struct VirtualCatalogProvider {
    schemas: HashMap<String, Arc<VirtualSchemaProvider>>,
}

impl std::fmt::Debug for VirtualCatalogProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualCatalogProvider")
            .field("schemas", &self.schemas.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl CatalogProvider for VirtualCatalogProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema_names(&self) -> Vec<String> {
        self.schemas.keys().cloned().collect()
    }

    fn schema(&self, name: &str) -> Option<Arc<dyn SchemaProvider>> {
        self.schemas
            .get(name)
            .map(|s| s.clone() as Arc<dyn SchemaProvider>)
    }

    fn register_schema(
        &self,
        _name: &str,
        _schema: Arc<dyn SchemaProvider>,
    ) -> datafusion::common::Result<Option<Arc<dyn SchemaProvider>>> {
        Err(datafusion::error::DataFusionError::NotImplemented(
            "VirtualCatalogProvider does not support register_schema".to_string(),
        ))
    }
}

/// Choose the DataFusion default schema name for a virtual catalog.
///
/// Walks the catalog map looking for the schema whose real upstream name is
/// `"public"` and returns that entry's key (which may be an alias). If no
/// `"public"` schema is present, the first key in iteration order is used.
/// Returns the literal string `"public"` when the map is empty.
fn select_default_schema(catalog_schemas: &HashMap<String, VirtualCatalogSchema>) -> String {
    catalog_schemas
        .iter()
        .find(|(_, vs)| vs.schema_name == "public")
        .map(|(alias, _)| alias.clone())
        .or_else(|| catalog_schemas.keys().next().cloned())
        .unwrap_or_else(|| "public".to_string())
}

// ---------- optimizer rule: fix scan projections for pushed-down filters ----------

/// **DataFusion 52 workaround — revisit on upgrade.**
///
/// When PolicyHook injects a `Filter(tenant = 'acme')` above a `TableScan`,
/// DataFusion's `PushDownFilter` optimizer pushes it into `TableScan.filters`
/// (because `SqlTable` reports `Exact` pushdown support) and removes the
/// `Filter` node.  Then `optimize_projections` narrows the scan projection to
/// `[]` for `COUNT(*)` — but doesn't account for columns needed by the
/// pushed-down filters in `TableScan.filters`.  The physical planner later
/// expands the scan projection to include filter columns, creating a schema
/// mismatch with the logical plan.
///
/// This rule runs after all built-in optimizer rules and ensures that any
/// `TableScan` whose `projection` is `Some(...)` includes all columns
/// referenced by its pushed-down `filters`.  When extra columns are added, a
/// wrapping `Projection` strips them so the node's output schema stays
/// unchanged — preventing mismatches in parent nodes (e.g. `Join`).
///
/// **To check on DataFusion upgrade:** run `cargo test --test policy_enforcement
/// aggregate_with_row_filter` without this rule.  If the test passes, the
/// workaround can be removed.
#[derive(Debug)]
struct ScanFilterProjectionFixRule;

impl datafusion::optimizer::OptimizerRule for ScanFilterProjectionFixRule {
    fn name(&self) -> &str {
        "scan_filter_projection_fix"
    }

    fn apply_order(&self) -> Option<datafusion::optimizer::ApplyOrder> {
        Some(datafusion::optimizer::ApplyOrder::BottomUp)
    }

    fn rewrite(
        &self,
        plan: datafusion::logical_expr::LogicalPlan,
        _config: &dyn datafusion::optimizer::OptimizerConfig,
    ) -> datafusion::error::Result<
        datafusion::common::tree_node::Transformed<datafusion::logical_expr::LogicalPlan>,
    > {
        use datafusion::common::tree_node::Transformed;
        use datafusion::logical_expr::{Expr, LogicalPlan};

        let LogicalPlan::TableScan(ref scan) = plan else {
            return Ok(Transformed::no(plan));
        };
        // Nothing to fix if no pushed-down filters or projection is already None (all cols).
        if scan.filters.is_empty() {
            return Ok(Transformed::no(plan));
        }
        let Some(projection) = &scan.projection else {
            return Ok(Transformed::no(plan));
        };

        // Collect columns referenced by pushed-down filters that are missing
        // from the current projection.
        let source_schema = scan.source.schema();
        let mut extras: Vec<usize> = Vec::new();
        for filter_expr in &scan.filters {
            for col_ref in filter_expr.column_refs() {
                if let Ok(idx) = source_schema.index_of(&col_ref.name)
                    && !projection.contains(&idx)
                    && !extras.contains(&idx)
                {
                    extras.push(idx);
                }
            }
        }
        if extras.is_empty() {
            return Ok(Transformed::no(plan));
        }

        // Build expanded scan with filter columns included.
        let original_proj = projection.clone();
        let mut new_proj = projection.clone();
        new_proj.extend(extras);
        new_proj.sort_unstable();

        let new_scan = datafusion::logical_expr::TableScan::try_new(
            scan.table_name.clone(),
            scan.source.clone(),
            Some(new_proj.clone()),
            scan.filters.clone(),
            scan.fetch,
        )?;

        // Wrap in a Projection that only exposes the original columns so the
        // output schema is unchanged — parent nodes (Join, Aggregate, etc.)
        // won't see the extra filter-only columns.
        let expanded_plan = LogicalPlan::TableScan(new_scan);
        let expanded_schema = expanded_plan.schema();
        // Map each original source-column index to its position in the
        // expanded (sorted) projection so we pull the right DFSchema field.
        let proj_exprs: Vec<Expr> = original_proj
            .iter()
            .map(|&src_idx| {
                let pos = new_proj
                    .iter()
                    .position(|&p| p == src_idx)
                    .expect("original column must be in expanded projection");
                let (qualifier, field) = expanded_schema.qualified_field(pos);
                Expr::Column(datafusion::common::Column::new(
                    qualifier.cloned(),
                    field.name(),
                ))
            })
            .collect();

        let projection_plan = LogicalPlan::Projection(
            datafusion::logical_expr::Projection::try_new(proj_exprs, Arc::new(expanded_plan))?,
        );

        Ok(Transformed::yes(projection_plan))
    }
}

/// Build a SessionContext from local catalog metadata using a shared LazyPool.
///
/// Pool creation is deferred until the first user-table query, so pg_catalog /
/// information_schema queries complete instantly without an upstream connection.
///
/// `default_schema` is the alias (or real name when no alias) of the schema to
/// use as the default search path (replaces the hard-coded `"public"`).
async fn create_session_context_from_catalog(
    catalog_schemas: HashMap<String, VirtualCatalogSchema>,
    lazy_pool: Arc<LazyPool>,
    default_schema: &str,
) -> Result<SessionContext, Box<dyn std::error::Error + Send + Sync>> {
    // Build VirtualSchemaProviders — all share the same lazy pool.
    // The HashMap key is the alias (user-facing); schema_name inside the
    // provider is the real upstream name used in TableReference.
    let mut schema_providers: HashMap<String, Arc<VirtualSchemaProvider>> = HashMap::new();
    for (alias_name, catalog_schema) in catalog_schemas {
        let tables: HashMap<String, SchemaRef> = catalog_schema
            .tables
            .into_values()
            .map(|t| (t.table_name, t.arrow_schema))
            .collect();

        schema_providers.insert(
            alias_name,
            Arc::new(VirtualSchemaProvider {
                schema_name: catalog_schema.schema_name,
                tables,
                pool: Arc::clone(&lazy_pool),
            }),
        );
    }

    let virtual_catalog = VirtualCatalogProvider {
        schemas: schema_providers,
    };
    let catalog = ExtensibleCatalogProvider::new(virtual_catalog);

    let config = SessionConfig::new()
        .with_information_schema(true)
        .with_default_catalog_and_schema("postgres", default_schema);
    let mut ctx = SessionContext::new_with_config(config);
    ctx.add_optimizer_rule(Arc::new(ScanFilterProjectionFixRule));
    ctx.register_catalog("postgres", Arc::new(catalog));

    setup_pg_catalog(&ctx, "postgres", ProxyCatalogContext)
        .map_err(|e| format!("Failed to setup pg_catalog: {}", e))?;

    datafusion_functions_json::register_all(&mut ctx)
        .map_err(|e| format!("Failed to register JSON UDFs: {}", e))?;

    tracing::debug!("SessionContext ready (pool deferred until first user-table query)");

    Ok(ctx)
}

// ---------- visibility matching ----------
// Delegated to crate::policy_match::matches_schema_table (single source of truth).

// ---------- engine cache ----------

#[derive(Debug)]
pub struct EngineError(pub String);

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EngineError {}

pub struct EngineCache {
    db: DatabaseConnection,
    master_key: [u8; 32],
    /// Cached raw catalog per datasource (schema/table/column metadata).
    /// Shared across all connections to the same datasource.
    catalogs: AsyncRwLock<HashMap<String, Arc<CachedCatalog>>>,
    /// Shared LazyPool per datasource. Survives catalog invalidation so
    /// re-discovery after catalog changes reuses the existing upstream connection pool.
    pools: AsyncRwLock<HashMap<String, Arc<LazyPool>>>,
    /// Shared WASM runtime for evaluating decision functions at visibility time.
    wasm_runtime: Arc<crate::decision::wasm::WasmDecisionRuntime>,
}

impl EngineCache {
    pub fn new(
        db: DatabaseConnection,
        master_key: [u8; 32],
        wasm_runtime: Arc<crate::decision::wasm::WasmDecisionRuntime>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            master_key,
            catalogs: AsyncRwLock::new(HashMap::new()),
            pools: AsyncRwLock::new(HashMap::new()),
            wasm_runtime,
        })
    }

    /// Validate a data source exists and is active (DB lookup only, no connection).
    pub async fn validate_data_source(
        &self,
        name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ds = data_source::Entity::find()
            .filter(data_source::Column::Name.eq(name))
            .one(&self.db)
            .await
            .map_err(|e| EngineError(format!("DB error: {e}")))?;

        match ds {
            Some(m) if m.is_active => Ok(()),
            Some(_) => Err(EngineError(format!("Data source '{}' is inactive", name)).into()),
            None => Err(EngineError(format!("Data source '{}' not found", name)).into()),
        }
    }

    /// Check if a user has been assigned access to a data source (by name).
    pub async fn check_access(
        &self,
        user_id: Uuid,
        datasource_name: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let ds = data_source::Entity::find()
            .filter(data_source::Column::Name.eq(datasource_name))
            .filter(data_source::Column::IsActive.eq(true))
            .one(&self.db)
            .await
            .map_err(|e| EngineError(format!("DB error: {e}")))?;

        let ds = match ds {
            Some(d) => d,
            None => return Ok(false),
        };

        Ok(
            crate::role_resolver::resolve_datasource_access(&self.db, user_id, ds.id)
                .await
                .map_err(|e| EngineError(format!("DB error: {e}")))?,
        )
    }

    /// Get (or lazily load) the raw catalog for a named data source.
    /// Loads schema/table/column metadata from the admin DB and caches it.
    /// Per-user visibility filtering happens in `build_user_context()`.
    async fn get_catalog(
        &self,
        name: &str,
    ) -> Result<Arc<CachedCatalog>, Box<dyn std::error::Error + Send + Sync>> {
        // Fast path: read lock
        {
            let map = self.catalogs.read().await;
            if let Some(catalog) = map.get(name) {
                return Ok(catalog.clone());
            }
        }

        // Slow path: write lock (double-check)
        let mut map = self.catalogs.write().await;
        if let Some(catalog) = map.get(name) {
            return Ok(catalog.clone());
        }

        let ds = data_source::Entity::find()
            .filter(data_source::Column::Name.eq(name))
            .filter(data_source::Column::IsActive.eq(true))
            .one(&self.db)
            .await
            .map_err(|e| EngineError(format!("DB error: {e}")))?
            .ok_or_else(|| EngineError(format!("Data source '{}' not found or inactive", name)))?;

        // Load stored catalog for this data source
        let schemas_with_tables: Vec<(discovered_schema::Model, Vec<discovered_table::Model>)> =
            discovered_schema::Entity::find()
                .filter(discovered_schema::Column::DataSourceId.eq(ds.id))
                .filter(discovered_schema::Column::IsSelected.eq(true))
                .find_with_related(discovered_table::Entity)
                .all(&self.db)
                .await
                .map_err(|e| EngineError(format!("DB error loading catalog: {e}")))?;

        let mut catalog_schemas: HashMap<String, VirtualCatalogSchema> = HashMap::new();

        for (schema, tables) in schemas_with_tables {
            let mut catalog_tables: HashMap<String, VirtualCatalogTable> = HashMap::new();

            for table in tables.into_iter().filter(|t| t.is_selected) {
                let columns: Vec<discovered_column::Model> = discovered_column::Entity::find()
                    .filter(discovered_column::Column::DiscoveredTableId.eq(table.id))
                    .all(&self.db)
                    .await
                    .map_err(|e| EngineError(format!("DB error loading columns: {e}")))?;

                let arrow_schema = build_arrow_schema(&columns);

                catalog_tables.insert(
                    table.table_name.clone(),
                    VirtualCatalogTable {
                        table_name: table.table_name,
                        arrow_schema,
                    },
                );
            }

            // Use alias as the user-facing catalog key; keep real name for upstream queries.
            let effective_name = schema
                .schema_alias
                .clone()
                .unwrap_or_else(|| schema.schema_name.clone());

            catalog_schemas.insert(
                effective_name,
                VirtualCatalogSchema {
                    schema_name: schema.schema_name,
                    tables: catalog_tables,
                },
            );
        }

        let default_schema = select_default_schema(&catalog_schemas);

        let catalog = Arc::new(CachedCatalog {
            datasource_id: ds.id,
            schemas: catalog_schemas,
            default_schema,
            access_mode: ds.access_mode,
        });

        map.insert(name.to_string(), catalog.clone());
        Ok(catalog)
    }

    /// Get or create the shared LazyPool for a datasource.
    async fn get_or_create_pool(&self, name: &str, cfg: &DataSourceConfig) -> Arc<LazyPool> {
        // Fast path
        {
            let pools = self.pools.read().await;
            if let Some(p) = pools.get(name) {
                return p.clone();
            }
        }
        let mut pools = self.pools.write().await;
        if let Some(p) = pools.get(name) {
            return p.clone();
        }
        let params = build_postgres_params(cfg);
        let p = Arc::new(LazyPool::new(params));
        pools.insert(name.to_string(), p.clone());
        p
    }

    /// Evaluate a decision function at visibility time (session context only).
    ///
    /// Returns `true` if the policy should fire (apply its effect), `false` to skip.
    /// - `is_enabled = false` → `true` (gate disabled, apply unconditionally)
    /// - `evaluate_context = "query"` → `false` (skip at visibility; policy deferred to query time)
    /// - `decision_wasm` is None or empty → `true` (not compiled yet, apply unconditionally)
    /// - `evaluate_context = "session"` → evaluate WASM and return `fire` result
    async fn evaluate_visibility_decision_fn(
        &self,
        df: &decision_function::Model,
        session_ctx: &serde_json::Value,
    ) -> bool {
        if !df.is_enabled {
            return true; // Gate disabled → apply unconditionally
        }

        if df.evaluate_context == "query" {
            return false; // Deferred to query time — skip visibility effect
        }

        let wasm_bytes = match &df.decision_wasm {
            Some(bytes) if !bytes.is_empty() => bytes.clone(),
            _ => return true, // Not compiled yet → apply unconditionally
        };

        let config: serde_json::Value = df
            .decision_config
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::json!({}));

        let fuel_limit = crate::decision::wasm::DEFAULT_FUEL_LIMIT;
        let log_level = df.log_level.clone();
        let on_error = df.on_error.clone();
        let df_name = df.name.clone();

        let spawn_result = self
            .wasm_runtime
            .evaluate_bytes(&wasm_bytes, session_ctx, &config, fuel_limit, &log_level)
            .await;

        match spawn_result {
            Ok(result) => result.fire,
            Err(e) => {
                tracing::error!(
                    decision_function = %df_name,
                    error = %e,
                    "Visibility decision function evaluation failed"
                );
                on_error == "deny"
            }
        }
    }

    /// Compute what tables and columns a user can see, given their policy assignments.
    async fn compute_user_visibility(
        &self,
        user_id: Uuid,
        catalog: &CachedCatalog,
    ) -> Result<UserVisibility, Box<dyn std::error::Error + Send + Sync>> {
        // Build df_alias → upstream_name mapping from catalog
        let df_to_upstream: HashMap<String, String> = catalog
            .schemas
            .iter()
            .map(|(alias, vs)| (alias.clone(), vs.schema_name.clone()))
            .collect();

        // Load policy assignments for this datasource + user (user-specific, role-based, or wildcard)
        let relevant = crate::role_resolver::resolve_effective_assignments(
            &self.db,
            user_id,
            catalog.datasource_id,
        )
        .await
        .map_err(|e| EngineError(format!("DB error loading assignments: {e}")))?;

        if relevant.is_empty() {
            if catalog.access_mode == "policy_required" {
                // No policies → no tables visible
                return Ok(UserVisibility {
                    filter: Some(VisibilityFilter {
                        visible_tables: Some(HashSet::new()),
                        denied_columns: HashSet::new(),
                        denied_schemas: HashSet::new(),
                        denied_tables: HashSet::new(),
                    }),
                });
            }
            return Ok(UserVisibility { filter: None });
        }

        let policy_ids: Vec<Uuid> = relevant
            .iter()
            .map(|a| a.policy_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let policies = policy::Entity::find()
            .filter(policy::Column::Id.is_in(policy_ids.clone()))
            .filter(policy::Column::IsEnabled.eq(true))
            .all(&self.db)
            .await
            .map_err(|e| EngineError(format!("DB error loading policies: {e}")))?;

        // Batch-load decision functions referenced by visibility-affecting policies
        let df_ids: Vec<Uuid> = policies
            .iter()
            .filter(|p| {
                p.policy_type
                    .parse::<PolicyType>()
                    .map(|pt| pt.affects_visibility())
                    .unwrap_or(false)
            })
            .filter_map(|p| p.decision_function_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let df_map: HashMap<Uuid, decision_function::Model> = if !df_ids.is_empty() {
            decision_function::Entity::find()
                .filter(decision_function::Column::Id.is_in(df_ids))
                .all(&self.db)
                .await
                .map_err(|e| EngineError(format!("DB error loading decision functions: {e}")))?
                .into_iter()
                .map(|df| (df.id, df))
                .collect()
        } else {
            HashMap::new()
        };

        // Build session context for decision function evaluation (only if needed)
        let session_ctx = if !df_map.is_empty() {
            let user = proxy_user::Entity::find_by_id(user_id)
                .one(&self.db)
                .await
                .map_err(|e| EngineError(format!("DB error loading user: {e}")))?
                .ok_or_else(|| EngineError(format!("User {user_id} not found")))?;

            let role_ids = crate::role_resolver::resolve_user_roles(&self.db, user_id)
                .await
                .map_err(|e| EngineError(format!("DB error resolving roles: {e}")))?;
            let role_names: Vec<String> = if !role_ids.is_empty() {
                role::Entity::find()
                    .filter(role::Column::Id.is_in(role_ids))
                    .all(&self.db)
                    .await
                    .map_err(|e| EngineError(format!("DB error loading roles: {e}")))?
                    .into_iter()
                    .map(|r| r.name)
                    .collect()
            } else {
                vec![]
            };

            // Resolve datasource name from catalog (catalog stores it)
            let ds = data_source::Entity::find_by_id(catalog.datasource_id)
                .one(&self.db)
                .await
                .map_err(|e| EngineError(format!("DB error loading datasource: {e}")))?
                .ok_or_else(|| EngineError("Datasource not found".to_string()))?;

            // Build typed attributes for decision function context
            let raw_attrs = crate::entity::proxy_user::parse_attributes(&user.attributes);
            let typed_attrs = build_typed_json_attributes(&self.db, &raw_attrs).await;

            let session_info = crate::decision::context::SessionInfo {
                user_id,
                username: user.username,
                roles: role_names,
                datasource_name: ds.name,
                access_mode: catalog.access_mode.clone(),
                attributes: typed_attrs,
            };
            Some(crate::decision::context::build_session_context(
                &session_info,
            ))
        } else {
            None
        };

        let mut visible_tables: HashSet<(String, String)> = HashSet::new();
        let mut denied_columns: HashSet<(String, String, String)> = HashSet::new();
        let mut denied_schemas: HashSet<String> = HashSet::new();
        let mut denied_tables: HashSet<(String, String)> = HashSet::new();
        // Track column_allow patterns per table to compute denied columns later.
        let mut column_allow_patterns: HashMap<(String, String), Vec<String>> = HashMap::new();

        for p in &policies {
            let policy_type = match p.policy_type.parse::<PolicyType>() {
                Ok(pt) => pt,
                Err(_) => continue,
            };

            // Evaluate decision function for visibility-affecting policies
            if policy_type.affects_visibility()
                && let Some(df_id) = p.decision_function_id
                && let Some(df) = df_map.get(&df_id)
                && let Some(ctx) = &session_ctx
                && !self.evaluate_visibility_decision_fn(df, ctx).await
            {
                continue; // Decision function says skip this policy
            }

            let targets: Vec<TargetEntry> = serde_json::from_str(&p.targets).unwrap_or_default();

            match policy_type {
                PolicyType::ColumnAllow => {
                    // Grants table visibility and restricts columns to the allow list.
                    for (df_alias, vs) in &catalog.schemas {
                        for table_name in vs.tables.keys() {
                            for entry in &targets {
                                if entry.matches_table(df_alias, table_name, &df_to_upstream) {
                                    let key = (df_alias.clone(), table_name.clone());
                                    visible_tables.insert(key.clone());
                                    if let Some(cols) = &entry.columns {
                                        column_allow_patterns
                                            .entry(key)
                                            .or_default()
                                            .extend(cols.iter().cloned());
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                PolicyType::ColumnDeny => {
                    // Hides specific columns in the schema.
                    for (df_alias, vs) in &catalog.schemas {
                        for (table_name, table) in &vs.tables {
                            for entry in &targets {
                                if entry.matches_table(df_alias, table_name, &df_to_upstream) {
                                    if let Some(cols) = &entry.columns {
                                        let actual_cols: Vec<&str> = table
                                            .arrow_schema
                                            .fields()
                                            .iter()
                                            .map(|f| f.name().as_str())
                                            .collect();
                                        for col_name in crate::policy_match::expand_column_patterns(
                                            cols,
                                            &actual_cols,
                                        ) {
                                            denied_columns.insert((
                                                df_alias.clone(),
                                                table_name.clone(),
                                                col_name,
                                            ));
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                PolicyType::TableDeny => {
                    // Hides entire tables or schemas.
                    for (df_alias, vs) in &catalog.schemas {
                        let upstream = df_to_upstream
                            .get(df_alias)
                            .map(|s| s.as_str())
                            .unwrap_or(df_alias.as_str());
                        for entry in &targets {
                            if !entry
                                .schemas
                                .iter()
                                .any(|sp| crate::policy_match::matches_pattern(sp, upstream))
                            {
                                continue;
                            }
                            // tables: ["*"] or absent → deny entire schema
                            let all_tables = entry.tables.iter().any(|t| t == "*");
                            if all_tables {
                                denied_schemas.insert(df_alias.clone());
                            } else {
                                for table_name in vs.tables.keys() {
                                    if entry.tables.iter().any(|tp| {
                                        crate::policy_match::matches_pattern(tp, table_name)
                                    }) {
                                        denied_tables
                                            .insert((df_alias.clone(), table_name.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
                // RowFilter and ColumnMask don't affect catalog-level visibility.
                PolicyType::RowFilter | PolicyType::ColumnMask => {}
            }
        }

        // Convert column_allow restrictions into denied_columns entries.
        // Any field not in the allowed set is denied.
        for ((df_alias, table_name), allow_patterns) in &column_allow_patterns {
            if let Some(vs) = catalog.schemas.get(df_alias)
                && let Some(table) = vs.tables.get(table_name)
            {
                let actual_cols: Vec<&str> = table
                    .arrow_schema
                    .fields()
                    .iter()
                    .map(|f| f.name().as_str())
                    .collect();
                let allowed =
                    crate::policy_match::expand_column_patterns(allow_patterns, &actual_cols);
                for field in table.arrow_schema.fields() {
                    if !allowed.contains(field.name()) {
                        denied_columns.insert((
                            df_alias.clone(),
                            table_name.clone(),
                            field.name().clone(),
                        ));
                    }
                }
            }
        }

        if catalog.access_mode == "policy_required" {
            Ok(UserVisibility {
                filter: Some(VisibilityFilter {
                    visible_tables: Some(visible_tables),
                    denied_columns,
                    denied_schemas,
                    denied_tables,
                }),
            })
        } else {
            // Open mode: only filter if there are any denies
            if denied_columns.is_empty() && denied_schemas.is_empty() && denied_tables.is_empty() {
                Ok(UserVisibility { filter: None })
            } else {
                Ok(UserVisibility {
                    filter: Some(VisibilityFilter {
                        visible_tables: None,
                        denied_columns,
                        denied_schemas,
                        denied_tables,
                    }),
                })
            }
        }
    }

    /// Build a per-user SessionContext filtered by the user's policy visibility.
    ///
    /// Called once at connection time (in `on_startup`). The context is stored in
    /// the handler's `connection_contexts` map for the lifetime of the connection.
    pub async fn build_user_context(
        &self,
        user_id: Uuid,
        datasource_name: &str,
    ) -> Result<Arc<SessionContext>, Box<dyn std::error::Error + Send + Sync>> {
        let catalog = self.get_catalog(datasource_name).await?;

        // Load datasource config for pool creation (only queries DB if pool not yet cached)
        let lazy_pool = {
            let existing = self.pools.read().await.get(datasource_name).cloned();
            if let Some(p) = existing {
                p
            } else {
                let ds = data_source::Entity::find()
                    .filter(data_source::Column::Name.eq(datasource_name))
                    .filter(data_source::Column::IsActive.eq(true))
                    .one(&self.db)
                    .await
                    .map_err(|e| EngineError(format!("DB error: {e}")))?
                    .ok_or_else(|| {
                        EngineError(format!(
                            "Data source '{}' not found or inactive",
                            datasource_name
                        ))
                    })?;
                let cfg = DataSourceConfig::from_model(&ds, &self.master_key)?;
                self.get_or_create_pool(datasource_name, &cfg).await
            }
        };

        // Compute per-user visibility from policy assignments
        let visibility = self.compute_user_visibility(user_id, &catalog).await?;

        // Build filtered catalog schemas
        let filtered_schemas: HashMap<String, VirtualCatalogSchema> =
            if let Some(filter) = &visibility.filter {
                let mut schemas = HashMap::new();
                for (df_alias, vs) in &catalog.schemas {
                    // Schema-level object_access deny
                    if filter.denied_schemas.contains(df_alias) {
                        continue;
                    }

                    let tables: HashMap<String, VirtualCatalogTable> = vs
                        .tables
                        .iter()
                        .filter_map(|(table_name, table)| {
                            // Table-level object_access deny (404-not-403 principle).
                            // table_deny tables are intentionally removed from the catalog so
                            // queries fail with "table not found" rather than "access denied",
                            // avoiding metadata leakage about the existence of denied tables.
                            // Audit status is "error", not "denied".
                            if filter
                                .denied_tables
                                .contains(&(df_alias.clone(), table_name.clone()))
                            {
                                return None;
                            }

                            // Table visibility check
                            if filter.visible_tables.as_ref().is_some_and(|visible_set| {
                                !visible_set.contains(&(df_alias.clone(), table_name.clone()))
                            }) {
                                return None;
                            }

                            // Column visibility: remove denied columns from Arrow schema
                            let schema_ref = if filter.denied_columns.is_empty() {
                                table.arrow_schema.clone()
                            } else {
                                let fields: Vec<_> = table
                                    .arrow_schema
                                    .fields()
                                    .iter()
                                    .filter(|f| {
                                        !filter.denied_columns.contains(&(
                                            df_alias.clone(),
                                            table_name.clone(),
                                            f.name().clone(),
                                        ))
                                    })
                                    .cloned()
                                    .collect();
                                Arc::new(Schema::new(fields))
                            };

                            Some((
                                table_name.clone(),
                                VirtualCatalogTable {
                                    table_name: table_name.clone(),
                                    arrow_schema: schema_ref,
                                },
                            ))
                        })
                        .collect();

                    // Include schema if it has visible tables, or if no table filtering
                    if filter.visible_tables.is_none() || !tables.is_empty() {
                        schemas.insert(
                            df_alias.clone(),
                            VirtualCatalogSchema {
                                schema_name: vs.schema_name.clone(),
                                tables,
                            },
                        );
                    }
                }
                schemas
            } else {
                // No filtering — clone catalog schemas as-is
                catalog
                    .schemas
                    .iter()
                    .map(|(alias, vs)| {
                        let tables = vs
                            .tables
                            .iter()
                            .map(|(tname, t)| {
                                (
                                    tname.clone(),
                                    VirtualCatalogTable {
                                        table_name: t.table_name.clone(),
                                        arrow_schema: t.arrow_schema.clone(),
                                    },
                                )
                            })
                            .collect();
                        (
                            alias.clone(),
                            VirtualCatalogSchema {
                                schema_name: vs.schema_name.clone(),
                                tables,
                            },
                        )
                    })
                    .collect()
            };

        let default_schema = if filtered_schemas.contains_key(&catalog.default_schema) {
            catalog.default_schema.clone()
        } else {
            select_default_schema(&filtered_schemas)
        };

        let ctx = create_session_context_from_catalog(filtered_schemas, lazy_pool, &default_schema)
            .await?;
        Ok(Arc::new(ctx))
    }

    /// Remove a data source's cached catalog (call after catalog re-discovery).
    /// Keeps the shared pool so subsequent connections don't need a new upstream connection.
    pub async fn invalidate(&self, name: &str) {
        self.catalogs.write().await.remove(name);
    }

    /// Remove both the cached catalog AND the shared pool
    /// (call after datasource connection params are edited or datasource is deleted).
    pub async fn invalidate_all(&self, name: &str) {
        self.catalogs.write().await.remove(name);
        self.pools.write().await.remove(name);
    }

    /// Eagerly initialise the LazyPool for a datasource.
    ///
    /// Call this from a background task after auth to amortise the first-query latency.
    pub async fn warmup(&self, name: &str) {
        let pool = self.pools.read().await.get(name).cloned();
        if let Some(lazy_pool) = pool {
            match lazy_pool.get().await {
                Ok(_) => tracing::debug!(datasource = %name, "Pool warmed up"),
                Err(e) => {
                    tracing::debug!(datasource = %name, error = %e, "Pool warmup failed (non-fatal)")
                }
            }
        }
    }

    /// Attempt a test connection for a data source config (no caching).
    ///
    /// Only creates the connection pool (which internally runs `SELECT 1`).
    pub async fn test_connection(
        cfg: &DataSourceConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let postgres_params = to_secret_map(build_postgres_params(cfg));
        PostgresConnectionPool::new(postgres_params)
            .await
            .map(|_| ())
            .map_err(|e| format!("Failed to create Postgres pool: {}", e).into())
    }
}

/// Build typed JSON attributes from parsed attribute values and attribute definitions.
/// Used for building the decision function context at visibility time.
async fn build_typed_json_attributes(
    db: &sea_orm::DatabaseConnection,
    raw_attrs: &std::collections::HashMap<String, serde_json::Value>,
) -> std::collections::HashMap<String, serde_json::Value> {
    use crate::entity::attribute_definition;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    if raw_attrs.is_empty() {
        return std::collections::HashMap::new();
    }

    let keys: Vec<String> = raw_attrs.keys().cloned().collect();
    let defs = attribute_definition::Entity::find()
        .filter(attribute_definition::Column::EntityType.eq("user"))
        .filter(attribute_definition::Column::Key.is_in(keys))
        .all(db)
        .await
        .unwrap_or_default();

    let def_map: std::collections::HashMap<&str, &str> = defs
        .iter()
        .map(|d| (d.key.as_str(), d.value_type.as_str()))
        .collect();

    let mut result = std::collections::HashMap::new();
    for (key, value) in raw_attrs {
        let value_type = def_map.get(key.as_str()).unwrap_or(&"string");
        let json_val = match *value_type {
            // List values are already JSON arrays — pass through directly
            "list" => value.clone(),
            "integer" => {
                let s = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                s.parse::<i64>()
                    .map(|n| serde_json::json!(n))
                    .unwrap_or_else(|_| serde_json::json!(s))
            }
            "boolean" => {
                let s = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                s.parse::<bool>()
                    .map(|b| serde_json::json!(b))
                    .unwrap_or_else(|_| serde_json::json!(s))
            }
            _ => match value {
                serde_json::Value::String(s) => serde_json::json!(s),
                other => other.clone(),
            },
        };
        result.insert(key.clone(), json_val);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::catalog::{CatalogProvider, SchemaProvider};
    use datafusion::datasource::TableProvider;
    use datafusion::error::Result as DFResult;
    use std::any::Any;
    use std::sync::OnceLock;

    fn shared_wasm_runtime() -> Arc<crate::decision::wasm::WasmDecisionRuntime> {
        static RUNTIME: OnceLock<Arc<crate::decision::wasm::WasmDecisionRuntime>> = OnceLock::new();
        RUNTIME
            .get_or_init(|| Arc::new(crate::decision::wasm::WasmDecisionRuntime::new().unwrap()))
            .clone()
    }

    /// Mock CatalogProvider for testing
    #[derive(Debug)]
    struct MockCatalogProvider {
        schemas: RwLock<HashMap<String, Arc<dyn SchemaProvider>>>,
    }

    impl MockCatalogProvider {
        fn new() -> Self {
            Self {
                schemas: RwLock::new(HashMap::new()),
            }
        }
    }

    impl CatalogProvider for MockCatalogProvider {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn schema_names(&self) -> Vec<String> {
            self.schemas.read().unwrap().keys().cloned().collect()
        }

        fn schema(&self, name: &str) -> Option<Arc<dyn SchemaProvider>> {
            self.schemas.read().unwrap().get(name).cloned()
        }

        fn register_schema(
            &self,
            name: &str,
            schema: Arc<dyn SchemaProvider>,
        ) -> DFResult<Option<Arc<dyn SchemaProvider>>> {
            Ok(self
                .schemas
                .write()
                .unwrap()
                .insert(name.to_string(), schema))
        }
    }

    /// Mock SchemaProvider for testing
    #[derive(Debug)]
    struct MockSchemaProvider {
        name: String,
    }

    impl MockSchemaProvider {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl SchemaProvider for MockSchemaProvider {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn table_names(&self) -> Vec<String> {
            vec![]
        }

        async fn table(&self, _name: &str) -> DFResult<Option<Arc<dyn TableProvider>>> {
            Ok(None)
        }

        fn table_exist(&self, _name: &str) -> bool {
            false
        }
    }

    // Mock inner catalog that doesn't support register_schema
    #[derive(Debug)]
    struct MockInnerCatalogProvider {
        inner: MockCatalogProvider,
    }

    impl MockInnerCatalogProvider {
        fn new(inner: MockCatalogProvider) -> Self {
            Self { inner }
        }
    }

    impl CatalogProvider for MockInnerCatalogProvider {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn schema_names(&self) -> Vec<String> {
            self.inner.schema_names()
        }

        fn schema(&self, name: &str) -> Option<Arc<dyn SchemaProvider>> {
            self.inner.schema(name)
        }

        fn register_schema(
            &self,
            _name: &str,
            _schema: Arc<dyn SchemaProvider>,
        ) -> DFResult<Option<Arc<dyn SchemaProvider>>> {
            Err(datafusion::error::DataFusionError::NotImplemented(
                "MockInnerCatalogProvider does not support register_schema".to_string(),
            ))
        }
    }

    #[test]
    fn test_schema_names_includes_extra() {
        let inner = MockCatalogProvider::new();
        let catalog = ExtensibleCatalogProvider::new(MockInnerCatalogProvider::new(inner));

        let schema = Arc::new(MockSchemaProvider::new("test_schema")) as Arc<dyn SchemaProvider>;
        catalog
            .register_schema("test_schema", schema)
            .expect("Failed to register schema");

        let names = catalog.schema_names();
        assert!(
            names.contains(&"test_schema".to_string()),
            "Expected test_schema to appear in schema_names, got: {:?}",
            names
        );
    }

    #[test]
    fn test_schema_lookup_extra_first() {
        let inner = MockCatalogProvider::new();

        let inner_schema =
            Arc::new(MockSchemaProvider::new("inner_version")) as Arc<dyn SchemaProvider>;
        inner
            .register_schema("shared", inner_schema.clone())
            .expect("Failed to register inner schema");

        let catalog = ExtensibleCatalogProvider::new(MockInnerCatalogProvider::new(inner));

        let extra_schema =
            Arc::new(MockSchemaProvider::new("extra_version")) as Arc<dyn SchemaProvider>;
        catalog
            .register_schema("shared", extra_schema.clone())
            .expect("Failed to register extra schema");

        let retrieved = catalog.schema("shared").expect("Failed to retrieve schema");
        let as_mock = retrieved
            .as_any()
            .downcast_ref::<MockSchemaProvider>()
            .expect("Failed to downcast to MockSchemaProvider");

        assert_eq!(
            as_mock.name, "extra_version",
            "Expected extra schema to take priority over inner schema"
        );
    }

    #[test]
    fn test_register_schema_returns_previous() {
        let inner = MockCatalogProvider::new();
        let catalog = ExtensibleCatalogProvider::new(MockInnerCatalogProvider::new(inner));

        let schema1 = Arc::new(MockSchemaProvider::new("version1")) as Arc<dyn SchemaProvider>;
        let schema2 = Arc::new(MockSchemaProvider::new("version2")) as Arc<dyn SchemaProvider>;

        let result1 = catalog
            .register_schema("my_schema", schema1)
            .expect("Failed to register schema");
        assert!(result1.is_none(), "Expected None on first registration");

        let result2 = catalog
            .register_schema("my_schema", schema2)
            .expect("Failed to register schema");
        assert!(result2.is_some(), "Expected Some on second registration");

        let prev_schema = result2.unwrap();
        let as_mock = prev_schema
            .as_any()
            .downcast_ref::<MockSchemaProvider>()
            .expect("Failed to downcast to MockSchemaProvider");
        assert_eq!(
            as_mock.name, "version1",
            "Expected previous schema to be returned"
        );
    }

    #[test]
    fn test_data_source_config_from_model() {
        use crate::entity::data_source;
        use chrono::Utc;

        let master_key = [42u8; 32];
        let secure = serde_json::json!({"password": "secret123"});
        let encrypted = crate::crypto::encrypt_json(&secure, &master_key).unwrap();

        let model = data_source::Model {
            id: Uuid::new_v4(),
            name: "test-ds".to_string(),
            ds_type: "postgres".to_string(),
            config: serde_json::json!({
                "host": "localhost",
                "port": 5432,
                "database": "mydb",
                "username": "alice",
                "sslmode": "require"
            })
            .to_string(),
            secure_config: encrypted,
            is_active: true,
            access_mode: "policy_required".to_string(),
            last_sync_at: None,
            last_sync_result: None,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
        };

        let cfg = DataSourceConfig::from_model(&model, &master_key).unwrap();

        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 5432);
        assert_eq!(cfg.database, "mydb");
        assert_eq!(cfg.username, "alice");
        assert_eq!(cfg.password, "secret123");
        assert_eq!(cfg.ssl_mode, "require");
    }

    #[test]
    fn test_build_postgres_params_correct_keys() {
        // datafusion-table-providers' PostgresConnectionPool reads "db" and "pass"
        // (not "dbname" / "password"). Using the wrong keys silently drops the database
        // name and password from the connection string, causing "connection closed".
        let cfg = DataSourceConfig {
            host: "db.example.com".to_string(),
            port: 5432,
            database: "mydb".to_string(),
            username: "alice".to_string(),
            password: "s3cr3t".to_string(),
            ssl_mode: "require".to_string(),
        };

        let params = build_postgres_params(&cfg);

        // Keys the pool actually reads
        assert!(params.contains_key("host"), "missing 'host'");
        assert!(params.contains_key("user"), "missing 'user'");
        assert!(
            params.contains_key("db"),
            "missing 'db' (pool reads 'db', not 'dbname')"
        );
        assert!(
            params.contains_key("pass"),
            "missing 'pass' (pool reads 'pass', not 'password')"
        );
        assert!(params.contains_key("port"), "missing 'port'");
        assert!(params.contains_key("sslmode"), "missing 'sslmode'");

        // Regression guard: these keys are silently ignored by the pool
        assert!(
            !params.contains_key("dbname"),
            "'dbname' is ignored by the pool — use 'db'"
        );
        assert!(
            !params.contains_key("password"),
            "'password' is ignored by the pool — use 'pass'"
        );
        assert!(
            !params.contains_key("username"),
            "'username' is ignored by the pool — use 'user'"
        );

        // Values should be correctly mapped
        assert_eq!(params["host"], "db.example.com");
        assert_eq!(params["user"], "alice");
        assert_eq!(params["db"], "mydb");
        assert_eq!(params["pass"], "s3cr3t");
        assert_eq!(params["port"], "5432");
        assert_eq!(params["sslmode"], "require");
    }

    #[test]
    fn test_data_source_config_unsupported_type() {
        use crate::entity::data_source;
        use chrono::Utc;

        let master_key = [42u8; 32];

        let model = data_source::Model {
            id: Uuid::new_v4(),
            name: "test-ds".to_string(),
            ds_type: "mysql".to_string(),
            config: "{}".to_string(),
            secure_config: "".to_string(),
            is_active: true,
            access_mode: "policy_required".to_string(),
            last_sync_at: None,
            last_sync_result: None,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
        };

        let result = DataSourceConfig::from_model(&model, &master_key);
        assert!(result.is_err(), "Expected error for unsupported type");
    }

    #[test]
    fn test_parse_arrow_type_known() {
        assert!(matches!(parse_arrow_type("Int8"), Some(DataType::Int8)));
        assert!(matches!(parse_arrow_type("Int16"), Some(DataType::Int16)));
        assert!(matches!(parse_arrow_type("Int32"), Some(DataType::Int32)));
        assert!(matches!(parse_arrow_type("Int64"), Some(DataType::Int64)));
        assert!(matches!(parse_arrow_type("UInt32"), Some(DataType::UInt32)));
        assert!(matches!(
            parse_arrow_type("Float32"),
            Some(DataType::Float32)
        ));
        assert!(matches!(
            parse_arrow_type("Float64"),
            Some(DataType::Float64)
        ));
        assert!(matches!(
            parse_arrow_type("Boolean"),
            Some(DataType::Boolean)
        ));
        assert!(matches!(parse_arrow_type("Utf8"), Some(DataType::Utf8)));
        assert!(matches!(parse_arrow_type("Date32"), Some(DataType::Date32)));
        assert!(matches!(parse_arrow_type("Binary"), Some(DataType::Binary)));
        assert!(matches!(
            parse_arrow_type("Time64(Nanosecond)"),
            Some(DataType::Time64(TimeUnit::Nanosecond))
        ));
        // Generic Decimal128 parser
        assert!(matches!(
            parse_arrow_type("Decimal128(38,10)"),
            Some(DataType::Decimal128(38, 10))
        ));
        assert!(matches!(
            parse_arrow_type("Decimal128(38,20)"),
            Some(DataType::Decimal128(38, 20))
        ));
        assert!(matches!(
            parse_arrow_type("Decimal128(10,2)"),
            Some(DataType::Decimal128(10, 2))
        ));
        // Nanosecond timestamps (library-native)
        assert!(matches!(
            parse_arrow_type("Timestamp(Nanosecond,None)"),
            Some(DataType::Timestamp(TimeUnit::Nanosecond, None))
        ));
        assert!(matches!(
            parse_arrow_type("Timestamp(Nanosecond,Some(\"UTC\"))"),
            Some(DataType::Timestamp(TimeUnit::Nanosecond, Some(_)))
        ));
        // Microsecond timestamps (backward compat)
        assert!(matches!(
            parse_arrow_type("Timestamp(Microsecond,None)"),
            Some(DataType::Timestamp(TimeUnit::Microsecond, None))
        ));
    }

    #[test]
    fn test_parse_arrow_type_unsupported() {
        assert!(parse_arrow_type("json").is_none());
        assert!(parse_arrow_type("jsonb").is_none());
        assert!(parse_arrow_type("unknown").is_none());
    }

    #[test]
    fn test_arrow_type_round_trip() {
        // All types that the library produces must round-trip through string serialization
        let cases: &[DataType] = &[
            DataType::Int8,
            DataType::Int16,
            DataType::Int32,
            DataType::Int64,
            DataType::UInt32,
            DataType::Float32,
            DataType::Float64,
            DataType::Boolean,
            DataType::Utf8,
            DataType::Date32,
            DataType::Binary,
            DataType::Time64(TimeUnit::Nanosecond),
            DataType::Decimal128(38, 20),
            DataType::Decimal128(38, 10),
            DataType::Decimal128(10, 2),
            DataType::Timestamp(TimeUnit::Nanosecond, None),
            DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
            DataType::Timestamp(TimeUnit::Microsecond, None),
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        ];
        for dt in cases {
            let stored = arrow_type_to_string(dt);
            let recovered = parse_arrow_type(&stored)
                .unwrap_or_else(|| panic!("parse_arrow_type({stored:?}) returned None for {dt:?}"));
            assert_eq!(
                dt, &recovered,
                "Round-trip failed for {dt:?}: stored as {stored:?}"
            );
        }
    }

    #[test]
    fn test_build_arrow_schema() {
        use crate::entity::discovered_column;
        use chrono::Utc;

        let now = Utc::now().naive_utc();
        let table_id = Uuid::new_v4();
        let columns = vec![
            discovered_column::Model {
                id: Uuid::new_v4(),
                discovered_table_id: table_id,
                column_name: "id".to_string(),
                ordinal_position: 1,
                data_type: "integer".to_string(),
                is_nullable: false,
                column_default: None,
                arrow_type: Some("Int32".to_string()),
                is_selected: true,
                discovered_at: now,
            },
            discovered_column::Model {
                id: Uuid::new_v4(),
                discovered_table_id: table_id,
                column_name: "name".to_string(),
                ordinal_position: 2,
                data_type: "text".to_string(),
                is_nullable: true,
                column_default: None,
                arrow_type: Some("Utf8".to_string()),
                is_selected: true,
                discovered_at: now,
            },
            discovered_column::Model {
                id: Uuid::new_v4(),
                discovered_table_id: table_id,
                column_name: "metadata".to_string(),
                ordinal_position: 3,
                data_type: "jsonb".to_string(),
                is_nullable: true,
                column_default: None,
                arrow_type: None, // unsupported — should be skipped
                is_selected: true,
                discovered_at: now,
            },
        ];

        let schema = build_arrow_schema(&columns);
        assert_eq!(schema.fields().len(), 2, "jsonb column should be skipped");

        // Also verify that deselected columns are excluded
        let columns_with_deselected = vec![
            discovered_column::Model {
                id: Uuid::new_v4(),
                discovered_table_id: table_id,
                column_name: "id".to_string(),
                ordinal_position: 1,
                data_type: "integer".to_string(),
                is_nullable: false,
                column_default: None,
                arrow_type: Some("Int32".to_string()),
                is_selected: true,
                discovered_at: now,
            },
            discovered_column::Model {
                id: Uuid::new_v4(),
                discovered_table_id: table_id,
                column_name: "secret".to_string(),
                ordinal_position: 2,
                data_type: "text".to_string(),
                is_nullable: true,
                column_default: None,
                arrow_type: Some("Utf8".to_string()),
                is_selected: false, // deselected — should be excluded
                discovered_at: now,
            },
        ];
        let schema2 = build_arrow_schema(&columns_with_deselected);
        assert_eq!(
            schema2.fields().len(),
            1,
            "deselected column should be excluded"
        );
        assert!(schema2.field_with_name("id").is_ok());
        assert!(schema2.field_with_name("secret").is_err());

        let id_field = schema.field_with_name("id").unwrap();
        assert_eq!(*id_field.data_type(), DataType::Int32);
        assert!(!id_field.is_nullable());

        let name_field = schema.field_with_name("name").unwrap();
        assert_eq!(*name_field.data_type(), DataType::Utf8);
        assert!(name_field.is_nullable());
    }

    // --- select_default_schema ---

    fn make_virtual_schema(real_name: &str) -> VirtualCatalogSchema {
        VirtualCatalogSchema {
            schema_name: real_name.to_string(),
            tables: HashMap::new(),
        }
    }

    #[test]
    fn test_select_default_schema_public_no_alias() {
        let mut map = HashMap::new();
        map.insert("public".to_string(), make_virtual_schema("public"));
        assert_eq!(select_default_schema(&map), "public");
    }

    #[test]
    fn test_select_default_schema_public_with_alias() {
        // "public" schema is exposed under the alias "main"
        let mut map = HashMap::new();
        map.insert("main".to_string(), make_virtual_schema("public"));
        assert_eq!(select_default_schema(&map), "main");
    }

    #[test]
    fn test_select_default_schema_no_public_uses_first_key() {
        let mut map = HashMap::new();
        map.insert("analytics".to_string(), make_virtual_schema("analytics"));
        // Only one schema present — must be returned
        assert_eq!(select_default_schema(&map), "analytics");
    }

    #[test]
    fn test_select_default_schema_prefers_public_over_first() {
        // "public" is not the first key in insertion order; it should still win.
        let mut map = HashMap::new();
        map.insert("analytics".to_string(), make_virtual_schema("analytics"));
        map.insert("public".to_string(), make_virtual_schema("public"));
        assert_eq!(select_default_schema(&map), "public");
    }

    #[test]
    fn test_select_default_schema_aliased_public_over_first() {
        let mut map = HashMap::new();
        map.insert("analytics".to_string(), make_virtual_schema("analytics"));
        map.insert("main".to_string(), make_virtual_schema("public"));
        assert_eq!(select_default_schema(&map), "main");
    }

    #[test]
    fn test_select_default_schema_empty_map_returns_literal_public() {
        let map: HashMap<String, VirtualCatalogSchema> = HashMap::new();
        assert_eq!(select_default_schema(&map), "public");
    }

    // ── BetweenRowsPostgresDialect JSON unparsing tests ───────────────────────

    fn unparse(expr: &Expr) -> String {
        let unparser = Unparser::new(&BetweenRowsPostgresDialect);
        unparser.expr_to_sql(expr).unwrap().to_string()
    }

    #[test]
    fn test_dialect_json_as_text_single_key() {
        // json_as_text(payload, 'name') → "payload"->>'name'
        let func = datafusion_functions_json::udfs::json_as_text_udf();
        let expr = func.call(vec![
            datafusion::prelude::col("payload"),
            datafusion::prelude::lit("name"),
        ]);
        assert_eq!(unparse(&expr), r#""payload" ->> 'name'"#);
    }

    #[test]
    fn test_dialect_json_as_text_chained() {
        // json_as_text(payload, 'a', 'b') → "payload" -> 'a' ->> 'b'
        let func = datafusion_functions_json::udfs::json_as_text_udf();
        let expr = func.call(vec![
            datafusion::prelude::col("payload"),
            datafusion::prelude::lit("a"),
            datafusion::prelude::lit("b"),
        ]);
        assert_eq!(unparse(&expr), r#""payload" -> 'a' ->> 'b'"#);
    }

    #[test]
    fn test_dialect_json_get_single_key() {
        // json_get(payload, 'tags') → "payload" -> 'tags'
        let func = datafusion_functions_json::udfs::json_get_udf();
        let expr = func.call(vec![
            datafusion::prelude::col("payload"),
            datafusion::prelude::lit("tags"),
        ]);
        assert_eq!(unparse(&expr), r#""payload" -> 'tags'"#);
    }

    #[test]
    fn test_dialect_json_get_chained() {
        // json_get(payload, 'a', 'b') → "payload" -> 'a' -> 'b'
        let func = datafusion_functions_json::udfs::json_get_udf();
        let expr = func.call(vec![
            datafusion::prelude::col("payload"),
            datafusion::prelude::lit("a"),
            datafusion::prelude::lit("b"),
        ]);
        assert_eq!(unparse(&expr), r#""payload" -> 'a' -> 'b'"#);
    }

    #[test]
    fn test_dialect_json_contains() {
        // json_contains(payload, 'key') → "payload" ? 'key'
        let func = datafusion_functions_json::udfs::json_contains_udf();
        let expr = func.call(vec![
            datafusion::prelude::col("payload"),
            datafusion::prelude::lit("key"),
        ]);
        assert_eq!(unparse(&expr), r#""payload" ? 'key'"#);
    }

    // ─── compute_user_visibility regression tests ───────────────────────────

    async fn setup_visibility_db() -> sea_orm::DatabaseConnection {
        use migration::MigratorTrait as _;
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        db
    }

    fn make_catalog(ds_id: Uuid, ds_access_mode: &str) -> CachedCatalog {
        let mut tables = HashMap::new();

        // employees: multiple columns including name, secret_, and _at suffix columns
        let emp_schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("first_name", DataType::Utf8, true),
            Field::new("last_name", DataType::Utf8, true),
            Field::new("email", DataType::Utf8, true),
            Field::new("ssn", DataType::Utf8, true),
            Field::new("secret_key", DataType::Utf8, true),
            Field::new("secret_token", DataType::Utf8, true),
            Field::new("created_at", DataType::Utf8, true),
        ]));
        tables.insert(
            "employees".to_string(),
            VirtualCatalogTable {
                table_name: "employees".to_string(),
                arrow_schema: emp_schema,
            },
        );

        // orders: separate table to test cross-table isolation
        let ord_schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("total", DataType::Float64, true),
            Field::new("placed_at", DataType::Utf8, true),
        ]));
        tables.insert(
            "orders".to_string(),
            VirtualCatalogTable {
                table_name: "orders".to_string(),
                arrow_schema: ord_schema,
            },
        );

        let mut schemas = HashMap::new();
        schemas.insert(
            "public".to_string(),
            VirtualCatalogSchema {
                schema_name: "public".to_string(),
                tables,
            },
        );
        CachedCatalog {
            datasource_id: ds_id,
            schemas,
            default_schema: "public".to_string(),
            access_mode: ds_access_mode.to_string(),
        }
    }

    async fn insert_test_user(db: &sea_orm::DatabaseConnection, user_id: Uuid) {
        use crate::entity::proxy_user;
        use chrono::Utc;
        use sea_orm::ActiveModelTrait;

        let now = Utc::now().naive_utc();
        proxy_user::ActiveModel {
            id: sea_orm::Set(user_id),
            username: sea_orm::Set(format!("user-{user_id}")),
            password_hash: sea_orm::Set("hash".to_string()),
            is_admin: sea_orm::Set(false),
            is_active: sea_orm::Set(true),
            email: sea_orm::Set(None),
            display_name: sea_orm::Set(None),
            last_login_at: sea_orm::Set(None),
            created_at: sea_orm::Set(now),
            updated_at: sea_orm::Set(now),
            attributes: sea_orm::Set("{}".to_string()),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn insert_test_datasource(db: &sea_orm::DatabaseConnection, ds_id: Uuid) {
        use crate::entity::data_source;
        use chrono::Utc;
        use sea_orm::ActiveModelTrait;

        let now = Utc::now().naive_utc();
        data_source::ActiveModel {
            id: sea_orm::Set(ds_id),
            name: sea_orm::Set(format!("ds-{ds_id}")),
            ds_type: sea_orm::Set("postgres".to_string()),
            config: sea_orm::Set("{}".to_string()),
            secure_config: sea_orm::Set(String::new()),
            is_active: sea_orm::Set(true),
            access_mode: sea_orm::Set("open".to_string()),
            last_sync_at: sea_orm::Set(None),
            last_sync_result: sea_orm::Set(None),
            created_at: sea_orm::Set(now),
            updated_at: sea_orm::Set(now),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn insert_policy_with_column_deny(
        db: &sea_orm::DatabaseConnection,
        ds_id: Uuid,
        user_id: Uuid,
        is_enabled: bool,
        // ignored — column_deny type is always a deny type now
        _effect: &str,
    ) {
        insert_policy_with_custom_column_deny(
            db,
            ds_id,
            user_id,
            is_enabled,
            "public",
            "employees",
            &["ssn"],
        )
        .await;
    }

    async fn insert_policy_with_custom_column_deny(
        db: &sea_orm::DatabaseConnection,
        ds_id: Uuid,
        user_id: Uuid,
        is_enabled: bool,
        schema: &str,
        table: &str,
        columns: &[&str],
    ) {
        use crate::entity::{policy, policy_assignment};
        use chrono::Utc;
        use sea_orm::ActiveModelTrait;

        let policy_id = Uuid::now_v7();
        let now = Utc::now().naive_utc();
        let cols: Vec<serde_json::Value> = columns.iter().map(|c| serde_json::json!(c)).collect();
        let targets_json = serde_json::json!([{
            "schemas": [schema],
            "tables": [table],
            "columns": cols
        }])
        .to_string();

        policy::ActiveModel {
            id: sea_orm::Set(policy_id),
            name: sea_orm::Set(format!("test-policy-{policy_id}")),
            description: sea_orm::Set(None),
            policy_type: sea_orm::Set("column_deny".to_string()),
            targets: sea_orm::Set(targets_json),
            definition: sea_orm::Set(None),
            is_enabled: sea_orm::Set(is_enabled),
            version: sea_orm::Set(1),
            decision_function_id: sea_orm::Set(None),
            created_by: sea_orm::Set(user_id),
            updated_by: sea_orm::Set(user_id),
            created_at: sea_orm::Set(now),
            updated_at: sea_orm::Set(now),
        }
        .insert(db)
        .await
        .unwrap();

        policy_assignment::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            policy_id: sea_orm::Set(policy_id),
            data_source_id: sea_orm::Set(ds_id),
            user_id: sea_orm::Set(Some(user_id)),
            role_id: sea_orm::Set(None),
            assignment_scope: sea_orm::Set("user".to_string()),
            priority: sea_orm::Set(100),
            created_at: sea_orm::Set(now),
            updated_at: sea_orm::Set(now),
        }
        .insert(db)
        .await
        .unwrap();
    }

    /// Disabled `column_deny` policy must NOT appear in denied_columns.
    #[tokio::test]
    async fn test_disabled_policy_column_deny_not_applied() {
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_test_datasource(&db, ds_id).await;
        insert_policy_with_column_deny(&db, ds_id, user_id, false, "deny").await;

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let catalog = make_catalog(ds_id, "open");
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        // No effects from disabled policy → no denied columns
        match vis.filter {
            None => {} // expected for open mode with no denies
            Some(f) => assert!(
                f.denied_columns.is_empty(),
                "Disabled policy should not contribute denied columns, got: {:?}",
                f.denied_columns
            ),
        }
    }

    /// Enabled `column_deny` policy MUST appear in denied_columns.
    #[tokio::test]
    async fn test_enabled_policy_column_deny_applied() {
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_test_datasource(&db, ds_id).await;
        insert_policy_with_column_deny(&db, ds_id, user_id, true, "deny").await;

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let catalog = make_catalog(ds_id, "open");
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        let denied = vis
            .filter
            .expect("Expected a visibility filter with denied columns")
            .denied_columns;

        assert!(
            denied.iter().any(|(_, _, col)| col == "ssn"),
            "Expected 'ssn' in denied_columns, got: {:?}",
            denied
        );
    }

    // --- column glob expansion tests ---

    fn denied_col_names(
        denied: &std::collections::HashSet<(String, String, String)>,
        table: &str,
    ) -> Vec<String> {
        let mut cols: Vec<String> = denied
            .iter()
            .filter(|(_, t, _)| t == table)
            .map(|(_, _, c)| c.clone())
            .collect();
        cols.sort();
        cols
    }

    #[tokio::test]
    async fn test_column_deny_total_blackout() {
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_test_datasource(&db, ds_id).await;
        insert_policy_with_custom_column_deny(
            &db,
            ds_id,
            user_id,
            true,
            "public",
            "employees",
            &["*"],
        )
        .await;

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let catalog = make_catalog(ds_id, "open");
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        let denied = vis
            .filter
            .expect("Expected visibility filter")
            .denied_columns;
        let emp_denied = denied_col_names(&denied, "employees");

        // All 8 columns in employees should be denied
        assert_eq!(
            emp_denied,
            vec![
                "created_at",
                "email",
                "first_name",
                "id",
                "last_name",
                "secret_key",
                "secret_token",
                "ssn"
            ],
            "Wildcard * should deny all columns: {emp_denied:?}"
        );
    }

    #[tokio::test]
    async fn test_column_deny_suffix_glob() {
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_test_datasource(&db, ds_id).await;
        insert_policy_with_custom_column_deny(
            &db,
            ds_id,
            user_id,
            true,
            "public",
            "employees",
            &["*_name"],
        )
        .await;

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let catalog = make_catalog(ds_id, "open");
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        let denied = vis
            .filter
            .expect("Expected visibility filter")
            .denied_columns;
        let emp_denied = denied_col_names(&denied, "employees");

        assert!(
            emp_denied.contains(&"first_name".to_string()),
            "first_name should be denied: {emp_denied:?}"
        );
        assert!(
            emp_denied.contains(&"last_name".to_string()),
            "last_name should be denied: {emp_denied:?}"
        );
        // columns NOT ending in _name must not be denied
        for col in &[
            "email",
            "id",
            "ssn",
            "secret_key",
            "secret_token",
            "created_at",
        ] {
            assert!(
                !emp_denied.contains(&col.to_string()),
                "{col} should NOT be denied: {emp_denied:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_column_deny_prefix_glob() {
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_test_datasource(&db, ds_id).await;
        insert_policy_with_custom_column_deny(
            &db,
            ds_id,
            user_id,
            true,
            "public",
            "employees",
            &["secret_*"],
        )
        .await;

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let catalog = make_catalog(ds_id, "open");
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        let denied = vis
            .filter
            .expect("Expected visibility filter")
            .denied_columns;
        let emp_denied = denied_col_names(&denied, "employees");

        assert!(
            emp_denied.contains(&"secret_key".to_string()),
            "secret_key should be denied: {emp_denied:?}"
        );
        assert!(
            emp_denied.contains(&"secret_token".to_string()),
            "secret_token should be denied: {emp_denied:?}"
        );
        for col in &[
            "first_name",
            "last_name",
            "email",
            "id",
            "ssn",
            "created_at",
        ] {
            assert!(
                !emp_denied.contains(&col.to_string()),
                "{col} should NOT be denied: {emp_denied:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_column_deny_multiple_patterns() {
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_test_datasource(&db, ds_id).await;
        insert_policy_with_custom_column_deny(
            &db,
            ds_id,
            user_id,
            true,
            "public",
            "employees",
            &["*_at", "secret_*"],
        )
        .await;

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let catalog = make_catalog(ds_id, "open");
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        let denied = vis
            .filter
            .expect("Expected visibility filter")
            .denied_columns;
        let emp_denied = denied_col_names(&denied, "employees");

        for col in &["created_at", "secret_key", "secret_token"] {
            assert!(
                emp_denied.contains(&col.to_string()),
                "{col} should be denied: {emp_denied:?}"
            );
        }
        for col in &["first_name", "last_name", "email", "id", "ssn"] {
            assert!(
                !emp_denied.contains(&col.to_string()),
                "{col} should NOT be denied: {emp_denied:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_column_deny_cross_table_isolation() {
        // Deny all columns on employees — orders table must remain fully visible.
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_test_datasource(&db, ds_id).await;
        insert_policy_with_custom_column_deny(
            &db,
            ds_id,
            user_id,
            true,
            "public",
            "employees",
            &["*"],
        )
        .await;

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let catalog = make_catalog(ds_id, "open");
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        let denied = vis
            .filter
            .expect("Expected visibility filter")
            .denied_columns;
        let orders_denied = denied_col_names(&denied, "orders");

        assert!(
            orders_denied.is_empty(),
            "orders columns must not be denied when deny only targets employees: {orders_denied:?}"
        );
    }

    #[tokio::test]
    async fn test_column_deny_exact_regression() {
        // Exact column name — only that one column should appear in denied.
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_test_datasource(&db, ds_id).await;
        insert_policy_with_custom_column_deny(
            &db,
            ds_id,
            user_id,
            true,
            "public",
            "employees",
            &["ssn"],
        )
        .await;

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let catalog = make_catalog(ds_id, "open");
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        let denied = vis
            .filter
            .expect("Expected visibility filter")
            .denied_columns;
        let emp_denied = denied_col_names(&denied, "employees");

        assert_eq!(
            emp_denied,
            vec!["ssn"],
            "Only ssn should be denied: {emp_denied:?}"
        );
    }

    #[tokio::test]
    async fn test_column_deny_large_schema() {
        // Table with 100 columns, deny ["*"] — all 100 must appear in denied_columns.
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_test_datasource(&db, ds_id).await;
        insert_policy_with_custom_column_deny(
            &db,
            ds_id,
            user_id,
            true,
            "public",
            "big_table",
            &["*"],
        )
        .await;

        // Build a catalog with a "big_table" of 100 columns
        let fields: Vec<Field> = (0..100)
            .map(|i| Field::new(format!("col_{i:03}"), DataType::Utf8, true))
            .collect();
        let big_schema = Arc::new(Schema::new(fields));
        let mut tables = HashMap::new();
        tables.insert(
            "big_table".to_string(),
            VirtualCatalogTable {
                table_name: "big_table".to_string(),
                arrow_schema: big_schema,
            },
        );
        let mut schemas = HashMap::new();
        schemas.insert(
            "public".to_string(),
            VirtualCatalogSchema {
                schema_name: "public".to_string(),
                tables,
            },
        );
        let catalog = CachedCatalog {
            datasource_id: ds_id,
            schemas,
            default_schema: "public".to_string(),
            access_mode: "open".to_string(),
        };

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        let denied = vis
            .filter
            .expect("Expected visibility filter")
            .denied_columns;
        let big_denied = denied_col_names(&denied, "big_table");

        assert_eq!(big_denied.len(), 100, "All 100 columns should be denied");
    }

    // ─── schema alias + policy_required regression tests ────────────────────

    async fn insert_datasource_with_access_mode(
        db: &sea_orm::DatabaseConnection,
        ds_id: Uuid,
        access_mode: &str,
    ) {
        use crate::entity::data_source;
        use chrono::Utc;
        use sea_orm::ActiveModelTrait;

        let now = Utc::now().naive_utc();
        data_source::ActiveModel {
            id: sea_orm::Set(ds_id),
            name: sea_orm::Set(format!("ds-alias-{ds_id}")),
            ds_type: sea_orm::Set("postgres".to_string()),
            config: sea_orm::Set("{}".to_string()),
            secure_config: sea_orm::Set(String::new()),
            is_active: sea_orm::Set(true),
            access_mode: sea_orm::Set(access_mode.to_string()),
            last_sync_at: sea_orm::Set(None),
            last_sync_result: sea_orm::Set(None),
            created_at: sea_orm::Set(now),
            updated_at: sea_orm::Set(now),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn insert_permit_policy_column_access_all(
        db: &sea_orm::DatabaseConnection,
        ds_id: Uuid,
        user_id: Uuid,
    ) {
        use crate::entity::{policy, policy_assignment};
        use chrono::Utc;
        use sea_orm::ActiveModelTrait;

        let policy_id = Uuid::now_v7();
        let now = Utc::now().naive_utc();
        let targets_json = serde_json::json!([{
            "schemas": ["*"],
            "tables": ["*"],
            "columns": ["*"]
        }])
        .to_string();

        policy::ActiveModel {
            id: sea_orm::Set(policy_id),
            name: sea_orm::Set(format!("permit-col-allow-all-{policy_id}")),
            description: sea_orm::Set(None),
            policy_type: sea_orm::Set("column_allow".to_string()),
            targets: sea_orm::Set(targets_json),
            definition: sea_orm::Set(None),
            is_enabled: sea_orm::Set(true),
            version: sea_orm::Set(1),
            decision_function_id: sea_orm::Set(None),
            created_by: sea_orm::Set(user_id),
            updated_by: sea_orm::Set(user_id),
            created_at: sea_orm::Set(now),
            updated_at: sea_orm::Set(now),
        }
        .insert(db)
        .await
        .unwrap();

        policy_assignment::ActiveModel {
            id: sea_orm::Set(Uuid::now_v7()),
            policy_id: sea_orm::Set(policy_id),
            data_source_id: sea_orm::Set(ds_id),
            user_id: sea_orm::Set(None), // wildcard — applies to all users
            role_id: sea_orm::Set(None),
            assignment_scope: sea_orm::Set("all".to_string()),
            priority: sea_orm::Set(100),
            created_at: sea_orm::Set(now),
            updated_at: sea_orm::Set(now),
        }
        .insert(db)
        .await
        .unwrap();
    }

    fn make_aliased_catalog(ds_id: Uuid, access_mode: &str) -> CachedCatalog {
        // Source schema "public" is exposed to DataFusion under alias "public_alias".
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, true),
        ]));
        let mut tables = HashMap::new();
        tables.insert(
            "performer".to_string(),
            VirtualCatalogTable {
                table_name: "performer".to_string(),
                arrow_schema: schema,
            },
        );
        let mut schemas = HashMap::new();
        schemas.insert(
            "public_alias".to_string(),
            VirtualCatalogSchema {
                schema_name: "public".to_string(),
                tables,
            },
        );
        CachedCatalog {
            datasource_id: ds_id,
            schemas,
            default_schema: "public_alias".to_string(),
            access_mode: access_mode.to_string(),
        }
    }

    /// Regression: a permit policy with `column_access` columns:["*"] must grant full
    /// visibility in policy_required mode. Before the action-field removal, the same
    /// policy with action:"deny" would produce visible_tables=Some(empty) — making all
    /// tables invisible and causing "table not found" on unqualified queries due to the
    /// default schema falling back to the hardcoded "public" instead of the alias.
    #[tokio::test]
    async fn test_permit_column_access_wildcard_grants_full_visibility_policy_required() {
        let db = setup_visibility_db().await;
        let ds_id = Uuid::now_v7();
        let user_id = Uuid::now_v7();

        insert_test_user(&db, user_id).await;
        insert_datasource_with_access_mode(&db, ds_id, "policy_required").await;
        insert_permit_policy_column_access_all(&db, ds_id, user_id).await;

        let cache = EngineCache::new(db, [0u8; 32], shared_wasm_runtime());
        let catalog = make_aliased_catalog(ds_id, "policy_required");
        let vis = cache
            .compute_user_visibility(user_id, &catalog)
            .await
            .unwrap();

        let filter = vis
            .filter
            .expect("Expected a visibility filter for policy_required datasource");
        let visible = filter
            .visible_tables
            .expect("Expected Some(visible_tables) in policy_required mode");

        // permit + column_access ["*"] must make the table visible.
        assert!(
            visible.contains(&("public_alias".to_string(), "performer".to_string())),
            "Expected performer to be visible; got: {visible:?}"
        );
    }

    /// Regression: select_default_schema returns "public" for an empty map (when all tables
    /// are filtered by policy). The caller (build_user_context) must fall back to
    /// catalog.default_schema rather than this hardcoded value so that unqualified table
    /// references resolve to the correct alias schema.
    #[test]
    fn test_select_default_schema_empty_map_falls_back_to_public() {
        let empty: HashMap<String, VirtualCatalogSchema> = HashMap::new();
        // Documents the fallback: caller must prefer catalog.default_schema over this.
        assert_eq!(select_default_schema(&empty), "public");
    }
}
