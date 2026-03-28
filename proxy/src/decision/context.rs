//! Decision context builder — constructs the `ctx` JSON passed to decision functions.
//!
//! Two modes:
//! - Session context: `ctx.session` only (user, time, datasource).
//! - Query context: `ctx.session` + `ctx.query` (tables, columns, join_count, etc.).
//!
//! `time.now` is the **evaluation time** — the moment the context is built, not
//! the session start time. For visibility-level decision functions this is when
//! the connection context is computed; for query-level functions it is when the
//! query is processed.

use chrono::Utc;
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

/// Session-level context available to all decision functions.
pub struct SessionInfo {
    pub user_id: Uuid,
    pub username: String,
    pub tenant: String,
    pub roles: Vec<String>,
    pub datasource_name: String,
    pub access_mode: String,
    /// User attributes with typed JSON values (string/number/boolean).
    pub attributes: HashMap<String, serde_json::Value>,
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
                "attributes": session.attributes,
            },
            "time": {
                "now": now.to_rfc3339(),
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
                "attributes": session.attributes,
            },
            "time": {
                "now": now.to_rfc3339(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn test_session() -> SessionInfo {
        SessionInfo {
            user_id: Uuid::nil(),
            username: "alice".to_string(),
            tenant: "acme".to_string(),
            roles: vec!["analyst".to_string()],
            datasource_name: "prod".to_string(),
            access_mode: "policy_required".to_string(),
            attributes: HashMap::new(),
        }
    }

    #[test]
    fn session_context_has_time_now_rfc3339() {
        let ctx = build_session_context(&test_session());
        let now_str = ctx["session"]["time"]["now"].as_str().unwrap();
        // Must parse as RFC 3339
        DateTime::parse_from_rfc3339(now_str).expect("time.now must be valid RFC 3339");
    }

    #[test]
    fn query_context_has_time_now_rfc3339() {
        let ctx = build_query_context(&test_session(), &QueryMetadata::default());
        let now_str = ctx["session"]["time"]["now"].as_str().unwrap();
        DateTime::parse_from_rfc3339(now_str).expect("time.now must be valid RFC 3339");
    }

    #[test]
    fn session_context_preserves_existing_time_fields() {
        let ctx = build_session_context(&test_session());
        let time = &ctx["session"]["time"];
        assert!(time["hour"].is_number());
        assert!(time["day_of_week"].is_string());
        assert!(time["now"].is_string());
    }

    #[test]
    fn session_context_has_empty_attributes() {
        let ctx = build_session_context(&test_session());
        let attrs = &ctx["session"]["user"]["attributes"];
        assert!(attrs.is_object());
        assert_eq!(attrs.as_object().unwrap().len(), 0);
    }

    #[test]
    fn session_context_has_typed_attributes() {
        let mut attrs = HashMap::new();
        attrs.insert("region".to_string(), serde_json::json!("us-east"));
        attrs.insert("clearance".to_string(), serde_json::json!(3));
        attrs.insert("is_vip".to_string(), serde_json::json!(true));

        let session = SessionInfo {
            user_id: Uuid::nil(),
            username: "alice".to_string(),
            tenant: "acme".to_string(),
            roles: vec![],
            datasource_name: "prod".to_string(),
            access_mode: "open".to_string(),
            attributes: attrs,
        };
        let ctx = build_session_context(&session);
        let user_attrs = &ctx["session"]["user"]["attributes"];
        assert_eq!(user_attrs["region"].as_str().unwrap(), "us-east");
        assert_eq!(user_attrs["clearance"].as_i64().unwrap(), 3);
        assert_eq!(user_attrs["is_vip"].as_bool().unwrap(), true);
    }

    #[test]
    fn query_context_has_attributes() {
        let mut attrs = HashMap::new();
        attrs.insert("dept".to_string(), serde_json::json!("engineering"));
        let session = SessionInfo {
            user_id: Uuid::nil(),
            username: "bob".to_string(),
            tenant: "acme".to_string(),
            roles: vec![],
            datasource_name: "prod".to_string(),
            access_mode: "open".to_string(),
            attributes: attrs,
        };
        let ctx = build_query_context(&session, &QueryMetadata::default());
        assert_eq!(
            ctx["session"]["user"]["attributes"]["dept"]
                .as_str()
                .unwrap(),
            "engineering"
        );
    }
}
