//! Decision context builder — constructs the `ctx` JSON passed to decision functions.
//!
//! Two modes:
//! - Session context: `ctx.session` only (user, time, datasource).
//! - Query context: `ctx.session` + `ctx.query` (tables, columns, join_count, etc.).

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

/// Session-level context available to all decision functions.
pub struct SessionInfo {
    pub user_id: Uuid,
    pub username: String,
    pub tenant: String,
    pub roles: Vec<String>,
    pub datasource_name: String,
    pub access_mode: String,
}

/// Query-level metadata extracted from the logical plan.
#[derive(Debug, Clone, Default)]
pub struct QueryMetadata {
    pub tables: Vec<String>,
    pub columns: Vec<String>,
    pub join_count: usize,
    pub has_aggregation: bool,
    pub has_subquery: bool,
    pub has_where: bool,
    pub statement_type: String,
}

/// Build session-only context JSON (`evaluate_context = "session"`).
pub fn build_session_context(session: &SessionInfo) -> serde_json::Value {
    let now = Utc::now();
    json!({
        "session": {
            "user": {
                "id": session.user_id.to_string(),
                "username": session.username,
                "tenant": session.tenant,
                "roles": session.roles,
            },
            "time": {
                "hour": now.format("%H").to_string().parse::<u32>().unwrap_or(0),
                "day_of_week": now.format("%A").to_string(),
            },
            "datasource": {
                "name": session.datasource_name,
                "access_mode": session.access_mode,
            }
        }
    })
}

/// Build full context JSON (`evaluate_context = "query"`).
pub fn build_query_context(session: &SessionInfo, query: &QueryMetadata) -> serde_json::Value {
    let now = Utc::now();
    json!({
        "session": {
            "user": {
                "id": session.user_id.to_string(),
                "username": session.username,
                "tenant": session.tenant,
                "roles": session.roles,
            },
            "time": {
                "hour": now.format("%H").to_string().parse::<u32>().unwrap_or(0),
                "day_of_week": now.format("%A").to_string(),
            },
            "datasource": {
                "name": session.datasource_name,
                "access_mode": session.access_mode,
            }
        },
        "query": {
            "tables": query.tables,
            "columns": query.columns,
            "join_count": query.join_count,
            "has_aggregation": query.has_aggregation,
            "has_subquery": query.has_subquery,
            "has_where": query.has_where,
            "statement_type": query.statement_type,
        }
    })
}
