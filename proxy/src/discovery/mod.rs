use std::fmt;
use tokio_util::sync::CancellationToken;

pub mod postgres;

// ---------- DTOs ----------

#[derive(Debug, Clone)]
pub struct DiscoveredSchema {
    pub schema_name: String,
}

#[derive(Debug, Clone)]
pub struct DiscoveredTable {
    pub schema_name: String,
    pub table_name: String,
    /// "TABLE", "VIEW", or "MATERIALIZED VIEW"
    pub table_type: String,
}

#[derive(Debug, Clone)]
pub struct DiscoveredColumn {
    pub schema_name: String,
    pub table_name: String,
    pub column_name: String,
    pub ordinal_position: i32,
    /// Upstream Postgres type string (e.g. "character varying", "int4")
    pub data_type: String,
    pub is_nullable: bool,
    pub column_default: Option<String>,
    /// Mapped Arrow/DataFusion type string. None = unsupported.
    pub arrow_type: Option<String>,
}

/// A foreign key introspected from the upstream database.
///
/// Only single-column FKs referencing a PK or single-column unique on the parent
/// are returned — this matches the rewriter's at-most-one-parent-per-child safety
/// invariant used by `column_anchor` resolution.
#[derive(Debug, Clone)]
pub struct DiscoveredForeignKey {
    pub child_schema: String,
    pub child_table: String,
    pub child_column: String,
    pub parent_schema: String,
    pub parent_table: String,
    pub parent_column: String,
    /// `pg_constraint.conname` — displayed to admins in the UI picker; not persisted.
    pub fk_constraint_name: String,
}

// ---------- errors ----------

#[derive(Debug)]
pub enum DiscoveryError {
    Connect(String),
    Query(String),
    UnsupportedType(String),
    Cancelled,
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscoveryError::Connect(msg) => write!(f, "Connection error: {msg}"),
            DiscoveryError::Query(msg) => write!(f, "Query error: {msg}"),
            DiscoveryError::UnsupportedType(msg) => write!(f, "Unsupported type: {msg}"),
            DiscoveryError::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl std::error::Error for DiscoveryError {}

// ---------- trait ----------

#[async_trait::async_trait]
pub trait DiscoveryProvider: Send + Sync {
    async fn discover_schemas(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<DiscoveredSchema>, DiscoveryError>;

    async fn discover_tables(
        &self,
        schemas: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<DiscoveredTable>, DiscoveryError>;

    async fn discover_columns(
        &self,
        tables: &[(String, String)], // (schema_name, table_name)
        cancel: &CancellationToken,
    ) -> Result<Vec<DiscoveredColumn>, DiscoveryError>;

    /// Live-introspect single-column foreign keys whose parent column is a PK or
    /// single-column unique. Results are filtered to FKs whose both endpoints
    /// are in the given `tables` list.
    ///
    /// Called on-demand by the `fk-suggestions` admin endpoint. Nothing is
    /// persisted by this call — it purely fuels the admin UI's designation picker.
    async fn discover_foreign_keys(
        &self,
        tables: &[(String, String)], // (schema_name, table_name)
        cancel: &CancellationToken,
    ) -> Result<Vec<DiscoveredForeignKey>, DiscoveryError>;
}

// ---------- factory ----------

use crate::engine::DataSourceConfig;

pub fn create_provider(
    ds_type: &str,
    cfg: DataSourceConfig,
) -> Result<Box<dyn DiscoveryProvider>, DiscoveryError> {
    match ds_type {
        "postgres" => Ok(Box::new(postgres::PostgresDiscoveryProvider::new(cfg))),
        other => Err(DiscoveryError::UnsupportedType(format!(
            "No discovery provider for data source type: {other}"
        ))),
    }
}
