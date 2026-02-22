use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit};
use datafusion::catalog::{CatalogProvider, SchemaProvider};
use datafusion::prelude::{SessionContext, SessionConfig};
use datafusion::sql::TableReference;
use datafusion::sql::unparser::dialect::PostgreSqlDialect;
use datafusion_table_providers::{
    postgres::DynPostgresConnectionPool,
    sql::{
        db_connection_pool::postgrespool::PostgresConnectionPool,
        sql_provider_datafusion::SqlTable,
    },
    util::secrets::to_secret_map,
    UnsupportedTypeAction,
};
use datafusion_pg_catalog::pg_catalog::{setup_pg_catalog, context::PgCatalogContextProvider};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::any::Any;
use uuid::Uuid;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::RwLock as AsyncRwLock;

use crate::entity::{data_source, discovered_column, discovered_schema, discovered_table, user_data_source};

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

        let config: serde_json::Value = serde_json::from_str(&model.config)
            .map_err(|e| format!("Invalid config JSON: {e}"))?;

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
            port: config["port"]
                .as_u64()
                .ok_or("missing port in config")? as u16,
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
    tables: HashMap<String, VirtualCatalogTable>,
}

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
            .with_unsupported_type_action(UnsupportedTypeAction::Warn);
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
        "Timestamp(Nanosecond,None)" => {
            Some(DataType::Timestamp(TimeUnit::Nanosecond, None))
        }
        "Timestamp(Microsecond,None)" => {
            Some(DataType::Timestamp(TimeUnit::Microsecond, None))
        }
        s if s.starts_with("Timestamp(Nanosecond,Some(") => {
            Some(DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())))
        }
        s if s.starts_with("Timestamp(Microsecond,Some(") => {
            Some(DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())))
        }
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
        DataType::Timestamp(TimeUnit::Nanosecond, None) => {
            "Timestamp(Nanosecond,None)".to_string()
        }
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
            let arrow_type_str = col.arrow_type.as_deref()?;
            let data_type = parse_arrow_type(arrow_type_str)?;
            Some((col.ordinal_position, Field::new(&col.column_name, data_type, col.is_nullable)))
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
            datafusion::error::DataFusionError::External(
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, e))
            )
        })?;

        let table = SqlTable::new_with_schema(
            "postgres",
            &pool,
            arrow_schema,
            TableReference::full("postgres", self.schema_name.as_str(), name),
        )
        .with_dialect(Arc::new(PostgreSqlDialect {}));

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
        self.schemas.get(name).map(|s| s.clone() as Arc<dyn SchemaProvider>)
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

/// Build a SessionContext from local catalog metadata using a shared LazyPool.
///
/// Pool creation is deferred until the first user-table query, so pg_catalog /
/// information_schema queries complete instantly without an upstream connection.
async fn create_session_context_from_catalog(
    catalog_schemas: HashMap<String, VirtualCatalogSchema>,
    lazy_pool: Arc<LazyPool>,
) -> Result<SessionContext, Box<dyn std::error::Error + Send + Sync>> {
    // Build VirtualSchemaProviders — all share the same lazy pool
    let mut schema_providers: HashMap<String, Arc<VirtualSchemaProvider>> = HashMap::new();
    for (schema_name, catalog_schema) in catalog_schemas {
        let tables: HashMap<String, SchemaRef> = catalog_schema
            .tables
            .into_values()
            .map(|t| (t.table_name, t.arrow_schema))
            .collect();

        schema_providers.insert(
            schema_name.clone(),
            Arc::new(VirtualSchemaProvider {
                schema_name,
                tables,
                pool: Arc::clone(&lazy_pool),
            }),
        );
    }

    let virtual_catalog = VirtualCatalogProvider { schemas: schema_providers };
    let catalog = ExtensibleCatalogProvider::new(virtual_catalog);

    let config = SessionConfig::new()
        .with_information_schema(true)
        .with_default_catalog_and_schema("postgres", "public");

    let ctx = SessionContext::new_with_config(config);
    ctx.register_catalog("postgres", Arc::new(catalog));

    setup_pg_catalog(&ctx, "postgres", ProxyCatalogContext)
        .map_err(|e| format!("Failed to setup pg_catalog: {}", e))?;

    tracing::debug!("SessionContext ready (pool deferred until first user-table query)");

    Ok(ctx)
}

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
    contexts: AsyncRwLock<HashMap<String, Arc<SessionContext>>>,
    /// Shared LazyPool per datasource. Survives SessionContext invalidation so
    /// re-discovery after catalog changes reuses the existing upstream connection pool.
    pools: AsyncRwLock<HashMap<String, Arc<LazyPool>>>,
}

impl EngineCache {
    pub fn new(db: DatabaseConnection, master_key: [u8; 32]) -> Arc<Self> {
        Arc::new(Self {
            db,
            master_key,
            contexts: AsyncRwLock::new(HashMap::new()),
            pools: AsyncRwLock::new(HashMap::new()),
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

        let assignment = user_data_source::Entity::find()
            .filter(user_data_source::Column::UserId.eq(user_id))
            .filter(user_data_source::Column::DataSourceId.eq(ds.id))
            .one(&self.db)
            .await
            .map_err(|e| EngineError(format!("DB error: {e}")))?;

        Ok(assignment.is_some())
    }

    /// Get (or lazily create) the SessionContext for a named data source.
    /// Uses virtual schema from local catalog metadata — no live introspection.
    /// The upstream connection pool is initialised lazily on the first user-table query.
    pub async fn get_context(
        &self,
        name: &str,
    ) -> Result<Arc<SessionContext>, Box<dyn std::error::Error + Send + Sync>> {
        // Fast path: read lock
        {
            let map = self.contexts.read().await;
            if let Some(ctx) = map.get(name) {
                return Ok(ctx.clone());
            }
        }

        // Slow path: write lock (double-check)
        let mut map = self.contexts.write().await;
        if let Some(ctx) = map.get(name) {
            return Ok(ctx.clone());
        }

        let ds = data_source::Entity::find()
            .filter(data_source::Column::Name.eq(name))
            .filter(data_source::Column::IsActive.eq(true))
            .one(&self.db)
            .await
            .map_err(|e| EngineError(format!("DB error: {e}")))?
            .ok_or_else(|| EngineError(format!("Data source '{}' not found or inactive", name)))?;

        let cfg = DataSourceConfig::from_model(&ds, &self.master_key)?;

        // Get or create LazyPool (shared across SessionContext rebuilds).
        // Acquiring pools.write() while holding contexts.write() is safe because
        // all other code paths acquire at most one of these locks at a time.
        let lazy_pool = {
            let mut pools = self.pools.write().await;
            if let Some(p) = pools.get(name) {
                p.clone()
            } else {
                let params = build_postgres_params(&cfg);
                let p = Arc::new(LazyPool::new(params));
                pools.insert(name.to_string(), p.clone());
                p
            }
        };

        // Load stored catalog for this data source
        let schemas_with_tables: Vec<(discovered_schema::Model, Vec<discovered_table::Model>)> =
            discovered_schema::Entity::find()
                .filter(discovered_schema::Column::DataSourceId.eq(ds.id))
                .filter(discovered_schema::Column::IsSelected.eq(true))
                .find_with_related(discovered_table::Entity)
                .all(&self.db)
                .await
                .map_err(|e| EngineError(format!("DB error loading catalog: {e}")))?;

        // Build virtual catalog data
        let mut catalog_schemas: HashMap<String, VirtualCatalogSchema> = HashMap::new();

        for (schema, tables) in schemas_with_tables {
            let mut catalog_tables: HashMap<String, VirtualCatalogTable> = HashMap::new();

            for table in tables.into_iter().filter(|t| t.is_selected) {
                // Load columns for this table
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

            catalog_schemas.insert(
                schema.schema_name.clone(),
                VirtualCatalogSchema { tables: catalog_tables },
            );
        }

        let ctx = create_session_context_from_catalog(catalog_schemas, lazy_pool).await?;
        let ctx = Arc::new(ctx);
        map.insert(name.to_string(), ctx.clone());
        Ok(ctx)
    }

    /// Remove a data source's cached SessionContext (call after catalog re-discovery).
    /// Keeps the shared pool so subsequent queries don't need a new connection.
    pub async fn invalidate(&self, name: &str) {
        self.contexts.write().await.remove(name);
    }

    /// Remove both the cached SessionContext AND the shared pool
    /// (call after datasource connection params are edited or datasource is deleted).
    pub async fn invalidate_all(&self, name: &str) {
        self.contexts.write().await.remove(name);
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
                Err(e) => tracing::debug!(datasource = %name, error = %e, "Pool warmup failed (non-fatal)"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::catalog::{CatalogProvider, SchemaProvider};
    use datafusion::datasource::TableProvider;
    use datafusion::error::Result as DFResult;
    use std::any::Any;

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
            Ok(self.schemas.write().unwrap().insert(name.to_string(), schema))
        }
    }

    /// Mock SchemaProvider for testing
    #[derive(Debug)]
    struct MockSchemaProvider {
        name: String,
    }

    impl MockSchemaProvider {
        fn new(name: &str) -> Self {
            Self { name: name.to_string() }
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
                "MockInnerCatalogProvider does not support register_schema".to_string()
            ))
        }
    }

    #[test]
    fn test_schema_names_includes_extra() {
        let inner = MockCatalogProvider::new();
        let catalog = ExtensibleCatalogProvider::new(MockInnerCatalogProvider::new(inner));

        let schema = Arc::new(MockSchemaProvider::new("test_schema")) as Arc<dyn SchemaProvider>;
        catalog.register_schema("test_schema", schema).expect("Failed to register schema");

        let names = catalog.schema_names();
        assert!(names.contains(&"test_schema".to_string()),
                "Expected test_schema to appear in schema_names, got: {:?}", names);
    }

    #[test]
    fn test_schema_lookup_extra_first() {
        let inner = MockCatalogProvider::new();

        let inner_schema = Arc::new(MockSchemaProvider::new("inner_version")) as Arc<dyn SchemaProvider>;
        inner.register_schema("shared", inner_schema.clone()).expect("Failed to register inner schema");

        let catalog = ExtensibleCatalogProvider::new(MockInnerCatalogProvider::new(inner));

        let extra_schema = Arc::new(MockSchemaProvider::new("extra_version")) as Arc<dyn SchemaProvider>;
        catalog.register_schema("shared", extra_schema.clone()).expect("Failed to register extra schema");

        let retrieved = catalog.schema("shared").expect("Failed to retrieve schema");
        let as_mock = retrieved.as_any().downcast_ref::<MockSchemaProvider>()
            .expect("Failed to downcast to MockSchemaProvider");

        assert_eq!(as_mock.name, "extra_version",
                "Expected extra schema to take priority over inner schema");
    }

    #[test]
    fn test_register_schema_returns_previous() {
        let inner = MockCatalogProvider::new();
        let catalog = ExtensibleCatalogProvider::new(MockInnerCatalogProvider::new(inner));

        let schema1 = Arc::new(MockSchemaProvider::new("version1")) as Arc<dyn SchemaProvider>;
        let schema2 = Arc::new(MockSchemaProvider::new("version2")) as Arc<dyn SchemaProvider>;

        let result1 = catalog.register_schema("my_schema", schema1)
            .expect("Failed to register schema");
        assert!(result1.is_none(), "Expected None on first registration");

        let result2 = catalog.register_schema("my_schema", schema2)
            .expect("Failed to register schema");
        assert!(result2.is_some(), "Expected Some on second registration");

        let prev_schema = result2.unwrap();
        let as_mock = prev_schema.as_any().downcast_ref::<MockSchemaProvider>()
            .expect("Failed to downcast to MockSchemaProvider");
        assert_eq!(as_mock.name, "version1",
                "Expected previous schema to be returned");
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
            }).to_string(),
            secure_config: encrypted,
            is_active: true,
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
        assert!(params.contains_key("db"), "missing 'db' (pool reads 'db', not 'dbname')");
        assert!(params.contains_key("pass"), "missing 'pass' (pool reads 'pass', not 'password')");
        assert!(params.contains_key("port"), "missing 'port'");
        assert!(params.contains_key("sslmode"), "missing 'sslmode'");

        // Regression guard: these keys are silently ignored by the pool
        assert!(!params.contains_key("dbname"), "'dbname' is ignored by the pool — use 'db'");
        assert!(!params.contains_key("password"), "'password' is ignored by the pool — use 'pass'");
        assert!(!params.contains_key("username"), "'username' is ignored by the pool — use 'user'");

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
        assert!(matches!(parse_arrow_type("Float32"), Some(DataType::Float32)));
        assert!(matches!(parse_arrow_type("Float64"), Some(DataType::Float64)));
        assert!(matches!(parse_arrow_type("Boolean"), Some(DataType::Boolean)));
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
            assert_eq!(dt, &recovered, "Round-trip failed for {dt:?}: stored as {stored:?}");
        }
    }

    #[test]
    fn test_build_arrow_schema() {
        use chrono::Utc;
        use crate::entity::discovered_column;

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
                discovered_at: now,
            },
        ];

        let schema = build_arrow_schema(&columns);
        assert_eq!(schema.fields().len(), 2, "jsonb column should be skipped");

        let id_field = schema.field_with_name("id").unwrap();
        assert_eq!(*id_field.data_type(), DataType::Int32);
        assert!(!id_field.is_nullable());

        let name_field = schema.field_with_name("name").unwrap();
        assert_eq!(*name_field.data_type(), DataType::Utf8);
        assert!(name_field.is_nullable());
    }
}
