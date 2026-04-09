use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use uuid::Uuid;

use crate::{
    auth::Auth,
    entity::{attribute_definition, proxy_user},
};

use super::{
    AdminState, ApiErr,
    admin_audit::{AuditAction, AuditedTxn},
    dto::{
        ChangePasswordRequest, CreateUserRequest, ListUsersQuery, PaginatedResponse,
        UpdateUserRequest, UserResponse, validate_username,
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

fn validate_password(password: &str) -> Result<(), ApiErr> {
    if password.len() < 8 {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Password must be at least 8 characters",
        ));
    }
    if !password.chars().any(|c| c.is_uppercase()) {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Password must contain at least one uppercase letter",
        ));
    }
    if !password.chars().any(|c| c.is_lowercase()) {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Password must contain at least one lowercase letter",
        ));
    }
    if !password.chars().any(|c| c.is_ascii_digit()) {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Password must contain at least one digit",
        ));
    }
    if !password.chars().any(|c| !c.is_alphanumeric()) {
        return Err(ApiErr::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Password must contain at least one special character",
        ));
    }
    Ok(())
}

pub async fn create_user(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), ApiErr> {
    validate_username(&body.username)
        .map_err(|e| ApiErr::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    validate_password(&body.password)?;
    let password_hash = Auth::hash_password(&body.password).map_err(ApiErr::internal)?;

    let now = Utc::now().naive_utc();
    let id = Uuid::now_v7();

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let model = proxy_user::ActiveModel {
        id: Set(id),
        username: Set(body.username.clone()),
        password_hash: Set(password_hash),
        is_admin: Set(body.is_admin),
        is_active: Set(true),
        email: Set(body.email.clone()),
        display_name: Set(body.display_name.clone()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&*txn)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE") || msg.contains("unique") {
            ApiErr::conflict("Username already exists")
        } else {
            ApiErr::internal(e)
        }
    })?;

    txn.audit(
        "proxy_user",
        id,
        AuditAction::Create,
        claims.sub,
        serde_json::json!({
            "after": {
                "username": model.username,
                "is_admin": model.is_admin,
                "is_active": model.is_active,
                "email": model.email,
                "display_name": model.display_name,
            }
        }),
    );
    txn.commit().await.map_err(ApiErr::internal)?;

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
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, ApiErr> {
    let user = proxy_user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User not found"))?;

    // Guard: prevent demoting the last admin or revoking your own privileges.
    if body.is_admin == Some(false) && user.is_admin {
        if claims.sub == id {
            return Err(ApiErr::conflict("Cannot revoke your own admin privileges"));
        }
        if count_admins(&state.db).await? == 1 {
            return Err(ApiErr::conflict(
                "Cannot revoke admin from the last admin user",
            ));
        }
    }

    let mut changes_before = serde_json::Map::new();
    let mut changes_after = serde_json::Map::new();

    let mut active: proxy_user::ActiveModel = user.clone().into();

    let is_active_changed = body.is_active.is_some();

    if let Some(is_admin) = body.is_admin {
        changes_before.insert("is_admin".into(), serde_json::json!(user.is_admin));
        changes_after.insert("is_admin".into(), serde_json::json!(is_admin));
        active.is_admin = Set(is_admin);
    }
    if let Some(is_active) = body.is_active {
        changes_before.insert("is_active".into(), serde_json::json!(user.is_active));
        changes_after.insert("is_active".into(), serde_json::json!(is_active));
        active.is_active = Set(is_active);
    }
    if let Some(ref email) = body.email {
        changes_before.insert("email".into(), serde_json::json!(user.email));
        changes_after.insert("email".into(), serde_json::json!(email));
        active.email = Set(Some(email.clone()));
    }
    if let Some(ref display_name) = body.display_name {
        changes_before.insert("display_name".into(), serde_json::json!(user.display_name));
        changes_after.insert("display_name".into(), serde_json::json!(display_name));
        active.display_name = Set(Some(display_name.clone()));
    }

    // Handle attributes (full-replace semantics)
    let mut attributes_changed = false;
    if let Some(ref attrs) = body.attributes {
        // Validate against attribute definitions
        if !attrs.is_empty() {
            let defs = attribute_definition::Entity::find()
                .filter(attribute_definition::Column::EntityType.eq("user"))
                .all(&state.db)
                .await
                .map_err(ApiErr::internal)?;

            let defs_by_key = attribute_definition::definitions_by_key(&defs);

            for (key, value) in attrs {
                let def = defs_by_key.get(key.as_str()).ok_or_else(|| {
                    ApiErr::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!("Unknown attribute key '{key}'. Define it first via attribute definitions."),
                    )
                })?;

                if def.value_type == "list" {
                    // List type: value must be a JSON array of strings
                    let elems = value.as_array().ok_or_else(|| {
                        ApiErr::new(
                            StatusCode::UNPROCESSABLE_ENTITY,
                            format!(
                                "Attribute '{key}' has type 'list' — value must be a JSON array"
                            ),
                        )
                    })?;

                    // Validate via validate_value (checks element count, types, lengths)
                    let json_str = serde_json::to_string(value).unwrap_or_default();
                    attribute_definition::validate_value(&json_str, "list").map_err(|e| {
                        ApiErr::new(
                            StatusCode::UNPROCESSABLE_ENTITY,
                            format!("Invalid value for attribute '{key}': {e}"),
                        )
                    })?;

                    // Check each element against allowed_values
                    if let Some(ref av_json) = def.allowed_values {
                        let allowed = attribute_definition::parse_allowed_values(av_json);
                        if !allowed.is_empty() {
                            for elem in elems {
                                let s = elem.as_str().unwrap_or_default();
                                if !allowed.contains(&s.to_string()) {
                                    return Err(ApiErr::new(
                                        StatusCode::UNPROCESSABLE_ENTITY,
                                        format!(
                                            "List element '{s}' for attribute '{key}' is not in allowed values: {allowed:?}",
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                } else {
                    // Scalar types: value must be a JSON string
                    let str_value = value.as_str().ok_or_else(|| {
                        ApiErr::new(
                            StatusCode::UNPROCESSABLE_ENTITY,
                            format!(
                                "Attribute '{key}' has type '{}' — value must be a string",
                                def.value_type
                            ),
                        )
                    })?;

                    attribute_definition::validate_value(str_value, &def.value_type).map_err(
                        |e| {
                            ApiErr::new(
                                StatusCode::UNPROCESSABLE_ENTITY,
                                format!("Invalid value for attribute '{key}': {e}"),
                            )
                        },
                    )?;

                    if let Some(ref av_json) = def.allowed_values {
                        let allowed = attribute_definition::parse_allowed_values(av_json);
                        if !allowed.is_empty() && !allowed.contains(&str_value.to_string()) {
                            return Err(ApiErr::new(
                                StatusCode::UNPROCESSABLE_ENTITY,
                                format!(
                                    "Value '{str_value}' for attribute '{key}' is not in allowed values: {allowed:?}",
                                ),
                            ));
                        }
                    }
                }
            }
        }

        changes_before.insert("attributes".into(), serde_json::json!(user.attributes));
        changes_after.insert("attributes".into(), serde_json::json!(attrs));
        let attrs_json = serde_json::to_string(attrs).map_err(ApiErr::internal)?;
        active.attributes = Set(attrs_json);
        attributes_changed = true;
    }

    active.updated_at = Set(Utc::now().naive_utc());

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let updated = active.update(&*txn).await.map_err(ApiErr::internal)?;

    txn.audit(
        "proxy_user",
        id,
        AuditAction::Update,
        claims.sub,
        serde_json::json!({ "before": changes_before, "after": changes_after }),
    );
    txn.commit().await.map_err(ApiErr::internal)?;

    // Cache invalidation after commit
    if attributes_changed || is_active_changed {
        if let Some(hook) = &state.policy_hook {
            hook.invalidate_user(id).await;
        }
        if let Some(ph) = &state.proxy_handler {
            ph.rebuild_contexts_for_user(id);
        }
    }

    Ok(Json(UserResponse::from(updated)))
}

pub async fn change_password(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<UserResponse>, ApiErr> {
    validate_password(&body.password)?;

    let user = proxy_user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User not found"))?;

    let hash = Auth::hash_password(&body.password).map_err(ApiErr::internal)?;

    let mut active: proxy_user::ActiveModel = user.into();
    active.password_hash = Set(hash);
    active.updated_at = Set(Utc::now().naive_utc());

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    let updated = active.update(&*txn).await.map_err(ApiErr::internal)?;

    txn.audit(
        "proxy_user",
        id,
        AuditAction::Update,
        claims.sub,
        serde_json::json!({ "field": "password", "changed": true }),
    );
    txn.commit().await.map_err(ApiErr::internal)?;

    Ok(Json(UserResponse::from(updated)))
}

async fn count_admins(db: &DatabaseConnection) -> Result<u64, ApiErr> {
    proxy_user::Entity::find()
        .filter(proxy_user::Column::IsAdmin.eq(true))
        .count(db)
        .await
        .map_err(ApiErr::internal)
}

pub async fn delete_user(
    AdminClaims(claims): AdminClaims,
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiErr> {
    let user = proxy_user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(ApiErr::internal)?
        .ok_or_else(|| ApiErr::not_found("User not found"))?;

    // Guard: last-admin check takes priority so the message is unambiguous.
    if user.is_admin && count_admins(&state.db).await? == 1 {
        return Err(ApiErr::conflict("Cannot delete the last admin user"));
    }

    // Guard: cannot delete your own account.
    if claims.sub == id {
        return Err(ApiErr::conflict("Cannot delete your own account"));
    }

    let mut txn = AuditedTxn::begin(&state.db)
        .await
        .map_err(ApiErr::internal)?;

    txn.audit(
        "proxy_user",
        id,
        AuditAction::Delete,
        claims.sub,
        serde_json::json!({
            "before": {
                "username": user.username,
                "is_admin": user.is_admin,
                "is_active": user.is_active,
                "email": user.email,
                "display_name": user.display_name,
            }
        }),
    );

    let active: proxy_user::ActiveModel = user.into();
    active.delete(&*txn).await.map_err(ApiErr::internal)?;

    txn.commit().await.map_err(ApiErr::internal)?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        admin::{discovery_job, jwt},
        auth::Auth,
        engine::EngineCache,
        entity::proxy_user,
    };
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use chrono::Utc;
    use migration::MigratorTrait as _;
    use sea_orm::{ActiveModelTrait, Database, DatabaseConnection, Set};
    use std::sync::{Arc, OnceLock};
    use tokio::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    const JWT_SECRET: &str = "test-jwt-secret-key-32-chars-pad";

    fn shared_wasm_runtime() -> Arc<crate::decision::wasm::WasmDecisionRuntime> {
        static RUNTIME: OnceLock<Arc<crate::decision::wasm::WasmDecisionRuntime>> = OnceLock::new();
        RUNTIME
            .get_or_init(|| Arc::new(crate::decision::wasm::WasmDecisionRuntime::new().unwrap()))
            .clone()
    }

    async fn setup_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        db
    }

    fn make_state(db: DatabaseConnection) -> AdminState {
        let wasm_runtime = shared_wasm_runtime();
        let engine_cache = EngineCache::new(db.clone(), [0u8; 32], wasm_runtime.clone());
        AdminState {
            auth: Arc::new(Auth::new(db.clone())),
            db,
            jwt_secret: JWT_SECRET.to_string(),
            jwt_expiry_hours: 1,
            engine_cache,
            master_key: [0u8; 32],
            job_store: Arc::new(Mutex::new(discovery_job::JobStore::new())),
            policy_hook: None,
            proxy_handler: None,
            wasm_runtime,
        }
    }

    fn admin_token(id: Uuid) -> String {
        let claims = jwt::Claims {
            sub: id,
            username: "admin".to_string(),
            is_admin: true,
            exp: (Utc::now().timestamp() as u64) + 3600,
        };
        jwt::encode_jwt(&claims, JWT_SECRET).unwrap()
    }

    async fn insert_user(db: &DatabaseConnection, id: Uuid, username: &str, is_admin: bool) {
        let now = Utc::now().naive_utc();
        proxy_user::ActiveModel {
            id: Set(id),
            username: Set(username.to_string()),
            password_hash: Set("hash".to_string()),
            is_admin: Set(is_admin),
            is_active: Set(true),
            email: Set(None),
            display_name: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    fn make_router(state: AdminState) -> Router {
        Router::new()
            .route(
                "/users/{id}",
                axum::routing::put(update_user).delete(delete_user),
            )
            .with_state(state)
    }

    fn json_body(value: serde_json::Value) -> Body {
        Body::from(serde_json::to_string(&value).unwrap())
    }

    // ===== DELETE tests =====

    #[tokio::test]
    async fn delete_last_admin_rejected() {
        let db = setup_db().await;
        let target_id = Uuid::now_v7();
        // caller == target: sole admin trying to delete themselves — last-admin guard fires first
        let caller_id = target_id;
        insert_user(&db, target_id, "admin1", true).await;

        let token = admin_token(caller_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/users/{target_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn delete_self_rejected() {
        let db = setup_db().await;
        let caller_id = Uuid::now_v7();
        let other_id = Uuid::now_v7();
        insert_user(&db, caller_id, "admin1", true).await;
        insert_user(&db, other_id, "admin2", true).await;

        let token = admin_token(caller_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/users/{caller_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn delete_non_admin_ok() {
        let db = setup_db().await;
        let caller_id = Uuid::now_v7();
        let target_id = Uuid::now_v7();
        insert_user(&db, caller_id, "admin1", true).await;
        insert_user(&db, target_id, "user1", false).await;

        let token = admin_token(caller_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/users/{target_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_non_last_admin_ok() {
        let db = setup_db().await;
        let caller_id = Uuid::now_v7();
        let target_id = Uuid::now_v7();
        insert_user(&db, caller_id, "admin1", true).await;
        insert_user(&db, target_id, "admin2", true).await;

        let token = admin_token(caller_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/users/{target_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NO_CONTENT);
    }

    // ===== UPDATE/DEMOTE tests =====

    #[tokio::test]
    async fn demote_last_admin_rejected() {
        let db = setup_db().await;
        let target_id = Uuid::now_v7();
        // caller is a phantom user so self-demotion guard won't fire
        let caller_id = Uuid::now_v7();
        insert_user(&db, target_id, "admin1", true).await;

        let token = admin_token(caller_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/users/{target_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({"is_admin": false})))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn demote_self_rejected() {
        let db = setup_db().await;
        let caller_id = Uuid::now_v7();
        let other_id = Uuid::now_v7();
        insert_user(&db, caller_id, "admin1", true).await;
        insert_user(&db, other_id, "admin2", true).await;

        let token = admin_token(caller_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/users/{caller_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({"is_admin": false})))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn demote_non_last_admin_ok() {
        let db = setup_db().await;
        let caller_id = Uuid::now_v7();
        let target_id = Uuid::now_v7();
        insert_user(&db, caller_id, "admin1", true).await;
        insert_user(&db, target_id, "admin2", true).await;

        let token = admin_token(caller_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/users/{target_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({"is_admin": false})))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn update_non_admin_field_ok() {
        let db = setup_db().await;
        let admin_id = Uuid::now_v7();
        insert_user(&db, admin_id, "admin1", true).await;

        let token = admin_token(admin_id);
        let res = make_router(make_state(db))
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/users/{admin_id}"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(json_body(serde_json::json!({"email": "new@example.com"})))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }
}
