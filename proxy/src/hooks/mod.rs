use async_trait::async_trait;
use datafusion::prelude::SessionContext;
use datafusion::sql::sqlparser::ast::Statement;
use pgwire::api::ClientInfo;
use pgwire::api::results::Response;
use pgwire::error::PgWireResult;

pub mod read_only;
pub mod rls;

/// QueryHook trait for intercepting and transforming queries
/// Inspired by datafusion-postgres hooks pattern
#[async_trait]
pub trait QueryHook: Send + Sync {
    /// Handle a query before it reaches DataFusion execution
    ///
    /// Returns:
    /// - `None` if this hook doesn't handle the query (pass to next hook)
    /// - `Some(Ok(Response))` if this hook handled the query successfully
    /// - `Some(Err(e))` if this hook encountered an error
    async fn handle_query(
        &self,
        statement: &Statement,
        session_context: &SessionContext,
        client: &(dyn ClientInfo + Sync),
    ) -> Option<PgWireResult<Response>>;
}
