use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::{admin_audit_log, query_audit_log};

use super::{
    AdminState, ApiErr,
    dto::{AuditLogResponse, ListAuditLogQuery, PaginatedResponse},
    jwt::AdminClaims,
};

pub async fn list_audit_logs(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Query(params): Query<ListAuditLogQuery>,
) -> Result<Json<PaginatedResponse<AuditLogResponse>>, ApiErr> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).min(200);

    let mut query = query_audit_log::Entity::find();

    if let Some(user_id) = params.user_id {
        query = query.filter(query_audit_log::Column::UserId.eq(user_id));
    }
    if let Some(ds_id) = params.datasource_id {
        query = query.filter(query_audit_log::Column::DataSourceId.eq(ds_id));
    }
    if let Some(ref status) = params.status {
        query = query.filter(query_audit_log::Column::Status.eq(status.clone()));
    }
    if let Some(ref from) = params.from {
        let dt =
            chrono::NaiveDateTime::parse_from_str(from, "%Y-%m-%dT%H:%M:%S").map_err(|_| {
                ApiErr::new(
                    StatusCode::BAD_REQUEST,
                    format!("Invalid 'from' datetime: {from}"),
                )
            })?;
        query = query.filter(query_audit_log::Column::CreatedAt.gte(dt));
    }
    if let Some(ref to) = params.to {
        let dt = chrono::NaiveDateTime::parse_from_str(to, "%Y-%m-%dT%H:%M:%S").map_err(|_| {
            ApiErr::new(
                StatusCode::BAD_REQUEST,
                format!("Invalid 'to' datetime: {to}"),
            )
        })?;
        query = query.filter(query_audit_log::Column::CreatedAt.lte(dt));
    }

    let paginator = query
        .order_by_desc(query_audit_log::Column::CreatedAt)
        .paginate(&state.db, page_size);

    let total = paginator.num_items().await.map_err(ApiErr::internal)?;
    let items = paginator
        .fetch_page(page - 1)
        .await
        .map_err(ApiErr::internal)?;

    let data: Result<Vec<AuditLogResponse>, ApiErr> = items
        .into_iter()
        .map(|m| {
            let policies_applied: serde_json::Value =
                serde_json::from_str(&m.policies_applied).unwrap_or(serde_json::json!([]));
            Ok(AuditLogResponse {
                id: m.id,
                user_id: m.user_id,
                username: m.username,
                data_source_id: m.data_source_id,
                datasource_name: m.datasource_name,
                original_query: m.original_query,
                rewritten_query: m.rewritten_query,
                policies_applied,
                execution_time_ms: m.execution_time_ms,
                client_info: m.client_info,
                created_at: m.created_at,
                status: m.status,
                error_message: m.error_message,
            })
        })
        .collect();

    Ok(Json(PaginatedResponse {
        data: data?,
        total,
        page,
        page_size,
    }))
}

// ---------- GET /audit/admin ----------

#[derive(Debug, Deserialize)]
pub struct ListAdminAuditQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AdminAuditLogResponse {
    pub id: Uuid,
    pub resource_type: String,
    pub resource_id: Uuid,
    pub action: String,
    pub actor_id: Uuid,
    pub changes: Option<serde_json::Value>,
    pub created_at: chrono::NaiveDateTime,
}

pub async fn list_admin_audit_logs(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Query(params): Query<ListAdminAuditQuery>,
) -> Result<Json<PaginatedResponse<AdminAuditLogResponse>>, ApiErr> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).min(200);

    let mut query = admin_audit_log::Entity::find();

    if let Some(ref rt) = params.resource_type {
        query = query.filter(admin_audit_log::Column::ResourceType.eq(rt.clone()));
    }
    if let Some(rid) = params.resource_id {
        query = query.filter(admin_audit_log::Column::ResourceId.eq(rid));
    }
    if let Some(aid) = params.actor_id {
        query = query.filter(admin_audit_log::Column::ActorId.eq(aid));
    }
    if let Some(ref from) = params.from {
        let dt =
            chrono::NaiveDateTime::parse_from_str(from, "%Y-%m-%dT%H:%M:%S").map_err(|_| {
                ApiErr::new(
                    StatusCode::BAD_REQUEST,
                    format!("Invalid 'from' datetime: {from}"),
                )
            })?;
        query = query.filter(admin_audit_log::Column::CreatedAt.gte(dt));
    }
    if let Some(ref to) = params.to {
        let dt = chrono::NaiveDateTime::parse_from_str(to, "%Y-%m-%dT%H:%M:%S").map_err(|_| {
            ApiErr::new(
                StatusCode::BAD_REQUEST,
                format!("Invalid 'to' datetime: {to}"),
            )
        })?;
        query = query.filter(admin_audit_log::Column::CreatedAt.lte(dt));
    }

    let paginator = query
        .order_by_desc(admin_audit_log::Column::CreatedAt)
        .paginate(&state.db, page_size);

    let total = paginator.num_items().await.map_err(ApiErr::internal)?;
    let items = paginator
        .fetch_page(page - 1)
        .await
        .map_err(ApiErr::internal)?;

    let data: Vec<AdminAuditLogResponse> = items
        .into_iter()
        .map(|m| {
            let changes: Option<serde_json::Value> = m
                .changes
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            AdminAuditLogResponse {
                id: m.id,
                resource_type: m.resource_type,
                resource_id: m.resource_id,
                action: m.action,
                actor_id: m.actor_id,
                changes,
                created_at: m.created_at,
            }
        })
        .collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        page_size,
    }))
}
