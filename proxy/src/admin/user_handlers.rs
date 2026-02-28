use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use uuid::Uuid;

use crate::{auth::Auth, entity::proxy_user};

use super::{
    AdminState, ApiErr,
    dto::{
        ChangePasswordRequest, CreateUserRequest, ListUsersQuery, PaginatedResponse,
        UpdateUserRequest, UserResponse,
    },
    jwt::AdminClaims,
};

pub async fn list_users(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Query(params): Query<ListUsersQuery>,
) -> Result<Json<PaginatedResponse<UserResponse>>, ApiErr> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).min(100);

    let mut query = proxy_user::Entity::find();

    if let Some(ref search) = params.search
        && !search.is_empty()
    {
        query = query.filter(proxy_user::Column::Username.contains(search.as_str()));
    }

    let paginator = query
        .order_by_asc(proxy_user::Column::CreatedAt)
        .paginate(&state.db, page_size);

    let total = paginator.num_items().await.map_err(ApiErr::internal)?;
    let users = paginator
        .fetch_page(page - 1)
        .await
        .map_err(ApiErr::internal)?;

    Ok(Json(PaginatedResponse {
        data: users.into_iter().map(UserResponse::from).collect(),
        total,
        page,
        page_size,
    }))
}

pub async fn create_user(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), ApiErr> {
    let password_hash = Auth::hash_password(&body.password).map_err(ApiErr::internal)?;

    let now = Utc::now().naive_utc();
    let model = proxy_user::ActiveModel {
        id: Set(Uuid::now_v7()),
        username: Set(body.username),
        password_hash: Set(password_hash),
        tenant: Set(body.tenant),
        is_admin: Set(body.is_admin),
        is_active: Set(true),
        email: Set(body.email),
        display_name: Set(body.display_name),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Username already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    Ok((StatusCode::CREATED, Json(UserResponse::from(model))))
}

pub async fn get_user(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<UserResponse>, ApiErr> {
    let user = proxy_user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User not found"))?;

    Ok(Json(UserResponse::from(user)))
}

pub async fn update_user(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, ApiErr> {
    let user = proxy_user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User not found"))?;

    let mut active: proxy_user::ActiveModel = user.into();

    if let Some(tenant) = body.tenant {
        active.tenant = Set(tenant);
    }
    if let Some(is_admin) = body.is_admin {
        active.is_admin = Set(is_admin);
    }
    if let Some(is_active) = body.is_active {
        active.is_active = Set(is_active);
    }
    if let Some(email) = body.email {
        active.email = Set(Some(email));
    }
    if let Some(display_name) = body.display_name {
        active.display_name = Set(Some(display_name));
    }
    active.updated_at = Set(Utc::now().naive_utc());

    let updated = active.update(&state.db).await.map_err(ApiErr::internal)?;

    Ok(Json(UserResponse::from(updated)))
}

pub async fn change_password(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<UserResponse>, ApiErr> {
    let user = proxy_user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User not found"))?;

    let hash = Auth::hash_password(&body.password).map_err(ApiErr::internal)?;

    let mut active: proxy_user::ActiveModel = user.into();
    active.password_hash = Set(hash);
    active.updated_at = Set(Utc::now().naive_utc());

    let updated = active.update(&state.db).await.map_err(ApiErr::internal)?;

    Ok(Json(UserResponse::from(updated)))
}

pub async fn delete_user(
    AdminClaims(_): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let user = proxy_user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User not found"))?;

    let active: proxy_user::ActiveModel = user.into();
    active.delete(&state.db).await.map_err(ApiErr::internal)?;

    Ok(StatusCode::NO_CONTENT)
}
