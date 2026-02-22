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
