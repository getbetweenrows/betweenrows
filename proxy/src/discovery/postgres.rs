use std::collections::HashMap;
use std::sync::Arc;
use datafusion::arrow::datatypes::Schema;
use datafusion::sql::TableReference;
use datafusion_table_providers::{
    sql::db_connection_pool::{
        DbConnectionPool,
        dbconnection::get_schema,
        postgrespool::PostgresConnectionPool,
    },
    util::secrets::to_secret_map,
    UnsupportedTypeAction,
};
use tokio_postgres::NoTls;
use tokio_util::sync::CancellationToken;

use crate::engine::{arrow_type_to_string, build_postgres_params, DataSourceConfig};
use super::{DiscoveredColumn, DiscoveredSchema, DiscoveredTable, DiscoveryError, DiscoveryProvider};

pub struct PostgresDiscoveryProvider {
    cfg: DataSourceConfig,
}

impl PostgresDiscoveryProvider {
    pub fn new(cfg: DataSourceConfig) -> Self {
        Self { cfg }
    }

    async fn connect_with_cancel(
        &self,
        cancel: &CancellationToken,
    ) -> Result<tokio_postgres::Client, DiscoveryError> {
        let ssl_mode = match self.cfg.ssl_mode.as_str() {
            "disable" => "disable",
            "allow" | "prefer" => "prefer",
            _ => "require",
        };

        let conn_str = format!(
            "host={} port={} dbname={} user={} password={} sslmode={} connect_timeout=30",
            self.cfg.host,
            self.cfg.port,
            self.cfg.database,
            self.cfg.username,
            self.cfg.password,
            ssl_mode,
        );

        let (client, connection) = tokio::select! {
            res = tokio_postgres::connect(&conn_str, NoTls) => {
                res.map_err(|e| DiscoveryError::Connect(e.to_string()))?
            }
            _ = cancel.cancelled() => {
                return Err(DiscoveryError::Cancelled);
            }
        };

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::warn!("Discovery connection error: {e}");
            }
        });

        // Set statement timeout to avoid hanging on slow queries
        tokio::select! {
            res = client.execute("SET statement_timeout = '60s'", &[]) => {
                res.map_err(|e| DiscoveryError::Query(e.to_string()))?;
            }
            _ = cancel.cancelled() => {
                return Err(DiscoveryError::Cancelled);
            }
        }

        Ok(client)
    }
}

#[async_trait::async_trait]
impl DiscoveryProvider for PostgresDiscoveryProvider {
    async fn discover_schemas(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<DiscoveredSchema>, DiscoveryError> {
        let client = self.connect_with_cancel(cancel).await?;

        let rows = tokio::select! {
            res = client.query(
                "SELECT schema_name FROM information_schema.schemata \
                 WHERE schema_name NOT IN ('pg_catalog', 'information_schema') \
                 AND schema_name !~ '^pg_toast' \
                 AND schema_name !~ '^pg_temp' \
                 ORDER BY schema_name",
                &[],
            ) => res.map_err(|e| DiscoveryError::Query(e.to_string()))?,
            _ = cancel.cancelled() => return Err(DiscoveryError::Cancelled),
        };

        Ok(rows
            .into_iter()
            .map(|row| DiscoveredSchema {
                schema_name: row.get(0),
            })
            .collect())
    }

    async fn discover_tables(
        &self,
        schemas: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<DiscoveredTable>, DiscoveryError> {
        if schemas.is_empty() {
            return Ok(vec![]);
        }

        let client = self.connect_with_cancel(cancel).await?;

        // Regular tables and views from information_schema
        let schema_refs: Vec<&str> = schemas.iter().map(|s| s.as_str()).collect();
        let schema_param: &(dyn tokio_postgres::types::ToSql + Sync) = &schema_refs;
        let schema_params: &[&(dyn tokio_postgres::types::ToSql + Sync)] = &[schema_param];

        let rows_fut = client.query(
            "SELECT table_schema, table_name, table_type \
             FROM information_schema.tables \
             WHERE table_schema = ANY($1) \
             ORDER BY table_schema, table_name",
            schema_params,
        );
        let rows = tokio::select! {
            res = rows_fut => res.map_err(|e| DiscoveryError::Query(e.to_string()))?,
            _ = cancel.cancelled() => return Err(DiscoveryError::Cancelled),
        };

        let mut tables: Vec<DiscoveredTable> = rows
            .into_iter()
            .map(|row| {
                let raw_type: String = row.get(2);
                let table_type = match raw_type.as_str() {
                    "BASE TABLE" => "TABLE",
                    "VIEW" => "VIEW",
                    "FOREIGN" | "FOREIGN TABLE" => "TABLE",
                    other => other,
                }
                .to_string();
                DiscoveredTable {
                    schema_name: row.get(0),
                    table_name: row.get(1),
                    table_type,
                }
            })
            .collect();

        // Materialized views from pg_matviews
        let matview_fut = client.query(
            "SELECT schemaname, matviewname \
             FROM pg_matviews \
             WHERE schemaname = ANY($1) \
             ORDER BY schemaname, matviewname",
            schema_params,
        );
        let matview_rows = tokio::select! {
            res = matview_fut => res.map_err(|e| DiscoveryError::Query(e.to_string()))?,
            _ = cancel.cancelled() => return Err(DiscoveryError::Cancelled),
        };

        for row in matview_rows {
            tables.push(DiscoveredTable {
                schema_name: row.get(0),
                table_name: row.get(1),
                table_type: "MATERIALIZED VIEW".to_string(),
            });
        }

        Ok(tables)
    }

    async fn discover_columns(
        &self,
        tables: &[(String, String)],
        cancel: &CancellationToken,
    ) -> Result<Vec<DiscoveredColumn>, DiscoveryError> {
        if tables.is_empty() {
            return Ok(vec![]);
        }

        // --- Step 1: Get authoritative Arrow schemas from the library ---
        // This ensures stored types match query-time types (no type mismatch overhead).
        let pool_params = to_secret_map(build_postgres_params(&self.cfg));
        let pool = tokio::select! {
            res = PostgresConnectionPool::new(pool_params) => {
                res.map_err(|e| DiscoveryError::Connect(e.to_string()))?
                   .with_unsupported_type_action(UnsupportedTypeAction::Warn)
            }
            _ = cancel.cancelled() => return Err(DiscoveryError::Cancelled),
        };

        // Fetch Arrow schema per table (one round-trip each, but discovery is one-time)
        let mut arrow_schemas: HashMap<(String, String), Arc<Schema>> = HashMap::new();
        for (schema, table) in tables {
            if cancel.is_cancelled() {
                return Err(DiscoveryError::Cancelled);
            }
            let conn = pool.connect().await
                .map_err(|e| DiscoveryError::Connect(e.to_string()))?;
            let table_ref = TableReference::full("postgres", schema.as_str(), table.as_str());
            match get_schema(conn, &table_ref).await {
                Ok(s) => {
                    arrow_schemas.insert((schema.clone(), table.clone()), s);
                }
                Err(e) => {
                    tracing::warn!(
                        schema = %schema,
                        table = %table,
                        error = %e,
                        "get_schema failed for table; columns will have no arrow_type"
                    );
                }
            }
        }

        // --- Step 2: Query information_schema for column metadata ---
        let client = self.connect_with_cancel(cancel).await?;

        let mut all_columns: Vec<DiscoveredColumn> = Vec::new();

        // Batch 50 tables per query to avoid PG parameter limits
        for chunk in tables.chunks(50) {
            // Check cancellation between batches
            if cancel.is_cancelled() {
                return Err(DiscoveryError::Cancelled);
            }

            // Build WHERE clause for (table_schema, table_name) IN (...)
            let mut conditions = Vec::new();
            // Use Vec<String> (Send+Sync) instead of Vec<Box<dyn ToSql + Sync>> (not Send)
            let mut flat_params: Vec<String> = Vec::new();
            for (i, (schema, table)) in chunk.iter().enumerate() {
                let s_idx = i * 2 + 1;
                let t_idx = i * 2 + 2;
                conditions.push(format!("(table_schema = ${s_idx} AND table_name = ${t_idx})"));
                flat_params.push(schema.clone());
                flat_params.push(table.clone());
            }
            let where_clause = conditions.join(" OR ");

            let sql = format!(
                "SELECT table_schema, table_name, column_name, ordinal_position, \
                 data_type, udt_name, is_nullable, column_default \
                 FROM information_schema.columns \
                 WHERE {where_clause} \
                 ORDER BY table_schema, table_name, ordinal_position"
            );

            // &(dyn ToSql + Sync) is Send because dyn ToSql + Sync: Sync
            let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                flat_params.iter().map(|p| p as &(dyn tokio_postgres::types::ToSql + Sync)).collect();

            let rows = tokio::select! {
                res = client.query(sql.as_str(), &param_refs) => {
                    res.map_err(|e| DiscoveryError::Query(e.to_string()))?
                }
                _ = cancel.cancelled() => return Err(DiscoveryError::Cancelled),
            };

            for row in rows {
                let schema_name: String = row.get(0);
                let table_name: String = row.get(1);
                let column_name: String = row.get(2);
                let is_nullable_str: String = row.get(6);

                // Get Arrow type from library's get_schema() result (guaranteed to match query-time)
                let arrow_type = arrow_schemas
                    .get(&(schema_name.clone(), table_name.clone()))
                    .and_then(|s| s.field_with_name(&column_name).ok())
                    .map(|f| arrow_type_to_string(f.data_type()));

                all_columns.push(DiscoveredColumn {
                    schema_name,
                    table_name,
                    column_name,
                    ordinal_position: {
                        let pos: i32 = row.get(3);
                        pos
                    },
                    data_type: row.get(4),
                    is_nullable: is_nullable_str.to_uppercase() == "YES",
                    column_default: row.get(7),
                    arrow_type,
                });
            }
        }

        // --- Step 3: Materialized views via pg_attribute ---
        // Materialized views are not in information_schema.columns — detect by absence.
        let covered: std::collections::HashSet<(&str, &str)> = all_columns
            .iter()
            .map(|c| (c.schema_name.as_str(), c.table_name.as_str()))
            .collect();

        let matview_tables: Vec<(&String, &String)> = tables
            .iter()
            .filter(|(s, t)| !covered.contains(&(s.as_str(), t.as_str())))
            .map(|(s, t)| (s, t))
            .collect();

        for (schema, table) in matview_tables {
            if cancel.is_cancelled() {
                return Err(DiscoveryError::Cancelled);
            }

            let schema_p: &(dyn tokio_postgres::types::ToSql + Sync) = schema;
            let table_p: &(dyn tokio_postgres::types::ToSql + Sync) = table;
            let matview_params: &[&(dyn tokio_postgres::types::ToSql + Sync)] = &[schema_p, table_p];
            let matview_fut = client.query(
                "SELECT a.attname, a.attnum, t.typname, NOT a.attnotnull \
                 FROM pg_attribute a \
                 JOIN pg_class c ON c.oid = a.attrelid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 JOIN pg_type t ON t.oid = a.atttypid \
                 WHERE n.nspname = $1 AND c.relname = $2 \
                 AND a.attnum > 0 AND NOT a.attisdropped \
                 ORDER BY a.attnum",
                matview_params,
            );
            let matview_rows = tokio::select! {
                res = matview_fut => res.map_err(|e| DiscoveryError::Query(e.to_string()))?,
                _ = cancel.cancelled() => return Err(DiscoveryError::Cancelled),
            };

            for row in matview_rows {
                let col_name: String = row.get(0);
                let attnum: i16 = row.get(1);
                let typname: String = row.get(2);
                let is_nullable: bool = row.get(3);

                // Arrow type from get_schema() — same source as regular tables
                let arrow_type = arrow_schemas
                    .get(&(schema.clone(), table.clone()))
                    .and_then(|s| s.field_with_name(&col_name).ok())
                    .map(|f| arrow_type_to_string(f.data_type()));

                all_columns.push(DiscoveredColumn {
                    schema_name: schema.clone(),
                    table_name: table.clone(),
                    column_name: col_name,
                    ordinal_position: attnum as i32,
                    data_type: typname,
                    is_nullable,
                    column_default: None,
                    arrow_type,
                });
            }
        }

        Ok(all_columns)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_arrow_type_to_string_round_trips() {
        use crate::engine::{arrow_type_to_string, parse_arrow_type_pub};
        use datafusion::arrow::datatypes::{DataType, TimeUnit};

        // Types the library produces — must survive the DB round-trip:
        // get_schema() → arrow_type_to_string() → parse_arrow_type()
        let cases: &[DataType] = &[
            DataType::Int16,
            DataType::Int32,
            DataType::Int64,
            DataType::Float32,
            DataType::Float64,
            DataType::Boolean,
            DataType::Utf8,
            DataType::Date32,
            DataType::Decimal128(38, 20), // library produces (38,20) for numeric
            DataType::Timestamp(TimeUnit::Nanosecond, None),
            DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
        ];
        for dt in cases {
            let stored = arrow_type_to_string(dt);
            let recovered = parse_arrow_type_pub(&stored)
                .unwrap_or_else(|| panic!("parse_arrow_type({stored:?}) returned None for {dt:?}"));
            assert_eq!(
                dt, &recovered,
                "Round-trip failed for {dt:?}: stored as {stored:?}"
            );
        }
    }
}
