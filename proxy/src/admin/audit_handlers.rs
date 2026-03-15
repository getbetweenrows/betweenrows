use axum::{
    extract::{Query, State},
    response::Json,
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};

use crate::entity::query_audit_log;

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
    if let Some(ref from) = params.from
        && let Ok(dt) = chrono::NaiveDateTime::parse_from_str(from, "%Y-%m-%dT%H:%M:%S")
    {
        query = query.filter(query_audit_log::Column::CreatedAt.gte(dt));
    }
    if let Some(ref to) = params.to
        && let Ok(dt) = chrono::NaiveDateTime::parse_from_str(to, "%Y-%m-%dT%H:%M:%S")
    {
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
                client_ip: m.client_ip,
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
