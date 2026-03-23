use crate::auth::Auth;
use crate::engine::EngineCache;
use crate::engine::rewrite::rewrite_statement;
use crate::hooks::{QueryHook, policy::PolicyHook, read_only::ReadOnlyHook};
use arrow_pg::datatypes::arrow_schema_to_pg_fields;
use arrow_pg::datatypes::df::encode_dataframe;
use async_trait::async_trait;
use dashmap::DashMap;
use datafusion::prelude::SessionContext;
use datafusion::sql::sqlparser::{dialect::PostgreSqlDialect, parser::Parser};
use futures::Sink;
use futures::sink::SinkExt;
use pgwire::api::auth::{
    DefaultServerParameterProvider, StartupHandler, finish_authentication, protocol_negotiation,
    save_startup_parameters_to_metadata,
};
use pgwire::api::portal::{Format, Portal};
use pgwire::api::query::{ExtendedQueryHandler, SimpleQueryHandler};
use pgwire::api::results::{
    DataRowEncoder, DescribePortalResponse, DescribeStatementResponse, FieldFormat, FieldInfo,
    QueryResponse, Response,
};
use pgwire::api::stmt::{NoopQueryParser, StoredStatement};
use pgwire::api::{ClientInfo, PgWireConnectionState, PgWireServerHandlers, Type};
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};
use pgwire::messages::startup::Authentication;
use pgwire::messages::{PgWireBackendMessage, PgWireFrontendMessage};
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-connection context entry. Stores enough metadata to rebuild the SessionContext
/// in-place when policy changes invalidate the cached schema.
struct ConnectionEntry {
    ctx: Arc<SessionContext>,
    user_id: uuid::Uuid,
    datasource_name: String,
}

/// Per-connection shared state. Wrapped in Arc so all handler clones share the same maps.
struct ConnectionStore {
    /// Per-connection SessionContext keyed by connection ID.
    connection_contexts: DashMap<u64, ConnectionEntry>,
    /// Handoff: accept loop → on_startup. Maps peer SocketAddr → connection ID.
    pending_conn_ids: DashMap<SocketAddr, u64>,
    /// Monotonic counter for generating unique connection IDs.
    next_connection_id: AtomicU64,
}

impl ConnectionStore {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            connection_contexts: DashMap::new(),
            pending_conn_ids: DashMap::new(),
            next_connection_id: AtomicU64::new(0),
        })
    }
}

pub struct ProxyHandler {
    engine_cache: Arc<EngineCache>,
    hooks: Vec<Arc<dyn QueryHook>>,
    query_parser: Arc<NoopQueryParser>,
    auth: Arc<Auth>,
    conn_store: Arc<ConnectionStore>,
}

impl ProxyHandler {
    pub fn new(
        auth: Arc<Auth>,
        engine_cache: Arc<EngineCache>,
        policy_hook: Arc<PolicyHook>,
    ) -> Self {
        // PolicyHook runs first so it can audit all statements (including writes that
        // ReadOnlyHook will reject). ReadOnlyHook runs second and enforces the allowlist.
        let hooks: Vec<Arc<dyn QueryHook>> = vec![policy_hook, Arc::new(ReadOnlyHook::new())];

        tracing::info!(hook_count = hooks.len(), "Initialized query hooks");

        ProxyHandler {
            engine_cache,
            hooks,
            query_parser: Arc::new(NoopQueryParser::new()),
            auth,
            conn_store: ConnectionStore::new(),
        }
    }

    /// Allocate a new connection ID (without registering a peer address).
    /// Used as a fallback when peer_addr is unavailable.
    pub fn alloc_connection_id(&self) -> u64 {
        self.conn_store
            .next_connection_id
            .fetch_add(1, Ordering::Relaxed)
    }

    /// Allocate a new connection ID and register it in pending_conn_ids keyed by peer address.
    /// Called from the accept loop before spawning the connection task.
    pub fn register_connection(&self, peer_addr: SocketAddr) -> u64 {
        let conn_id = self
            .conn_store
            .next_connection_id
            .fetch_add(1, Ordering::Relaxed);
        self.conn_store.pending_conn_ids.insert(peer_addr, conn_id);
        conn_id
    }

    /// Remove connection state after the connection closes.
    pub fn cleanup_connection(&self, conn_id: u64, peer_addr: Option<SocketAddr>) {
        self.conn_store.connection_contexts.remove(&conn_id);
        if let Some(addr) = peer_addr {
            self.conn_store.pending_conn_ids.remove(&addr);
        }
    }

    /// Rebuild the per-user `SessionContext` for all active connections on the given datasource.
    ///
    /// Called after a policy mutation so that connected users immediately see the updated schema
    /// (e.g. a newly-denied column disappears, or a re-enabled column reappears) without needing
    /// to reconnect. Rebuilding is done in the background via `tokio::spawn` so this method
    /// returns immediately.
    pub fn rebuild_contexts_for_datasource(&self, datasource: &str) {
        let entries: Vec<(u64, uuid::Uuid, String)> = self
            .conn_store
            .connection_contexts
            .iter()
            .filter(|e| e.value().datasource_name == datasource)
            .map(|e| {
                (
                    *e.key(),
                    e.value().user_id,
                    e.value().datasource_name.clone(),
                )
            })
            .collect();

        for (conn_id, user_id, ds_name) in entries {
            let engine_cache = self.engine_cache.clone();
            let conn_store = self.conn_store.clone();
            tokio::spawn(async move {
                match engine_cache.build_user_context(user_id, &ds_name).await {
                    Ok(new_ctx) => {
                        if let Some(mut entry) = conn_store.connection_contexts.get_mut(&conn_id) {
                            entry.ctx = new_ctx;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            conn_id,
                            "Failed to rebuild SessionContext — removing stale connection"
                        );
                        conn_store.connection_contexts.remove(&conn_id);
                    }
                }
            });
        }
    }

    /// Rebuild the per-user `SessionContext` for all active connections of a specific user.
    ///
    /// Called after role membership/inheritance changes so that the affected user immediately
    /// sees the updated schema without needing to reconnect.
    pub fn rebuild_contexts_for_user(&self, user_id: uuid::Uuid) {
        let entries: Vec<(u64, uuid::Uuid, String)> = self
            .conn_store
            .connection_contexts
            .iter()
            .filter(|e| e.value().user_id == user_id)
            .map(|e| {
                (
                    *e.key(),
                    e.value().user_id,
                    e.value().datasource_name.clone(),
                )
            })
            .collect();

        for (conn_id, uid, ds_name) in entries {
            let engine_cache = self.engine_cache.clone();
            let conn_store = self.conn_store.clone();
            tokio::spawn(async move {
                match engine_cache.build_user_context(uid, &ds_name).await {
                    Ok(new_ctx) => {
                        if let Some(mut entry) = conn_store.connection_contexts.get_mut(&conn_id) {
                            entry.ctx = new_ctx;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            conn_id,
                            user_id = %uid,
                            "Failed to rebuild SessionContext — removing stale connection"
                        );
                        conn_store.connection_contexts.remove(&conn_id);
                    }
                }
            });
        }
    }

    /// Get the per-connection SessionContext by looking up conn_id stored in client metadata.
    async fn get_ctx<C>(&self, client: &C) -> PgWireResult<Arc<SessionContext>>
    where
        C: ClientInfo,
    {
        let conn_id_str = client
            .metadata()
            .get("conn_id")
            .cloned()
            .unwrap_or_default();

        if conn_id_str.is_empty() {
            return Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                "ERROR".to_owned(),
                "08000".to_owned(),
                "Connection not initialized — authentication may have failed".to_owned(),
            ))));
        }

        let conn_id: u64 = conn_id_str.parse().map_err(|_| {
            PgWireError::ApiError(Box::new(std::io::Error::other(
                "Invalid conn_id in metadata",
            )))
        })?;

        self.conn_store
            .connection_contexts
            .get(&conn_id)
            .map(|entry| entry.value().ctx.clone())
            .ok_or_else(|| {
                PgWireError::UserError(Box::new(ErrorInfo::new(
                    "ERROR".to_owned(),
                    "08000".to_owned(),
                    "Session context not found — please reconnect".to_owned(),
                )))
            })
    }
}

/// Execute a DataFusion EXPLAIN statement and reformat its output into the single-column
/// "QUERY PLAN" format that PostgreSQL clients (TablePlus, DBeaver, psql, etc.) expect.
///
/// DataFusion returns two columns: `plan_type` (col 0) and `plan` (col 1).
/// We discard `plan_type` and emit each line of `plan` as a separate row under
/// a single column named "QUERY PLAN", matching the real PostgreSQL wire format.
async fn execute_explain(df: datafusion::prelude::DataFrame) -> PgWireResult<Response> {
    use datafusion::arrow::array::{Array, StringArray};

    let fields = Arc::new(vec![FieldInfo::new(
        "QUERY PLAN".to_string(),
        None,
        None,
        Type::TEXT,
        FieldFormat::Text,
    )]);

    let batches = df.collect().await.map_err(|e| {
        tracing::error!(error = %e, "DataFusion EXPLAIN error");
        PgWireError::ApiError(Box::new(e))
    })?;

    let mut rows = Vec::new();
    for batch in &batches {
        // DataFusion EXPLAIN schema: col 0 = plan_type, col 1 = plan (full text)
        if batch.num_columns() < 2 {
            continue;
        }
        if let Some(plan_array) = batch.column(1).as_any().downcast_ref::<StringArray>() {
            for i in 0..batch.num_rows() {
                let plan_text = if plan_array.is_null(i) {
                    ""
                } else {
                    plan_array.value(i)
                };
                for line in plan_text.lines() {
                    let mut encoder = DataRowEncoder::new(fields.clone());
                    encoder.encode_field(&Some(line))?;
                    rows.push(encoder.take_row());
                }
            }
        }
    }

    let stream = async_stream::stream! {
        for row in rows {
            yield Ok::<_, PgWireError>(row);
        }
    };

    Ok(Response::Query(QueryResponse::new(fields, stream)))
}

impl PgWireServerHandlers for ProxyHandler {
    fn simple_query_handler(&self) -> Arc<impl SimpleQueryHandler> {
        Arc::new(self.clone())
    }

    fn extended_query_handler(&self) -> Arc<impl ExtendedQueryHandler> {
        Arc::new(self.clone())
    }

    fn startup_handler(&self) -> Arc<impl StartupHandler> {
        Arc::new(self.clone())
    }
}

impl Clone for ProxyHandler {
    fn clone(&self) -> Self {
        ProxyHandler {
            engine_cache: self.engine_cache.clone(),
            hooks: self.hooks.clone(),
            query_parser: self.query_parser.clone(),
            auth: self.auth.clone(),
            conn_store: self.conn_store.clone(), // Arc::clone — shares state
        }
    }
}

#[async_trait]
impl StartupHandler for ProxyHandler {
    async fn on_startup<C>(
        &self,
        client: &mut C,
        message: PgWireFrontendMessage,
    ) -> PgWireResult<()>
    where
        C: ClientInfo + Sink<PgWireBackendMessage> + Unpin + Send,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        match message {
            PgWireFrontendMessage::Startup(ref startup) => {
                protocol_negotiation(client, startup).await?;
                save_startup_parameters_to_metadata(client, startup);
                client.set_state(PgWireConnectionState::AuthenticationInProgress);
                client
                    .send(PgWireBackendMessage::Authentication(
                        Authentication::CleartextPassword,
                    ))
                    .await?;
            }
            PgWireFrontendMessage::PasswordMessageFamily(pwd) => {
                let pwd = pwd.into_password()?;
                let username = client.metadata().get("user").cloned().unwrap_or_default();

                match self.auth.authenticate(&username, &pwd.password).await {
                    Ok(user) => {
                        // Store user context in metadata for PolicyHook
                        client
                            .metadata_mut()
                            .insert("user_id".to_owned(), user.id.to_string());
                        client
                            .metadata_mut()
                            .insert("tenant".to_owned(), user.tenant.clone());

                        // Read requested database name (= data source name)
                        let datasource_name = client
                            .metadata()
                            .get("database")
                            .cloned()
                            .unwrap_or_default();

                        if datasource_name.is_empty() {
                            return Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                                "FATAL".to_owned(),
                                "08006".to_owned(),
                                "No database specified — use -d <datasource_name> in your connection string".to_owned(),
                            ))));
                        }

                        // Validate data source exists and is active
                        self.engine_cache
                            .validate_data_source(&datasource_name)
                            .await
                            .map_err(|e| {
                                PgWireError::UserError(Box::new(ErrorInfo::new(
                                    "FATAL".to_owned(),
                                    "3D000".to_owned(),
                                    e.to_string(),
                                )))
                            })?;

                        // Check user is assigned to this data source
                        let has_access = self
                            .engine_cache
                            .check_access(user.id, &datasource_name)
                            .await
                            .map_err(|e| {
                                PgWireError::ApiError(Box::new(std::io::Error::other(
                                    e.to_string(),
                                )))
                            })?;

                        if !has_access {
                            return Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                                "FATAL".to_owned(),
                                "42501".to_owned(),
                                format!("Access denied to data source '{}'", datasource_name),
                            ))));
                        }

                        // Store data source name for query handlers
                        client
                            .metadata_mut()
                            .insert("datasource".to_owned(), datasource_name.clone());

                        // Retrieve the connection ID registered at accept time
                        let peer_addr = client.socket_addr();
                        let conn_id = self
                            .conn_store
                            .pending_conn_ids
                            .remove(&peer_addr)
                            .map(|(_, id)| id)
                            .ok_or_else(|| {
                                PgWireError::ApiError(Box::new(std::io::Error::other(
                                    "Connection ID not found — internal error",
                                )))
                            })?;

                        // Build per-user filtered SessionContext inline (not in background).
                        // This ensures the context is ready before the first query arrives,
                        // and that metadata visibility is correct from the first query onward.
                        let ctx = self
                            .engine_cache
                            .build_user_context(user.id, &datasource_name)
                            .await
                            .map_err(|e| {
                                PgWireError::ApiError(Box::new(std::io::Error::other(
                                    e.to_string(),
                                )))
                            })?;

                        self.conn_store.connection_contexts.insert(
                            conn_id,
                            ConnectionEntry {
                                ctx,
                                user_id: user.id,
                                datasource_name: datasource_name.clone(),
                            },
                        );
                        client
                            .metadata_mut()
                            .insert("conn_id".to_owned(), conn_id.to_string());

                        tracing::info!(
                            username = %username,
                            tenant = %user.tenant,
                            datasource = %datasource_name,
                            conn_id = conn_id,
                            addr = %peer_addr,
                            "Authenticated user"
                        );

                        finish_authentication(client, &DefaultServerParameterProvider::default())
                            .await?;

                        // Warm up the upstream pool in the background (amortises first-query latency)
                        let cache = self.engine_cache.clone();
                        let ds_name = datasource_name.clone();
                        tokio::spawn(async move {
                            cache.warmup(&ds_name).await;
                        });
                    }
                    Err(e) => return Err(e),
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[async_trait]
impl SimpleQueryHandler for ProxyHandler {
    async fn do_query<C>(&self, client: &mut C, query: &str) -> PgWireResult<Vec<Response>>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        tracing::debug!(query = %query, "Received simple query");

        let ctx = self.get_ctx(client).await?;

        // Parse SQL to Statement
        let statements = Parser::parse_sql(&PostgreSqlDialect {}, query).map_err(|e| {
            tracing::error!(error = %e, "SQL parse error");
            PgWireError::ApiError(Box::new(e))
        })?;

        let mut responses = Vec::new();

        for mut statement in statements {
            // Rewrite AST for PostgreSQL compatibility before processing
            rewrite_statement(&mut statement);

            // Execute hook pipeline
            let mut hook_response = None;
            for hook in &self.hooks {
                if let Some(response) = hook
                    .handle_query(&statement, &ctx, client as &(dyn ClientInfo + Sync))
                    .await
                {
                    hook_response = Some(response);
                    break;
                }
            }

            let response = if let Some(r) = hook_response {
                r?
            } else {
                let sql = statement.to_string();
                tracing::debug!(sql = %sql, "Executing via DataFusion");

                let df = ctx.sql(&sql).await.map_err(|e| {
                    tracing::error!(error = %e, "DataFusion query error");
                    PgWireError::ApiError(Box::new(e))
                })?;

                let query_start = std::time::Instant::now();
                if matches!(
                    statement,
                    datafusion::sql::sqlparser::ast::Statement::Explain { .. }
                ) {
                    execute_explain(df).await?
                } else {
                    let qr = encode_dataframe(df, &Format::UnifiedText, None)
                        .await
                        .map_err(|e| {
                            tracing::error!(error = %e, "DataFusion encoding error");
                            e
                        })?;
                    tracing::debug!(elapsed = ?query_start.elapsed(), "Query completed");
                    Response::Query(qr)
                }
            };

            responses.push(response);
        }

        Ok(responses)
    }
}

#[async_trait]
impl ExtendedQueryHandler for ProxyHandler {
    type Statement = String;
    type QueryParser = NoopQueryParser;

    fn query_parser(&self) -> Arc<Self::QueryParser> {
        self.query_parser.clone()
    }

    async fn do_query<C>(
        &self,
        client: &mut C,
        portal: &Portal<Self::Statement>,
        _max_rows: usize,
    ) -> PgWireResult<Response>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        let query = &portal.statement.statement;

        tracing::debug!(query = %query, "Extended query");

        let ctx = self.get_ctx(client).await?;

        let statements = Parser::parse_sql(&PostgreSqlDialect {}, query).map_err(|e| {
            tracing::error!(error = %e, "SQL parse error");
            PgWireError::ApiError(Box::new(e))
        })?;

        if statements.is_empty() {
            return Ok(Response::EmptyQuery);
        }

        let mut statement = statements.into_iter().next().unwrap();
        rewrite_statement(&mut statement);

        let mut hook_response = None;
        for hook in &self.hooks {
            if let Some(response) = hook
                .handle_query(&statement, &ctx, client as &(dyn ClientInfo + Sync))
                .await
            {
                hook_response = Some(response);
                break;
            }
        }

        if let Some(r) = hook_response {
            return r;
        }

        let sql = statement.to_string();
        tracing::debug!(sql = %sql, "Executing via DataFusion");

        let df = ctx.sql(&sql).await.map_err(|e| {
            tracing::error!(error = %e, "DataFusion query error");
            PgWireError::ApiError(Box::new(e))
        })?;

        let query_start = std::time::Instant::now();
        if matches!(
            statement,
            datafusion::sql::sqlparser::ast::Statement::Explain { .. }
        ) {
            execute_explain(df).await
        } else {
            let qr = encode_dataframe(df, &Format::UnifiedText, None)
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "DataFusion encoding error");
                    e
                })?;
            tracing::debug!(elapsed = ?query_start.elapsed(), "Query completed");
            Ok(Response::Query(qr))
        }
    }

    async fn do_describe_statement<C>(
        &self,
        client: &mut C,
        target: &StoredStatement<Self::Statement>,
    ) -> PgWireResult<DescribeStatementResponse>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        let query = &target.statement;
        let ctx = self.get_ctx(client).await?;

        let statements = Parser::parse_sql(&PostgreSqlDialect {}, query)
            .map_err(|e| PgWireError::ApiError(Box::new(e)))?;

        if statements.is_empty() {
            return Ok(DescribeStatementResponse::new(vec![], vec![]));
        }

        let mut statement = statements.into_iter().next().unwrap();
        rewrite_statement(&mut statement);

        let sql = statement.to_string();
        let df = ctx
            .sql(&sql)
            .await
            .map_err(|e| PgWireError::ApiError(Box::new(e)))?;

        let schema = df.schema();
        let fields = arrow_schema_to_pg_fields(schema.inner(), &Format::UnifiedText, None)
            .map_err(|e| PgWireError::ApiError(Box::new(e)))?;

        Ok(DescribeStatementResponse::new(vec![], fields))
    }

    async fn do_describe_portal<C>(
        &self,
        client: &mut C,
        portal: &Portal<Self::Statement>,
    ) -> PgWireResult<DescribePortalResponse>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        let query = &portal.statement.statement;
        let ctx = self.get_ctx(client).await?;

        let statements = Parser::parse_sql(&PostgreSqlDialect {}, query)
            .map_err(|e| PgWireError::ApiError(Box::new(e)))?;

        if statements.is_empty() {
            return Ok(DescribePortalResponse::new(vec![]));
        }

        let mut statement = statements.into_iter().next().unwrap();
        rewrite_statement(&mut statement);

        let sql = statement.to_string();
        let df = ctx
            .sql(&sql)
            .await
            .map_err(|e| PgWireError::ApiError(Box::new(e)))?;

        let schema = df.schema();
        let fields = arrow_schema_to_pg_fields(schema.inner(), &Format::UnifiedText, None)
            .map_err(|e| PgWireError::ApiError(Box::new(e)))?;

        Ok(DescribePortalResponse::new(fields))
    }
}
