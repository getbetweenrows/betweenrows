use crate::auth::Auth;
use crate::engine::EngineCache;
use crate::engine::rewrite::rewrite_statement;
use crate::hooks::{QueryHook, read_only::ReadOnlyHook, rls::RLSHook};
use arrow_pg::datatypes::arrow_schema_to_pg_fields;
use arrow_pg::datatypes::df::encode_dataframe;
use async_trait::async_trait;
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
use std::sync::Arc;

pub struct ProxyHandler {
    engine_cache: Arc<EngineCache>,
    hooks: Vec<Arc<dyn QueryHook>>,
    query_parser: Arc<NoopQueryParser>,
    auth: Arc<Auth>,
}

impl ProxyHandler {
    pub fn new(auth: Arc<Auth>, engine_cache: Arc<EngineCache>) -> Self {
        let hooks: Vec<Arc<dyn QueryHook>> =
            vec![Arc::new(ReadOnlyHook::new()), Arc::new(RLSHook::new())];

        tracing::info!(hook_count = hooks.len(), "Initialized query hooks");

        ProxyHandler {
            engine_cache,
            hooks,
            query_parser: Arc::new(NoopQueryParser::new()),
            auth,
        }
    }

    /// Get the SessionContext for the current connection's data source.
    async fn get_ctx<C>(&self, client: &C) -> PgWireResult<Arc<SessionContext>>
    where
        C: ClientInfo,
    {
        let datasource = client
            .metadata()
            .get("datasource")
            .cloned()
            .unwrap_or_default();

        if datasource.is_empty() {
            return Err(PgWireError::UserError(Box::new(ErrorInfo::new(
                "ERROR".to_owned(),
                "08000".to_owned(),
                "No data source selected — specify a database name in your connection string"
                    .to_owned(),
            ))));
        }

        let start = std::time::Instant::now();
        let ctx = self
            .engine_cache
            .get_context(&datasource)
            .await
            .map_err(|e| PgWireError::ApiError(Box::new(std::io::Error::other(e.to_string()))))?;
        tracing::debug!(datasource = %datasource, elapsed = ?start.elapsed(), "SessionContext ready");
        Ok(ctx)
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
                        // Store tenant in metadata for RLS hook
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

                        tracing::info!(
                            username = %username,
                            tenant = %user.tenant,
                            datasource = %datasource_name,
                            addr = %client.socket_addr(),
                            "Authenticated user"
                        );

                        finish_authentication(client, &DefaultServerParameterProvider::default())
                            .await?;

                        // Pre-warm SessionContext + pool in the background so the
                        // first user query doesn't pay the catalog-load latency.
                        let cache = self.engine_cache.clone();
                        let ds_name = datasource_name.clone();
                        tokio::spawn(async move {
                            if let Err(e) = cache.get_context(&ds_name).await {
                                tracing::debug!(datasource = %ds_name, error = %e, "Context warmup failed (non-fatal)");
                            } else {
                                cache.warmup(&ds_name).await;
                            }
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

        for hook in &self.hooks {
            if let Some(response) = hook
                .handle_query(&statement, &ctx, client as &(dyn ClientInfo + Sync))
                .await
            {
                response?;
            }
        }

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

        for hook in &self.hooks {
            if let Some(response) = hook
                .handle_query(&statement, &ctx, client as &(dyn ClientInfo + Sync))
                .await
            {
                response?;
            }
        }

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
